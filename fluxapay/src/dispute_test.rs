use crate::{
    ArbitratorVoteChoice, DataKey, Dispute, DisputeStatus, Error, PaymentProcessor,
    PaymentProcessorClient, Refund, RefundManager, RefundManagerClient, RefundStatus,
};
use soroban_sdk::{
    testutils::{Address as _, BytesN as _, Events as _, Ledger as _},
    token, vec, Address, BytesN, Env, String, Symbol, TryIntoVal,
};

fn setup_contracts(env: &Env) -> (Address, PaymentProcessorClient<'_>, RefundManagerClient<'_>) {
    let payment_processor = env.register(PaymentProcessor, ());
    let refund_manager = env.register(RefundManager, ());

    let refund_client = RefundManagerClient::new(env, &refund_manager);
    let payment_client = PaymentProcessorClient::new(env, &payment_processor);
    let admin = Address::generate(env);
    let token_admin = Address::generate(env);
    let usdc_token = env
        .register_stellar_asset_contract_v2(token_admin)
        .address();
    refund_client.initialize_refund_manager(&admin, &usdc_token);
    let token_admin_client = token::StellarAssetClient::new(env, &usdc_token);
    token_admin_client.mint(&refund_manager, &1_000_000_000_000i128);

    // Existing suite uses free-form evidence; CID enforcement is covered by dedicated tests.
    refund_client.set_require_evidence_cid(&admin, &false);

    payment_client.initialize_payment_processor(&admin);

    (admin, payment_client, refund_client)
}

fn create_payment_args(
    env: &Env,
    payment_id: &String,
    merchant_id: &Address,
    amount: i128,
) -> crate::CreatePaymentArgs {
    crate::CreatePaymentArgs {
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
        idempotency_key: None,
        metadata_hash: None,
        metadata: None,
        fee_waiver_code: None,
        retry_of_payment_id: None,
        payer_muxed_id: None,
        tip_enabled: false,
    }
}

fn setup_open_dispute<'a>(
    env: &'a Env,
    payment_id_text: &str,
) -> (Address, Address, RefundManagerClient<'a>, String) {
    let (admin, payment_client, refund_client) = setup_contracts(env);
    let merchant = Address::generate(env);
    let customer = Address::generate(env);
    let operator = Address::generate(env);
    let payment_id = String::from_str(env, payment_id_text);
    let amount = 1000i128;

    payment_client.grant_role(&admin, &Symbol::new(env, "MERCHANT"), &merchant);
    payment_client.create_payment(&create_payment_args(env, &payment_id, &merchant, amount));

    let oracle = Address::generate(env);
    payment_client.grant_role(&admin, &Symbol::new(env, "ORACLE"), &oracle);
    payment_client.verify_payment(
        &oracle,
        &payment_id,
        &BytesN::from_array(env, &[7u8; 32]),
        &customer,
        &amount,
        &None::<u64>,
    );

    let token_address = env.as_contract(&refund_client.address, || {
        env.storage()
            .persistent()
            .get::<DataKey, Address>(&DataKey::UsdcToken)
            .unwrap()
    });
    let token_admin_client = token::StellarAssetClient::new(env, &token_address);
    token_admin_client.mint(&customer, &100_000);
    token_admin_client.mint(&merchant, &100_000);

    refund_client.register_payment(&payment_id, &merchant, &amount, &Symbol::new(env, "USDC"));
    let dispute_id = refund_client.create_dispute(
        &payment_id,
        &amount,
        &String::from_str(env, "Deadline coverage"),
        &String::from_str(env, "f000000000000000000000000000000000"),
        &customer,
        &vec![env],
    );
    refund_client.grant_role(&admin, &Symbol::new(env, "SETTLEMENT_OPERATOR"), &operator);

    (admin, operator, refund_client, dispute_id)
}

fn has_dispute_event(env: &Env, event_name: &str) -> bool {
    use soroban_sdk::xdr::{ContractEventBody, ScVal};
    env.events().all().iter().any(|event| {
        let ContractEventBody::V0(v0) = &event.body;
        let topics = &v0.topics;
        if topics.len() != 2 {
            return false;
        }
        let namespace: Result<Symbol, _> = ScVal::from(topics[0].clone()).try_into_val(env);
        let name: Result<Symbol, _> = ScVal::from(topics[1].clone()).try_into_val(env);
        matches!(
            (namespace, name),
            (Ok(namespace), Ok(name))
                if namespace == Symbol::new(env, "DISPUTE")
                    && name == Symbol::new(env, event_name)
        )
    })
}

#[test]
fn test_operator_sets_dispute_deadline() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, operator, refund_client, dispute_id) = setup_open_dispute(&env, "deadline_stored");
    let deadline = env.ledger().timestamp() + 3600;

    refund_client.set_dispute_deadline(&operator, &dispute_id, &deadline);

    assert!(has_dispute_event(&env, "DEADLINE_SET"));
    let dispute = refund_client.get_dispute(&dispute_id);
    assert_eq!(dispute.review_deadline, Some(deadline));
}

#[test]
fn test_non_operator_cannot_set_dispute_deadline() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, _, refund_client, dispute_id) = setup_open_dispute(&env, "deadline_unauthorized");
    let non_operator = Address::generate(&env);

    let result = refund_client.try_set_dispute_deadline(
        &non_operator,
        &dispute_id,
        &(env.ledger().timestamp() + 3600),
    );

    assert_eq!(result, Err(Ok(crate::Error::Unauthorized)));
}

#[test]
fn test_past_dispute_deadline_escalates_immediately() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(100);
    let (_, operator, refund_client, dispute_id) = setup_open_dispute(&env, "deadline_past");

    refund_client.set_dispute_deadline(&operator, &dispute_id, &99);

    assert!(has_dispute_event(&env, "ESCALATED"));
    let dispute = refund_client.get_dispute(&dispute_id);
    assert!(dispute.escalated);
}

#[test]
fn test_future_dispute_deadline_does_not_escalate() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, operator, refund_client, dispute_id) = setup_open_dispute(&env, "deadline_future");

    refund_client.set_dispute_deadline(&operator, &dispute_id, &(env.ledger().timestamp() + 3600));

    assert!(!refund_client.get_dispute(&dispute_id).escalated);
}

#[test]
fn test_cannot_set_deadline_on_resolved_dispute() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, operator, refund_client, dispute_id) = setup_open_dispute(&env, "deadline_resolved");
    refund_client.reject_dispute(
        &operator,
        &dispute_id,
        &String::from_str(&env, "Resolved"),
        &String::from_str(&env, "operator-signature"),
    );

    let result = refund_client.try_set_dispute_deadline(
        &operator,
        &dispute_id,
        &(env.ledger().timestamp() + 3600),
    );

    assert_eq!(result, Err(Ok(crate::Error::DisputeAlreadyResolved)));
}

