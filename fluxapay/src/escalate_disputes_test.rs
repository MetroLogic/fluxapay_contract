use crate::{
    PaymentProcessor, PaymentProcessorClient, RefundManager, RefundManagerClient,
};
use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    token, Address, Env, String,
};

#[test]
fn test_escalate_expired_disputes_past_deadline() {
    let env = Env::default();
    env.mock_all_auths();

    let payment_processor = env.register(PaymentProcessor, ());
    let refund_manager = env.register(RefundManager, ());

    let payment_client = PaymentProcessorClient::new(&env, &payment_processor);
    let refund_client = RefundManagerClient::new(&env, &refund_manager);

    let admin = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let usdc_token = env
        .register_stellar_asset_contract_v2(token_admin)
        .address();

    refund_client.initialize_refund_manager(&admin, &usdc_token);
    let token_admin_client = token::StellarAssetClient::new(&env, &usdc_token);
    token_admin_client.mint(&refund_manager, &1_000_000_000_000i128);

    payment_client.initialize_payment_processor(&admin);

    let dispute_id = String::from_str(&env, "dispute_001");
    let payer = Address::generate(&env);

    refund_client.create_dispute(
        &dispute_id,
        &100_000_000i128,
        &String::from_str(&env, "reason"),
        &String::from_str(&env, "evidence"),
        &payer,
        &soroban_sdk::vec![&env],
    );

    env.ledger().with_timestamp(env.ledger().timestamp() + 10_000_000);

    let dispute_ids = soroban_sdk::vec![&env, dispute_id];
    let count = payment_client.escalate_expired_disputes(&dispute_ids);

    assert_eq!(count, 1);
}

#[test]
fn test_escalate_non_expired_dispute_not_escalated() {
    let env = Env::default();
    env.mock_all_auths();

    let payment_processor = env.register(PaymentProcessor, ());
    let refund_manager = env.register(RefundManager, ());

    let payment_client = PaymentProcessorClient::new(&env, &payment_processor);
    let refund_client = RefundManagerClient::new(&env, &refund_manager);

    let admin = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let usdc_token = env
        .register_stellar_asset_contract_v2(token_admin)
        .address();

    refund_client.initialize_refund_manager(&admin, &usdc_token);
    let token_admin_client = token::StellarAssetClient::new(&env, &usdc_token);
    token_admin_client.mint(&refund_manager, &1_000_000_000_000i128);

    payment_client.initialize_payment_processor(&admin);

    let dispute_id = String::from_str(&env, "dispute_002");
    let payer = Address::generate(&env);

    refund_client.create_dispute(
        &dispute_id,
        &100_000_000i128,
        &String::from_str(&env, "reason"),
        &String::from_str(&env, "evidence"),
        &payer,
        &soroban_sdk::vec![&env],
    );

    let dispute_ids = soroban_sdk::vec![&env, dispute_id];
    let count = payment_client.escalate_expired_disputes(&dispute_ids);

    assert_eq!(count, 0);
}

#[test]
fn test_escalate_already_escalated_not_double_counted() {
    let env = Env::default();
    env.mock_all_auths();

    let payment_processor = env.register(PaymentProcessor, ());
    let refund_manager = env.register(RefundManager, ());

    let payment_client = PaymentProcessorClient::new(&env, &payment_processor);
    let refund_client = RefundManagerClient::new(&env, &refund_manager);

    let admin = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let usdc_token = env
        .register_stellar_asset_contract_v2(token_admin)
        .address();

    refund_client.initialize_refund_manager(&admin, &usdc_token);
    let token_admin_client = token::StellarAssetClient::new(&env, &usdc_token);
    token_admin_client.mint(&refund_manager, &1_000_000_000_000i128);

    payment_client.initialize_payment_processor(&admin);

    let dispute_id = String::from_str(&env, "dispute_003");
    let payer = Address::generate(&env);

    refund_client.create_dispute(
        &dispute_id,
        &100_000_000i128,
        &String::from_str(&env, "reason"),
        &String::from_str(&env, "evidence"),
        &payer,
        &soroban_sdk::vec![&env],
    );

    env.ledger().with_timestamp(env.ledger().timestamp() + 10_000_000);

    let dispute_ids = soroban_sdk::vec![&env, dispute_id.clone()];
    let count = payment_client.escalate_expired_disputes(&dispute_ids);
    assert_eq!(count, 1);

    let dispute_ids_again = soroban_sdk::vec![&env, dispute_id];
    let count_again = payment_client.escalate_expired_disputes(&dispute_ids_again);

    assert_eq!(count_again, 0);
}

