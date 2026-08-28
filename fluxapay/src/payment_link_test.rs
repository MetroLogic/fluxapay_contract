use crate::{
    FXOracle, FXOracleClient, FiatConfig, LinkAnalytics, MaybeFiatConfig, PaymentLinkManager,
    PaymentLinkManagerClient,
};

use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    token, vec, Address, BytesN, Env, Map, String, Symbol, TryIntoVal,
};

fn setup_payment_link(env: &Env) -> (Address, PaymentLinkManagerClient<'_>) {
    let contract_id = env.register(PaymentLinkManager, ());
    let client = PaymentLinkManagerClient::new(env, &contract_id);
    let admin = Address::generate(env);
    (admin, client)
}

#[test]
fn test_create_link() {
    let env = Env::default();
    env.mock_all_auths();
    let (merchant, client) = setup_payment_link(&env);

    let link_id = String::from_str(&env, "link_123");
    let amount = Some(1000i128);
    let currency = Symbol::new(&env, "USDC");
    let description = String::from_str(&env, "Test Link");

    let id = client.create_link(
        &merchant,
        &link_id,
        &amount,
        &currency,
        &description,
        &None,
        &None,
        &false,
        &None,
        &MaybeFiatConfig::None,
        &None,
    );

    assert_eq!(id, link_id);
    let link = client.get_link(&link_id);
    assert_eq!(link.merchant_id, merchant);
    assert_eq!(link.amount, amount);
    assert!(link.active);
    assert!(!link.direct_transfer);
}

#[test]
fn test_use_link_fixed_amount() {
    let env = Env::default();
    env.mock_all_auths();
    let (merchant, client) = setup_payment_link(&env);
    let payer = Address::generate(&env);

    let link_id = String::from_str(&env, "fixed_link");
    let amount = 1000i128;
    client.create_link(
        &merchant,
        &link_id,
        &Some(amount),
        &Symbol::new(&env, "USDC"),
        &String::from_str(&env, "Fixed"),
        &None,
        &None,
        &false,
        &None,
        &MaybeFiatConfig::None,
        &None,
    );

    let payment_id = client.use_link(&payer, &link_id, &amount, &None);
    assert!(!payment_id.is_empty());

    let link = client.get_link(&link_id);
    assert_eq!(link.use_count, 1);
}

#[test]
fn test_use_link_unique_payment_ids_same_ledger() {
    let env = Env::default();
    env.mock_all_auths();
    let (merchant, client) = setup_payment_link(&env);
    let payer1 = Address::generate(&env);
    let payer2 = Address::generate(&env);

    let link_id = String::from_str(&env, "unique_pay_link");
    let amount = 100i128;
    client.create_link(
        &merchant,
        &link_id,
        &Some(amount),
        &Symbol::new(&env, "USDC"),
        &String::from_str(&env, "Unique IDs"),
        &None,
        &Some(10),
        &false,
        &None,
        &MaybeFiatConfig::None,
        &None,
    );

    let payment_id_1 = client.use_link(&payer1, &link_id, &amount, &None);
    let payment_id_2 = client.use_link(&payer2, &link_id, &amount, &None);
    assert_ne!(payment_id_1, payment_id_2);
}

#[test]
#[should_panic(expected = "Error(Contract, #406)")]
fn test_use_link_wrong_amount() {
    let env = Env::default();
    env.mock_all_auths();
    let (merchant, client) = setup_payment_link(&env);
    let payer = Address::generate(&env);

    let link_id = String::from_str(&env, "fixed_link_wrong");
    client.create_link(
        &merchant,
        &link_id,
        &Some(1000i128),
        &Symbol::new(&env, "USDC"),
        &String::from_str(&env, "Fixed"),
        &None,
        &None,
        &false,
        &None,
        &MaybeFiatConfig::None,
        &None,
    );

    client.use_link(&payer, &link_id, &500i128, &None);
}

#[test]
fn test_use_link_open_amount() {
    let env = Env::default();
    env.mock_all_auths();
    let (merchant, client) = setup_payment_link(&env);
    let payer = Address::generate(&env);

    let link_id = String::from_str(&env, "open_link");
    client.create_link(
        &merchant,
        &link_id,
        &None,
        &Symbol::new(&env, "USDC"),
        &String::from_str(&env, "Open"),
        &None,
        &None,
        &false,
        &None,
        &MaybeFiatConfig::None,
        &None,
    );

    client.use_link(&payer, &link_id, &1500i128, &None);
    let link = client.get_link(&link_id);
    assert_eq!(link.use_count, 1);
}

#[test]
fn test_deactivate_link() {
    let env = Env::default();
    env.mock_all_auths();
    let (merchant, client) = setup_payment_link(&env);

    let link_id = String::from_str(&env, "deactivate_me");
    client.create_link(
        &merchant,
        &link_id,
        &None,
        &Symbol::new(&env, "USDC"),
        &String::from_str(&env, "Bye"),
        &None,
        &None,
        &false,
        &None,
        &MaybeFiatConfig::None,
        &None,
    );

    client.deactivate_link(&merchant, &link_id);
    let link = client.get_link(&link_id);
    assert!(!link.active);
}

/// Issue #634: create 5 links, deactivate 1 → `active_only = true` returns 4;
/// the full list returns all 5, and pagination slices the index.
#[test]
fn test_get_merchant_links_active_only_and_pagination() {
    let env = Env::default();
    env.mock_all_auths();
    let (merchant, client) = setup_payment_link(&env);

    let mut ids = vec![&env];
    for i in 0..5u32 {
        let link_id = crate::format_id(&env, "mlink_", i as u64);
        client.create_link(
            &merchant,
            &link_id,
            &None,
            &Symbol::new(&env, "USDC"),
            &String::from_str(&env, "L"),
            &None,
            &None,
            &false,
            &None,
            &MaybeFiatConfig::None,
            &None,
        );
        ids.push_back(link_id);
    }

    // Deactivate the 3rd link.
    client.deactivate_link(&merchant, &ids.get(2).unwrap());

    // Full list: all 5, in creation order.
    let all = client.get_merchant_links(&merchant, &0u32, &50u32, &false);
    assert_eq!(all.len(), 5);
    assert_eq!(all.get(0).unwrap().link_id, ids.get(0).unwrap());

    // active_only = true → 4.
    let active = client.get_merchant_links(&merchant, &0u32, &50u32, &true);
    assert_eq!(active.len(), 4);
    for link in active.iter() {
        assert!(link.active);
    }

    // Pagination over the active set: offset 2, limit 1 → the 4th active link.
    let page = client.get_merchant_links(&merchant, &2u32, &1u32, &true);
    assert_eq!(page.len(), 1);
    assert_eq!(page.get(0).unwrap().link_id, ids.get(3).unwrap());

    // Unknown merchant → empty.
    let other = Address::generate(&env);
    assert_eq!(
        client
            .get_merchant_links(&other, &0u32, &10u32, &false)
            .len(),
        0
    );
}