#[test]
fn test_create_dispute() {
    let env = Env::default();
    env.mock_all_auths();

    let (admin, payment_client, refund_client) = setup_contracts(&env);
    let merchant = Address::generate(&env);
    let customer = Address::generate(&env);

    // Create and verify a payment
    let payment_id = String::from_str(&env, "payment_001");
    let amount = 1000i128;

    payment_client.grant_role(&admin, &Symbol::new(&env, "MERCHANT"), &merchant);
    let args = create_payment_args(&env, &payment_id, &merchant, amount);
    payment_client.create_payment(&args);

    // Verify payment
    let transaction_hash = BytesN::from_array(&env, &[0u8; 32]);
    let oracle = Address::generate(&env);
    payment_client.grant_role(&admin, &Symbol::new(&env, "ORACLE"), &oracle);
    payment_client.verify_payment(
        &oracle,
        &payment_id,
        &transaction_hash,
        &customer,
        &amount,
        &None::<u64>,
    );

    // Register payment with refund manager for amount validation
    refund_client.register_payment(&payment_id, &merchant, &amount, &Symbol::new(&env, "USDC"));

    // Create dispute
    let dispute_reason = String::from_str(&env, "Product not received");
    let evidence = String::from_str(&env, "Tracking shows delivery failed");

    let dispute_id = refund_client.create_dispute(
        &payment_id,
        &amount,
        &dispute_reason,
        &evidence,
        &customer,
        &vec![&env],
    );

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

    payment_client.grant_role(&admin, &Symbol::new(&env, "MERCHANT"), &merchant);
    let args = create_payment_args(&env, &payment_id, &merchant, amount);
    payment_client.create_payment(&args);

    let transaction_hash = BytesN::from_array(&env, &[0u8; 32]);
    let oracle = Address::generate(&env);
    payment_client.grant_role(&admin, &Symbol::new(&env, "ORACLE"), &oracle);
    payment_client.verify_payment(
        &oracle,
        &payment_id,
        &transaction_hash,
        &customer,
        &amount,
        &None::<u64>,
    );

    // Register payment with refund manager for amount validation
    refund_client.register_payment(&payment_id, &merchant, &amount, &Symbol::new(&env, "USDC"));

    // Create dispute
    let dispute_reason = String::from_str(&env, "Wrong item received");
    let evidence = String::from_str(&env, "Photo evidence attached");

    let dispute_id = refund_client.create_dispute(
        &payment_id,
        &amount,
        &dispute_reason,
        &evidence,
        &customer,
        &vec![&env],
    );

    // Review dispute
    refund_client.review_dispute(&operator, &dispute_id);

    // Verify dispute status changed
    let dispute: Dispute = refund_client.get_dispute(&dispute_id);
    assert_eq!(dispute.status, DisputeStatus::UnderReview);
}

#[test]
fn test_check_dispute_deadline_escalates_once() {
    let env = Env::default();
    env.mock_all_auths();

    let (admin, payment_client, refund_client) = setup_contracts(&env);
    let merchant = Address::generate(&env);
    let customer = Address::generate(&env);
    let operator = Address::generate(&env);

    refund_client.grant_role(&admin, &Symbol::new(&env, "SETTLEMENT_OPERATOR"), &operator);

    let payment_id = String::from_str(&env, "payment_deadline_001");
    let amount = 750i128;

    payment_client.grant_role(&admin, &Symbol::new(&env, "MERCHANT"), &merchant);
    let args = create_payment_args(&env, &payment_id, &merchant, amount);
    payment_client.create_payment(&args);

    let transaction_hash = BytesN::from_array(&env, &[0u8; 32]);
    let oracle = Address::generate(&env);
    payment_client.grant_role(&admin, &Symbol::new(&env, "ORACLE"), &oracle);
    payment_client.verify_payment(
        &oracle,
        &payment_id,
        &transaction_hash,
        &customer,
        &amount,
        &None::<u64>,
    );

    refund_client.register_payment(&payment_id, &merchant, &amount, &Symbol::new(&env, "USDC"));

    let dispute_id = refund_client.create_dispute(
        &payment_id,
        &amount,
        &String::from_str(&env, "Deadline test"),
        &String::from_str(&env, "Evidence"),
        &customer,
        &vec![&env],
    );

    let now = env.ledger().timestamp();
    refund_client.set_dispute_deadline(&operator, &dispute_id, &(now + 10));

    let events_after_deadline = env.events().all().len();

    refund_client.check_dispute_deadline(&dispute_id);
    let dispute = refund_client.get_dispute(&dispute_id);
    assert!(!dispute.escalated);
    assert_eq!(env.events().all().len(), events_after_deadline);

    env.ledger().set_timestamp(now + 11);
    refund_client.check_dispute_deadline(&dispute_id);

    let escalated = refund_client.get_dispute(&dispute_id);
    assert!(escalated.escalated);
    assert_eq!(env.events().all().len(), events_after_deadline + 1);

    refund_client.check_dispute_deadline(&dispute_id);
    assert_eq!(env.events().all().len(), events_after_deadline + 1);
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

    payment_client.grant_role(&admin, &Symbol::new(&env, "MERCHANT"), &merchant);
    let args = create_payment_args(&env, &payment_id, &merchant, amount);
    payment_client.create_payment(&args);

    let transaction_hash = BytesN::from_array(&env, &[0u8; 32]);
    let oracle = Address::generate(&env);
    payment_client.grant_role(&admin, &Symbol::new(&env, "ORACLE"), &oracle);
    payment_client.verify_payment(
        &oracle,
        &payment_id,
        &transaction_hash,
        &customer,
        &amount,
        &None::<u64>,
    );

    // Register payment with refund manager for amount validation
    refund_client.register_payment(&payment_id, &merchant, &amount, &Symbol::new(&env, "USDC"));

    // Create dispute
    let dispute_reason = String::from_str(&env, "Defective product");
    let evidence = String::from_str(&env, "Video evidence of defect");

    let dispute_id = refund_client.create_dispute(
        &payment_id,
        &amount,
        &dispute_reason,
        &evidence,
        &customer,
        &vec![&env],
    );

    // Resolve dispute with refund
    let resolution_notes = String::from_str(&env, "Dispute valid, issuing full refund");
    let operator_sig = String::from_str(&env, "base64sig==");
    let refund_id = refund_client.resolve_dispute_with_refund(
        &operator,
        &dispute_id,
        &resolution_notes,
        &operator_sig,
    );

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

    payment_client.grant_role(&admin, &Symbol::new(&env, "MERCHANT"), &merchant);
    let args = create_payment_args(&env, &payment_id, &merchant, amount);
    payment_client.create_payment(&args);

    let transaction_hash = BytesN::from_array(&env, &[0u8; 32]);
    let oracle = Address::generate(&env);
    payment_client.grant_role(&admin, &Symbol::new(&env, "ORACLE"), &oracle);
    payment_client.verify_payment(
        &oracle,
        &payment_id,
        &transaction_hash,
        &customer,
        &amount,
        &None::<u64>,
    );

    // Register payment with refund manager for amount validation
    refund_client.register_payment(&payment_id, &merchant, &amount, &Symbol::new(&env, "USDC"));

    // Create dispute
    let dispute_reason = String::from_str(&env, "Unauthorized charge");
    let evidence = String::from_str(&env, "No evidence provided");

    let dispute_id = refund_client.create_dispute(
        &payment_id,
        &amount,
        &dispute_reason,
        &evidence,
        &customer,
        &vec![&env],
    );

    // Reject dispute
    let resolution_notes = String::from_str(&env, "Insufficient evidence, dispute rejected");
    let operator_sig = String::from_str(&env, "base64sig==");
    refund_client.reject_dispute(&operator, &dispute_id, &resolution_notes, &operator_sig);

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

    let (admin, payment_client, refund_client) = setup_contracts(&env);
    let merchant = Address::generate(&env);
    let customer = Address::generate(&env);

    // Create and verify payment
    let payment_id = String::from_str(&env, "payment_005");
    let amount = 1200i128;

    payment_client.grant_role(&admin, &Symbol::new(&env, "MERCHANT"), &merchant);
    let args = create_payment_args(&env, &payment_id, &merchant, amount);
    payment_client.create_payment(&args);

    let transaction_hash = BytesN::from_array(&env, &[0u8; 32]);
    let oracle = Address::generate(&env);
    payment_client.grant_role(&admin, &Symbol::new(&env, "ORACLE"), &oracle);
    payment_client.verify_payment(
        &oracle,
        &payment_id,
        &transaction_hash,
        &customer,
        &amount,
        &None::<u64>,
    );

    // Register payment with refund manager for amount validation
    refund_client.register_payment(&payment_id, &merchant, &amount, &Symbol::new(&env, "USDC"));

    // Create multiple disputes
    let _dispute_id1 = refund_client.create_dispute(
        &payment_id,
        &500i128,
        &String::from_str(&env, "Partial refund needed"),
        &String::from_str(&env, "Evidence 1"),
        &customer,
        &vec![&env],
    );

    let _dispute_id2 = refund_client.create_dispute(
        &payment_id,
        &700i128,
        &String::from_str(&env, "Additional dispute"),
        &String::from_str(&env, "Evidence 2"),
        &customer,
        &vec![&env],
    );

    // Get all disputes for payment
    let disputes = refund_client.get_payment_disputes(&payment_id);
    assert_eq!(disputes.len(), 2);
}

