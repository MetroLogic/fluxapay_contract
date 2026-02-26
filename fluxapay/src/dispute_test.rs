use crate::{
    Dispute, DisputeStatus, PaymentProcessor, PaymentProcessorClient, Refund, RefundManager,
    RefundManagerClient, RefundStatus,
};
use soroban_sdk::{
    testutils::{Address as _, BytesN as _},
    Address, BytesN, Env, String, Symbol,
};

fn setup_contracts(env: &Env) -> (Address, PaymentProcessorClient<'_>, RefundManagerClient<'_>) {
    let payment_processor = env.register(PaymentProcessor, ());
    let refund_manager = env.register(RefundManager, ());

    let refund_client = RefundManagerClient::new(env, &refund_manager);
    let admin = Address::generate(env);
    refund_client.initialize_refund_manager(&admin);

    (
        admin,
        PaymentProcessorClient::new(env, &payment_processor),
        refund_client,
    )
}

#[test]
fn test_create_dispute() {
    let env = Env::default();
    env.mock_all_auths();

    let (_admin, payment_client, refund_client) = setup_contracts(&env);
    let merchant = Address::generate(&env);
    let customer = Address::generate(&env);

    // Create and verify a payment
    let payment_id = String::from_str(&env, "payment_001");
    let amount = 1000i128;
    let currency = Symbol::new(&env, "USDC");
    let deposit_address = Address::generate(&env);
    let expires_at = env.ledger().timestamp() + 3600;

    payment_client.create_payment(
        &payment_id,
        &merchant,
        &amount,
        &currency,
        &deposit_address,
        &expires_at,
    );

    // Verify payment
    let transaction_hash = BytesN::<32>::random(&env);
    payment_client.verify_payment(&payment_id, &transaction_hash, &customer, &amount);

    // Create dispute
    let dispute_reason = String::from_str(&env, "Product not received");
    let evidence = String::from_str(&env, "Tracking shows delivery failed");

    let dispute_id =
        refund_client.create_dispute(&payment_id, &amount, &dispute_reason, &evidence, &customer);

    // Verify dispute was created
    let dispute: Dispute = refund_client.get_dispute(&dispute_id);
    assert_eq!(dispute.payment_id, payment_id);
    assert_eq!(dispute.amount, amount);
    assert_eq!(dispute.status, DisputeStatus::Open);
    assert_eq!(dispute.disputer, customer);
}

#[test]
fn test_review_dispute() {
    let env = Env::default();
    env.mock_all_auths();

    let (admin, payment_client, refund_client) = setup_contracts(&env);
    let merchant = Address::generate(&env);
    let customer = Address::generate(&env);
    let operator = Address::generate(&env);

    // Grant operator role
    let settlement_role = Symbol::new(&env, "SETTLEMENT_OPERATOR");
    refund_client.grant_role(&admin, &settlement_role, &operator);

    // Create and verify payment
    let payment_id = String::from_str(&env, "payment_002");
    let amount = 500i128;
    let currency = Symbol::new(&env, "USDC");
    let deposit_address = Address::generate(&env);
    let expires_at = env.ledger().timestamp() + 3600;

    payment_client.create_payment(
        &payment_id,
        &merchant,
        &amount,
        &currency,
        &deposit_address,
        &expires_at,
    );

    let transaction_hash = BytesN::<32>::random(&env);
    payment_client.verify_payment(&payment_id, &transaction_hash, &customer, &amount);

    // Create dispute
    let dispute_reason = String::from_str(&env, "Wrong item received");
    let evidence = String::from_str(&env, "Photo evidence attached");

    let dispute_id =
        refund_client.create_dispute(&payment_id, &amount, &dispute_reason, &evidence, &customer);

    // Review dispute
    refund_client.review_dispute(&operator, &dispute_id);

    // Verify dispute status changed
    let dispute: Dispute = refund_client.get_dispute(&dispute_id);
    assert_eq!(dispute.status, DisputeStatus::UnderReview);
}

#[test]
fn test_resolve_dispute_with_refund() {
    let env = Env::default();
    env.mock_all_auths();

    let (admin, payment_client, refund_client) = setup_contracts(&env);
    let merchant = Address::generate(&env);
    let customer = Address::generate(&env);
    let operator = Address::generate(&env);

    // Grant operator role
    let settlement_role = Symbol::new(&env, "SETTLEMENT_OPERATOR");
    refund_client.grant_role(&admin, &settlement_role, &operator);

    // Create and verify payment
    let payment_id = String::from_str(&env, "payment_003");
    let amount = 750i128;
    let currency = Symbol::new(&env, "USDC");
    let deposit_address = Address::generate(&env);
    let expires_at = env.ledger().timestamp() + 3600;

    payment_client.create_payment(
        &payment_id,
        &merchant,
        &amount,
        &currency,
        &deposit_address,
        &expires_at,
    );

    let transaction_hash = BytesN::<32>::random(&env);
    payment_client.verify_payment(&payment_id, &transaction_hash, &customer, &amount);

    // Create dispute
    let dispute_reason = String::from_str(&env, "Defective product");
    let evidence = String::from_str(&env, "Video evidence of defect");

    let dispute_id =
        refund_client.create_dispute(&payment_id, &amount, &dispute_reason, &evidence, &customer);

    // Resolve dispute with refund
    let resolution_notes = String::from_str(&env, "Dispute valid, issuing full refund");
    let refund_id =
        refund_client.resolve_dispute_with_refund(&operator, &dispute_id, &resolution_notes);

    // Verify dispute was resolved
    let dispute: Dispute = refund_client.get_dispute(&dispute_id);
    assert_eq!(dispute.status, DisputeStatus::Resolved);
    assert!(dispute.refund_id.is_some());
    assert!(dispute.resolved_at.is_some());

    // Verify refund was created and processed
    let refund: Refund = refund_client.get_refund(&refund_id);
    assert_eq!(refund.payment_id, payment_id);
    assert_eq!(refund.amount, amount);
    assert_eq!(refund.status, RefundStatus::Completed);
}

