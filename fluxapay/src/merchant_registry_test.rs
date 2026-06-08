use super::merchant_registry::*;
use crate::{PaymentProcessor, PaymentProcessorClient, RefundManager, RefundManagerClient};
use soroban_sdk::{testutils::Address as _, testutils::Ledger, Address, Env, String, Symbol};

#[test]
fn test_merchant_registration() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|li| li.timestamp = 1000);

    let contract_id = env.register(MerchantRegistry, ());
    let client = MerchantRegistryClient::new(&env, &contract_id);

    let merchant_id = Address::generate(&env);
    let business_name = String::from_str(&env, "Test Merchant");
    let settlement_currency = String::from_str(&env, "USDC");

    let payout_addr = Address::generate(&env);
    client.register_merchant(
        &merchant_id,
        &business_name,
        &settlement_currency,
        &Some(payout_addr.clone()),
        &Some(String::from_str(&env, "BANK-001")),
        &None,
    );

    let merchant = client.get_merchant(&merchant_id);

    assert_eq!(merchant.merchant_id, merchant_id);
    assert_eq!(merchant.business_name, business_name);
    assert_eq!(merchant.settlement_currency, settlement_currency);
    assert_eq!(merchant.payout_address, Some(payout_addr));
    assert_eq!(
        merchant.bank_account,
        Some(String::from_str(&env, "BANK-001"))
    );
    // New: kyc_tier starts as Unverified
    assert_eq!(merchant.kyc_tier, KycTier::Unverified);
    assert!(merchant.active);
    assert!(merchant.created_at > 0);
}

#[test]
fn test_merchant_update() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(MerchantRegistry, ());
    let client = MerchantRegistryClient::new(&env, &contract_id);

    let merchant_id = Address::generate(&env);
    let business_name = String::from_str(&env, "Initial name");
    let settlement_currency = String::from_str(&env, "USD");

    client.register_merchant(
        &merchant_id,
        &business_name,
        &settlement_currency,
        &None,
        &None,
        &None,
    );

    let new_name = String::from_str(&env, "New name");
    let new_currency = String::from_str(&env, "EUR");
    let new_payout = Address::generate(&env);

    client.update_merchant(
        &merchant_id,
        &Some(new_name.clone()),
        &Some(new_currency.clone()),
        &Some(false),
        &Some(new_payout.clone()),
        &Some(String::from_str(&env, "BANK-002")),
        &None,
    );

    let updated_merchant = client.get_merchant(&merchant_id);

    assert_eq!(updated_merchant.business_name, new_name);
    assert_eq!(updated_merchant.settlement_currency, new_currency);
    assert!(!updated_merchant.active);
    assert_eq!(updated_merchant.payout_address, Some(new_payout));
    assert_eq!(
        updated_merchant.bank_account,
        Some(String::from_str(&env, "BANK-002"))
    );
}

#[test]
fn test_merchant_verification() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(MerchantRegistry, ());
    let client = MerchantRegistryClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let merchant_id = Address::generate(&env);

    client.initialize(&admin);

    client.register_merchant(
        &merchant_id,
        &String::from_str(&env, "Merchant"),
        &String::from_str(&env, "USDC"),
        &None,
        &None,
        &None,
    );

    // verify_merchant sets KycTier::Basic for backward compatibility
    client.verify_merchant(&admin, &merchant_id);

    let merchant = client.get_merchant(&merchant_id);
    assert_eq!(merchant.kyc_tier, KycTier::Basic);
}

#[test]
#[should_panic(expected = "HostError: Error(Contract, #3)")]
fn test_unauthorized_verification() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(MerchantRegistry, ());
    let client = MerchantRegistryClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let attacker = Address::generate(&env);
    let merchant_id = Address::generate(&env);

    client.initialize(&admin);

    client.register_merchant(
        &merchant_id,
        &String::from_str(&env, "Merchant"),
        &String::from_str(&env, "USDC"),
        &None,
        &None,
        &None,
    );

    // Attacker tries to verify the merchant
    client.verify_merchant(&attacker, &merchant_id);
}

#[test]
fn test_set_kyc_tier() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(MerchantRegistry, ());
    let client = MerchantRegistryClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let merchant_id = Address::generate(&env);

    client.initialize(&admin);
    client.register_merchant(
        &merchant_id,
        &String::from_str(&env, "BigCorp"),
        &String::from_str(&env, "USDC"),
        &None::<Address>,
        &None::<String>,
        &None,
    );

    // Promote through tiers
    client.set_kyc_tier(&admin, &merchant_id, &KycTier::Full);
    assert_eq!(client.get_merchant(&merchant_id).kyc_tier, KycTier::Full);

    client.set_kyc_tier(&admin, &merchant_id, &KycTier::Business);
    assert_eq!(
        client.get_merchant(&merchant_id).kyc_tier,
        KycTier::Business
    );
}

#[test]
#[should_panic(expected = "HostError: Error(Contract, #3)")]
fn test_set_kyc_tier_unauthorized() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(MerchantRegistry, ());
    let client = MerchantRegistryClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let attacker = Address::generate(&env);
    let merchant_id = Address::generate(&env);

    client.initialize(&admin);
    client.register_merchant(
        &merchant_id,
        &String::from_str(&env, "Merchant"),
        &String::from_str(&env, "USDC"),
        &None,
        &None,
        &None,
    );

    // Non-admin tries to set KYC tier
    client.set_kyc_tier(&attacker, &merchant_id, &KycTier::Business);
}