/// Issue #575: Filter payment disputes by DisputeStatus.
#[test]
fn test_get_payment_disputes_by_status() {
    let env = Env::default();
    env.mock_all_auths();

    let (admin, payment_client, refund_client) = setup_contracts(&env);
    let merchant = Address::generate(&env);
    let customer = Address::generate(&env);

    let payment_id = String::from_str(&env, "payment_status_filter");
    let amount = 2000i128;

    payment_client.grant_role(&admin, &Symbol::new(&env, "MERCHANT"), &merchant);
    let args = create_payment_args(&env, &payment_id, &merchant, amount);
    payment_client.create_payment(&args);

    let transaction_hash = BytesN::from_array(&env, &[0u8; 32]);
    let oracle = Address::generate(&env);
    payment_client.grant_role(&admin, &Symbol::new(&env, "ORACLE"), &oracle);
    payment_client.verify_payment(
        &oracle,
        &payment_id,
        &transaction_hash,
        &customer,
        &amount,
        &None::<u64>,
    );

    refund_client.register_payment(&payment_id, &merchant, &amount, &Symbol::new(&env, "USDC"));

    // Create dispute 1
    let dispute_id1 = refund_client.create_dispute(
        &payment_id,
        &500i128,
        &String::from_str(&env, "First dispute"),
        &String::from_str(&env, "Evidence 1"),
        &customer,
        &vec![&env],
    );

    // Create dispute 2
    let dispute_id2 = refund_client.create_dispute(
        &payment_id,
        &700i128,
        &String::from_str(&env, "Second dispute"),
        &String::from_str(&env, "Evidence 2"),
        &customer,
        &vec![&env],
    );

    // Initial status: both are Open
    let open_disputes =
        refund_client.get_payment_disputes_by_status(&payment_id, &DisputeStatus::Open);
    assert_eq!(open_disputes.len(), 2);

    let review_disputes =
        refund_client.get_payment_disputes_by_status(&payment_id, &DisputeStatus::UnderReview);
    assert_eq!(review_disputes.len(), 0);

    // Transition dispute 1 to UnderReview
    let operator = Address::generate(&env);
    refund_client.grant_role(&admin, &Symbol::new(&env, "SETTLEMENT_OPERATOR"), &operator);
    refund_client.review_dispute(&operator, &dispute_id1);

    // Now dispute 1 is UnderReview, dispute 2 is Open
    let open_after =
        refund_client.get_payment_disputes_by_status(&payment_id, &DisputeStatus::Open);
    assert_eq!(open_after.len(), 1);
    assert_eq!(open_after.get(0).unwrap().id, dispute_id2);

    let review_after =
        refund_client.get_payment_disputes_by_status(&payment_id, &DisputeStatus::UnderReview);
    assert_eq!(review_after.len(), 1);
    assert_eq!(review_after.get(0).unwrap().id, dispute_id1);

    // Status with no matches returns empty vec
    let resolved =
        refund_client.get_payment_disputes_by_status(&payment_id, &DisputeStatus::Resolved);
    assert_eq!(resolved.len(), 0);

    // Backward-compatible original method still returns all
    let all = refund_client.get_payment_disputes(&payment_id);
    assert_eq!(all.len(), 2);
}

#[test]
#[should_panic(expected = "Error(Contract, #406)")]
fn test_dispute_invalid_amount() {
    let env = Env::default();
    env.mock_all_auths();

    let (admin, payment_client, refund_client) = setup_contracts(&env);
    let merchant = Address::generate(&env);
    let customer = Address::generate(&env);

    // Create payment but don't verify it
    let payment_id = String::from_str(&env, "payment_006");
    let amount = 500i128;

    payment_client.grant_role(&admin, &Symbol::new(&env, "MERCHANT"), &merchant);
    let args = create_payment_args(&env, &payment_id, &merchant, amount);
    payment_client.create_payment(&args);

    // Try to create dispute with invalid amount - should fail
    refund_client.create_dispute(
        &payment_id,
        &0i128, // Invalid amount
        &String::from_str(&env, "Dispute reason"),
        &String::from_str(&env, "Evidence"),
        &customer,
        &vec![&env],
    );
}

#[test]
fn test_resolve_dispute_with_only_operator_auth() {
    let env = Env::default();
    env.mock_all_auths();

    let (admin, payment_client, refund_client) = setup_contracts(&env);
    let merchant = Address::generate(&env);
    let customer = Address::generate(&env);
    let operator = Address::generate(&env);

    refund_client.grant_role(&admin, &Symbol::new(&env, "SETTLEMENT_OPERATOR"), &operator);

    let payment_id = String::from_str(&env, "pay_auth_test");
    let amount = 500i128;
    payment_client.grant_role(&admin, &Symbol::new(&env, "MERCHANT"), &merchant);
    let args = create_payment_args(&env, &payment_id, &merchant, amount);
    payment_client.create_payment(&args);

    let oracle = Address::generate(&env);
    payment_client.grant_role(&admin, &Symbol::new(&env, "ORACLE"), &oracle);
    let tx_hash = BytesN::<32>::random(&env);
    payment_client.verify_payment(
        &oracle,
        &payment_id,
        &tx_hash,
        &customer,
        &amount,
        &None::<u64>,
    );

    // Register payment with refund manager for amount validation
    refund_client.register_payment(&payment_id, &merchant, &amount, &Symbol::new(&env, "USDC"));

    let dispute_id = refund_client.create_dispute(
        &payment_id,
        &amount,
        &String::from_str(&env, "Item not received"),
        &String::from_str(&env, "Tracking shows lost"),
        &customer,
        &vec![&env],
    );

    // Resolve — the internal create_refund_internal must NOT call
    // disputer.require_auth(), so only the operator's auth is needed.
    let refund_id = refund_client.resolve_dispute_with_refund(
        &operator,
        &dispute_id,
        &String::from_str(&env, "Refund approved"),
        &String::from_str(&env, "base64sig=="),
    );

    // Verify the auth invocations: only the operator should have been required
    // at the top level (not the disputer/customer).
    let auths = env.auths();
    let operator_auth_count = auths.iter().filter(|(addr, _)| addr == &operator).count();
    assert!(operator_auth_count >= 1, "operator auth must be present");

    // The disputer (customer) must NOT appear as a top-level auth requirement.
    let customer_top_level = auths.iter().any(|(addr, _)| addr == &customer);
    assert!(
        !customer_top_level,
        "disputer must not be required as top-level auth in resolve_dispute_with_refund"
    );

    let dispute = refund_client.get_dispute(&dispute_id);
    assert_eq!(dispute.status, DisputeStatus::Resolved);

    let refund = refund_client.get_refund(&refund_id);
    assert_eq!(refund.status, RefundStatus::Completed);
}

