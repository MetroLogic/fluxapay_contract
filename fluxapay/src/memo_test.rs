use crate::{
    access_control::role_merchant, CreatePaymentArgs, Error, PaymentProcessor,
    PaymentProcessorClient, PaymentStatus,
};
use soroban_sdk::{
    testutils::Address as _, testutils::BytesN as _, Address, BytesN, Env, String, Symbol,
};

fn setup_payment_processor(env: &Env) -> (Address, PaymentProcessorClient<'_>) {
    let contract_id = env.register(PaymentProcessor, ());
    let client = PaymentProcessorClient::new(env, &contract_id);
    let admin = Address::generate(env);
    client.initialize_payment_processor(&admin);
    (admin, client)
}

fn create_payment_args(
    env: &Env,
    payment_id: &String,
    merchant_id: &Address,
    amount: i128,
) -> CreatePaymentArgs {
    CreatePaymentArgs {
        payment_id: payment_id.clone(),
        merchant_id: merchant_id.clone(),
        payer: None,
        amount,
        currency: Symbol::new(env, "USDC"),
        deposit_address: Address::generate(env),
        expires_at: Some(env.ledger().timestamp() + 3600),
        duration_secs: None,
        memo: None,
        memo_type: None,
        token_address: None,
        client_token: None,
        metadata_hash: None, metadata: None,
        fee_waiver_code: None,
    }
}

#[test]
fn test_create_payment_with_memo() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    let payment_id = String::from_str(&env, "payment_with_memo");
    let merchant_id = Address::generate(&env);
    let amount = 1000i128;
    let memo = Some(String::from_str(&env, "ORDER-12345"));
    let memo_type = Some(String::from_str(&env, "Text"));

    client.grant_role(&admin, &role_merchant(&env), &merchant_id);

    let mut args = create_payment_args(&env, &payment_id, &merchant_id, amount);
    args.memo = memo;
    args.memo_type = memo_type;

    let payment = client.create_payment(&args);

    assert_eq!(payment.payment_id, payment_id);
    assert_eq!(payment.memo, Some(String::from_str(&env, "ORDER-12345")));
    assert_eq!(payment.memo_type, Some(String::from_str(&env, "Text")));
}

#[test]
fn test_create_payment_without_memo() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    let payment_id = String::from_str(&env, "payment_no_memo");
    let merchant_id = Address::generate(&env);
    let amount = 1000i128;

    client.grant_role(&admin, &role_merchant(&env), &merchant_id);

    let args = create_payment_args(&env, &payment_id, &merchant_id, amount);
    let payment = client.create_payment(&args);

    assert_eq!(payment.payment_id, payment_id);
    assert_eq!(payment.memo, None);
    assert_eq!(payment.memo_type, None);
}

#[test]
fn test_create_payment_with_id_memo() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    let payment_id = String::from_str(&env, "payment_id_memo");
    let merchant_id = Address::generate(&env);
    let amount = 2000i128;
    let memo = Some(String::from_str(&env, "123456789"));
    let memo_type = Some(String::from_str(&env, "Id"));

    client.grant_role(&admin, &role_merchant(&env), &merchant_id);

    let mut args = create_payment_args(&env, &payment_id, &merchant_id, amount);
    args.memo = memo;
    args.memo_type = memo_type;

    let payment = client.create_payment(&args);

    assert_eq!(payment.memo, Some(String::from_str(&env, "123456789")));
    assert_eq!(payment.memo_type, Some(String::from_str(&env, "Id")));
}

#[test]
fn test_create_payment_with_hash_memo() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    let payment_id = String::from_str(&env, "payment_hash_memo");
    let merchant_id = Address::generate(&env);
    let amount = 3000i128;
    let memo = Some(String::from_str(&env, "abcdef1234567890"));
    let memo_type = Some(String::from_str(&env, "Hash"));

    client.grant_role(&admin, &role_merchant(&env), &merchant_id);

    let mut args = create_payment_args(&env, &payment_id, &merchant_id, amount);
    args.memo = memo;
    args.memo_type = memo_type;

    let payment = client.create_payment(&args);

    assert_eq!(
        payment.memo,
        Some(String::from_str(&env, "abcdef1234567890"))
    );
    assert_eq!(payment.memo_type, Some(String::from_str(&env, "Hash")));
}

#[test]
fn test_memo_persists_after_verification() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    let payment_id = String::from_str(&env, "payment_memo_persist");
    let merchant_id = Address::generate(&env);
    let amount = 1500i128;
    let memo = Some(String::from_str(&env, "INVOICE-999"));
    let memo_type = Some(String::from_str(&env, "Text"));

    client.grant_role(&admin, &role_merchant(&env), &merchant_id);

    let mut args = create_payment_args(&env, &payment_id, &merchant_id, amount);
    args.memo = memo;
    args.memo_type = memo_type;

    client.create_payment(&args);

    // Verify payment
    let oracle = Address::generate(&env);
    client.grant_role(&admin, &crate::access_control::role_oracle(&env), &oracle);
    client.verify_payment(
        &oracle,
        &payment_id,
        &BytesN::<32>::random(&env),
        &Address::generate(&env),
        &amount,
        &None::<u64>,
    );

    // Check memo persists after verification
    let payment = client.get_payment(&payment_id);
    assert_eq!(payment.status, PaymentStatus::Confirmed);
    assert_eq!(payment.memo, Some(String::from_str(&env, "INVOICE-999")));
    assert_eq!(payment.memo_type, Some(String::from_str(&env, "Text")));
}