#[test]
fn test_merchant_enumeration() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(MerchantRegistry, ());
    let client = MerchantRegistryClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    client.initialize(&admin);

    // Register multiple merchants
    let merchant1 = Address::generate(&env);
    let merchant2 = Address::generate(&env);
    let merchant3 = Address::generate(&env);

    client.register_merchant(
        &merchant1,
        &String::from_str(&env, "Merchant 1"),
        &String::from_str(&env, "USDC"),
        &None,
        &None,
        &None,
    );
    client.register_merchant(
        &merchant2,
        &String::from_str(&env, "Merchant 2"),
        &String::from_str(&env, "USDC"),
        &None,
        &None,
        &None,
    );
    client.register_merchant(
        &merchant3,
        &String::from_str(&env, "Merchant 3"),
        &String::from_str(&env, "USDC"),
        &None,
        &None,
        &None,
    );

    // Get all merchants - should return all 3
    let all_merchants = client.get_all_merchants(&0, &10);
    assert_eq!(all_merchants.len(), 3);

    // Verify pagination works
    let first_two = client.get_all_merchants(&0, &2);
    assert_eq!(first_two.len(), 2);

    let third_only = client.get_all_merchants(&2, &10);
    assert_eq!(third_only.len(), 1);
}

#[test]
fn test_verified_merchants_filter() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(MerchantRegistry, ());
    let client = MerchantRegistryClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    client.initialize(&admin);

    // Register merchants
    let merchant1 = Address::generate(&env);
    let merchant2 = Address::generate(&env);
    let merchant3 = Address::generate(&env);

    client.register_merchant(
        &merchant1,
        &String::from_str(&env, "Merchant 1"),
        &String::from_str(&env, "USDC"),
        &None,
        &None,
        &None,
    );
    client.register_merchant(
        &merchant2,
        &String::from_str(&env, "Merchant 2"),
        &String::from_str(&env, "USDC"),
        &None,
        &None,
        &None,
    );
    client.register_merchant(
        &merchant3,
        &String::from_str(&env, "Merchant 3"),
        &String::from_str(&env, "USDC"),
        &None,
        &None,
        &None,
    );

    // Verify only merchant2
    client.verify_merchant(&admin, &merchant2);

    // Get verified merchants - should return only merchant2
    let verified = client.get_verified_merchants();
    assert_eq!(verified.len(), 1);
    assert_eq!(verified.get(0).unwrap().merchant_id, merchant2);
    assert_eq!(verified.get(0).unwrap().kyc_tier, KycTier::Basic);
}

#[test]
#[should_panic(expected = "HostError: Error(Contract, #1)")]
fn test_unverified_merchant_cannot_create_payment() {
    let env = Env::default();
    env.mock_all_auths();

    let payment_processor = env.register(PaymentProcessor, ());
    let refund_manager = env.register(RefundManager, ());
    let merchant_registry = env.register(MerchantRegistry, ());

    let payment_client = PaymentProcessorClient::new(&env, &payment_processor);
    let refund_client = RefundManagerClient::new(&env, &refund_manager);
    let merchant_client = MerchantRegistryClient::new(&env, &merchant_registry);

    let admin = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let usdc_token = env
        .register_stellar_asset_contract_v2(token_admin)
        .address();

    // Initialize contracts
    refund_client.initialize_refund_manager(&admin, &usdc_token);
    payment_client.initialize_payment_processor(&admin);
    merchant_client.initialize(&admin);

    // Register merchant but DON'T verify them
    let merchant = Address::generate(&env);
    merchant_client.register_merchant(
        &merchant,
        &String::from_str(&env, "Unverified Merchant"),
        &String::from_str(&env, "USDC"),
        &None,
        &None,
        &None,
    );

    // Try to create payment - should fail because merchant is not verified
    let payment_id = String::from_str(&env, "PAY_01");
    let amount = 1000i128;

    let args = crate::CreatePaymentArgs {
        payment_id,
        merchant_id: merchant.clone(),
        amount,
        currency: Symbol::new(&env, "USDC"),
        deposit_address: Address::generate(&env),
        expires_at: Some(env.ledger().timestamp() + 3600),
        duration_secs: None,
        memo: None,
        memo_type: None,
        token_address: None,
        client_token: None,
        metadata_hash: None,
    };

    // This should panic with Unauthorized error
    payment_client.create_payment(&args);
}

#[test]
fn test_verified_merchant_can_create_payment() {
    let env = Env::default();
    env.mock_all_auths();

    let payment_processor = env.register(PaymentProcessor, ());
    let refund_manager = env.register(RefundManager, ());
    let merchant_registry = env.register(MerchantRegistry, ());

    let payment_client = PaymentProcessorClient::new(&env, &payment_processor);
    let refund_client = RefundManagerClient::new(&env, &refund_manager);
    let merchant_client = MerchantRegistryClient::new(&env, &merchant_registry);

    let admin = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let usdc_token = env
        .register_stellar_asset_contract_v2(token_admin)
        .address();

    // Initialize contracts
    refund_client.initialize_refund_manager(&admin, &usdc_token);
    payment_client.initialize_payment_processor(&admin);
    merchant_client.initialize(&admin);

    // Register and verify merchant
    let merchant = Address::generate(&env);
    merchant_client.register_merchant(
        &merchant,
        &String::from_str(&env, "Verified Merchant"),
        &String::from_str(&env, "USDC"),
        &None,
        &None,
        &None,
    );

    // Manually grant MERCHANT role (simulating what would happen with set_refund_manager_address)
    payment_client.grant_role(&admin, &crate::role_merchant(&env), &merchant);

    // Now create payment should succeed
    let payment_id = String::from_str(&env, "PAY_01");
    let amount = 1000i128;

    let args = crate::CreatePaymentArgs {
        payment_id: payment_id.clone(),
        merchant_id: merchant.clone(),
        amount,
        currency: Symbol::new(&env, "USDC"),
        deposit_address: Address::generate(&env),
        expires_at: Some(env.ledger().timestamp() + 3600),
        duration_secs: None,
        memo: None,
        memo_type: None,
        token_address: None,
        client_token: None,
        metadata_hash: None,
    };

    let payment = payment_client.create_payment(&args);

    assert_eq!(payment.payment_id, payment_id);
    assert_eq!(payment.merchant_id, merchant);
    assert_eq!(payment.amount, amount);
}