// ─── ARBITRATOR-role vote_dispute ──────────────────────────────────────────────

fn setup_dispute_under_review(
    env: &Env,
    admin: &Address,
    payment_client: &PaymentProcessorClient,
    refund_client: &RefundManagerClient,
    payment_id: &String,
    amount: i128,
) -> String {
    let merchant = Address::generate(env);
    let customer = Address::generate(env);
    let operator = Address::generate(env);

    refund_client.grant_role(admin, &Symbol::new(env, "SETTLEMENT_OPERATOR"), &operator);
    payment_client.grant_role(admin, &Symbol::new(env, "MERCHANT"), &merchant);
    let args = create_payment_args(env, payment_id, &merchant, amount);
    payment_client.create_payment(&args);

    let oracle = Address::generate(env);
    payment_client.grant_role(admin, &Symbol::new(env, "ORACLE"), &oracle);
    let tx_hash = BytesN::<32>::random(env);
    payment_client.verify_payment(
        &oracle,
        payment_id,
        &tx_hash,
        &customer,
        &amount,
        &None::<u64>,
    );

    refund_client.register_payment(payment_id, &merchant, &amount, &Symbol::new(env, "USDC"));

    let dispute_id = refund_client.create_dispute(
        payment_id,
        &amount,
        &String::from_str(env, "Item not as described"),
        &String::from_str(env, "Photo evidence"),
        &customer,
    );
    refund_client.review_dispute(&operator, &dispute_id);
    dispute_id
}

#[test]
fn test_vote_dispute_auto_resolves_on_three_approvals() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, payment_client, refund_client) = setup_contracts(&env);

    let payment_id = String::from_str(&env, "payment_vote_approve");
    let dispute_id = setup_dispute_under_review(
        &env,
        &admin,
        &payment_client,
        &refund_client,
        &payment_id,
        400i128,
    );

    let arbitrator_role = Symbol::new(&env, "ARBITRATOR");
    let arb1 = Address::generate(&env);
    let arb2 = Address::generate(&env);
    let arb3 = Address::generate(&env);
    refund_client.grant_role(&admin, &arbitrator_role, &arb1);
    refund_client.grant_role(&admin, &arbitrator_role, &arb2);
    refund_client.grant_role(&admin, &arbitrator_role, &arb3);

    refund_client.vote_dispute(&arb1, &dispute_id, &ArbitratorVoteChoice::Approve);
    refund_client.vote_dispute(&arb2, &dispute_id, &ArbitratorVoteChoice::Approve);

    // Only 2 of 3 votes in — dispute must still be under review.
    let dispute = refund_client.get_dispute(&dispute_id);
    assert_eq!(dispute.status, DisputeStatus::UnderReview);

    refund_client.vote_dispute(&arb3, &dispute_id, &ArbitratorVoteChoice::Approve);

    let dispute = refund_client.get_dispute(&dispute_id);
    assert_eq!(dispute.status, DisputeStatus::Resolved);
    assert!(dispute.resolved_at.is_some());
}

#[test]
fn test_vote_dispute_auto_rejects_on_three_rejections() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, payment_client, refund_client) = setup_contracts(&env);

    let payment_id = String::from_str(&env, "payment_vote_reject");
    let dispute_id = setup_dispute_under_review(
        &env,
        &admin,
        &payment_client,
        &refund_client,
        &payment_id,
        400i128,
    );

    let arbitrator_role = Symbol::new(&env, "ARBITRATOR");
    let arb1 = Address::generate(&env);
    let arb2 = Address::generate(&env);
    let arb3 = Address::generate(&env);
    refund_client.grant_role(&admin, &arbitrator_role, &arb1);
    refund_client.grant_role(&admin, &arbitrator_role, &arb2);
    refund_client.grant_role(&admin, &arbitrator_role, &arb3);

    refund_client.vote_dispute(&arb1, &dispute_id, &ArbitratorVoteChoice::Reject);
    refund_client.vote_dispute(&arb2, &dispute_id, &ArbitratorVoteChoice::Reject);
    refund_client.vote_dispute(&arb3, &dispute_id, &ArbitratorVoteChoice::Reject);

    let dispute = refund_client.get_dispute(&dispute_id);
    assert_eq!(dispute.status, DisputeStatus::Rejected);
    assert!(dispute.resolved_at.is_some());
}

#[test]
fn test_vote_dispute_duplicate_vote_blocked() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, payment_client, refund_client) = setup_contracts(&env);

    let payment_id = String::from_str(&env, "payment_vote_dup");
    let dispute_id = setup_dispute_under_review(
        &env,
        &admin,
        &payment_client,
        &refund_client,
        &payment_id,
        400i128,
    );

    let arbitrator_role = Symbol::new(&env, "ARBITRATOR");
    let arb1 = Address::generate(&env);
    refund_client.grant_role(&admin, &arbitrator_role, &arb1);

    refund_client.vote_dispute(&arb1, &dispute_id, &ArbitratorVoteChoice::Approve);
    let err = refund_client.try_vote_dispute(&arb1, &dispute_id, &ArbitratorVoteChoice::Approve);
    assert_eq!(err, Err(Ok(Error::AlreadyVoted)));
}

#[test]
fn test_vote_dispute_non_arbitrator_blocked() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, payment_client, refund_client) = setup(&env);

    let payment_id = String::from_str(&env, "payment_vote_non_arb");
    let dispute_id = setup_dispute_under_review(
        &env,
        &admin,
        &payment_client,
        &refund_client,
        &payment_id,
        400i128,
    );

    let stranger = Address::generate(&env);
    let err =
        refund_client.try_vote_dispute(&stranger, &dispute_id, &ArbitratorVoteChoice::Approve);
    assert_eq!(err, Err(Ok(Error::Unauthorized)));
}

const VALID_CID_V0: &str = "QmYwAPJzv5CZsnA625s3Xf2nemtYgPpHdWEz79ojWnPbdG";
const VALID_CID_V1: &str = "bafybeigdyrzt5sfp7udm7hu76uh7y26nf3efuylqabf3oclgtqy55fbzdi";