#[test]
#[should_panic(expected = "Error(Contract, #3)")]
fn test_link_expired() {
    let env = Env::default();
    env.mock_all_auths();
    let (merchant, client) = setup_payment_link(&env);
    let payer = Address::generate(&env);

    let link_id = String::from_str(&env, "expired_link");
    let expiry = 1000u64;
    client.create_link(
        &merchant,
        &link_id,
        &None,
        &Symbol::new(&env, "USDC"),
        &String::from_str(&env, "Old"),
        &Some(expiry),
        &None,
        &false,
        &None,
        &MaybeFiatConfig::None,
        &None,
    );

    env.ledger().set_timestamp(expiry + 1);
    client.use_link(&payer, &link_id, &100i128, &None);
}

#[test]
fn test_verify_batch_returns_status_for_active_links() {
    let env = Env::default();
    env.mock_all_auths();
    let (merchant, client) = setup_payment_link(&env);

    let link_id1 = String::from_str(&env, "batch_link_1");
    let link_id2 = String::from_str(&env, "batch_link_2");

    client.create_link(
        &merchant,
        &link_id1,
        &Some(500i128),
        &Symbol::new(&env, "USDC"),
        &String::from_str(&env, "Batch 1"),
        &None,
        &None,
        &false,
        &None,
        &MaybeFiatConfig::None,
        &None,
    );
    client.create_link(
        &merchant,
        &link_id2,
        &Some(1000i128),
        &Symbol::new(&env, "USDC"),
        &String::from_str(&env, "Batch 2"),
        &None,
        &None,
        &false,
        &None,
        &MaybeFiatConfig::None,
        &None,
    );

    let results = client.verify_batch(&vec![&env, link_id1.clone(), link_id2.clone()]);
    assert_eq!(results.len(), 2);
    assert_eq!(results.get(0).unwrap(), (link_id1.clone(), true, 0, None));
    assert_eq!(results.get(1).unwrap(), (link_id2.clone(), true, 0, None));
}

#[test]
fn test_verify_batch_handles_missing_links() {
    let env = Env::default();
    env.mock_all_auths();
    let (merchant, client) = setup_payment_link(&env);

    let existing_link = String::from_str(&env, "existing_batch_link");
    let missing_link = String::from_str(&env, "missing_batch_link");

    client.create_link(
        &merchant,
        &existing_link,
        &Some(1000i128),
        &Symbol::new(&env, "USDC"),
        &String::from_str(&env, "Existing"),
        &None,
        &None,
        &false,
        &None,
        &MaybeFiatConfig::None,
        &None,
    );

    let results = client.verify_batch(&vec![&env, existing_link.clone(), missing_link.clone()]);
    assert_eq!(results.len(), 2);
    assert_eq!(
        results.get(0).unwrap(),
        (existing_link.clone(), true, 0, None)
    );
    assert_eq!(
        results.get(1).unwrap(),
        (missing_link.clone(), false, 0, None)
    );
}

#[test]
fn test_verify_batch_returns_inactive_for_deactivated_link() {
    let env = Env::default();
    env.mock_all_auths();
    let (merchant, client) = setup_payment_link(&env);

    let link_id = String::from_str(&env, "deactivated_batch_link");
    client.create_link(
        &merchant,
        &link_id,
        &Some(1000i128),
        &Symbol::new(&env, "USDC"),
        &String::from_str(&env, "Deactivated"),
        &None,
        &Some(10),
        &false,
        &None,
        &MaybeFiatConfig::None,
        &None,
    );

    client.deactivate_link(&merchant, &link_id);

    let results = client.verify_batch(&vec![&env, link_id.clone()]);
    assert_eq!(results.len(), 1);
    assert_eq!(
        results.get(0).unwrap(),
        (link_id.clone(), false, 0, Some(10))
    );
}

#[test]
fn test_verify_batch_empty_input_returns_empty_vec() {
    let env = Env::default();
    env.mock_all_auths();
    let (_merchant, client) = setup_payment_link(&env);

    let results = client.verify_batch(&soroban_sdk::vec![&env]);
    assert!(results.is_empty());
}

#[test]
fn test_max_uses() {
    let env = Env::default();
    env.mock_all_auths();
    let (merchant, client) = setup_payment_link(&env);
    let payer = Address::generate(&env);

    let link_id = String::from_str(&env, "limited_link");
    client.create_link(
        &merchant,
        &link_id,
        &None,
        &Symbol::new(&env, "USDC"),
        &String::from_str(&env, "Limit"),
        &None,
        &Some(1),
        &false,
        &None,
        &MaybeFiatConfig::None,
        &None,
    );

    client.use_link(&payer, &link_id, &100i128, &None);
    let link = client.get_link(&link_id);
    assert_eq!(link.use_count, 1);

    // Should fail on second use with LinkMaxUsesReached
    let result = client.try_use_link(&payer, &link_id, &100i128, &None);
    assert_eq!(result, Err(Ok(crate::Error::LinkMaxUsesReached)));
}

#[test]
fn test_max_uses_exact_accepted_and_emits_event() {
    let env = Env::default();
    env.mock_all_auths();
    let (merchant, client) = setup_payment_link(&env);
    let payer = Address::generate(&env);

    let link_id = String::from_str(&env, "exact_max_link");
    client.create_link(
        &merchant,
        &link_id,
        &Some(50i128),
        &Symbol::new(&env, "USDC"),
        &String::from_str(&env, "Exact"),
        &None,
        &Some(2),
        &false,
        &None,
        &MaybeFiatConfig::None,
        &None,
    );

    client.use_link(&payer, &link_id, &50i128, &None);
    assert_eq!(client.get_link(&link_id).use_count, 1);

    client.use_link(&payer, &link_id, &50i128, &None);
    assert_eq!(client.get_link(&link_id).use_count, 2);

    let emitted = env.events().all().iter().any(|(_, topics, _)| {
        if topics.len() < 2 {
            return false;
        }
        let t0: Result<Symbol, _> = topics.get(0).unwrap().try_into_val(&env);
        let t1: Result<Symbol, _> = topics.get(1).unwrap().try_into_val(&env);
        matches!(
            (t0, t1),
            (Ok(a), Ok(b))
                if a == Symbol::new(&env, "LINK") && b == Symbol::new(&env, "MAX_USES_REACHED")
        )
    });
    assert!(
        emitted,
        "LINK/MAX_USES_REACHED must fire when final use is consumed"
    );

    let rejected = client.try_use_link(&payer, &link_id, &50i128, &None);
    assert_eq!(rejected, Err(Ok(crate::Error::LinkMaxUsesReached)));
}

