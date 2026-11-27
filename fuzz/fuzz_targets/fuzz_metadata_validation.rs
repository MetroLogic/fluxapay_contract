//! Issue #1024: Fuzz `fluxapay::utils::validate_metadata` at and beyond the
//! documented metadata limits (`MAX_METADATA_KEYS` = 20 keys,
//! `MAX_METADATA_KEY_LEN` = 64, `MAX_METADATA_VALUE_LEN` = 256 chars/bytes),
//! as enforced on the optional metadata map accepted by `create_payment` /
//! `create_payment_link`.
//!
//! Caps the generated entry count and per-entry length a little past each
//! limit (not unboundedly) so the fuzzer's time budget is spent near the
//! actual boundaries rather than on multi-megabyte maps that would just
//! slow it down without adding coverage. Keys/values are built from raw
//! bytes (not `arbitrary`'s `String`) so non-UTF8 input — like a malicious
//! client could submit — is exercised too, matching the convention in
//! `fuzz_validate_payment_id.rs`.
#![no_main]

use arbitrary::Arbitrary;
use fluxapay::utils::{
    validate_metadata, MAX_METADATA_KEYS, MAX_METADATA_KEY_LEN, MAX_METADATA_VALUE_LEN,
};
use libfuzzer_sys::fuzz_target;
use soroban_sdk::{Env, Map, String as SorobanString};

// A bit past each real limit, not unbounded (see module doc comment).
const MAX_ENTRIES: usize = 32; // MAX_METADATA_KEYS is 20
const MAX_KEY_LEN: usize = 96; // MAX_METADATA_KEY_LEN is 64
const MAX_VALUE_LEN: usize = 320; // MAX_METADATA_VALUE_LEN is 256

#[derive(Debug, Arbitrary)]
struct Entry {
    key: Vec<u8>,
    value: Vec<u8>,
}

#[derive(Debug, Arbitrary)]
struct Input {
    entries: Vec<Entry>,
}

fn cap(bytes: &[u8], max: usize) -> &[u8] {
    if bytes.len() > max {
        &bytes[..max]
    } else {
        bytes
    }
}

fuzz_target!(|input: Input| {
    let env = Env::default();
    let entries: Vec<Entry> = input.entries.into_iter().take(MAX_ENTRIES).collect();

    let mut map: Map<SorobanString, SorobanString> = Map::new(&env);
    let mut any_oversized_key = false;
    let mut any_oversized_value = false;

    for entry in &entries {
        let key_bytes = cap(&entry.key, MAX_KEY_LEN);
        let value_bytes = cap(&entry.value, MAX_VALUE_LEN);
        if key_bytes.len() as u32 > MAX_METADATA_KEY_LEN {
            any_oversized_key = true;
        }
        if value_bytes.len() as u32 > MAX_METADATA_VALUE_LEN {
            any_oversized_value = true;
        }
        let key = SorobanString::from_bytes(&env, key_bytes);
        let value = SorobanString::from_bytes(&env, value_bytes);
        map.set(key, value);
    }

    // Must never panic, regardless of map size/content.
    let result = validate_metadata(&map);

    let too_many_keys = map.len() > MAX_METADATA_KEYS;
    let expect_err = too_many_keys || any_oversized_key || any_oversized_value;

    match result {
        Ok(()) => assert!(
            !expect_err,
            "validate_metadata accepted a map that should have been rejected \
             (len={}, oversized_key={}, oversized_value={})",
            map.len(),
            any_oversized_key,
            any_oversized_value
        ),
        Err(_) => assert!(
            expect_err,
            "validate_metadata rejected a map within all documented limits (len={})",
            map.len()
        ),
    }
});