fn setup_confirmed_payment_for_dispute<'a>(
    env: &'a Env,
    payment_id_text: &str,
    amount: i128,
) -> (
    Address,
    Address,
    Address,
    PaymentProcessorClient<'a>,
    RefundManagerClient<'a>,
    String,
) {
    let (admin, payment_client, refund_client) = setup_contracts(env);
    let merchant = Address::generate(env);
    let customer = Address::generate(env);
    let payment_id = String::from_str(env, payment_id_text);

    payment_client.grant_role(&admin, &Symbol::new(env, "MERCHANT"), &merchant);
    payment_client.create_payment(&create_payment_args(env, &payment_id, &merchant, amount));

    let oracle = Address::generate(env);
    payment_client.grant_role(&admin, &Symbol::new(env, "ORACLE"), &oracle);
    payment_client.verify_payment(
        &oracle,
        &payment_id,
        &BytesN::from_array(env, &[7u8; 32]),
        &customer,
        &amount,
        &None::<u64>,
    );

    let token_address = env.as_contract(&refund_client.address, || {
        env.storage()
            .persistent()
            .get::<DataKey, Address>(&DataKey::UsdcToken)
            .unwrap()
    });
    let token_admin_client = token::StellarAssetClient::new(env, &token_address);
    token_admin_client.mint(&customer, &1_000_000);
    token_admin_client.mint(&merchant, &1_000_000);

    refund_client.register_payment(&payment_id, &merchant, &amount, &Symbol::new(env, "USDC"));

    (
        admin,
        merchant,
        customer,
        payment_client,
        refund_client,
        payment_id,
    )
}

#[test]
fn test_dispute_rate_limit_sixth_open_rejected() {
    let env = Env::default();
    env.mock_all_auths();

    let (admin, _merchant, customer, payment_client, refund_client, _) =
        setup_confirmed_payment_for_dispute(&env, "rl_base", 100i128);
    // Raise global hourly so we only hit the per-payer open cap.
    refund_client.set_dispute_rate_limits(&admin, &5u32, &1000u32);

    for i in 0..5u32 {
        let pid = match i {
            0 => String::from_str(&env, "rl_pay_0"),
            1 => String::from_str(&env, "rl_pay_1"),
            2 => String::from_str(&env, "rl_pay_2"),
            3 => String::from_str(&env, "rl_pay_3"),
            _ => String::from_str(&env, "rl_pay_4"),
        };
        let amount = 100i128;
        let merchant = Address::generate(&env);
        payment_client.grant_role(&admin, &Symbol::new(&env, "MERCHANT"), &merchant);
        payment_client.create_payment(&create_payment_args(&env, &pid, &merchant, amount));
        let oracle = Address::generate(&env);
        payment_client.grant_role(&admin, &Symbol::new(&env, "ORACLE"), &oracle);
        payment_client.verify_payment(
            &oracle,
            &pid,
            &BytesN::from_array(&env, &[(i + 1) as u8; 32]),
            &customer,
            &amount,
        );
        let token_address = env.as_contract(&refund_client.address, || {
            env.storage()
                .persistent()
                .get::<DataKey, Address>(&DataKey::UsdcToken)
                .unwrap()
        });
        token::StellarAssetClient::new(&env, &token_address).mint(&merchant, &100_000);
        refund_client.register_payment(&pid, &merchant, &amount, &Symbol::new(&env, "USDC"));
        refund_client.create_dispute(
            &pid,
            &amount,
            &String::from_str(&env, "reason"),
            &String::from_str(&env, VALID_CID_V0),
            &customer,
            &vec![&env],
        );
    }

    let pid6 = String::from_str(&env, "rl_pay_5");
    let amount = 100i128;
    let merchant = Address::generate(&env);
    payment_client.grant_role(&admin, &Symbol::new(&env, "MERCHANT"), &merchant);
    payment_client.create_payment(&create_payment_args(&env, &pid6, &merchant, amount));
    let oracle = Address::generate(&env);
    payment_client.grant_role(&admin, &Symbol::new(&env, "ORACLE"), &oracle);
    payment_client.verify_payment(
        &oracle,
        &pid6,
        &BytesN::from_array(&env, &[9u8; 32]),
        &customer,
        &amount,
        &None::<u64>,
    );
    let token_address = env.as_contract(&refund_client.address, || {
        env.storage()
            .persistent()
            .get::<DataKey, Address>(&DataKey::UsdcToken)
            .unwrap()
    });
    token::StellarAssetClient::new(&env, &token_address).mint(&merchant, &100_000);
    refund_client.register_payment(&pid6, &merchant, &amount, &Symbol::new(&env, "USDC"));

    let result = refund_client.try_create_dispute(
        &pid6,
        &amount,
        &String::from_str(&env, "reason"),
        &String::from_str(&env, VALID_CID_V0),
        &customer,
        &vec![&env],
    );
    assert!(result.is_err());
}

#[test]
fn test_dispute_global_hourly_rate_limit() {
    let env = Env::default();
    env.mock_all_auths();

    let (admin, _m, customer, payment_client, refund_client, _) =
        setup_confirmed_payment_for_dispute(&env, "g_base", 100i128);
    refund_client.set_dispute_rate_limits(&admin, &50u32, &2u32);

    for i in 0..2u32 {
        let pid = if i == 0 {
            String::from_str(&env, "g_pay_0")
        } else {
            String::from_str(&env, "g_pay_1")
        };
        let amount = 100i128;
        let merchant = Address::generate(&env);
        payment_client.grant_role(&admin, &Symbol::new(&env, "MERCHANT"), &merchant);
        payment_client.create_payment(&create_payment_args(&env, &pid, &merchant, amount));
        let oracle = Address::generate(&env);
        payment_client.grant_role(&admin, &Symbol::new(&env, "ORACLE"), &oracle);
        payment_client.verify_payment(
            &oracle,
            &pid,
            &BytesN::from_array(&env, &[(i + 1) as u8; 32]),
            &customer,
            &amount,
        );
        let token_address = env.as_contract(&refund_client.address, || {
            env.storage()
                .persistent()
                .get::<DataKey, Address>(&DataKey::UsdcToken)
                .unwrap()
        });
        token::StellarAssetClient::new(&env, &token_address).mint(&merchant, &100_000);
        refund_client.register_payment(&pid, &merchant, &amount, &Symbol::new(&env, "USDC"));
        refund_client.create_dispute(
            &pid,
            &amount,
            &String::from_str(&env, "reason"),
            &String::from_str(&env, VALID_CID_V1),
            &customer,
            &vec![&env],
        );
    }

    let pid3 = String::from_str(&env, "g_pay_2");
    let amount = 100i128;
    let merchant = Address::generate(&env);
    payment_client.grant_role(&admin, &Symbol::new(&env, "MERCHANT"), &merchant);
    payment_client.create_payment(&create_payment_args(&env, &pid3, &merchant, amount));
    let oracle = Address::generate(&env);
    payment_client.grant_role(&admin, &Symbol::new(&env, "ORACLE"), &oracle);
    payment_client.verify_payment(
        &oracle,
        &pid3,
        &BytesN::from_array(&env, &[3u8; 32]),
        &customer,
        &amount,
        &None::<u64>,
    );
    let token_address = env.as_contract(&refund_client.address, || {
        env.storage()
            .persistent()
            .get::<DataKey, Address>(&DataKey::UsdcToken)
            .unwrap()
    });
    token::StellarAssetClient::new(&env, &token_address).mint(&merchant, &100_000);
    refund_client.register_payment(&pid3, &merchant, &amount, &Symbol::new(&env, "USDC"));

    let result = refund_client.try_create_dispute(
        &pid3,
        &amount,
        &String::from_str(&env, "reason"),
        &String::from_str(&env, VALID_CID_V1),
        &customer,
        &vec![&env],
    );
    assert!(result.is_err());
}