#[test]
fn test_unlimited_link_never_blocked_by_max_uses() {
    let env = Env::default();
    env.mock_all_auths();
    let (merchant, client) = setup_payment_link(&env);
    let payer = Address::generate(&env);

    let link_id = String::from_str(&env, "unlimited_link");
    client.create_link(
        &merchant,
        &link_id,
        &Some(25i128),
        &Symbol::new(&env, "USDC"),
        &String::from_str(&env, "Open"),
        &None,
        &None, // unlimited
        &false,
        &None,
        &MaybeFiatConfig::None,
        &None,
    );

    for _ in 0..5 {
        client.use_link(&payer, &link_id, &25i128, &None);
    }
    assert_eq!(client.get_link(&link_id).use_count, 5);
}

// -- Issue #111: Direct-to-Merchant Payment Flow ------------------------------

#[test]
fn test_direct_transfer_link_transfers_to_merchant() {
    let env = Env::default();
    env.mock_all_auths();

    let token_admin = Address::generate(&env);
    let usdc_token = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();
    let token_admin_client = token::StellarAssetClient::new(&env, &usdc_token);

    let (merchant, client) = setup_payment_link(&env);
    let payer = Address::generate(&env);

    // Fund payer
    token_admin_client.mint(&payer, &5000i128);

    let link_id = String::from_str(&env, "direct_link");
    let amount = 1000i128;
    client.create_link(
        &merchant,
        &link_id,
        &Some(amount),
        &Symbol::new(&env, "USDC"),
        &String::from_str(&env, "Direct"),
        &None,
        &None,
        &true,
        &None,
        &MaybeFiatConfig::None,
        &None,
    );

    let link = client.get_link(&link_id);
    assert!(link.direct_transfer);

    let token_client = token::TokenClient::new(&env, &usdc_token);
    let merchant_balance_before = token_client.balance(&merchant);

    client.use_link(&payer, &link_id, &amount, &Some(usdc_token.clone()));

    let merchant_balance_after = token_client.balance(&merchant);
    assert_eq!(merchant_balance_after - merchant_balance_before, amount);
}

#[test]
#[should_panic(expected = "Error(Contract, #1)")]
fn test_direct_transfer_without_token_address_fails() {
    let env = Env::default();
    env.mock_all_auths();

    let (merchant, client) = setup_payment_link(&env);
    let payer = Address::generate(&env);

    let link_id = String::from_str(&env, "direct_no_token");
    client.create_link(
        &merchant,
        &link_id,
        &Some(500i128),
        &Symbol::new(&env, "USDC"),
        &String::from_str(&env, "Direct no token"),
        &None,
        &None,
        &true,
        &None,
        &MaybeFiatConfig::None,
        &None,
    );

    // Should fail because usdc_token is None but direct_transfer is true
    client.use_link(&payer, &link_id, &500i128, &None);
}

// -- Issue #317: Payment Link Metadata Validation ----------------------------

#[test]
#[should_panic(expected = "Error(Contract, #49)")]
fn test_metadata_too_large_21_keys() {
    let env = Env::default();
    env.mock_all_auths();
    let (merchant, client) = setup_payment_link(&env);

    let link_id = String::from_str(&env, "meta_large");
    let keys_21 = [
        "k0", "k1", "k2", "k3", "k4", "k5", "k6", "k7", "k8", "k9", "k10", "k11", "k12", "k13",
        "k14", "k15", "k16", "k17", "k18", "k19", "k20",
    ];
    let mut metadata = Map::new(&env);
    for k in keys_21.iter() {
        metadata.set(String::from_str(&env, k), String::from_str(&env, "v"));
    }

    client.create_link(
        &merchant,
        &link_id,
        &None,
        &Symbol::new(&env, "USDC"),
        &String::from_str(&env, "Meta Test"),
        &None,
        &None,
        &false,
        &Some(metadata),
        &MaybeFiatConfig::None,
        &None,
    );
}

#[test]
#[should_panic(expected = "Error(Contract, #47)")]
fn test_metadata_value_too_long_257_chars() {
    let env = Env::default();
    env.mock_all_auths();
    let (merchant, client) = setup_payment_link(&env);

    let link_id = String::from_str(&env, "meta_long");
    let mut metadata = Map::new(&env);
    let long_value = String::from_str(&env, "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx");
    metadata.set(String::from_str(&env, "key"), long_value);

    client.create_link(
        &merchant,
        &link_id,
        &None,
        &Symbol::new(&env, "USDC"),
        &String::from_str(&env, "Meta Test"),
        &None,
        &None,
        &false,
        &Some(metadata),
        &MaybeFiatConfig::None,
        &None,
    );
}

#[test]
fn test_metadata_20_keys_256_char_values_succeeds() {
    let env = Env::default();
    env.mock_all_auths();
    let (merchant, client) = setup_payment_link(&env);

    let link_id = String::from_str(&env, "meta_valid");
    let mut metadata = Map::new(&env);
    let keys_20 = [
        "k0", "k1", "k2", "k3", "k4", "k5", "k6", "k7", "k8", "k9", "k10", "k11", "k12", "k13",
        "k14", "k15", "k16", "k17", "k18", "k19",
    ];
    let val256 = "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx";
    for k in keys_20.iter() {
        metadata.set(String::from_str(&env, k), String::from_str(&env, val256));
    }

    let id = client.create_link(
        &merchant,
        &link_id,
        &None,
        &Symbol::new(&env, "USDC"),
        &String::from_str(&env, "Meta Test"),
        &None,
        &None,
        &false,
        &Some(metadata),
        &MaybeFiatConfig::None,
        &None,
    );

    assert_eq!(id, link_id);
    let link = client.get_link(&link_id);
    assert!(link.metadata.is_some());
}

