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
        &None::<Address>,
        &None::<String>,
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

    client.merchant_initialize(&admin);

    client.register_merchant(
        &merchant_id,
        &String::from_str(&env, "Merchant"),
        &String::from_str(&env, "USDC"),
        &None::<Address>,
        &None::<String>,
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

    client.merchant_initialize(&admin);

    client.register_merchant(
        &merchant_id,
        &String::from_str(&env, "Merchant"),
        &String::from_str(&env, "USDC"),
        &None::<Address>,
        &None::<String>,
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

    client.merchant_initialize(&admin);
    client.register_merchant(
        &merchant_id,
        &String::from_str(&env, "BigCorp"),
        &String::from_str(&env, "USDC"),
        &None::<Address>,
        &None::<String>,
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

    client.merchant_initialize(&admin);
    client.register_merchant(
        &merchant_id,
        &String::from_str(&env, "Merchant"),
        &String::from_str(&env, "USDC"),
        &None::<Address>,
        &None::<String>,
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
    client.merchant_initialize(&admin);

    // Register multiple merchants
    let merchant1 = Address::generate(&env);
    let merchant2 = Address::generate(&env);
    let merchant3 = Address::generate(&env);

    client.register_merchant(
        &merchant1,
        &String::from_str(&env, "Merchant 1"),
        &String::from_str(&env, "USDC"),
        &None::<Address>,
        &None::<String>,
    );
    client.register_merchant(
        &merchant2,
        &String::from_str(&env, "Merchant 2"),
        &String::from_str(&env, "USDC"),
        &None::<Address>,
        &None::<String>,
    );
    client.register_merchant(
        &merchant3,
        &String::from_str(&env, "Merchant 3"),
        &String::from_str(&env, "USDC"),
        &None::<Address>,
        &None::<String>,
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
    client.merchant_initialize(&admin);

    // Register merchants
    let merchant1 = Address::generate(&env);
    let merchant2 = Address::generate(&env);
    let merchant3 = Address::generate(&env);

    client.register_merchant(
        &merchant1,
        &String::from_str(&env, "Merchant 1"),
        &String::from_str(&env, "USDC"),
        &None::<Address>,
        &None::<String>,
    );
    client.register_merchant(
        &merchant2,
        &String::from_str(&env, "Merchant 2"),
        &String::from_str(&env, "USDC"),
        &None::<Address>,
        &None::<String>,
    );
    client.register_merchant(
        &merchant3,
        &String::from_str(&env, "Merchant 3"),
        &String::from_str(&env, "USDC"),
        &None::<Address>,
        &None::<String>,
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
#[should_panic(expected = "HostError: Error(Contract, #10)")]
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
    let usdc_token = env.register_stellar_asset_contract_v2(token_admin).address();

    // Initialize contracts
    refund_client.initialize_refund_manager(&admin, &usdc_token);
    payment_client.initialize_payment_processor(&admin);
    merchant_client.merchant_initialize(&admin);

    // Register merchant but DON'T verify them
    let merchant = Address::generate(&env);
    merchant_client.register_merchant(
        &merchant,
        &String::from_str(&env, "Unverified Merchant"),
        &String::from_str(&env, "USDC"),
        &None::<Address>,
        &None::<String>,
    );

    // Try to create payment - should fail because merchant is not verified
    let payment_id = String::from_str(&env, "PAY_01");
    let amount = 1000i128;
    let expires_at = env.ledger().timestamp() + 3600;

    // This should panic with Unauthorized error
    payment_client.create_payment(
        &payment_id,
        &merchant,
        &amount,
        &Symbol::new(&env, "USDC"),
        &Address::generate(&env),
        &expires_at,
    );
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
    let usdc_token = env.register_stellar_asset_contract_v2(token_admin).address();

    // Initialize contracts
    refund_client.initialize_refund_manager(&admin, &usdc_token);
    payment_client.initialize_payment_processor(&admin);
    merchant_client.merchant_initialize(&admin);

    // Register and verify merchant
    let merchant = Address::generate(&env);
    merchant_client.register_merchant(
        &merchant,
        &String::from_str(&env, "Verified Merchant"),
        &String::from_str(&env, "USDC"),
        &None::<Address>,
        &None::<String>,
    );

    // Manually grant MERCHANT role on the payment processor
    payment_client.payment_grant_role(&admin, &Symbol::new(&env, "MERCHANT"), &merchant);

    // Now create payment should succeed
    let payment_id = String::from_str(&env, "PAY_01");
    let amount = 1000i128;
    let expires_at = env.ledger().timestamp() + 3600;

    let payment = payment_client.create_payment(
        &payment_id,
        &merchant,
        &amount,
        &Symbol::new(&env, "USDC"),
        &Address::generate(&env),
        &expires_at,
    );

    assert_eq!(payment.payment_id, payment_id);
    assert_eq!(payment.merchant_id, merchant);
    assert_eq!(payment.amount, amount);
}