#[test]
fn test_create_dispute_evidence_valid_cid_v0() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, _, customer, _, refund_client, payment_id) =
        setup_confirmed_payment_for_dispute(&env, "ev_v0", 1000i128);
    refund_client.set_require_evidence_cid(&admin, &true);

    let dispute_id = refund_client.create_dispute(
        &payment_id,
        &500i128,
        &String::from_str(&env, "reason"),
        &String::from_str(&env, VALID_CID_V0),
        &customer,
        &vec![&env],
    );
    assert!(!dispute_id.is_empty());
}

#[test]
fn test_create_dispute_evidence_valid_cid_v1() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, _, customer, _, refund_client, payment_id) =
        setup_confirmed_payment_for_dispute(&env, "ev_v1", 1000i128);
    refund_client.set_require_evidence_cid(&admin, &true);

    let dispute_id = refund_client.create_dispute(
        &payment_id,
        &500i128,
        &String::from_str(&env, "reason"),
        &String::from_str(&env, VALID_CID_V1),
        &customer,
        &vec![&env],
    );
    assert!(!dispute_id.is_empty());
}

#[test]
fn test_create_dispute_evidence_invalid_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, _, customer, _, refund_client, payment_id) =
        setup_confirmed_payment_for_dispute(&env, "ev_bad", 1000i128);
    refund_client.set_require_evidence_cid(&admin, &true);

    let result = refund_client.try_create_dispute(
        &payment_id,
        &500i128,
        &String::from_str(&env, "reason"),
        &String::from_str(&env, "not-a-cid"),
        &customer,
        &vec![&env],
    );
    assert!(result.is_err());
}

#[test]
fn test_create_dispute_evidence_empty_allowed() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, _, customer, _, refund_client, payment_id) =
        setup_confirmed_payment_for_dispute(&env, "ev_empty", 1000i128);
    refund_client.set_require_evidence_cid(&admin, &true);

    let dispute_id = refund_client.create_dispute(
        &payment_id,
        &500i128,
        &String::from_str(&env, "reason"),
        &String::from_str(&env, ""),
        &customer,
        &vec![&env],
    );
    assert!(!dispute_id.is_empty());
}

#[test]
fn test_batch_create_disputes_full_success() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, payment_client, refund_client) = setup_contracts(&env);

    let mut batch = soroban_sdk::vec![&env];
    for i in 0..3u32 {
        let pid = crate::utils::format_id(&env, "batch_ok_", i as u64);
        let (merchant, customer, payment_id) = setup_confirmed_payment_for_dispute(
            &env,
            &payment_client,
            &refund_client,
            &admin,
            &pid,
            1_000,
        );
        let _ = merchant;
        batch.push_back(crate::CreateDisputeArgs {
            payment_id,
            amount: 500i128,
            reason: String::from_str(&env, "bulk"),
            evidence: valid_evidence(&env),
            disputer: customer,
            payout_splits: vec![&env],
        });
    }

    let results = refund_client.batch_create_disputes(&batch, &20u32);
    assert_eq!(results.len(), 3);
    for r in results.iter() {
        match r {
            crate::DisputeBatchItemResult::Ok(_) => {}
            crate::DisputeBatchItemResult::Err(code) => panic!("unexpected err {code}"),
        }
    }

    assert!(env.events().all().iter().any(|(_, topics, _)| {
        if topics.len() < 2 {
            return false;
        }
        let ns: Result<Symbol, _> = topics.get(0).unwrap().try_into_val(&env);
        let name: Result<Symbol, _> = topics.get(1).unwrap().try_into_val(&env);
        matches!(
            (ns, name),
            (Ok(a), Ok(b))
                if a == Symbol::new(&env, "DISPUTE") && b == Symbol::new(&env, "BATCH_CREATED")
        )
    }));
}

#[test]
fn test_batch_create_disputes_mixed_success_fail() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, payment_client, refund_client) = setup_contracts(&env);

    let payment_id = String::from_str(&env, "payment_vote_unauth");
    let dispute_id = setup_dispute_under_review(
        &env,
        &admin,
        &payment_client,
        &refund_client,
        &payment_id,
        400i128,
    );

    let impostor = Address::generate(&env);
    let err =
        refund_client.try_vote_dispute(&impostor, &dispute_id, &ArbitratorVoteChoice::Approve);
    assert_eq!(err, Err(Ok(Error::Unauthorized)));
    let (_m1, c1, pay1) = setup_confirmed_payment_for_dispute(
        &env,
        &payment_client,
        &refund_client,
        &admin,
        "batch_mix_ok",
        1_000,
    );
    let (_m2, c2, pay2) = setup_confirmed_payment_for_dispute(
        &env,
        &payment_client,
        &refund_client,
        &admin,
        "batch_mix_bad",
        1_000,
    );

    let batch = soroban_sdk::vec![
        &env,
        crate::CreateDisputeArgs {
            payment_id: pay1,
            amount: 500i128,
            reason: String::from_str(&env, "ok"),
            evidence: valid_evidence(&env),
            disputer: c1,
            payout_splits: vec![&env],
        },
        crate::CreateDisputeArgs {
            payment_id: pay2,
            amount: 0i128, // invalid → fail
            reason: String::from_str(&env, "bad"),
            evidence: valid_evidence(&env),
            disputer: c2,
            payout_splits: vec![&env],
        },
        crate::CreateDisputeArgs {
            payment_id: String::from_str(&env, "missing_payment_xyz"),
            amount: 100i128,
            reason: String::from_str(&env, "missing"),
            evidence: valid_evidence(&env),
            disputer: Address::generate(&env),
            payout_splits: vec![&env],
        },
    ];

    let results = refund_client.batch_create_disputes(&batch, &20u32);
    assert_eq!(results.len(), 3);
    assert!(matches!(
        results.get(0).unwrap(),
        crate::DisputeBatchItemResult::Ok(_)
    ));
    assert!(matches!(
        results.get(1).unwrap(),
        crate::DisputeBatchItemResult::Err(_)
    ));
    assert!(matches!(
        results.get(2).unwrap(),
        crate::DisputeBatchItemResult::Err(_)
    ));
}