#[test]
fn test_metadata_none_succeeds() {
    let env = Env::default();
    env.mock_all_auths();
    let (merchant, client) = setup_payment_link(&env);

    let link_id = String::from_str(&env, "meta_none");
    let id = client.create_link(
        &merchant,
        &link_id,
        &None,
        &Symbol::new(&env, "USDC"),
        &String::from_str(&env, "Meta Test"),
        &None,
        &None,
        &false,
        &None,
        &MaybeFiatConfig::None,
        &None,
    );

    assert_eq!(id, link_id);
    let link = client.get_link(&link_id);
    assert!(link.metadata.is_none());
}

#[test]
fn test_create_link_with_metadata_stores_and_returns_correct_values() {
    let env = Env::default();
    env.mock_all_auths();
    let (merchant, client) = setup_payment_link(&env);

    let link_id = String::from_str(&env, "meta_store_test");
    let mut metadata = Map::new(&env);
    metadata.set(
        String::from_str(&env, "order_id"),
        String::from_str(&env, "ORD-2026-001"),
    );
    metadata.set(
        String::from_str(&env, "campaign"),
        String::from_str(&env, "summer_sale"),
    );

    let id = client.create_link(
        &merchant,
        &link_id,
        &Some(2000i128),
        &Symbol::new(&env, "USDC"),
        &String::from_str(&env, "Metadata Store Test"),
        &None,
        &None,
        &false,
        &Some(metadata),
        &MaybeFiatConfig::None,
        &None,
    );

    assert_eq!(id, link_id);
    let link = client.get_link(&link_id);
    assert!(link.metadata.is_some());
    let stored = link.metadata.unwrap();
    assert_eq!(
        stored.get(String::from_str(&env, "order_id")),
        Some(String::from_str(&env, "ORD-2026-001"))
    );
    assert_eq!(
        stored.get(String::from_str(&env, "campaign")),
        Some(String::from_str(&env, "summer_sale"))
    );
}

// -- Issue #413: Multi-Currency Invoicing (Fiat) ----------------------------

#[test]
fn test_create_fiat_link_and_use_with_rate() {
    let env = Env::default();
    env.mock_all_auths();

    // Deploy FX oracle
    let oracle_id = env.register(FXOracle, ());
    let oracle_client = FXOracleClient::new(&env, &oracle_id);
    let oracle_admin = Address::generate(&env);
    oracle_client.oracle_initialize(&oracle_admin, &86400);
    let oracle = Address::generate(&env);
    oracle_client.oracle_grant_role(&oracle_admin, &Symbol::new(&env, "ORACLE"), &oracle);

    // Set rate: 1.0 USD per USDC (rate = 1_0000000, 7 decimals)
    oracle_client.set_rate(&oracle, &Symbol::new(&env, "USD"), &1_0000000i128, &7);

    // Deploy payment link manager
    let (merchant, client) = setup_payment_link(&env);

    let link_id = String::from_str(&env, "fiat_link");
    let fiat = FiatConfig {
        amount: 100i128,
        currency: Symbol::new(&env, "USD"),
        oracle: oracle_id.clone(),
    };

    let id = client.create_link(
        &merchant,
        &link_id,
        &None, // amount: open (allow any USDC)
        &Symbol::new(&env, "USDC"),
        &String::from_str(&env, "Fiat Invoice"),
        &None,
        &None,
        &false,
        &None,
        &MaybeFiatConfig::Some(fiat),
        &None,
    );

    assert_eq!(id, link_id);
    let link = client.get_link(&link_id);
    let stored_fiat = link.fiat.into_option().unwrap();
    assert_eq!(stored_fiat.amount, 100);
    assert_eq!(stored_fiat.currency, Symbol::new(&env, "USD"));
    assert_eq!(stored_fiat.oracle, oracle_id);
}

#[test]
fn test_use_fiat_link_requires_correct_usdc() {
    let env = Env::default();
    env.mock_all_auths();

    let oracle_id = env.register(FXOracle, ());
    let oracle_client = FXOracleClient::new(&env, &oracle_id);
    let oracle_admin = Address::generate(&env);
    oracle_client.oracle_initialize(&oracle_admin, &86400);
    let oracle = Address::generate(&env);
    oracle_client.oracle_grant_role(&oracle_admin, &Symbol::new(&env, "ORACLE"), &oracle);

    // Rate: 1 USD = 2 USDC
    oracle_client.set_rate(&oracle, &Symbol::new(&env, "USD"), &2_0000000i128, &7);

    let (merchant, client) = setup_payment_link(&env);
    let payer = Address::generate(&env);

    let link_id = String::from_str(&env, "fiat_use");
    let fiat = FiatConfig {
        amount: 50i128, // $50 ? should require 25 USDC (50/2)
        currency: Symbol::new(&env, "USD"),
        oracle: oracle_id,
    };

    client.create_link(
        &merchant,
        &link_id,
        &None, // amount: open
        &Symbol::new(&env, "USDC"),
        &String::from_str(&env, "Fiat Use"),
        &None,
        &None,
        &false,
        &None,
        &MaybeFiatConfig::Some(fiat),
        &None,
    );

    // Should succeed with correct USDC equivalent (50 * 10^7 / 2_0000000 = 25)
    let payment_id = client.use_link(&payer, &link_id, &25i128, &None);
    assert!(!payment_id.is_empty());
    let link = client.get_link(&link_id);
    assert_eq!(link.use_count, 1);
}

#[test]
#[should_panic(expected = "Error(Contract, #406)")]
fn test_use_fiat_link_rejects_wrong_usdc() {
    let env = Env::default();
    env.mock_all_auths();

    let oracle_id = env.register(FXOracle, ());
    let oracle_client = FXOracleClient::new(&env, &oracle_id);
    let oracle_admin = Address::generate(&env);
    oracle_client.oracle_initialize(&oracle_admin, &86400);
    let oracle = Address::generate(&env);
    oracle_client.oracle_grant_role(&oracle_admin, &Symbol::new(&env, "ORACLE"), &oracle);

    oracle_client.set_rate(&oracle, &Symbol::new(&env, "USD"), &1_0000000i128, &7);

    let (merchant, client) = setup_payment_link(&env);
    let payer = Address::generate(&env);

    let link_id = String::from_str(&env, "fiat_wrong");
    let fiat = FiatConfig {
        amount: 100i128, // $100 ? should require 100 USDC (rate 1.0)
        currency: Symbol::new(&env, "USD"),
        oracle: oracle_id,
    };

    client.create_link(
        &merchant,
        &link_id,
        &None,
        &Symbol::new(&env, "USDC"),
        &String::from_str(&env, "Fiat Wrong"),
        &None,
        &None,
        &false,
        &None,
        &MaybeFiatConfig::Some(fiat),
        &None,
    );

    // 50 USDC is wrong when fiat_amount=100 at rate=1
    client.use_link(&payer, &link_id, &50i128, &None);
}

