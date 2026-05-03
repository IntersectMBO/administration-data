//! CBOR datum parser for Plutus vendor contract datums
//!
//! Parses inline datum CBOR hex from `address_utxo.inline_datum` into structured data.
//!
//! Datum structure (from Plutus vendor contract):
//! ```text
//! Constr(0, [
//!   Constr(0, [ByteString(vendor_payment_key_hash)]),
//!   Array([
//!     Constr(0, [BigInt(time_limit), Map(value), Constr(0|1, [])]),  // per milestone
//!     ...
//!   ])
//! ])
//! ```
//!
//! pallas uses tag 121 = constructor 0, tag 122 = constructor 1.

use anyhow::{anyhow, Context};
use pallas_primitives::alonzo::{BigInt, PlutusData};

/// Parsed vendor contract datum.
///
/// Partial: each section may independently succeed or fail. The call site
/// persists what's `Ok` and writes the error string to `datum_parse_error`
/// for the relevant row, so failures are queryable in SQL rather than
/// scraped from logs.
#[derive(Debug, Clone)]
pub struct ParsedProjectDatum {
    /// Vendor payment key hash (hex). `None` means vendor-info section
    /// failed to parse — see `vendor_info_error`.
    pub vendor_payment_key_hash: Option<String>,
    /// Error message if vendor-info section couldn't be parsed.
    pub vendor_info_error: Option<String>,
    /// Per-milestone results. `Ok` rows update the milestone row;
    /// `Err` rows record the error in `treasury.milestones.datum_parse_error`.
    pub milestones: Vec<Result<ParsedMilestoneDatum, String>>,
    /// Top-level CBOR / shape error. When set, every other field is empty.
    pub top_level_error: Option<String>,
}

/// Parsed milestone data from inline datum
#[derive(Debug, Clone)]
pub struct ParsedMilestoneDatum {
    /// POSIXTime in milliseconds
    pub time_limit: i64,
    /// Lovelace amount from Value map {"": {"": amount}}
    pub amount_lovelace: i64,
    /// Constructor 0 = active, Constructor 1 = paused
    pub paused: bool,
}

/// Parse a vendor contract datum from CBOR hex string.
///
/// Two on-chain vendor-info shapes are supported:
///
/// 1. Single-key (`m-N` projects):
///    `Constr(0, [Constr(0, [bytes(28)]), [milestones]])`
///
/// 2. Multi-key (`UTXO-*` projects, e.g. `UTXO-EC-0002-25-*`):
///    `Constr(0, [Constr(1, [(Constr(0, [bytes(28)]), Constr(N, [bytes(28)]))]), [milestones]])`
///    — vendor info wraps a tuple of two parties (different signature
///    types). We walk the subtree and collect every 28-byte BoundedBytes
///    (Cardano key-hash size, Blake2b-224), joining them with `,` for
///    storage in the single `vendor_payment_key_hash` column.
///
/// The milestones array structure is identical between the two formats.
///
/// Returns a partial result: a top-level CBOR/shape error short-circuits
/// the whole datum, otherwise vendor info and each milestone parse
/// independently so a single bad milestone doesn't lose the rest.
pub fn parse_project_datum(cbor_hex: &str) -> ParsedProjectDatum {
    let mut out = ParsedProjectDatum {
        vendor_payment_key_hash: None,
        vendor_info_error: None,
        milestones: Vec::new(),
        top_level_error: None,
    };

    let bytes = match hex::decode(cbor_hex).context("invalid hex in datum") {
        Ok(b) => b,
        Err(e) => { out.top_level_error = Some(format!("{:#}", e)); return out; }
    };
    let datum: PlutusData = match pallas_codec::minicbor::decode(&bytes)
        .context("failed to decode CBOR datum") {
        Ok(d) => d,
        Err(e) => { out.top_level_error = Some(format!("{:#}", e)); return out; }
    };

    // Top-level: Constr(0, [vendor_info, milestones_array])
    let top_fields = match expect_constr(&datum, 0, "top-level datum") {
        Ok(f) => f,
        Err(e) => { out.top_level_error = Some(format!("{:#}", e)); return out; }
    };
    if top_fields.len() < 2 {
        out.top_level_error = Some(format!(
            "top-level datum has {} fields, expected 2",
            top_fields.len()
        ));
        return out;
    }

    // Field 0: vendor info. Walk the subtree and collect every 28-byte
    // BoundedBytes — handles both single-key and multi-key shapes uniformly.
    let mut hashes: Vec<String> = Vec::new();
    collect_key_hashes(&top_fields[0], &mut hashes);
    if hashes.is_empty() {
        out.vendor_info_error = Some("no 28-byte key hash found in vendor info".to_string());
    } else {
        out.vendor_payment_key_hash = Some(hashes.join(","));
    }

    // Field 1: Array of milestone Constrs
    match expect_array(&top_fields[1], "milestones array") {
        Ok(milestone_data_list) => {
            out.milestones.reserve(milestone_data_list.len());
            for (idx, ms_datum) in milestone_data_list.iter().enumerate() {
                let r = parse_milestone_datum(ms_datum, idx)
                    .with_context(|| format!("milestone {}", idx))
                    .map_err(|e| format!("{:#}", e));
                out.milestones.push(r);
            }
        }
        Err(e) => {
            // Surface as a top-level error so the call site doesn't silently
            // skip the milestones UPDATE loop.
            out.top_level_error = Some(format!("{:#}", e));
        }
    }

    out
}