#[test]
fn test_escalate_mixed_batch() {
    let env = Env::default();
    env.mock_all_auths();

    let payment_processor = env.register(PaymentProcessor, ());
    let refund_manager = env.register(RefundManager, ());

    let payment_client = PaymentProcessorClient::new(&env, &payment_processor);
    let refund_client = RefundManagerClient::new(&env, &refund_manager);

    let admin = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let usdc_token = env
        .register_stellar_asset_contract_v2(token_admin)
        .address();

    refund_client.initialize_refund_manager(&admin, &usdc_token);
    let token_admin_client = token::StellarAssetClient::new(&env, &usdc_token);
    token_admin_client.mint(&refund_manager, &1_000_000_000_000i128);

    payment_client.initialize_payment_processor(&admin);

    let payer = Address::generate(&env);

    let dispute_id_1 = String::from_str(&env, "dispute_mix_001");
    let dispute_id_2 = String::from_str(&env, "dispute_mix_002");
    let dispute_id_3 = String::from_str(&env, "dispute_mix_003");

    refund_client.create_dispute(
        &dispute_id_1,
        &100_000_000i128,
        &String::from_str(&env, "reason"),
        &String::from_str(&env, "evidence"),
        &payer,
        &soroban_sdk::vec![&env],
    );

    refund_client.create_dispute(
        &dispute_id_2,
        &100_000_000i128,
        &String::from_str(&env, "reason"),
        &String::from_str(&env, "evidence"),
        &payer,
        &soroban_sdk::vec![&env],
    );

    refund_client.create_dispute(
        &dispute_id_3,
        &100_000_000i128,
        &String::from_str(&env, "reason"),
        &String::from_str(&env, "evidence"),
        &payer,
        &soroban_sdk::vec![&env],
    );

    env.ledger().with_timestamp(env.ledger().timestamp() + 10_000_000);

    let dispute_ids_1 = soroban_sdk::vec![&env, dispute_id_1];
    let count_1 = payment_client.escalate_expired_disputes(&dispute_ids_1);
    assert_eq!(count_1, 1);

    let dispute_ids_2 = soroban_sdk::vec![&env, dispute_id_2];
    let count_2 = payment_client.escalate_expired_disputes(&dispute_ids_2);
    assert_eq!(count_2, 1);

    let dispute_ids_batch = soroban_sdk::vec![&env, dispute_id_1, dispute_id_2, dispute_id_3];
    let count_batch = payment_client.escalate_expired_disputes(&dispute_ids_batch);

    assert_eq!(count_batch, 1);
}

#[test]
fn test_escalate_nonexistent_id_silently_skipped() {
    let env = Env::default();
    env.mock_all_auths();

    let payment_processor = env.register(PaymentProcessor, ());
    let refund_manager = env.register(RefundManager, ());

    let payment_client = PaymentProcessorClient::new(&env, &payment_processor);
    let refund_client = RefundManagerClient::new(&env, &refund_manager);

    let admin = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let usdc_token = env
        .register_stellar_asset_contract_v2(token_admin)
        .address();

    refund_client.initialize_refund_manager(&admin, &usdc_token);
    let token_admin_client = token::StellarAssetClient::new(&env, &usdc_token);
    token_admin_client.mint(&refund_manager, &1_000_000_000_000i128);

    payment_client.initialize_payment_processor(&admin);

    let payer = Address::generate(&env);

    let dispute_id_1 = String::from_str(&env, "dispute_exist_001");
    let dispute_id_nonexistent = String::from_str(&env, "dispute_nonexistent_999");

    refund_client.create_dispute(
        &dispute_id_1,
        &100_000_000i128,
        &String::from_str(&env, "reason"),
        &String::from_str(&env, "evidence"),
        &payer,
        &soroban_sdk::vec![&env],
    );

    env.ledger().with_timestamp(env.ledger().timestamp() + 10_000_000);

    let dispute_ids = soroban_sdk::vec![&env, dispute_id_1, dispute_id_nonexistent];
    let count = payment_client.escalate_expired_disputes(&dispute_ids);

    assert_eq!(count, 1);
}