#[test]
fn test_suspend_merchant() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(MerchantRegistry, ());
    let client = MerchantRegistryClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let merchant_id = Address::generate(&env);

    client.initialize(&admin);

    client.register_merchant(
        &merchant_id,
        &String::from_str(&env, "Merchant"),
        &String::from_str(&env, "USDC"),
        &None,
        &None,
        &None,
    );

    let reason = String::from_str(&env, "Fraudulent activity");
    client.suspend_merchant(&admin, &merchant_id, &reason, &0u64);

    let merchant = client.get_merchant(&merchant_id);
    assert!(!merchant.active);
    assert_eq!(merchant.suspension_reason, Some(reason));
    assert!(merchant.suspended_at.is_some());
}

#[test]
fn test_reinstate_merchant() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(MerchantRegistry, ());
    let client = MerchantRegistryClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let merchant_id = Address::generate(&env);

    client.initialize(&admin);

    client.register_merchant(
        &merchant_id,
        &String::from_str(&env, "Merchant"),
        &String::from_str(&env, "USDC"),
        &None,
        &None,
        &None,
    );

    let reason = String::from_str(&env, "Fraudulent activity");
    client.suspend_merchant(&admin, &merchant_id, &reason, &0u64);

    // Check it's suspended
    let suspended = client.get_merchant(&merchant_id);
    assert!(!suspended.active);

    client.reinstate_merchant(&admin, &merchant_id);

    let reinstated = client.get_merchant(&merchant_id);
    assert!(reinstated.active);
    assert_eq!(reinstated.suspension_reason, None);
    assert_eq!(reinstated.suspended_at, None);
}

#[test]
fn test_automatic_suspension_recovery() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(MerchantRegistry, ());
    let client = MerchantRegistryClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let merchant_id = Address::generate(&env);

    client.initialize(&admin);

    client.register_merchant(
        &merchant_id,
        &String::from_str(&env, "Merchant"),
        &String::from_str(&env, "USDC"),
        &None,
        &None,
        &None,
    );

    let reason = String::from_str(&env, "Fraudulent activity");
    // Suspend with 1 second expiration
    client.suspend_merchant(&admin, &merchant_id, &reason, &1);

    // Advance ledger time past expiration
    env.ledger().with_mut(|li| li.timestamp += 2);

    // Get merchant - should be automatically reinstated
    let merchant = client.get_merchant(&merchant_id);
    assert!(merchant.active);
    assert_eq!(merchant.suspension_reason, None);
    assert_eq!(merchant.suspended_at, None);
    assert_eq!(merchant.suspension_expires_at, None);
}

#[test]
#[should_panic(expected = "HostError: Error(Contract, #3)")]
fn test_payout_address_rotation_delay() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(MerchantRegistry, ());
    let client = MerchantRegistryClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let merchant_id = Address::generate(&env);

    client.initialize(&admin);

    client.register_merchant(
        &merchant_id,
        &String::from_str(&env, "Merchant"),
        &String::from_str(&env, "USDC"),
        &None,
        &None,
        &None,
    );

    // Set initial payout address
    let payout_addr1 = Address::generate(&env);
    client.update_merchant(
        &merchant_id,
        &None,
        &None,
        &None,
        &Some(payout_addr1.clone()),
        &None,
        &None,
    );

    // Try to update payout address again within 48 hours - should fail
    let payout_addr2 = Address::generate(&env);
    client.update_merchant(
        &merchant_id,
        &None,
        &None,
        &None,
        &Some(payout_addr2.clone()),
        &None,
        &None,
    );
}

#[test]
fn test_payout_address_rotation_delay_success_after_48_hours() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(MerchantRegistry, ());
    let client = MerchantRegistryClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let merchant_id = Address::generate(&env);

    client.initialize(&admin);

    client.register_merchant(
        &merchant_id,
        &String::from_str(&env, "Merchant"),
        &String::from_str(&env, "USDC"),
        &None,
        &None,
        &None,
    );

    // Set initial payout address
    let payout_addr1 = Address::generate(&env);
    client.update_merchant(
        &merchant_id,
        &None,
        &None,
        &None,
        &Some(payout_addr1.clone()),
        &None,
        &None,
    );

    // Advance ledger time by 48 hours + 1 second
    env.ledger().with_mut(|li| li.timestamp += 48 * 60 * 60 + 1);

    // Now update payout address should succeed
    let payout_addr2 = Address::generate(&env);
    client.update_merchant(
        &merchant_id,
        &None,
        &None,
        &None,
        &Some(payout_addr2.clone()),
        &None,
        &None,
    );

    let merchant = client.get_merchant(&merchant_id);
    assert_eq!(merchant.payout_address, Some(payout_addr2));
}

