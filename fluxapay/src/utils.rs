use soroban_sdk::{Bytes, Env, Map, String};

/// Maximum number of key/value pairs allowed in payment/link metadata.
pub const MAX_METADATA_KEYS: u32 = 20;
/// Maximum length of a metadata key (characters).
pub const MAX_METADATA_KEY_LEN: u32 = 64;
/// Maximum length of a metadata value (characters).
pub const MAX_METADATA_VALUE_LEN: u32 = 256;

/// Validate metadata map size and key/value length limits.
///
/// Limits: ≤20 keys, each key ≤64 chars, each value ≤256 chars.
pub fn validate_metadata(meta_map: &Map<String, String>) -> Result<(), crate::Error> {
    if meta_map.len() > MAX_METADATA_KEYS {
        return Err(crate::Error::MetadataTooLarge);
    }
    for (key, value) in meta_map.iter() {
        if key.len() > MAX_METADATA_KEY_LEN || value.len() > MAX_METADATA_VALUE_LEN {
            return Err(crate::Error::MetadataValueTooLong);
        }
    }
    Ok(())
}

/// Validates that a string is a valid IPFS CID (CIDv0 or CIDv1).
///
/// Issue #622: Strengthened from length-only to prefix + length structural check.
///
/// CIDv0: Base58-encoded SHA2-256 multihash. Always starts with "Qm" and is exactly 46 chars.
/// CIDv1 base32 (most common): Starts with "bafy" or "BAFY" (base32 SHA2-256) and is ≥ 59 chars.
/// CIDv1 base16 (hex): Starts with "f" followed by hex digits, at least 34 chars.
///
/// This performs a lightweight structural check (prefix + length) without
/// a full base58/base32 decode, which is not feasible in `no_std` without
/// additional crates.
pub fn is_valid_cid(s: &String) -> bool {
    let len = s.len() as usize;

    // Need at least 4 bytes to check prefix
    if len < 4 {
        return false;
    }

    let mut buf = [0u8; 64];
    let read_len = len.min(64);
    s.copy_into_slice(&mut buf[..read_len]);

    // CIDv0: starts with "Qm" and is exactly 46 characters
    if buf[0] == b'Q' && buf[1] == b'm' {
        return len == 46;
    }

    // CIDv1 base32 (most common): starts with "bafy" and is at least 59 characters
    if buf[0] == b'b' && buf[1] == b'a' && buf[2] == b'f' && buf[3] == b'y' {
        return len >= 59;
    }

    // CIDv1 base32 upper: starts with "BAFY"
    if buf[0] == b'B' && buf[1] == b'A' && buf[2] == b'F' && buf[3] == b'Y' {
        return len >= 59;
    }

    // CIDv1 base16 (hex): starts with "f" followed by hex digits, at least 34 chars
    if buf[0] == b'f' && len >= 34 {
        return true;
    }

    false
}

/// Alias for [`is_valid_cid`] — kept for backward compatibility.
#[inline]
pub fn validate_ipfs_multihash(s: &String) -> bool {
    is_valid_cid(s)
}

/// Validates a user-supplied ID (payment_id, dispute_id, etc.).
///
/// Rules (issue #404):
/// - Length: 3–64 characters (inclusive)
/// - Allowed characters: ASCII alphanumeric, `-`, `_`
pub fn validate_id(s: &String) -> bool {
    let len = s.len() as usize;
    if !(3..=64).contains(&len) {
        return false;
    }
    let mut buf = [0u8; 64];
    s.copy_into_slice(&mut buf[..len]);
    for b in buf[..len].iter() {
        let valid = b.is_ascii_alphanumeric() || *b == b'-' || *b == b'_';
        if !valid {
            return false;
        }
    }
    true
}

/// Converts a `u64` counter to a Soroban `String` with the given prefix.
///
/// Examples: `format_id(env, "refund_", 1)` → `"refund_1"`
///           `format_id(env, "dispute_", 20)` → `"dispute_20"`
pub fn format_id(env: &Env, prefix: &str, n: u64) -> String {
    let mut result = Bytes::new(env);

    // Write prefix bytes
    for byte in prefix.as_bytes() {
        result.push_back(*byte);
    }

    // Build digits in reverse, then reverse them into result
    let mut temp = Bytes::new(env);
    let mut num = n;
    loop {
        temp.push_back((num % 10) as u8 + b'0');
        num /= 10;
        if num == 0 {
            break;
        }
    }
    let len = temp.len();
    for i in 0..len {
        result.push_back(temp.get(len - i - 1).unwrap());
    }

    // Copy into a fixed-size slice and convert to Soroban String
    let mut arr = [0u8; 64];
    let final_len = result.len().min(64);
    for i in 0..final_len {
        arr[i as usize] = result.get(i).unwrap();
    }
    String::from_bytes(env, &arr[..final_len as usize])
}

/// Concatenate Soroban strings (used for shareable payment URLs).
pub fn concat_strings(env: &Env, parts: &[String]) -> String {
    let mut result = Bytes::new(env);
    for part in parts {
        let bytes = part.to_bytes();
        for i in 0..bytes.len() {
            result.push_back(bytes.get(i).unwrap());
        }
    }
    let final_len = result.len().min(512);
    let mut arr = [0u8; 512];
    for i in 0..final_len {
        arr[i as usize] = result.get(i).unwrap();
    }
    String::from_bytes(env, &arr[..final_len as usize])
}

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::Env;

    fn id_str<'a>(_env: &Env, s: &String, buf: &'a mut [u8; 64]) -> &'a str {
        let len = s.len() as usize;
        s.copy_into_slice(&mut buf[..len]);
        core::str::from_utf8(&buf[..len]).unwrap()
    }

    #[test]
    fn test_single_digit() {
        let env = Env::default();
        let mut buf = [0u8; 64];
        let id = format_id(&env, "refund_", 1);
        assert_eq!(id_str(&env, &id, &mut buf), "refund_1");
    }

    #[test]
    fn test_double_digit() {
        let env = Env::default();
        let mut buf = [0u8; 64];
        let id = format_id(&env, "refund_", 20);
        assert_eq!(id_str(&env, &id, &mut buf), "refund_20");
    }

    #[test]
    fn test_large_number() {
        let env = Env::default();
        let mut buf = [0u8; 64];
        let id = format_id(&env, "dispute_", 1_000_000);
        assert_eq!(id_str(&env, &id, &mut buf), "dispute_1000000");
    }

    #[test]
    fn test_zero() {
        let env = Env::default();
        let mut buf = [0u8; 64];
        let id = format_id(&env, "refund_", 0);
        assert_eq!(id_str(&env, &id, &mut buf), "refund_0");
    }

    #[test]
    fn test_u64_max() {
        let env = Env::default();
        let mut buf = [0u8; 64];
        let id = format_id(&env, "id_", u64::MAX);
        // u64::MAX = 18446744073709551615 (20 digits) + prefix "id_" (3) = 23 bytes, fits in 64
        assert_eq!(id_str(&env, &id, &mut buf), "id_18446744073709551615");
    }

    #[test]
    fn test_dispute_prefix() {
        let env = Env::default();
        let mut buf = [0u8; 64];
        let id = format_id(&env, "dispute_", 7);
        assert_eq!(id_str(&env, &id, &mut buf), "dispute_7");
    }

    #[test]
    fn test_uniqueness() {
        let env = Env::default();
        let id1 = format_id(&env, "refund_", 1);
        let id2 = format_id(&env, "refund_", 2);
        assert_ne!(id1, id2);
    }
}