// ── Issue #397: Stellar memo type validation tests ──────────────────────────

#[test]
fn test_invalid_memo_type_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    let payment_id = String::from_str(&env, "memo_invalid_type");
    let merchant_id = Address::generate(&env);
    client.grant_role(&admin, &role_merchant(&env), &merchant_id);

    let mut args = create_payment_args(&env, &payment_id, &merchant_id, 1000);
    args.memo = Some(String::from_str(&env, "hello"));
    args.memo_type = Some(String::from_str(&env, "invalid_type"));

    let result = client.try_create_payment(&args);
    assert_eq!(result, Err(Ok(Error::InvalidMemoType)));
}

#[test]
fn test_text_memo_too_long_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    let payment_id = String::from_str(&env, "memo_text_too_long");
    let merchant_id = Address::generate(&env);
    client.grant_role(&admin, &role_merchant(&env), &merchant_id);

    // 29 bytes — one byte over the 28-byte Stellar limit
    let long_memo = String::from_str(&env, "12345678901234567890123456789");
    let mut args = create_payment_args(&env, &payment_id, &merchant_id, 1000);
    args.memo = Some(long_memo);
    args.memo_type = Some(String::from_str(&env, "Text"));

    let result = client.try_create_payment(&args);
    assert_eq!(result, Err(Ok(Error::MemoTooLong)));
}

#[test]
fn test_text_memo_exactly_28_bytes_accepted() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    let payment_id = String::from_str(&env, "memo_text_28bytes");
    let merchant_id = Address::generate(&env);
    client.grant_role(&admin, &role_merchant(&env), &merchant_id);

    // exactly 28 bytes
    let memo_28 = String::from_str(&env, "1234567890123456789012345678");
    let mut args = create_payment_args(&env, &payment_id, &merchant_id, 1000);
    args.memo = Some(memo_28.clone());
    args.memo_type = Some(String::from_str(&env, "Text"));

    let payment = client.create_payment(&args);
    assert_eq!(payment.memo, Some(memo_28));
    assert_eq!(payment.memo_type, Some(String::from_str(&env, "Text")));
}

#[test]
fn test_id_memo_non_numeric_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    let payment_id = String::from_str(&env, "memo_id_non_numeric");
    let merchant_id = Address::generate(&env);
    client.grant_role(&admin, &role_merchant(&env), &merchant_id);

    let mut args = create_payment_args(&env, &payment_id, &merchant_id, 1000);
    args.memo = Some(String::from_str(&env, "not-a-number"));
    args.memo_type = Some(String::from_str(&env, "Id"));

    let result = client.try_create_payment(&args);
    assert_eq!(result, Err(Ok(Error::InvalidMemoId)));
}

#[test]
fn test_id_memo_valid_u64_accepted() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    let payment_id = String::from_str(&env, "memo_id_valid");
    let merchant_id = Address::generate(&env);
    client.grant_role(&admin, &role_merchant(&env), &merchant_id);

    let mut args = create_payment_args(&env, &payment_id, &merchant_id, 1000);
    args.memo = Some(String::from_str(&env, "9876543210"));
    args.memo_type = Some(String::from_str(&env, "Id"));

    let payment = client.create_payment(&args);
    assert_eq!(payment.memo, Some(String::from_str(&env, "9876543210")));
    assert_eq!(payment.memo_type, Some(String::from_str(&env, "Id")));
}

#[test]
fn test_hash_memo_accepted() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    let payment_id = String::from_str(&env, "memo_hash_type");
    let merchant_id = Address::generate(&env);
    client.grant_role(&admin, &role_merchant(&env), &merchant_id);

    let mut args = create_payment_args(&env, &payment_id, &merchant_id, 1000);
    args.memo = Some(String::from_str(&env, "deadbeef"));
    args.memo_type = Some(String::from_str(&env, "Hash"));

    let payment = client.create_payment(&args);
    assert_eq!(payment.memo_type, Some(String::from_str(&env, "Hash")));
}

#[test]
fn test_return_memo_accepted() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    let payment_id = String::from_str(&env, "memo_return_type");
    let merchant_id = Address::generate(&env);
    client.grant_role(&admin, &role_merchant(&env), &merchant_id);

    let mut args = create_payment_args(&env, &payment_id, &merchant_id, 1000);
    args.memo = Some(String::from_str(&env, "cafebabe"));
    args.memo_type = Some(String::from_str(&env, "Return"));

    let payment = client.create_payment(&args);
    assert_eq!(payment.memo_type, Some(String::from_str(&env, "Return")));
}

#[test]
fn test_no_memo_type_no_validation() {
    // When memo_type is None, even a very long memo string is accepted (no validation runs).
    let env = Env::default();
    env.mock_all_auths();
    let (admin, client) = setup_payment_processor(&env);

    let payment_id = String::from_str(&env, "memo_no_type");
    let merchant_id = Address::generate(&env);
    client.grant_role(&admin, &role_merchant(&env), &merchant_id);

    let mut args = create_payment_args(&env, &payment_id, &merchant_id, 1000);
    args.memo = Some(String::from_str(&env, "this memo has no type so no validation"));
    args.memo_type = None;

    let payment = client.create_payment(&args);
    assert!(payment.memo.is_some());
    assert!(payment.memo_type.is_none());
}