/// Parse a single milestone datum: Constr(0, [BigInt(time_limit), Map(value), Constr(0|1, [])])
fn parse_milestone_datum(datum: &PlutusData, _idx: usize) -> anyhow::Result<ParsedMilestoneDatum> {
    let fields = expect_constr(datum, 0, "milestone")?;
    if fields.len() < 3 {
        return Err(anyhow!(
            "milestone datum has {} fields, expected 3",
            fields.len()
        ));
    }

    // Field 0: time_limit as BigInt
    let time_limit = expect_integer(&fields[0], "time_limit")?;

    // Field 1: Value as Map - extract lovelace from {"": {"": amount}}
    let amount_lovelace = extract_lovelace_from_value(&fields[1])?;

    // Field 2: Constr(0|1, []) — 0=active, 1=paused
    let paused = match &fields[2] {
        PlutusData::Constr(constr) => {
            // pallas tag: 121 = constructor 0 (active), 122 = constructor 1 (paused)
            match constr.tag {
                121 => false,
                122 => true,
                _ => {
                    return Err(anyhow!(
                        "unexpected pause constructor tag: {}",
                        constr.tag
                    ))
                }
            }
        }
        _ => return Err(anyhow!("expected Constr for pause flag")),
    };

    Ok(ParsedMilestoneDatum {
        time_limit,
        amount_lovelace,
        paused,
    })
}

/// Extract lovelace amount from a Plutus Value:
/// Map({ ByteString("") => Map({ ByteString("") => BigInt(amount) }) })
fn extract_lovelace_from_value(datum: &PlutusData) -> anyhow::Result<i64> {
    match datum {
        PlutusData::Map(entries) => {
            let pairs: Vec<_> = entries.clone().to_vec();
            // Look for the empty-bytestring key (ADA policy ID)
            for (key, val) in &pairs {
                if is_empty_bytes(key) {
                    // Inner map: {"": amount}
                    match val {
                        PlutusData::Map(inner_entries) => {
                            let inner_pairs: Vec<_> = inner_entries.clone().to_vec();
                            for (inner_key, inner_val) in &inner_pairs {
                                if is_empty_bytes(inner_key) {
                                    return expect_integer(inner_val, "lovelace amount");
                                }
                            }
                            return Err(anyhow!("no empty-key entry in inner Value map"));
                        }
                        // Some datums encode Value as Map({ "" => amount }) (flat)
                        _ => return expect_integer(val, "lovelace amount"),
                    }
                }
            }
            Err(anyhow!("no ADA (empty policy) key in Value map"))
        }
        _ => Err(anyhow!("expected Map for Value, got {:?}", datum_type_name(datum))),
    }
}

// ============================================================================
// Helpers
// ============================================================================

