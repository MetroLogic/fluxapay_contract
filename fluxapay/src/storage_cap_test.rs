//! Issue #1170: persistent storage entry caps for per-merchant payment maps.

use crate::{access_control::role_merchant, CreatePaymentArgs, DataKey, Error, PaymentProcessor, PaymentProcessorClient, MAX_PAYMENTS_PER_MERCHANT};
use soroban_sdk::{testutils::Address as _, Address, Env, String, Symbol, Vec};

#[test]
fn create_payment_rejected_when_merchant_payments_index_is_full() {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register(PaymentProcessor, ());
    let client = PaymentProcessorClient::new(&env, &id);
    let admin = Address::generate(&env);
    client.initialize_payment_processor(&admin);
    let merchant = Address::generate(&env);
    client.grant_role(&admin, &role_merchant(&env), &merchant);

    let mut full = Vec::new(&env);
    for _ in 0..MAX_PAYMENTS_PER_MERCHANT {
        full.push_back(String::from_str(&env, "existing-payment"));
    }
    env.as_contract(&id, || {
        env.storage()
            .persistent()
            .set(&DataKey::MerchantPayments(merchant.clone()), &full);
    });

    let args = CreatePaymentArgs {
        payment_id: String::from_str(&env, "over-cap-payment"),
        merchant_id: merchant,
        payer: None,
        amount: 1_000i128,
        currency: Symbol::new(&env, "USDC"),
        deposit_address: Address::generate(&env),
        expires_at: Some(env.ledger().timestamp() + 3_600),
        duration_secs: None,
        memo: None,
        memo_type: None,
        token_address: None,
        client_token: None,
        idempotency_key: None,
        metadata_hash: None,
        metadata: None,
        fee_waiver_code: None,
        retry_of_payment_id: None,
        payer_muxed_id: None,
        allow_partial: None,
        tip_enabled: false,
        settle_at: None,
    };
    assert_eq!(
        client.try_create_payment(&args),
        Err(Ok(Error::StorageEntryLimitExceeded))
    );
}