// ── Payment Link Analytics (view_count, total_revenue, conversion_rate) ─────

#[test]
fn test_record_link_view_increments_view_count() {
    let env = Env::default();
    env.mock_all_auths();
    let (_merchant, client) = setup_payment_link(&env);

    let link_id = String::from_str(&env, "view_link");
    client.create_link(
        &_merchant,
        &link_id,
        &None,
        &Symbol::new(&env, "USDC"),
        &String::from_str(&env, "View Test"),
        &None,
        &None,
        &false,
        &None,
        &MaybeFiatConfig::None,
        &None,
    );

    // Initially zero views
    let link = client.get_link(&link_id);
    assert_eq!(link.view_count, 0);

    // Record 3 views
    client.record_link_view(&link_id);
    client.record_link_view(&link_id);
    client.record_link_view(&link_id);

    let link = client.get_link(&link_id);
    assert_eq!(link.view_count, 3);
}

#[test]
fn test_use_link_accumulates_revenue() {
    let env = Env::default();
    env.mock_all_auths();
    let (merchant, client) = setup_payment_link(&env);
    let payer = Address::generate(&env);

    let link_id = String::from_str(&env, "revenue_link");
    let amount = 1000i128;
    client.create_link(
        &merchant,
        &link_id,
        &Some(amount),
        &Symbol::new(&env, "USDC"),
        &String::from_str(&env, "Revenue"),
        &None,
        &None,
        &false,
        &None,
        &MaybeFiatConfig::None,
        &None,
    );

    // Initially zero revenue
    let link = client.get_link(&link_id);
    assert_eq!(link.total_revenue, 0);

    // First use
    client.use_link(&payer, &link_id, &amount, &None);
    let link = client.get_link(&link_id);
    assert_eq!(link.total_revenue, amount);

    // Second use (different payer)
    let payer2 = Address::generate(&env);
    client.use_link(&payer2, &link_id, &amount, &None);
    let link = client.get_link(&link_id);
    assert_eq!(link.total_revenue, amount * 2);
}

#[test]
fn test_get_link_analytics_conversion_rate() {
    let env = Env::default();
    env.mock_all_auths();
    let (merchant, client) = setup_payment_link(&env);
    let payer = Address::generate(&env);

    let link_id = String::from_str(&env, "analytics_link");
    let amount = 1000i128;
    client.create_link(
        &merchant,
        &link_id,
        &Some(amount),
        &Symbol::new(&env, "USDC"),
        &String::from_str(&env, "Analytics"),
        &None,
        &None,
        &false,
        &None,
        &MaybeFiatConfig::None,
        &None,
    );

    // Record 10 views
    for _ in 0..10 {
        client.record_link_view(&link_id);
    }

    // Use the link once (1 conversion out of 10 views = 10%)
    client.use_link(&payer, &link_id, &amount, &None);

    let analytics = client.get_link_analytics(&link_id);
    assert_eq!(analytics.merchant_id, merchant);
    assert_eq!(analytics.view_count, 10);
    assert_eq!(analytics.use_count, 1);
    assert_eq!(analytics.total_revenue, amount);
    // 1/10 = 10% = 1000 bps
    assert_eq!(analytics.conversion_rate, 1000);
}

#[test]
fn test_get_link_analytics_zero_views() {
    let env = Env::default();
    env.mock_all_auths();
    let (merchant, client) = setup_payment_link(&env);
    let payer = Address::generate(&env);

    let link_id = String::from_str(&env, "zero_views_link");
    let amount = 500i128;
    client.create_link(
        &merchant,
        &link_id,
        &Some(amount),
        &Symbol::new(&env, "USDC"),
        &String::from_str(&env, "Zero Views"),
        &None,
        &None,
        &false,
        &None,
        &MaybeFiatConfig::None,
        &None,
    );

    // Use the link without any views
    client.use_link(&payer, &link_id, &amount, &None);

    let analytics = client.get_link_analytics(&link_id);
    assert_eq!(analytics.view_count, 0);
    assert_eq!(analytics.use_count, 1);
    assert_eq!(analytics.total_revenue, amount);
    // No views → conversion rate is 0 (avoid division by zero)
    assert_eq!(analytics.conversion_rate, 0);
}

#[test]
fn test_get_link_analytics_full_conversion() {
    let env = Env::default();
    env.mock_all_auths();
    let (merchant, client) = setup_payment_link(&env);
    let payer = Address::generate(&env);

    let link_id = String::from_str(&env, "full_conv_link");
    let amount = 1000i128;
    client.create_link(
        &merchant,
        &link_id,
        &Some(amount),
        &Symbol::new(&env, "USDC"),
        &String::from_str(&env, "Full Conversion"),
        &None,
        &None,
        &false,
        &None,
        &MaybeFiatConfig::None,
        &None,
    );

    // 4 views, 4 uses → 100% conversion = 10000 bps
    for _ in 0..4 {
        client.record_link_view(&link_id);
        let p = Address::generate(&env);
        client.use_link(&p, &link_id, &amount, &None);
    }

    let analytics = client.get_link_analytics(&link_id);
    assert_eq!(analytics.view_count, 4);
    assert_eq!(analytics.use_count, 4);
    assert_eq!(analytics.total_revenue, amount * 4);
    assert_eq!(analytics.conversion_rate, 10000);
}

#[test]
#[should_panic(expected = "Error(Contract, #1)")]
fn test_record_link_view_rejects_inactive_link() {
    let env = Env::default();
    env.mock_all_auths();
    let (merchant, client) = setup_payment_link(&env);

    let link_id = String::from_str(&env, "inactive_view_link");
    client.create_link(
        &merchant,
        &link_id,
        &None,
        &Symbol::new(&env, "USDC"),
        &String::from_str(&env, "Inactive View"),
        &None,
        &None,
        &false,
        &None,
        &MaybeFiatConfig::None,
        &None,
    );

    client.deactivate_link(&merchant, &link_id);
    // Should fail because the link is no longer active
    client.record_link_view(&link_id);
}

