#![cfg(test)]

use crate::{
    merchant_registry::{MerchantRegistry, MerchantRegistryClient},
    PaymentProcessor, PaymentProcessorClient, RefundManager, RefundManagerClient,
};
use soroban_sdk::{
    testutils::{Address as _, BytesN as _},
    token, Address, BytesN, Env, String, Symbol,
};

fn setup_contracts(
    env: &Env,
) -> (
    Address,
    PaymentProcessorClient<'_>,
    RefundManagerClient<'_>,
    MerchantRegistryClient<'_>,
) {
    let payment_processor = env.register(PaymentProcessor, ());
    let refund_manager = env.register(RefundManager, ());
    let merchant_registry = env.register(MerchantRegistry, ());

    let refund_client = RefundManagerClient::new(env, &refund_manager);
    let payment_client = PaymentProcessorClient::new(env, &payment_processor);
    let merchant_client = MerchantRegistryClient::new(env, &merchant_registry);

    let admin = Address::generate(env);
    let token_admin = Address::generate(env);
    let usdc_token = env.register_stellar_asset_contract_v2(token_admin).address();
    refund_client.initialize_refund_manager(&admin, &usdc_token);
    let token_admin_client = token::StellarAssetClient::new(env, &usdc_token);
    token_admin_client.mint(&refund_manager, &1_000_000_000_000i128);

    payment_client.initialize_payment_processor(&admin);
    merchant_client.initialize(&admin);

    (admin, payment_client, refund_client, merchant_client)
}

#[test]
#[should_panic(expected = "HostError: Error(Auth, InvalidAction)")]
fn test_grant_role_without_admin_signature() {
    let env = Env::default();
    let (admin, _payment_client, refund_client, _merchant_client) = setup_contracts(&env);
    let account = Address::generate(&env);
    let role = Symbol::new(&env, "ORACLE");
    refund_client.grant_role(&admin, &role, &account);
}

#[test]
#[should_panic(expected = "HostError: Error(Auth, InvalidAction)")]
fn test_revoke_role_without_admin_signature() {
    let env = Env::default();
    let (admin, _payment_client, refund_client, _merchant_client) = setup_contracts(&env);
    let account = Address::generate(&env);
    let role = Symbol::new(&env, "ORACLE");
    refund_client.revoke_role(&admin, &role, &account);
}

#[test]
#[should_panic(expected = "HostError: Error(Auth, InvalidAction)")]
fn test_transfer_admin_without_admin_signature() {
    let env = Env::default();
    let (admin, _payment_client, refund_client, _merchant_client) = setup_contracts(&env);
    let new_admin = Address::generate(&env);
    refund_client.transfer_admin(&admin, &new_admin);
}

#[test]
#[should_panic(expected = "HostError: Error(Auth, InvalidAction)")]
fn test_verify_payment_without_oracle_signature() {
    let env = Env::default();
    let (_admin, payment_client, _refund_client, _merchant_client) = setup_contracts(&env);
    let oracle = Address::generate(&env);
    payment_client.verify_payment(
        &oracle,
        &String::from_str(&env, "p1"),
        &BytesN::<32>::random(&env),
        &Address::generate(&env),
        &100i128,
    );
}

#[test]
#[should_panic(expected = "HostError: Error(Auth, InvalidAction)")]
fn test_create_dispute_without_disputer_signature() {
    let env = Env::default();
    let (_admin, _payment_client, refund_client, _merchant_client) = setup_contracts(&env);
    let customer = Address::generate(&env);
    refund_client.create_dispute(
        &String::from_str(&env, "p1"),
        &100i128,
        &String::from_str(&env, "r"),
        &String::from_str(&env, "e"),
        &customer,
    );
}

#[test]
#[should_panic(expected = "Error(Contract, #1)")]
fn test_process_refund_without_operator_role() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, _payment_client, refund_client, _merchant_client) = setup_contracts(&env);
    let non_operator = Address::generate(&env);
    refund_client.process_refund(&non_operator, &String::from_str(&env, "refund_1"));
}

#[test]
#[should_panic(expected = "Error(Contract, #3)")]
fn test_verify_merchant_called_by_non_admin() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, _payment_client, _refund_client, merchant_client) = setup_contracts(&env);
    let non_admin = Address::generate(&env);
    let merchant = Address::generate(&env);
    // Setup merchant
    merchant_client.register_merchant(
        &merchant,
        &String::from_str(&env, "M"),
        &String::from_str(&env, "USD"),
        &None::<Address>,
        &None::<String>,
    );
    merchant_client.verify_merchant(&non_admin, &merchant);
}
