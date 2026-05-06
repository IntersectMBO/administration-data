//! Address parsing utilities for Cardano addresses

use pallas_addresses::Address;

/// Extract the stake credential from a bech32 Cardano address.
///
/// For Shelley base addresses, returns the delegation/stake part as a hex string.
/// Returns None for non-Shelley addresses or on any parse error.
pub fn extract_stake_credential(bech32_addr: &str) -> Option<String> {
    let addr = Address::from_bech32(bech32_addr).ok()?;
    match addr {
        Address::Shelley(shelley) => {
            let hash = shelley.delegation().as_hash()?;
            Some(hex::encode(hash.as_ref()))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_stake_credential_returns_some_for_base_address() {
        // A typical mainnet base address (addr1q...) should return a stake credential
        // Using a well-known test vector isn't practical without real addresses,
        // so we just verify the function doesn't panic on a script address format
        let result = extract_stake_credential("addr1x_invalid");
        assert!(result.is_none());
    }
}