#[test]
#[should_panic(expected = "Error(Contract, #404)")]
fn test_get_link_analytics_not_found() {
    let env = Env::default();
    env.mock_all_auths();
    let (_merchant, client) = setup_payment_link(&env);

    let nonexistent = String::from_str(&env, "does_not_exist");
    client.get_link_analytics(&nonexistent);
}

#[test]
fn test_create_link_with_base_url_sets_shareable_url() {
    let env = Env::default();
    env.mock_all_auths();
    let (merchant, client) = setup_payment_link(&env);

    let link_id = String::from_str(&env, "share_link");
    let base = String::from_str(&env, "https://pay.fluxapay.app");
    client.create_link(
        &merchant,
        &link_id,
        &Some(1000i128),
        &Symbol::new(&env, "USDC"),
        &String::from_str(&env, "Shareable"),
        &None,
        &None,
        &None,
        &None,
        &false,
        &None,
        &MaybeFiatConfig::None,
        &Some(base),
    );

    let expected = String::from_str(&env, "https://pay.fluxapay.app/pay/share_link");
    let link = client.get_link(&link_id);
    assert_eq!(link.shareable_url, Some(expected.clone()));
    assert_eq!(client.get_link_url(&link_id), Some(expected));
}

#[test]
fn test_create_link_without_base_url_returns_none() {
    let env = Env::default();
    env.mock_all_auths();
    let (merchant, client) = setup_payment_link(&env);

    let link_id = String::from_str(&env, "no_base_link");
    client.create_link(
        &merchant,
        &link_id,
        &Some(1000i128),
        &Symbol::new(&env, "USDC"),
        &String::from_str(&env, "No Base"),
        &None,
        &String::from_str(&env, "Valid link"),
        &None,
        &None,
        &false,
        &None,
        &MaybeFiatConfig::None,
        &None,
    );

    let link = client.get_link(&link_id);
    assert!(link.shareable_url.is_none());
    assert!(client.get_link_url(&link_id).is_none());
}

#[test]
fn test_use_link_non_expired_accepts() {
    let env = Env::default();
    env.mock_all_auths();
    let (merchant, client) = setup_payment_link(&env);
    let payer = Address::generate(&env);

    let link_id = String::from_str(&env, "valid_link");
    let now = env.ledger().timestamp();
    client.create_link(
        &merchant,
        &link_id,
        &Some(1000i128),
        &Symbol::new(&env, "USDC"),
        &String::from_str(&env, "No Base"),
        &None,
        &String::from_str(&env, "Valid link"),
        &Some(now + 3600), // Expires in future
        &None,
        &false,
        &None,
        &MaybeFiatConfig::None,
        &None,
    );

    let link = client.get_link(&link_id);
    assert!(link.shareable_url.is_none());
    assert!(client.get_link_url(&link_id).is_none());
}

#[test]
fn test_set_payment_base_url_used_as_default() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(PaymentLinkManager, ());
    let client = PaymentLinkManagerClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.initialize(&admin);

    let merchant = Address::generate(&env);
    let base = String::from_str(&env, "https://checkout.example.com");
    client.set_payment_base_url(&admin, &base);

    let link_id = String::from_str(&env, "default_base");
    client.create_link(
        &merchant,
        &link_id,
        &Some(500i128),
        &Symbol::new(&env, "USDC"),
        &String::from_str(&env, "Default Base"),
        &None,
        &None,
        &false,
        &None,
        &MaybeFiatConfig::None,
        &None,
    );

    let payer = Address::generate(&env);
    let result = client.try_use_link(&payer, &link_id, &1000, &None);
    assert!(result.is_ok());
}

#[test]
fn test_expire_link_deactivates() {
    let env = Env::default();
    env.mock_all_auths();
    let (merchant, client) = setup_payment_link(&env);

    let link_id = String::from_str(&env, "deactivate_me");
    let now = env.ledger().timestamp();
    client.create_link(
        &merchant,
        &link_id,
        &Some(1000i128),
        &Symbol::new(&env, "USDC"),
        &String::from_str(&env, "To deactivate"),
        &Some(now - 100), // Already expired
        &None,
        &false,
        &None,
        &MaybeFiatConfig::None,
        &None,
    );

    assert_eq!(
        client.get_link_url(&link_id),
        Some(String::from_str(
            &env,
            "https://checkout.example.com/pay/default_base"
        ))
    );
}

#[test]
#[should_panic(expected = "Error(Contract, #47)")]
fn test_create_link_metadata_key_too_long() {
    let env = Env::default();
    env.mock_all_auths();
    let (merchant, client) = setup_payment_link(&env);

    let long_key = String::from_str(
        &env,
        "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx",
    );
    let mut metadata: Map<String, String> = Map::new(&env);
    metadata.set(long_key, String::from_str(&env, "v"));

    client.create_link(
        &merchant,
        &String::from_str(&env, "meta_key_long"),
        &None,
        &Symbol::new(&env, "USDC"),
        &String::from_str(&env, "Meta Key"),
        &None,
        &None,
        &false,
        &Some(metadata),
        &MaybeFiatConfig::None,
        &None,
    );
}

#[test]
fn test_expire_link_deactivates_manual() {
    let env = Env::default();
    env.mock_all_auths();
    let (merchant, client) = setup_payment_link(&env);
    let link_id = String::from_str(&env, "manual_link");
    client.create_link(
        &merchant,
        &link_id,
        &Some(1000i128),
        &Symbol::new(&env, "USDC"),
        &String::from_str(&env, "Manual"),
        &None,
        &None,
        &false,
        &None,
        &MaybeFiatConfig::None,
        &None,
    );
    client.expire_link(&link_id);
    let link = client.get_link(&link_id);
    assert!(!link.active);
}

#[test]
fn test_expire_link_idempotent() {
    let env = Env::default();
    env.mock_all_auths();
    let (merchant, client) = setup_payment_link(&env);

    let link_id = String::from_str(&env, "idempotent_link");
    let now = env.ledger().timestamp();
    client.create_link(
        &merchant,
        &link_id,
        &Some(1000i128),
        &Symbol::new(&env, "USDC"),
        &String::from_str(&env, "Idempotent"),
        &Some(now - 100),
        &None,
        &false,
        &None,
        &MaybeFiatConfig::None,
        &None,
    );

    // Calling expire_link twice should succeed both times.
    assert!(client.try_expire_link(&link_id).is_ok());
    assert!(client.try_expire_link(&link_id).is_ok());

    let link = client.get_link(&link_id);
    assert!(!link.active);
}

