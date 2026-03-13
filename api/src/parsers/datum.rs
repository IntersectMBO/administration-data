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

/// Parsed vendor contract datum
#[derive(Debug, Clone)]
pub struct ParsedVendorDatum {
    /// Vendor payment key hash (hex)
    pub vendor_payment_key_hash: String,
    /// Per-milestone data from datum
    pub milestones: Vec<ParsedMilestoneDatum>,
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

/// Parse a vendor contract datum from CBOR hex string
pub fn parse_vendor_contract_datum(cbor_hex: &str) -> anyhow::Result<ParsedVendorDatum> {
    let bytes = hex::decode(cbor_hex).context("invalid hex in datum")?;
    let datum: PlutusData =
        pallas_codec::minicbor::decode(&bytes).context("failed to decode CBOR datum")?;

    // Top-level: Constr(0, [vendor_info, milestones_array])
    let top_fields = expect_constr(&datum, 0, "top-level datum")?;
    if top_fields.len() < 2 {
        return Err(anyhow!(
            "top-level datum has {} fields, expected 2",
            top_fields.len()
        ));
    }

    // Field 0: Constr(0, [ByteString(vendor_payment_key_hash)])
    let vendor_fields = expect_constr(&top_fields[0], 0, "vendor info")?;
    if vendor_fields.is_empty() {
        return Err(anyhow!("vendor info has no fields"));
    }
    let vendor_payment_key_hash = expect_bytes(&vendor_fields[0], "vendor_payment_key_hash")?;

    // Field 1: Array of milestone Constrs
    let milestone_data_list = expect_array(&top_fields[1], "milestones array")?;

    let mut milestones = Vec::with_capacity(milestone_data_list.len());
    for (idx, ms_datum) in milestone_data_list.iter().enumerate() {
        let ms = parse_milestone_datum(ms_datum, idx)
            .with_context(|| format!("milestone {}", idx))?;
        milestones.push(ms);
    }

    Ok(ParsedVendorDatum {
        vendor_payment_key_hash,
        milestones,
    })
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
    fn test_parse_empty_hex_fails() {
        assert!(parse_vendor_contract_datum("").is_err());
    }

    #[test]
    fn test_parse_invalid_hex_fails() {
        assert!(parse_vendor_contract_datum("zzzz").is_err());
    }

    #[test]
    fn test_parse_invalid_cbor_fails() {
        assert!(parse_vendor_contract_datum("deadbeef").is_err());
    }
}