#[test]
fn test_payout_history_initially_empty_after_first_set() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(MerchantRegistry, ());
    let client = MerchantRegistryClient::new(&env, &contract_id);

    let merchant_id = Address::generate(&env);
    client.register_merchant(
        &merchant_id,
        &String::from_str(&env, "Merchant"),
        &String::from_str(&env, "USDC"),
        &None,
        &None,
        &None,
    );

    let events_before = env.events().all().len();
    let payout_addr = Address::generate(&env);
    client.update_merchant(
        &merchant_id,
        &None,
        &None,
        &None,
        &Some(payout_addr),
        &None,
        &None,
    );

    let history = client.get_payout_history(&merchant_id);
    assert_eq!(history.len(), 0);
    assert_eq!(env.events().all().len(), events_before + 1);
}

#[test]
fn test_payout_history_records_previous_addresses_in_order() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(MerchantRegistry, ());
    let client = MerchantRegistryClient::new(&env, &contract_id);

    let merchant_id = Address::generate(&env);
    client.register_merchant(
        &merchant_id,
        &String::from_str(&env, "Merchant"),
        &String::from_str(&env, "USDC"),
        &None,
        &None,
        &None,
    );

    let payout_addr1 = Address::generate(&env);
    let payout_addr2 = Address::generate(&env);
    let payout_addr3 = Address::generate(&env);

    client.update_merchant(
        &merchant_id,
        &None,
        &None,
        &None,
        &Some(payout_addr1.clone()),
        &None,
        &None,
    );

    env.ledger().with_mut(|li| li.timestamp += 48 * 60 * 60 + 1);
    let events_before_second_update = env.events().all().len();
    client.update_merchant(
        &merchant_id,
        &None,
        &None,
        &None,
        &Some(payout_addr2.clone()),
        &None,
        &None,
    );

    let history_after_second_update = client.get_payout_history(&merchant_id);
    assert_eq!(history_after_second_update.len(), 1);
    assert_eq!(history_after_second_update.get(0).unwrap(), payout_addr1.clone());
    assert_eq!(
        env.events().all().len(),
        events_before_second_update + 2
    );

    env.ledger().with_mut(|li| li.timestamp += 48 * 60 * 60 + 1);
    client.update_merchant(
        &merchant_id,
        &None,
        &None,
        &None,
        &Some(payout_addr3),
        &None,
        &None,
    );

    let history = client.get_payout_history(&merchant_id);
    assert_eq!(history.len(), 2);
    assert_eq!(history.get(0).unwrap(), payout_addr1);
    assert_eq!(history.get(1).unwrap(), payout_addr2);
}

#[test]
fn test_unchanged_payout_address_does_not_add_history() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(MerchantRegistry, ());
    let client = MerchantRegistryClient::new(&env, &contract_id);

    let merchant_id = Address::generate(&env);
    client.register_merchant(
        &merchant_id,
        &String::from_str(&env, "Merchant"),
        &String::from_str(&env, "USDC"),
        &None,
        &None,
        &None,
    );

    let payout_addr = Address::generate(&env);
    client.update_merchant(
        &merchant_id,
        &None,
        &None,
        &None,
        &Some(payout_addr.clone()),
        &None,
        &None,
    );

    let new_name = String::from_str(&env, "Merchant renamed");
    let events_before_same_address_update = env.events().all().len();
    client.update_merchant(
        &merchant_id,
        &Some(new_name.clone()),
        &None,
        &None,
        &Some(payout_addr.clone()),
        &None,
        &None,
    );

    let merchant = client.get_merchant(&merchant_id);
    assert_eq!(merchant.business_name, new_name);
    assert_eq!(merchant.payout_address, Some(payout_addr));
    assert_eq!(client.get_payout_history(&merchant_id).len(), 0);
    assert_eq!(
        env.events().all().len(),
        events_before_same_address_update + 1
    );
}

#[test]
#[should_panic(expected = "HostError: Error(Contract, #3)")]
fn test_suspend_merchant_unauthorized() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(MerchantRegistry, ());
    let client = MerchantRegistryClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let attacker = Address::generate(&env);
    let merchant_id = Address::generate(&env);

    client.initialize(&admin);

    client.register_merchant(
        &merchant_id,
        &String::from_str(&env, "Merchant"),
        &String::from_str(&env, "USDC"),
        &None,
        &None,
        &None,
    );

    client.suspend_merchant(&attacker, &merchant_id, &String::from_str(&env, "Reason"), &0u64);
}

// Tests for issue #208: Content-Addressable Merchant Profiles
#[test]
fn test_set_and_get_metadata_hash() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(MerchantRegistry, ());
    let client = MerchantRegistryClient::new(&env, &contract_id);

    let merchant_id = Address::generate(&env);
    client.register_merchant(
        &merchant_id,
        &String::from_str(&env, "Test Merchant"),
        &String::from_str(&env, "USDC"),
        &None,
        &None,
        &None,
    );

    // Set IPFS hash
    let ipfs_hash = String::from_str(&env, "QmXoypizjW3WknFiJnKLwHCnL72vedxjQkDDP1mXWo6uco");
    client.set_metadata_hash(&merchant_id, &ipfs_hash);

    // Get IPFS hash
    let retrieved_hash = client.get_metadata_hash(&merchant_id);
    assert_eq!(retrieved_hash, Some(ipfs_hash));
}

#[test]
fn test_metadata_hash_initially_none() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(MerchantRegistry, ());
    let client = MerchantRegistryClient::new(&env, &contract_id);

    let merchant_id = Address::generate(&env);
    client.register_merchant(
        &merchant_id,
        &String::from_str(&env, "Test Merchant"),
        &String::from_str(&env, "USDC"),
        &None,
        &None,
        &None,
    );

    let hash = client.get_metadata_hash(&merchant_id);
    assert_eq!(hash, None);
}

