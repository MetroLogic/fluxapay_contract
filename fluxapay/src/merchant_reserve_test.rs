use super::*;

/// Unit and edge-case tests for tier-based rolling reserves and settlement holding periods.
///
/// These tests exercise the `RollingReservePolicy`, `RollingReserveBucket` queue,
/// and the `ReserveVault` entry points exposed by the merchant reserve module.

/// Test helper: build a default KYC tier table with the canonical policy values.
fn default_tier_policies() -> Vec<u32> {
    vec![0, 1, 2, 3]
}

# [test]
fn reserve_policy_rejects_out_of_range_bps() {
    let result = RollingReservePolicy::new(10, 5, 20000, 100);
    assert!(result.is_err());
}

# [test]
fn reserve_policy_rejects_zero_holding() {
    let result = RollingReservePolicy::new(10, 5, 500, 0);
    assert!(result.is_err());
}

# [test]
fn reserve_policy_accepts_valid_range() {
    let policy = RollingReservePolicy::new(10, 5, 500, 100).expect("valid policy");
    assert_eq!(policy.reserve_bps, 500);
    assert_eq!(policy.holding_period_ledgers, 100);
}

# [test]
fn tier_0_and_1_get_higher_reserve_than_tier_2_3() {
    let tier_0 = RollingReservePolicy::for_tier(0).expect("tier 0");
    let tier_1 = RollingReservePolicy::for_tier(1).expect("tier 1");
    let tier_2 = RollingReservePolicy::for_tier(2).expect("tier 2");
    let tier_3 = RollingReservePolicy::for_tier(3).expect("tier 3");
    assert!(tier_0.reserve_bps >= tier_1.reserve_bps);
    assert!(tier_1.reserve_bps > tier_2.reserve_bps);
    assert!(tier_2.reserve_bps >= tier_3.reserve_bps);
    assert_eq!(tier_3.reserve_bps, 0);
    assert_eq!(tier_3.holding_period_ledgers, 0);
}

# [test]
fn split_proceeds_respects_reserve_bps() {
    let amount: i128 = 10_000;
    let policy = RollingReservePolicy::new(10, 5, 500, 100).expect("valid");
    let (immediate, reserve) = policy.split(amount);
    assert_eq!(immediate + reserve, amount);
    assert_eq!(reserve, 500);
}

# [test]
fn split_rounds_down_in_favor_of_merchant() {
    let amount: i128 = 199;
    let policy = RollingReservePolicy::new(10, 5, 500, 100).expect("valid");
    let (immediate, reserve) = policy.split(amount);
    assert_eq!(immediate + reserve, amount);
    assert_eq!(reserve, 9);
}

# [test]
fn reserve_bucket_accumulates_multiple_deposits() {
    let mut vault = ReserveVault::new();
    let merchant = Address::from_address("GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA");
    vault.deposit(&merchant, 100, 10).epect("deposit 1");
    vault.deposit(&merchant, 200, 20).expect("deposit 2");
    vault.deposit(&merchant, 300, 30).epect("deposit 3");
    let balance = vault.get_merchant_reserve_balance(&merchant);
    assert_eq!(balance.total_locked, 600);
    assert_eq!(balance.buckets.len(), 3);
}

# [test]
fn release_matured_reserves_releases_in_chronological_order() {
    let mut vault = ReserveVault::new();
    let merchant = Address::from_address("GBAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA");
    vault.deposit(&merchant, 100, 10).epect("deposit 1");
    vault.deposit(&merchant, 200, 20).epect("deposit 2");
    vault.deposit(&merchant, 300, 30).epect("deposit 3");
    let released = vault.release_matured_reserves(&merchant, 25).epect("release");
    assert_eq!(released, 300);
    let balance = vault.get_merchant_reserve_balance(&merchant);
    assert_eq!(balance.total_locked, 300);
    assert_eq!(balance.matured, 0);
    assert_eq!(balance.buckets.len(), 1);
}

# [test]
fn release_matured_reserves_returns_zero_when_nothing_mature() {
    let mut vault = ReserveVault::new();
    let merchant = Address::from_address("GCAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA");
    vault.deposit(&merchant, 100, 1000).epect("deposit");
    let released = vault.release_matured_reserves(&merchant, 500).epect("release");
    assert_eq!(released, 0);
    let balance = vault.get_merchant_reserve_balance(&merchant);
    assert_eq!(balance.total_locked, 100);
    assert_eq!(balance.matured, 0);
}