#[test]
fn test_reject_dispute() {
    let env = Env::default();
    env.mock_all_auths();

    let (admin, payment_client, refund_client) = setup_contracts(&env);
    let merchant = Address::generate(&env);
    let customer = Address::generate(&env);
    let operator = Address::generate(&env);

    // Grant operator role
    let oracle_role = Symbol::new(&env, "ORACLE");
    refund_client.grant_role(&admin, &oracle_role, &operator);

    // Create and verify payment
    let payment_id = String::from_str(&env, "payment_004");
    let amount = 300i128;
    let currency = Symbol::new(&env, "USDC");
    let deposit_address = Address::generate(&env);
    let expires_at = env.ledger().timestamp() + 3600;

    payment_client.create_payment(
        &payment_id,
        &merchant,
        &amount,
        &currency,
        &deposit_address,
        &expires_at,
    );

    let transaction_hash = BytesN::<32>::random(&env);
    payment_client.verify_payment(&payment_id, &transaction_hash, &customer, &amount);

    // Create dispute
    let dispute_reason = String::from_str(&env, "Unauthorized charge");
    let evidence = String::from_str(&env, "No evidence provided");

    let dispute_id =
        refund_client.create_dispute(&payment_id, &amount, &dispute_reason, &evidence, &customer);

    // Reject dispute
    let resolution_notes = String::from_str(&env, "Insufficient evidence, dispute rejected");
    refund_client.reject_dispute(&operator, &dispute_id, &resolution_notes);

    // Verify dispute was rejected
    let dispute: Dispute = refund_client.get_dispute(&dispute_id);
    assert_eq!(dispute.status, DisputeStatus::Rejected);
    assert!(dispute.resolved_at.is_some());
    assert!(dispute.refund_id.is_none());
}

#[test]
fn test_get_payment_disputes() {
    let env = Env::default();
    env.mock_all_auths();

    let (_, payment_client, refund_client) = setup_contracts(&env);
    let merchant = Address::generate(&env);
    let customer = Address::generate(&env);

    // Create and verify payment
    let payment_id = String::from_str(&env, "payment_005");
    let amount = 1200i128;
    let currency = Symbol::new(&env, "USDC");
    let deposit_address = Address::generate(&env);
    let expires_at = env.ledger().timestamp() + 3600;

    payment_client.create_payment(
        &payment_id,
        &merchant,
        &amount,
        &currency,
        &deposit_address,
        &expires_at,
    );

    let transaction_hash = BytesN::<32>::random(&env);
    payment_client.verify_payment(&payment_id, &transaction_hash, &customer, &amount);

    // Create multiple disputes
    let _dispute_id1 = refund_client.create_dispute(
        &payment_id,
        &500i128,
        &String::from_str(&env, "Partial refund needed"),
        &String::from_str(&env, "Evidence 1"),
        &customer,
    );

    let _dispute_id2 = refund_client.create_dispute(
        &payment_id,
        &700i128,
        &String::from_str(&env, "Additional dispute"),
        &String::from_str(&env, "Evidence 2"),
        &customer,
    );

    // Get all disputes for payment
    let disputes = refund_client.get_payment_disputes(&payment_id);
    assert_eq!(disputes.len(), 2);
}

#[test]
#[should_panic(expected = "Error(Contract, #3)")]
fn test_dispute_invalid_amount() {
    let env = Env::default();
    env.mock_all_auths();

    let (_, payment_client, refund_client) = setup_contracts(&env);
    let merchant = Address::generate(&env);
    let customer = Address::generate(&env);

    // Create payment but don't verify it
    let payment_id = String::from_str(&env, "payment_006");
    let amount = 500i128;
    let currency = Symbol::new(&env, "USDC");
    let deposit_address = Address::generate(&env);
    let expires_at = env.ledger().timestamp() + 3600;

    payment_client.create_payment(
        &payment_id,
        &merchant,
        &amount,
        &currency,
        &deposit_address,
        &expires_at,
    );

    // Try to create dispute with invalid amount - should fail
    refund_client.create_dispute(
        &payment_id,
        &0i128, // Invalid amount
        &String::from_str(&env, "Dispute reason"),
        &String::from_str(&env, "Evidence"),
        &customer,
    );
}