// Tests for issue #216: Multi-Currency Registry Mapping
#[test]
fn test_add_and_get_currency_payout() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(MerchantRegistry, ());
    let client = MerchantRegistryClient::new(&env, &contract_id);

    let merchant_id = Address::generate(&env);
    client.register_merchant(
        &merchant_id,
        &String::from_str(&env, "Test Merchant"),
        &String::from_str(&env, "USDC"),
        &None,
        &None,
        &None,
    );

    // Add payout addresses for different currencies
    let usdc_payout = Address::generate(&env);
    let eur_payout = Address::generate(&env);
    let gbp_payout = Address::generate(&env);

    client.add_currency_payout(&merchant_id, &String::from_str(&env, "USDC"), &usdc_payout);
    client.add_currency_payout(&merchant_id, &String::from_str(&env, "EUR"), &eur_payout);
    client.add_currency_payout(&merchant_id, &String::from_str(&env, "GBP"), &gbp_payout);

    // Verify each currency payout
    assert_eq!(
        client.get_currency_payout(&merchant_id, &String::from_str(&env, "USDC")),
        Some(usdc_payout)
    );
    assert_eq!(
        client.get_currency_payout(&merchant_id, &String::from_str(&env, "EUR")),
        Some(eur_payout)
    );
    assert_eq!(
        client.get_currency_payout(&merchant_id, &String::from_str(&env, "GBP")),
        Some(gbp_payout)
    );
}

#[test]
fn test_get_all_currency_payouts() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(MerchantRegistry, ());
    let client = MerchantRegistryClient::new(&env, &contract_id);

    let merchant_id = Address::generate(&env);
    client.register_merchant(
        &merchant_id,
        &String::from_str(&env, "Test Merchant"),
        &String::from_str(&env, "USDC"),
        &None,
        &None,
        &None,
    );

    let usdc_payout = Address::generate(&env);
    let eur_payout = Address::generate(&env);

    client.add_currency_payout(&merchant_id, &String::from_str(&env, "USDC"), &usdc_payout);
    client.add_currency_payout(&merchant_id, &String::from_str(&env, "EUR"), &eur_payout);

    let all_payouts = client.get_all_currency_payouts(&merchant_id);
    assert_eq!(all_payouts.len(), 2);
    assert_eq!(
        all_payouts.get(String::from_str(&env, "USDC")),
        Some(usdc_payout)
    );
    assert_eq!(
        all_payouts.get(String::from_str(&env, "EUR")),
        Some(eur_payout)
    );
}

// Tests for issue #210: Payout Address Whitelist Validation
#[test]
fn test_add_to_whitelist() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(MerchantRegistry, ());
    let client = MerchantRegistryClient::new(&env, &contract_id);

    let merchant_id = Address::generate(&env);
    client.register_merchant(
        &merchant_id,
        &String::from_str(&env, "Test Merchant"),
        &String::from_str(&env, "USDC"),
        &None,
        &None,
        &None,
    );

    let addr1 = Address::generate(&env);
    let addr2 = Address::generate(&env);

    client.add_to_whitelist(&merchant_id, &addr1);
    client.add_to_whitelist(&merchant_id, &addr2);

    let whitelist = client.get_whitelist(&merchant_id);
    assert_eq!(whitelist.len(), 2);
}

#[test]
fn test_remove_from_whitelist() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(MerchantRegistry, ());
    let client = MerchantRegistryClient::new(&env, &contract_id);

    let merchant_id = Address::generate(&env);
    client.register_merchant(
        &merchant_id,
        &String::from_str(&env, "Test Merchant"),
        &String::from_str(&env, "USDC"),
        &None,
        &None,
        &None,
    );

    let addr1 = Address::generate(&env);
    let addr2 = Address::generate(&env);

    client.add_to_whitelist(&merchant_id, &addr1);
    client.add_to_whitelist(&merchant_id, &addr2);

    client.remove_from_whitelist(&merchant_id, &addr1);

    let whitelist = client.get_whitelist(&merchant_id);
    assert_eq!(whitelist.len(), 1);
    assert_eq!(whitelist.get(0).unwrap(), addr2);
}

#[test]
fn test_is_address_whitelisted() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(MerchantRegistry, ());
    let client = MerchantRegistryClient::new(&env, &contract_id);

    let merchant_id = Address::generate(&env);
    client.register_merchant(
        &merchant_id,
        &String::from_str(&env, "Test Merchant"),
        &String::from_str(&env, "USDC"),
        &None,
        &None,
        &None,
    );

    let addr1 = Address::generate(&env);
    let addr2 = Address::generate(&env);

    // Empty whitelist allows all addresses
    assert!(client.is_address_whitelisted(&merchant_id, &addr1));
    assert!(client.is_address_whitelisted(&merchant_id, &addr2));

    // Add addr1 to whitelist
    client.add_to_whitelist(&merchant_id, &addr1);

    // Now only addr1 is whitelisted
    assert!(client.is_address_whitelisted(&merchant_id, &addr1));
    assert!(!client.is_address_whitelisted(&merchant_id, &addr2));
}