#[test]
fn test_expire_link_non_expired_skips() {
    let env = Env::default();
    env.mock_all_auths();
    let (merchant, client) = setup_payment_link(&env);

    let link_id = String::from_str(&env, "still_valid");
    let now = env.ledger().timestamp();
    client.create_link(
        &merchant,
        &link_id,
        &Some(1000i128),
        &Symbol::new(&env, "USDC"),
        &String::from_str(&env, "Still valid"),
        &Some(now + 3600),
        &None,
        &false,
        &None,
        &MaybeFiatConfig::None,
        &None,
    );

    client.expire_link(&link_id);
    let link = client.get_link(&link_id);
    assert!(link.active); // Should still be active
}

#[test]
fn test_get_link_auto_deactivates_expired() {
    let env = Env::default();
    env.mock_all_auths();
    let (merchant, client) = setup_payment_link(&env);

    let link_id = String::from_str(&env, "auto_deactivate");
    let now = env.ledger().timestamp();
    client.create_link(
        &merchant,
        &link_id,
        &Some(1000i128),
        &Symbol::new(&env, "USDC"),
        &String::from_str(&env, "Auto deactivate"),
        &Some(now - 100),
        &None,
        &false,
        &None,
        &MaybeFiatConfig::None,
        &None,
    );

    // get_link should return active: false for expired links.
    let link = client.get_link(&link_id);
    assert!(!link.active);
}

#[test]
fn test_batch_expire_links_sweep() {
    let env = Env::default();
    env.mock_all_auths();
    let (merchant, client) = setup_payment_link(&env);
    let now = env.ledger().timestamp();

    let mut link_ids = soroban_sdk::vec![&env];
    for i in 0..3 {
        let lid = crate::utils::format_id(&env, "batch_", i as u64);
        client.create_link(
            &merchant,
            &lid,
            &Some(1000i128),
            &Symbol::new(&env, "USDC"),
            &String::from_str(&env, "Batch"),
            &Some(now - 100),
            &None,
            &false,
            &None,
            &MaybeFiatConfig::None,
            &None,
        );
        link_ids.push_back(lid);
    }

    let count = client.batch_expire_links(&link_ids);
    assert_eq!(count, 3);
}

#[test]
fn test_batch_expire_links_max_20() {
    let env = Env::default();
    env.mock_all_auths();
    let (merchant, client) = setup_payment_link(&env);

    let mut oversized = soroban_sdk::vec![&env];
    for i in 0..21 {
        oversized.push_back(crate::utils::format_id(&env, "ovr_", i as u64));
    }

    let result = client.try_batch_expire_links(&oversized);
    assert_eq!(result, Err(Ok(crate::Error::BatchTooLarge)));
}

#[test]
fn test_expire_link_missing_is_idempotent() {
    let env = Env::default();
    env.mock_all_auths();
    let (_merchant, client) = setup_payment_link(&env);

    let result = client.try_expire_link(&String::from_str(&env, "nonexistent"));
    assert!(result.is_ok());
}

#[test]
fn test_link_expired_event_emitted() {
    let env = Env::default();
    env.mock_all_auths();
    let (merchant, client) = setup_payment_link(&env);

    let link_id = String::from_str(&env, "event_link");
    let now = env.ledger().timestamp();
    client.create_link(
        &merchant,
        &link_id,
        &Some(1000i128),
        &Symbol::new(&env, "USDC"),
        &String::from_str(&env, "Event test"),
        &Some(now - 100),
        &None,
        &false,
        &None,
        &MaybeFiatConfig::None,
        &None,
    );

    client.expire_link(&link_id);

    let has_expired_event = env.events().all().iter().any(|e| {
        let topics = e.0.clone();
        topics.len() >= 2
            && topics
                .get(0)
                .and_then(|t| t.try_into_val::<Symbol>(&env).ok())
                == Some(Symbol::new(&env, "LINK"))
            && topics
                .get(1)
                .and_then(|t| t.try_into_val::<Symbol>(&env).ok())
                == Some(Symbol::new(&env, "EXPIRED"))
    });
    assert!(has_expired_event);
}

// -- Issue #663: Per-link / global fee_bps override --------------------------

#[test]
fn test_use_link_zero_fee_bps_collects_no_fee() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(PaymentLinkManager, ());
    let client = PaymentLinkManagerClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.initialize(&admin);

    let token_admin = Address::generate(&env);
    let usdc_token = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();
    let token_admin_client = token::StellarAssetClient::new(&env, &usdc_token);
    let token_client = token::TokenClient::new(&env, &usdc_token);

    let merchant = Address::generate(&env);
    let payer = Address::generate(&env);
    token_admin_client.mint(&payer, &5000i128);

    let link_id = String::from_str(&env, "zero_fee_link");
    let amount = 1000i128;
    client.create_link(
        &merchant,
        &link_id,
        &Some(amount),
        &Symbol::new(&env, "USDC"),
        &String::from_str(&env, "Zero fee"),
        &None,
        &None,
        &true,
        &None,
        &MaybeFiatConfig::None,
        &None,
    );

    // Explicitly set a 0 bps override on this link.
    client.set_payment_link_fee_bps(&admin, &Some(link_id.clone()), &Some(0i128));
    assert_eq!(client.get_effective_fee_bps(&link_id), Some(0i128));

    let merchant_balance_before = token_client.balance(&merchant);
    client.use_link(&payer, &link_id, &amount, &Some(usdc_token.clone()));
    let merchant_balance_after = token_client.balance(&merchant);

    // Merchant receives the full amount; no fee is deducted.
    assert_eq!(merchant_balance_after - merchant_balance_before, amount);
}