#[test]
fn test_batch_create_disputes_full_failure() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, _payment_client, refund_client) = setup_contracts(&env);

    let batch = soroban_sdk::vec![
        &env,
        crate::CreateDisputeArgs {
            payment_id: String::from_str(&env, "nope_1"),
            amount: 100i128,
            reason: String::from_str(&env, "x"),
            evidence: valid_evidence(&env),
            disputer: Address::generate(&env),
            payout_splits: vec![&env],
        },
        crate::CreateDisputeArgs {
            payment_id: String::from_str(&env, "nope_2"),
            amount: -1i128,
            reason: String::from_str(&env, "y"),
            evidence: valid_evidence(&env),
            disputer: Address::generate(&env),
            payout_splits: vec![&env],
        },
    ];

    let results = refund_client.batch_create_disputes(&batch, &20u32);
    assert_eq!(results.len(), 2);
    assert!(matches!(
        results.get(0).unwrap(),
        crate::DisputeBatchItemResult::Err(_)
    ));
    assert!(matches!(
        results.get(1).unwrap(),
        crate::DisputeBatchItemResult::Err(_)
    ));
}

#[test]
fn test_batch_create_disputes_rejects_oversized() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, _payment_client, refund_client) = setup_contracts(&env);

    let mut batch = soroban_sdk::vec![&env];
    for i in 0..21u32 {
        batch.push_back(crate::CreateDisputeArgs {
            payment_id: crate::utils::format_id(&env, "p", i as u64),
            amount: 1i128,
            reason: String::from_str(&env, "r"),
            evidence: valid_evidence(&env),
            disputer: Address::generate(&env),
            payout_splits: vec![&env],
        });
    }

    let result = refund_client.try_batch_create_disputes(&batch, &20u32);
    assert_eq!(result, Err(Ok(crate::Error::BatchTooLarge)));

    let result2 = refund_client.try_batch_create_disputes(&soroban_sdk::vec![&env], &21u32);
    assert_eq!(result2, Err(Ok(crate::Error::BatchTooLarge)));
}

/// Issue #833: open_dispute cross-calls MerchantRegistry.increment_merchant_dispute_count
/// so get_merchant_dispute_count returns a non-zero value.
#[test]
fn test_open_dispute_increments_merchant_registry_dispute_count() {
    let env = Env::default();
    env.mock_all_auths();

    let (admin, payment_client, refund_client) = setup_contracts(&env);

    let registry_id = env.register(crate::merchant_registry::MerchantRegistry, ());
    let registry_client = crate::merchant_registry::MerchantRegistryClient::new(&env, &registry_id);
    registry_client.initialize(&admin);
    registry_client.set_refund_manager_address(&admin, &refund_client.address);

    // Wire RefundManager → MerchantRegistry for the cross-call.
    env.as_contract(&refund_client.address, || {
        env.storage()
            .persistent()
            .set(&DataKey::MerchantRegistryAddress, &registry_id);
    });

    let merchant = Address::generate(&env);
    let customer = Address::generate(&env);
    let payment_id = String::from_str(&env, "disp_count_833");
    let amount = 1000i128;

    registry_client.register_merchant(
        &merchant,
        &String::from_str(&env, "Dispute Count Merchant"),
        &String::from_str(&env, "USDC"),
        &None,
        &None,
        &crate::merchant_registry::MaybeFeeConfig::None,
    );

    assert_eq!(registry_client.get_merchant_dispute_count(&merchant), 0);

    payment_client.grant_role(&admin, &Symbol::new(&env, "MERCHANT"), &merchant);
    payment_client.create_payment(&create_payment_args(&env, &payment_id, &merchant, amount));

    let oracle = Address::generate(&env);
    payment_client.grant_role(&admin, &Symbol::new(&env, "ORACLE"), &oracle);
    payment_client.verify_payment(
        &oracle,
        &payment_id,
        &BytesN::from_array(&env, &[8u8; 32]),
        &customer,
        &amount,
        &None::<u64>,
    );

    let token_address = env.as_contract(&refund_client.address, || {
        env.storage()
            .persistent()
            .get::<DataKey, Address>(&DataKey::UsdcToken)
            .unwrap()
    });
    let token_admin_client = token::StellarAssetClient::new(&env, &token_address);
    token_admin_client.mint(&customer, &100_000);
    token_admin_client.mint(&merchant, &100_000);

    refund_client.register_payment(&payment_id, &merchant, &amount, &Symbol::new(&env, "USDC"));
    refund_client.create_dispute(
        &payment_id,
        &amount,
        &String::from_str(&env, "Issue 833 dispute"),
        &String::from_str(&env, "f000000000000000000000000000000000"),
        &customer,
        &vec![&env],
    );

    assert_eq!(registry_client.get_merchant_dispute_count(&merchant), 1);
}

/// Issue #843: a small-stake arbitrator is outvoted by a large-stake arbitrator
/// under weighted quorum (not raw vote count).
#[test]
fn test_stake_weighted_vote_large_stake_outvotes_small() {
    use crate::{VoteChoice, MIN_ARBITRATOR_STAKE, VOTE_WEIGHT_UNIT};

    let env = Env::default();
    env.mock_all_auths();
    let (admin, payment_client, refund_client) = setup_contracts(&env);

    let payment_id = String::from_str(&env, "payment_weighted_vote");
    let dispute_id = setup_dispute_under_review(
        &env,
        &admin,
        &payment_client,
        &refund_client,
        &payment_id,
        1_000i128,
    );

    let token_admin = Address::generate(&env);
    let stake_token = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();
    let token_admin_client = token::StellarAssetClient::new(&env, &stake_token);

    let small = Address::generate(&env);
    let large = Address::generate(&env);
    let operator = Address::generate(&env);
    refund_client.grant_role(&admin, &Symbol::new(&env, "SETTLEMENT_OPERATOR"), &operator);
    refund_client.grant_role(&admin, &Symbol::new(&env, "ARBITRATOR"), &small);
    refund_client.grant_role(&admin, &Symbol::new(&env, "ARBITRATOR"), &large);

    // Small: 1 weight unit; Large: 10 weight units. Total registered = 11.
    // Default quorum 51% of 11 ≈ 5.61, so large (10) alone exceeds quorum.
    let small_stake = MIN_ARBITRATOR_STAKE; // 1 weight
    let large_stake = VOTE_WEIGHT_UNIT * 10; // 10 weight
    token_admin_client.mint(&small, &small_stake);
    token_admin_client.mint(&large, &large_stake);

    refund_client.lock_stake(&small, &dispute_id, &stake_token, &small_stake);
    refund_client.lock_stake(&large, &dispute_id, &stake_token, &large_stake);

    // Small votes Favour, large votes Against — large must win on weight.
    refund_client.cast_vote(&small, &dispute_id, &VoteChoice::Favour);
    refund_client.cast_vote(&large, &dispute_id, &VoteChoice::Against);

    let tally = refund_client.get_vote_tally(&dispute_id);
    assert_eq!(tally.favour_weight, 1);
    assert_eq!(tally.against_weight, 10);
    assert_eq!(tally.total_registered_weight, 11);
    assert_eq!(tally.vote_count, 2);

    let mut arbs = vec![&env];
    arbs.push_back(small.clone());
    arbs.push_back(large.clone());
    refund_client.finalize_dispute_vote(&operator, &dispute_id, &stake_token, &arbs);

    let dispute = refund_client.get_dispute(&dispute_id);
    assert_eq!(dispute.status, DisputeStatus::Rejected);
}