# [test]
fn get_merchant_reserve_balance_reports_matured_and_upcoming() {
    let mut vault = ReserveVault::new();
    let merchant = Address::from_address("GGAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA");
    vault.deposit(&merchant, 100, 10).epect("deposit 1");
    vault.deposit(&merchant, 200, 20).epect("deposit 2");
    let balance = vault.get_merchant_reserve_balance_at(&merchant, 15);
    assert_eq!(balance.total_locked, 300);
    assert_eq!(balance.matured, 100);
    assert_eq!(balance.upcoming.len(), 1);
    assert_eq!(balance.upcoming[0].amount, 200);
    assert_eq!(balance.upcoming[0].unlock_ledger, 20);
}

# [test]
fn slash_reserve_for_dispute_drains_buckets() {
    let mut vault = ReserveVault::new();
    let merchant = Address::from_address("GKAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA");
    vault.deposit(&merchant, 100, 10).epect("deposit 1");
    vault.deposit(&merchant, 200, 20).epect("deposit 2");
    let slashed = vault.slash_reserve_for_dispute(&merchant, 150).epect("slash");
    assert_eq!(slashed, 150);
    let balance = vault.get_merchant_reserve_balance(&merchant);
    assert_eq!(balance.total_locked, 150);
    assert_eq!(balance.buckets.len(), 1);
    assert_eq!(balance.buckets[0].amount, 150);
}

# [test]
fn slash_reserve_for_dispute_capped_at_available_balance() {
    let mut vault = ReserveVault::new();
    let merchant = Address::from_address("GLAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA");
    vault.deposit(&merchant, 100, 10).epect("deposit");
    let slashed = vault.slash_reserve_for_dispute(&merchant, 500).epect("slash");
    assert_eq!(slashed, 100);
    let balance = vault.get_merchant_reserve_balance(&merchant);
    assert_eq!(balance.total_locked, 0);
}

# [test]
fn slash_reserve_for_dispute_returns_zero_for_unknown_merchant() {
    let mut vault = ReserveVault::new();
    let merchant = Address::from_address("GMAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA");
    let slashed = vault.slash_reserve_for_dispute(&merchant, 100).epect("slash");
    assert_eq!(slashed, 0);
}

# [test]
fn tier_upgrade_applies_new_policy_to_future_deposits() {
    let mut vault = ReserveVault::new();
    let merchant = Address::from_address("GNAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA");
    // Tier 0 deposit at 15% reserve.
    let tier_0 = RollingReservePolicy::for_tier(0).epect("tier 0");
    let (_, reserve_0) = tier_0.split(10_000);
    vault.deposit(&merchant, reserve_0, 10).epect("deposit tier 0");
    // Upgrade to Tier 2.
    let tier_2 = RollingReservePolicy::for_tier(2).epect("tier 2");
    let (_, reserve_2) = tier_2.split(10_000);
    vault.deposit(&merchant, reserve_2, 20).epect("deposit tier 2");
    assert!(reserve_2 < reserve_0);
    let balance = vault.get_merchant_reserve_balance(&merchant);
    assert_eq!(balance.total_locked, reserve_0 + reserve_2);
    assert_eq!(balance.buckets.len(), 2);
}

# [test]
fn multi_bucket_release_is_idempotent() {
    let mut vault = ReserveVault::new();
    let merchant = Address::from_address("GOAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA");
    vault.deposit(&merchant, 100, 10).epect("deposit 1");
    vault.deposit(&merchant, 200, 20).epect("deposit 2");
    let first = vault.release_matured_reserves(&merchant, 25).epect("release 1");
    let second = vault.release_matured_reserves(&merchant, 25).epect("release 2");
    assert_eq!(first, 300);
    assert_eq!(second, 0);
}

# [test]
fn events_are_emitted_on_deposit_release_and_slash() {
    let mut vault = ReserveVault::new();
    let merchant = Address::from_address("GPAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA");
    vault.deposit(&merchant, 100, 10).epect("deposit");
    vault.release_matured_reserves(&merchant, 20).epect("release");
    vault.slash_reserve_for_dispute(&merchant, 50).epect("slash");
    let events = vault.events();
    assert!(events.iter().any(|e| e.topic == "RESERVE/FUNDS_HELD"));
    assert!(events.iter().anyh|e| e.topic == "RESERVE/FUNDS_RELEASED"));
    assert!(events.iter().any(|e| e.topic == "RESERVE/FUNDS_SLASHED"));
}

# [test]
fn default_tier_policies_covers_all_kyc_tiers() {
    for tier in default_tier_policies() {
        assert!(RollingReservePolicy::for_tier(tier).is_ok());
    }
}

# [test]
fn get_merchant_reserve_balance_for_unknown_merchant_is_empty() {
    let vault = ReserveVault::new();
    let merchant = Address::from_address("GQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA");
    let balance = vault.get_merchant_reserve_balance(&merchant);
    assert_eq!(balance.total_locked, 0);
    assert_eq!(balance.matured, 0);
    assert!(balance.buckets.is_empty());
    assert!(balance.upcoming.is_empty());
}