#[test]
#[should_panic(expected = "HostError: Error(Contract, #6)")]
fn test_update_merchant_with_non_whitelisted_payout() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(MerchantRegistry, ());
    let client = MerchantRegistryClient::new(&env, &contract_id);

    let merchant_id = Address::generate(&env);
    client.register_merchant(
        &merchant_id,
        &String::from_str(&env, "Test Merchant"),
        &String::from_str(&env, "USDC"),
        &None,
        &None,
        &None,
    );

    let whitelisted_addr = Address::generate(&env);
    let non_whitelisted_addr = Address::generate(&env);

    // Add only one address to whitelist
    client.add_to_whitelist(&merchant_id, &whitelisted_addr);

    // Try to update with non-whitelisted address - should panic
    client.update_merchant(
        &merchant_id,
        &None,
        &None,
        &None,
        &Some(non_whitelisted_addr),
        &None,
        &None,
    );
}

#[test]
fn test_update_merchant_with_whitelisted_payout() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(MerchantRegistry, ());
    let client = MerchantRegistryClient::new(&env, &contract_id);

    let merchant_id = Address::generate(&env);
    client.register_merchant(
        &merchant_id,
        &String::from_str(&env, "Test Merchant"),
        &String::from_str(&env, "USDC"),
        &None,
        &None,
        &None,
    );

    let whitelisted_addr = Address::generate(&env);

    // Add address to whitelist
    client.add_to_whitelist(&merchant_id, &whitelisted_addr);

    // Update with whitelisted address - should succeed
    client.update_merchant(
        &merchant_id,
        &None,
        &None,
        &None,
        &Some(whitelisted_addr.clone()),
        &None,
        &None,
    );

    let merchant = client.get_merchant(&merchant_id);
    assert_eq!(merchant.payout_address, Some(whitelisted_addr));
}

#[test]
#[should_panic(expected = "HostError: Error(Contract, #6)")]
fn test_add_currency_payout_with_non_whitelisted_address() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(MerchantRegistry, ());
    let client = MerchantRegistryClient::new(&env, &contract_id);

    let merchant_id = Address::generate(&env);
    client.register_merchant(
        &merchant_id,
        &String::from_str(&env, "Test Merchant"),
        &String::from_str(&env, "USDC"),
        &None,
        &None,
        &None,
    );

    let whitelisted_addr = Address::generate(&env);
    let non_whitelisted_addr = Address::generate(&env);

    // Add only one address to whitelist
    client.add_to_whitelist(&merchant_id, &whitelisted_addr);

    // Try to add currency payout with non-whitelisted address - should panic
    client.add_currency_payout(
        &merchant_id,
        &String::from_str(&env, "EUR"),
        &non_whitelisted_addr,
    );
}

#[test]
fn test_add_currency_payout_with_whitelisted_address() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(MerchantRegistry, ());
    let client = MerchantRegistryClient::new(&env, &contract_id);

    let merchant_id = Address::generate(&env);
    client.register_merchant(
        &merchant_id,
        &String::from_str(&env, "Test Merchant"),
        &String::from_str(&env, "USDC"),
        &None,
        &None,
        &None,
    );

    let whitelisted_addr = Address::generate(&env);

    // Add address to whitelist
    client.add_to_whitelist(&merchant_id, &whitelisted_addr);

    // Add currency payout with whitelisted address - should succeed
    client.add_currency_payout(
        &merchant_id,
        &String::from_str(&env, "EUR"),
        &whitelisted_addr.clone(),
    );

    let payout = client.get_currency_payout(&merchant_id, &String::from_str(&env, "EUR"));
    assert_eq!(payout, Some(whitelisted_addr));
}

// Test for issue #213: Optimizing Registry Listing Pagination (already implemented)
#[test]
fn test_pagination_with_large_merchant_list() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(MerchantRegistry, ());
    let client = MerchantRegistryClient::new(&env, &contract_id);

    // Register 10 merchants
    let merchant_names = [
        "Merchant 0",
        "Merchant 1",
        "Merchant 2",
        "Merchant 3",
        "Merchant 4",
        "Merchant 5",
        "Merchant 6",
        "Merchant 7",
        "Merchant 8",
        "Merchant 9",
    ];

    for name in merchant_names.iter() {
        let merchant_id = Address::generate(&env);
        client.register_merchant(
            &merchant_id,
            &String::from_str(&env, name),
            &String::from_str(&env, "USDC"),
            &None,
            &None,
            &None,
        );
    }

    // Test pagination with page size of 3
    let page1 = client.get_all_merchants(&0, &3);
    assert_eq!(page1.len(), 3);

    let page2 = client.get_all_merchants(&3, &3);
    assert_eq!(page2.len(), 3);

    let page3 = client.get_all_merchants(&6, &3);
    assert_eq!(page3.len(), 3);

    let page4 = client.get_all_merchants(&9, &3);
    assert_eq!(page4.len(), 1);

    // Test that offset beyond list returns empty
    let page5 = client.get_all_merchants(&15, &3);
    assert_eq!(page5.len(), 0);
}

#[test]
fn test_get_all_merchants_pagination_offset_one() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(MerchantRegistry, ());
    let client = MerchantRegistryClient::new(&env, &contract_id);

    let merchant_names = [
        "Merchant A",
        "Merchant B",
        "Merchant C",
        "Merchant D",
        "Merchant E",
    ];

    for name in merchant_names.iter() {
        let merchant_id = Address::generate(&env);
        client.register_merchant(
            &merchant_id,
            &String::from_str(&env, name),
            &String::from_str(&env, "USDC"),
            &None,
            &None,
            &None,
        );
    }

    let page1 = client.get_all_merchants(&0, &3);
    assert_eq!(page1.len(), 3);

    let page2 = client.get_all_merchants(&1, &3);
    assert_eq!(page2.len(), 3);

    let page_out_of_range = client.get_all_merchants(&6, &3);
    assert_eq!(page_out_of_range.len(), 0);
}
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(MerchantRegistry, ());
    let client = MerchantRegistryClient::new(&env, &contract_id);

    let merchant_id = Address::generate(&env);
    client.register_merchant(
        &merchant_id,
        &String::from_str(&env, "Test Merchant"),
        &String::from_str(&env, "USDC"),
        &None,
        &None,
        &None,
    );

    // Zero limit should return empty vector
    let result = client.get_all_merchants(&0, &0);
    assert_eq!(result.len(), 0);
}