/// Issue #745: lock_stake and cast_vote require ARBITRATOR role.
#[test]
fn test_lock_stake_and_cast_vote_require_arbitrator_role() {
    use crate::{VoteChoice, MIN_ARBITRATOR_STAKE};

    let env = Env::default();
    env.mock_all_auths();
    let (admin, payment_client, refund_client) = setup_contracts(&env);

    let payment_id = String::from_str(&env, "payment_stake_auth");
    let dispute_id = setup_dispute_under_review(
        &env,
        &admin,
        &payment_client,
        &refund_client,
        &payment_id,
        1_000i128,
    );

    let token_admin = Address::generate(&env);
    let stake_token = env
        .register_stellar_asset_contract_v2(token_admin)
        .address();
    let token_admin_client = token::StellarAssetClient::new(&env, &stake_token);

    let attacker = Address::generate(&env);
    let stake_amount = MIN_ARBITRATOR_STAKE;
    token_admin_client.mint(&attacker, &stake_amount);

    // Attacker without ARBITRATOR role cannot lock_stake
    let lock_res = refund_client.try_lock_stake(&attacker, &dispute_id, &stake_token, &stake_amount);
    assert_eq!(lock_res, Err(Ok(Error::Unauthorized)));

    // Attacker without ARBITRATOR role cannot cast_vote
    let vote_res = refund_client.try_cast_vote(&attacker, &dispute_id, &VoteChoice::Favour);
    assert_eq!(vote_res, Err(Ok(Error::Unauthorized)));

    // Grant role, now lock_stake and cast_vote succeed
    refund_client.grant_role(&admin, &Symbol::new(&env, "ARBITRATOR"), &attacker);
    let lock_res_ok = refund_client.try_lock_stake(&attacker, &dispute_id, &stake_token, &stake_amount);
    assert!(lock_res_ok.is_ok());

    let vote_res_ok = refund_client.try_cast_vote(&attacker, &dispute_id, &VoteChoice::Favour);
    assert!(vote_res_ok.is_ok());
}

/// Issue #843: WEIGHTED_QUORUM_BPS is admin-configurable by name.
#[test]
fn test_set_weighted_quorum_bps() {
    use crate::WEIGHTED_QUORUM_BPS;

    let env = Env::default();
    env.mock_all_auths();
    let (admin, _payment_client, refund_client) = setup_contracts(&env);

    assert_eq!(refund_client.get_weighted_quorum_bps(), WEIGHTED_QUORUM_BPS);
    refund_client.set_weighted_quorum_bps(&admin, &6_000u32);
    assert_eq!(refund_client.get_weighted_quorum_bps(), 6_000u32);
}

/// Issue #844: tip_enabled + confirm_payment stores tip separately from amount.
#[test]
fn test_confirm_payment_tip_stored_separately() {
    use crate::ConfirmPaymentArgs;

    let env = Env::default();
    env.mock_all_auths();
    let (admin, payment_client, _refund_client) = setup_contracts(&env);

    let merchant = Address::generate(&env);
    let oracle = Address::generate(&env);
    let payer = Address::generate(&env);
    payment_client.grant_role(&admin, &Symbol::new(&env, "MERCHANT"), &merchant);
    payment_client.grant_role(&admin, &Symbol::new(&env, "ORACLE"), &oracle);

    let payment_id = String::from_str(&env, "payment_with_tip");
    let mut args = create_payment_args(&env, &payment_id, &merchant, 1_000i128);
    args.tip_enabled = true;
    payment_client.create_payment(&args);

    let tip = 150i128;
    payment_client.confirm_payment(
        &oracle,
        &ConfirmPaymentArgs {
            payment_id: payment_id.clone(),
            transaction_hash: BytesN::random(&env),
            payer_address: payer,
            amount_received: 1_000i128,
            tip_amount: Some(tip),
            payer_muxed_id: None,
        },
    );

    let payment = payment_client.get_payment(&payment_id);
    assert_eq!(payment.amount, 1_000i128);
    assert_eq!(payment.tip_amount, Some(tip));
    assert!(payment.tip_enabled);
}

/// Issue #844: tip_amount rejected when tip_enabled is false.
#[test]
fn test_confirm_payment_tip_rejected_when_disabled() {
    use crate::ConfirmPaymentArgs;

    let env = Env::default();
    env.mock_all_auths();
    let (admin, payment_client, _refund_client) = setup_contracts(&env);

    let merchant = Address::generate(&env);
    let oracle = Address::generate(&env);
    let payer = Address::generate(&env);
    payment_client.grant_role(&admin, &Symbol::new(&env, "MERCHANT"), &merchant);
    payment_client.grant_role(&admin, &Symbol::new(&env, "ORACLE"), &oracle);

    let payment_id = String::from_str(&env, "payment_no_tip");
    payment_client.create_payment(&create_payment_args(
        &env,
        &payment_id,
        &merchant,
        1_000i128,
    ));

    let result = payment_client.try_confirm_payment(
        &oracle,
        &ConfirmPaymentArgs {
            payment_id: payment_id.clone(),
            transaction_hash: BytesN::random(&env),
            payer_address: payer,
            amount_received: 1_000i128,
            tip_amount: Some(50i128),
            payer_muxed_id: None,
        },
    );
    assert_eq!(result, Err(Ok(Error::InvalidAmount)));
}

/// Issue #846: propose_upgrade stores hash + timelock; execute before earliest fails;
/// cancel clears the proposal.
#[test]
fn test_propose_upgrade_timelock_and_cancel() {
    use crate::{WasmUpgradeProposal, UPGRADE_TIMELOCK_LEDGERS};

    let env = Env::default();
    env.mock_all_auths();
    let (admin, payment_client, _refund_client) = setup_contracts(&env);

    let new_wasm_hash = BytesN::random(&env);
    let before = env.ledger().sequence();
    payment_client.propose_upgrade(&admin, &new_wasm_hash);

    let pending: WasmUpgradeProposal = payment_client.get_pending_upgrade().unwrap();
    assert_eq!(pending.new_wasm_hash, new_wasm_hash);
    assert_eq!(
        pending.earliest_execute,
        before.saturating_add(UPGRADE_TIMELOCK_LEDGERS)
    );

    // Too early
    let early = payment_client.try_execute_upgrade(&admin);
    assert_eq!(early, Err(Ok(Error::TimelockNotExpired)));

    // Cancel clears
    payment_client.cancel_upgrade(&admin);
    assert!(payment_client.get_pending_upgrade().is_none());
}

#[test]
fn test_open_dispute_stores_evidence_hash_and_emits_event() {
    let env = Env::default();
    env.mock_all_auths();

    let opener = Address::generate(&env);
    let payment_id = 42u64;
    let disputed_amount = 100_000_000i128; // 10 USDC
    let bond_amount = 10_000_000i128; // 1 USDC
    let evidence_hash = BytesN::from_array(&env, &[0xab; 32]);

    let dispute_id = crate::dispute::open_dispute(
        &env,
        opener.clone(),
        payment_id,
        disputed_amount,
        bond_amount,
        evidence_hash.clone(),
    )
    .expect("open_dispute should succeed");

    assert_eq!(dispute_id, 1);

    // Verify stored evidence hash via read-only view function
    let stored_hash =
        crate::dispute::verify_evidence(&env, dispute_id).expect("verify_evidence should succeed");
    assert_eq!(stored_hash, evidence_hash);

    // Check DISPUTE/OPENED event was published
    assert!(has_dispute_event(&env, "OPENED"));
}