/// Walk a Plutus datum subtree and collect every 28-byte BoundedBytes
/// (Cardano key-hash size, Blake2b-224) it contains, in pre-order.
fn collect_key_hashes(datum: &PlutusData, acc: &mut Vec<String>) {
    match datum {
        PlutusData::BoundedBytes(b) => {
            if b.len() == 28 {
                acc.push(hex::encode(b.as_slice()));
            }
        }
        PlutusData::Constr(c) => {
            for f in &c.fields {
                collect_key_hashes(f, acc);
            }
        }
        PlutusData::Array(arr) => {
            for x in arr {
                collect_key_hashes(x, acc);
            }
        }
        _ => {}
    }
}

fn expect_constr<'a>(
    datum: &'a PlutusData,
    expected_tag_offset: u64,
    context: &str,
) -> anyhow::Result<&'a Vec<PlutusData>> {
    match datum {
        PlutusData::Constr(constr) => {
            let expected_tag = 121 + expected_tag_offset;
            if constr.tag != expected_tag {
                return Err(anyhow!(
                    "{}: expected constructor tag {}, got {}",
                    context,
                    expected_tag,
                    constr.tag
                ));
            }
            Ok(&constr.fields)
        }
        _ => Err(anyhow!(
            "{}: expected Constr, got {:?}",
            context,
            datum_type_name(datum)
        )),
    }
}

fn expect_array<'a>(
    datum: &'a PlutusData,
    context: &str,
) -> anyhow::Result<&'a Vec<PlutusData>> {
    match datum {
        PlutusData::Array(arr) => Ok(arr),
        _ => Err(anyhow!(
            "{}: expected Array, got {:?}",
            context,
            datum_type_name(datum)
        )),
    }
}

#[allow(dead_code)]
fn expect_bytes(datum: &PlutusData, context: &str) -> anyhow::Result<String> {
    match datum {
        PlutusData::BoundedBytes(bytes) => Ok(hex::encode(bytes.as_slice())),
        _ => Err(anyhow!(
            "{}: expected BoundedBytes, got {:?}",
            context,
            datum_type_name(datum)
        )),
    }
}

fn expect_integer(datum: &PlutusData, context: &str) -> anyhow::Result<i64> {
    match datum {
        PlutusData::BigInt(big_int) => {
            match big_int {
                BigInt::Int(int_val) => {
                    // pallas_codec::utils::Int implements Into<i128>
                    let n: i128 = (*int_val).into();
                    Ok(n as i64)
                }
                BigInt::BigUInt(bytes) => {
                    let mut val: i64 = 0;
                    for b in bytes.as_slice() {
                        val = val.checked_mul(256).unwrap_or(i64::MAX);
                        val = val.checked_add(*b as i64).unwrap_or(i64::MAX);
                    }
                    Ok(val)
                }
                BigInt::BigNInt(bytes) => {
                    let mut val: i64 = 0;
                    for b in bytes.as_slice() {
                        val = val.checked_mul(256).unwrap_or(i64::MIN);
                        val = val.checked_add(*b as i64).unwrap_or(i64::MIN);
                    }
                    Ok(-val)
                }
            }
        }
        _ => Err(anyhow!(
            "{}: expected BigInt, got {:?}",
            context,
            datum_type_name(datum)
        )),
    }
}

fn is_empty_bytes(datum: &PlutusData) -> bool {
    matches!(datum, PlutusData::BoundedBytes(bytes) if bytes.is_empty())
}