// Integration test combining all features
#[test]
fn test_full_merchant_lifecycle_with_all_features() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(MerchantRegistry, ());
    let client = MerchantRegistryClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    client.initialize(&admin);

    let merchant_id = Address::generate(&env);

    // Register merchant
    client.register_merchant(
        &merchant_id,
        &String::from_str(&env, "Global Merchant"),
        &String::from_str(&env, "USDC"),
        &None,
        &None,
        &None,
    );

    // Set IPFS metadata hash
    let ipfs_hash = String::from_str(&env, "QmXoypizjW3WknFiJnKLwHCnL72vedxjQkDDP1mXWo6uco");
    client.set_metadata_hash(&merchant_id, &ipfs_hash);

    // Setup whitelist
    let addr1 = Address::generate(&env);
    let addr2 = Address::generate(&env);
    let addr3 = Address::generate(&env);

    client.add_to_whitelist(&merchant_id, &addr1);
    client.add_to_whitelist(&merchant_id, &addr2);
    client.add_to_whitelist(&merchant_id, &addr3);

    // Add multi-currency payouts
    client.add_currency_payout(&merchant_id, &String::from_str(&env, "USDC"), &addr1);
    client.add_currency_payout(&merchant_id, &String::from_str(&env, "EUR"), &addr2);
    client.add_currency_payout(&merchant_id, &String::from_str(&env, "GBP"), &addr3);

    // Verify all features
    let merchant = client.get_merchant(&merchant_id);
    assert_eq!(merchant.metadata_hash, Some(ipfs_hash));

    let whitelist = client.get_whitelist(&merchant_id);
    assert_eq!(whitelist.len(), 3);

    let all_payouts = client.get_all_currency_payouts(&merchant_id);
    assert_eq!(all_payouts.len(), 3);

    // Verify merchant
    client.verify_merchant(&admin, &merchant_id);
    let verified_merchant = client.get_merchant(&merchant_id);
    assert_eq!(verified_merchant.kyc_tier, KycTier::Basic);
}

#[test]
fn test_verify_merchant_with_oracle_signature() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(MerchantRegistry, ());
    let client = MerchantRegistryClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let merchant_id = Address::generate(&env);

    client.initialize(&admin);

    client.register_merchant(
        &merchant_id,
        &String::from_str(&env, "Merchant"),
        &String::from_str(&env, "USDC"),
        &None,
        &None,
        &None,
    );

    let signature = String::from_str(&env, "0x1234567890abcdef");
    
    // Verify merchant with oracle signature
    client.verify_merchant_with_signature(&admin, &merchant_id, &Some(signature.clone()));

    let merchant = client.get_merchant(&merchant_id);
    assert_eq!(merchant.oracle_signature, Some(signature));
}

#[test]
fn test_set_kyc_tier_with_oracle_signature() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(MerchantRegistry, ());
    let client = MerchantRegistryClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let merchant_id = Address::generate(&env);

    client.initialize(&admin);

    client.register_merchant(
        &merchant_id,
        &String::from_str(&env, "Merchant"),
        &String::from_str(&env, "USDC"),
        &None,
        &None,
        &None,
    );

    let signature = String::from_str(&env, "0xabcdef1234567890");
    
    // Set KYC tier with oracle signature
    client.set_kyc_tier_with_signature(&admin, &merchant_id, &KycTier::Full, &Some(signature.clone()));

    let merchant = client.get_merchant(&merchant_id);
    assert_eq!(merchant.oracle_signature, Some(signature));
}

// ─── Tier-Based Volume Cap Tests (Issue #63) ─────────────────────────────────

fn setup_volume_cap_env(
    env: &Env,
) -> (
    Address,
    PaymentProcessorClient<'_>,
    MerchantRegistryClient<'_>,
    Address, // merchant
    Address, // oracle
) {
    let payment_contract = env.register(crate::PaymentProcessor, ());
    let registry_contract = env.register(MerchantRegistry, ());

    let payment_client = PaymentProcessorClient::new(env, &payment_contract);
    let registry_client = MerchantRegistryClient::new(env, &registry_contract);

    let admin = Address::generate(env);
    payment_client.initialize_payment_processor(&admin);
    registry_client.initialize(&admin);

    // Link registry to payment processor
    payment_client.set_merchant_registry_address(&admin, &registry_contract);

    let merchant = Address::generate(env);
    let oracle = Address::generate(env);

    payment_client.grant_role(&admin, &Symbol::new(env, "MERCHANT"), &merchant);
    payment_client.grant_role(&admin, &Symbol::new(env, "ORACLE"), &oracle);

    registry_client.register_merchant(
        &merchant,
        &String::from_str(env, "Test Merchant"),
        &String::from_str(env, "USDC"),
        &None::<Address>,
        &None::<String>,
        &None,
    );

    (admin, payment_client, registry_client, merchant, oracle)
}