#[test]
fn test_use_link_falls_back_to_global_fee_bps_when_link_has_none() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(PaymentLinkManager, ());
    let client = PaymentLinkManagerClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.initialize(&admin);

    let token_admin = Address::generate(&env);
    let usdc_token = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();
    let token_admin_client = token::StellarAssetClient::new(&env, &usdc_token);
    let token_client = token::TokenClient::new(&env, &usdc_token);

    let merchant = Address::generate(&env);
    let payer = Address::generate(&env);
    token_admin_client.mint(&payer, &5000i128);

    // Admin sets a 5% (500 bps) global default fee for links.
    client.set_payment_link_fee_bps(&admin, &None, &Some(500i128));

    let link_id = String::from_str(&env, "global_fee_link");
    let amount = 1000i128;
    client.create_link(
        &merchant,
        &link_id,
        &Some(amount),
        &Symbol::new(&env, "USDC"),
        &String::from_str(&env, "Global fee fallback"),
        &None,
        &None,
        &true,
        &None,
        &MaybeFiatConfig::None,
        &None,
    );

    // Link itself has no fee_bps override -> global 500 bps applies.
    assert_eq!(client.get_effective_fee_bps(&link_id), Some(500i128));

    let merchant_balance_before = token_client.balance(&merchant);
    let admin_balance_before = token_client.balance(&admin);
    client.use_link(&payer, &link_id, &amount, &Some(usdc_token.clone()));
    let merchant_balance_after = token_client.balance(&merchant);
    let admin_balance_after = token_client.balance(&admin);

    let expected_fee = amount * 500 / 10_000;
    assert_eq!(
        merchant_balance_after - merchant_balance_before,
        amount - expected_fee
    );
    assert_eq!(admin_balance_after - admin_balance_before, expected_fee);
}

#[test]
#[should_panic(expected = "Error(Contract, #1)")]
fn test_set_payment_link_fee_bps_requires_admin() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(PaymentLinkManager, ());
    let client = PaymentLinkManagerClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.initialize(&admin);

    let not_admin = Address::generate(&env);

    // Non-admin caller must be rejected with Unauthorized (#1).
    client.set_payment_link_fee_bps(&not_admin, &None, &Some(100i128));
}

#[test]
fn test_link_analytics_revenue_and_conversion_rate() {
    let env = Env::default();
    env.mock_all_auths();
    let (merchant, client) = setup_payment_link(&env);
    let payer1 = Address::generate(&env);
    let payer2 = Address::generate(&env);

    let link_id = String::from_str(&env, "revenue_link");
    client.create_link(
        &merchant,
        &link_id,
        &None,
        &Symbol::new(&env, "USDC"),
        &String::from_str(&env, "Revenue Test"),
        &None,
        &None,
        &false,
        &None,
        &MaybeFiatConfig::None,
        &None,
    );

    // Record 3 views
    for _ in 0..3 {
        client.record_link_view(&link_id);
    }

    // Record 2 uses at 100 and 200 USDC
    client.use_link(&payer1, &link_id, &100i128, &None);
    client.use_link(&payer2, &link_id, &200i128, &None);

    let analytics = client.get_link_analytics(&link_id).unwrap();

    // Check totals
    assert_eq!(analytics.view_count, 3);
    assert_eq!(analytics.use_count, 2);
    assert_eq!(analytics.total_revenue, 300);

    // Check conversion rate: 2 * 10000 / 3 = 6666 (with integer truncation)
    assert_eq!(analytics.conversion_rate, 6666);

    // Check average payment: 300 / 2 = 150
    assert_eq!(analytics.average_payment, 150);

    // Check last_used_at is set
    assert!(analytics.last_used_at.is_some());
}

#[test]
fn test_link_analytics_zero_views() {
    let env = Env::default();
    env.mock_all_auths();
    let (merchant, client) = setup_payment_link(&env);
    let payer = Address::generate(&env);

    let link_id = String::from_str(&env, "zero_views_link");
    client.create_link(
        &merchant,
        &link_id,
        &None,
        &Symbol::new(&env, "USDC"),
        &String::from_str(&env, "Zero Views"),
        &None,
        &None,
        &false,
        &None,
        &MaybeFiatConfig::None,
        &None,
    );

    // Use the link without recording views
    client.use_link(&payer, &link_id, &100i128, &None);

    let analytics = client.get_link_analytics(&link_id).unwrap();

    // Conversion rate should be 0 when view_count is 0 (no division by zero)
    assert_eq!(analytics.view_count, 0);
    assert_eq!(analytics.use_count, 1);
    assert_eq!(analytics.total_revenue, 100);
    assert_eq!(analytics.conversion_rate, 0);
    assert_eq!(analytics.average_payment, 100);
}

#[test]
fn test_link_analytics_zero_uses() {
    let env = Env::default();
    env.mock_all_auths();
    let (merchant, client) = setup_payment_link(&env);

    let link_id = String::from_str(&env, "zero_uses_link");
    client.create_link(
        &merchant,
        &link_id,
        &None,
        &Symbol::new(&env, "USDC"),
        &String::from_str(&env, "Zero Uses"),
        &None,
        &None,
        &false,
        &None,
        &MaybeFiatConfig::None,
        &None,
    );

    // Record views but no uses
    for _ in 0..5 {
        client.record_link_view(&link_id);
    }

    let analytics = client.get_link_analytics(&link_id).unwrap();

    // All should be zero/empty when no uses
    assert_eq!(analytics.view_count, 5);
    assert_eq!(analytics.use_count, 0);
    assert_eq!(analytics.total_revenue, 0);
    assert_eq!(analytics.conversion_rate, 0);
    assert_eq!(analytics.average_payment, 0);
    assert!(analytics.last_used_at.is_none());
}

#[test]
fn test_link_analytics_last_used_at_tracking() {
    let env = Env::default();
    env.mock_all_auths();
    let (merchant, client) = setup_payment_link(&env);
    let payer = Address::generate(&env);

    let link_id = String::from_str(&env, "timestamp_link");
    client.create_link(
        &merchant,
        &link_id,
        &None,
        &Symbol::new(&env, "USDC"),
        &String::from_str(&env, "Timestamp Test"),
        &None,
        &None,
        &false,
        &None,
        &MaybeFiatConfig::None,
        &None,
    );

    // Initially no last_used_at
    let analytics_before = client.get_link_analytics(&link_id).unwrap();
    assert!(analytics_before.last_used_at.is_none());

    let initial_timestamp = env.ledger().timestamp();

    // Use the link
    client.use_link(&payer, &link_id, &100i128, &None);

    let analytics_after = client.get_link_analytics(&link_id).unwrap();

    // After using, last_used_at should be set to current or later timestamp
    assert!(analytics_after.last_used_at.is_some());
    assert!(analytics_after.last_used_at.unwrap() >= initial_timestamp);
}