fn datum_type_name(datum: &PlutusData) -> &'static str {
    match datum {
        PlutusData::Constr(_) => "Constr",
        PlutusData::Map(_) => "Map",
        PlutusData::BigInt(_) => "BigInt",
        PlutusData::BoundedBytes(_) => "BoundedBytes",
        PlutusData::Array(_) => "Array",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_empty_hex_returns_top_level_error() {
        let r = parse_project_datum("");
        assert!(r.top_level_error.is_some());
        assert!(r.vendor_payment_key_hash.is_none());
        assert!(r.milestones.is_empty());
    }

    #[test]
    fn test_parse_invalid_hex_returns_top_level_error() {
        let r = parse_project_datum("zzzz");
        assert!(r.top_level_error.is_some());
    }

    #[test]
    fn test_parse_invalid_cbor_returns_top_level_error() {
        let r = parse_project_datum("deadbeef");
        assert!(r.top_level_error.is_some());
    }

    #[test]
    fn test_utxo_emi_0001_25_fixture_parses() {
        // Real on-chain datum from project UTXO-EMI-0001-25
        // (tx 5849b0ec727e062ef2ee29076f0f5fcc72206081f40c2dc9cba604bca93c9e3c, output 0).
        // Multi-key vendor info (Constr(1, [Array([Constr(0, [bytes28]), Constr(N, [bytes28])])])).
        let hex = include_str!("../../tests/fixtures/utxo_emi_0001_25.hex").trim();
        let r = parse_project_datum(hex);
        assert!(r.top_level_error.is_none(), "top-level error: {:?}", r.top_level_error);
        assert!(r.vendor_info_error.is_none(), "vendor info error: {:?}", r.vendor_info_error);
        let kh = r.vendor_payment_key_hash.expect("key hash");
        assert!(kh.contains(','), "expected multi-key (comma-joined), got {}", kh);
        assert_eq!(r.milestones.len(), 5, "expected 5 milestones");
        for (i, m) in r.milestones.iter().enumerate() {
            assert!(m.is_ok(), "milestone {} failed: {:?}", i, m);
        }
    }

    #[test]
    fn test_utxo_ec_0002_25_01_fixture_parses() {
        // 16-milestone fund datum from UTXO-EC-0002-25-01 (largest variant).
        let hex = include_str!("../../tests/fixtures/utxo_ec_0002_25_01.hex").trim();
        let r = parse_project_datum(hex);
        assert!(r.top_level_error.is_none(), "top-level error: {:?}", r.top_level_error);
        assert!(r.vendor_payment_key_hash.is_some());
        assert_eq!(r.milestones.len(), 16);
        assert!(r.milestones.iter().all(|m| m.is_ok()));
    }

    #[test]
    fn test_utxo_ec_0002_25_03_fixture_parses() {
        // Real on-chain datum from project UTXO-EC-0002-25-03 (20 milestones).
        // This datum was historically corrupted by a prior bug; ensures the
        // post-fix parser still handles it correctly.
        let hex = include_str!("../../tests/fixtures/utxo_ec_0002_25_03.hex").trim();
        let r = parse_project_datum(hex);
        assert!(r.top_level_error.is_none(), "top-level error: {:?}", r.top_level_error);
        assert!(r.vendor_payment_key_hash.is_some());
        assert_eq!(r.milestones.len(), 20);
        assert!(r.milestones.iter().all(|m| m.is_ok()));
    }

    #[test]
    fn test_partial_parse_keeps_vendor_info_when_milestones_fail() {
        // Hand-crafted datum: valid vendor info, milestones array contains a
        // bogus milestone (tag 199 instead of 121). The partial parser should
        // still surface the vendor key hash.
        // Constr(0, [
        //   Constr(0, [bytes(28)]),                 // vendor_info: single-key
        //   [Constr(199, [])]                       // milestones with one bad entry
        // ])
        // tag 199 is not a valid Plutus constructor, so milestone parsing
        // returns Err for that entry but the vendor info is still extracted.
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&[0xd8, 0x79, 0x9f]); // Constr(0, [
        bytes.extend_from_slice(&[0xd8, 0x79, 0x9f]); //   Constr(0, [
        bytes.push(0x58); bytes.push(28);              //     bytes(28)
        bytes.extend_from_slice(&[0xaa; 28]);
        bytes.push(0xff);                              //   ])
        bytes.push(0x9f);                              //   [
        bytes.extend_from_slice(&[0xd9, 0x05, 0x06]); //     tag 1286 (constructor 6 in extended range)
        bytes.push(0x80);                              //     []
        bytes.push(0xff);                              //   ]
        bytes.push(0xff);                              // ])

        let hex = hex::encode(&bytes);
        let r = parse_project_datum(&hex);
        // Vendor info should succeed even though the milestone is malformed.
        assert_eq!(
            r.vendor_payment_key_hash.as_deref(),
            Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
        );
        assert!(r.vendor_info_error.is_none());
    }
}