#[test]
fn test_basic_tier_cap_enforced() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|li| li.timestamp = 1_000_000);

    let (admin, payment_client, registry_client, merchant, oracle) =
        setup_volume_cap_env(&env);

    // Set merchant to Basic tier ($10,000 cap = 100_000_000_000 stroops)
    registry_client.set_kyc_tier_with_signature(&admin, &merchant, &KycTier::Basic, &Some(String::from_str(&env, "sig")));

    let deposit = Address::generate(&env);

    // First payment: $9,000 — should succeed
    let pid1 = String::from_str(&env, "pay_1");
    payment_client.create_payment(&crate::CreatePaymentArgs {
        payment_id: pid1.clone(),
        merchant_id: merchant.clone(),
        amount: 90_000_000_000,
        currency: Symbol::new(&env, "USDC"),
        deposit_address: deposit.clone(),
        expires_at: Some(env.ledger().timestamp() + 3600),
        duration_secs: None,
        memo: None,
        memo_type: None,
        token_address: None,
        client_token: None,
        metadata_hash: None,
    });
    payment_client.verify_payment(
        &oracle,
        &pid1,
        &soroban_sdk::BytesN::from_array(&env, &[0u8; 32]),
        &Address::generate(&env),
        &90_000_000_000,
    );

    // Second payment: $2,000 — would push total to $11,000, exceeding $10,000 cap
    let pid2 = String::from_str(&env, "pay_2");
    payment_client.create_payment(&crate::CreatePaymentArgs {
        payment_id: pid2.clone(),
        merchant_id: merchant.clone(),
        amount: 20_000_000_000,
        currency: Symbol::new(&env, "USDC"),
        deposit_address: deposit.clone(),
        expires_at: Some(env.ledger().timestamp() + 3600),
        duration_secs: None,
        memo: None,
        memo_type: None,
        token_address: None,
        client_token: None,
        metadata_hash: None,
    });

    let result = payment_client.try_verify_payment(
        &oracle,
        &pid2,
        &soroban_sdk::BytesN::from_array(&env, &[0u8; 32]),
        &Address::generate(&env),
        &20_000_000_000,
    );
    assert!(result.is_err(), "Expected TierVolumeLimitExceeded error");
}

#[test]
fn test_business_tier_no_cap() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|li| li.timestamp = 1_000_000);

    let (admin, payment_client, registry_client, merchant, oracle) =
        setup_volume_cap_env(&env);

    // Business tier: unlimited
    registry_client.set_kyc_tier_with_signature(&admin, &merchant, &KycTier::Business, &Some(String::from_str(&env, "sig")));

    let deposit = Address::generate(&env);

    // Pay well above any cap — should succeed
    let pid = String::from_str(&env, "pay_big");
    payment_client.create_payment(&crate::CreatePaymentArgs {
        payment_id: pid.clone(),
        merchant_id: merchant.clone(),
        amount: 10_000_000_000_000, // $1,000,000
        currency: Symbol::new(&env, "USDC"),
        deposit_address: deposit.clone(),
        expires_at: Some(env.ledger().timestamp() + 3600),
        duration_secs: None,
        memo: None,
        memo_type: None,
        token_address: None,
        client_token: None,
        metadata_hash: None,
    });
    payment_client.verify_payment(
        &oracle,
        &pid,
        &soroban_sdk::BytesN::from_array(&env, &[0u8; 32]),
        &Address::generate(&env),
        &10_000_000_000_000,
    );
}

#[test]
fn test_volume_resets_next_month() {
    let env = Env::default();
    env.mock_all_auths();
    // Start at beginning of a month epoch
    env.ledger().with_mut(|li| li.timestamp = 2_592_000); // epoch 1

    let (admin, payment_client, registry_client, merchant, oracle) =
        setup_volume_cap_env(&env);

    registry_client.set_kyc_tier_with_signature(&admin, &merchant, &KycTier::Basic, &Some(String::from_str(&env, "sig")));

    let deposit = Address::generate(&env);

    // Fill up the cap this month ($10,000)
    let pid1 = String::from_str(&env, "pay_m1");
    payment_client.create_payment(&crate::CreatePaymentArgs {
        payment_id: pid1.clone(),
        merchant_id: merchant.clone(),
        amount: 100_000_000_000,
        currency: Symbol::new(&env, "USDC"),
        deposit_address: deposit.clone(),
        expires_at: Some(env.ledger().timestamp() + 3600),
        duration_secs: None,
        memo: None,
        memo_type: None,
        token_address: None,
        client_token: None,
        metadata_hash: None,
    });
    payment_client.verify_payment(
        &oracle,
        &pid1,
        &soroban_sdk::BytesN::from_array(&env, &[0u8; 32]),
        &Address::generate(&env),
        &100_000_000_000,
    );

    // Advance to next month epoch
    env.ledger().with_mut(|li| li.timestamp = 2 * 2_592_000 + 1); // epoch 2

    // Same amount should succeed in the new month
    let pid2 = String::from_str(&env, "pay_m2");
    payment_client.create_payment(&crate::CreatePaymentArgs {
        payment_id: pid2.clone(),
        merchant_id: merchant.clone(),
        amount: 100_000_000_000,
        currency: Symbol::new(&env, "USDC"),
        deposit_address: deposit.clone(),
        expires_at: Some(env.ledger().timestamp() + 3600),
        duration_secs: None,
        memo: None,
        memo_type: None,
        token_address: None,
        client_token: None,
        metadata_hash: None,
    });
    payment_client.verify_payment(
        &oracle,
        &pid2,
        &soroban_sdk::BytesN::from_array(&env, &[0u8; 32]),
        &Address::generate(&env),
        &100_000_000_000,
    );
}
