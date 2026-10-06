#![cfg(test)]

use crate::access_control::role_merchant;
use crate::meta_transaction::{
    self, MetaTransactionPayload, MetaTxError, RefundAuthRequest, FN_AUTHORIZE_REFUND,
    FN_CREATE_CHARGE, FN_ROTATE_WEBHOOK_KEY, FN_SET_FEE_RECIPIENT,
};
use crate::{CreatePaymentArgs, PaymentProcessor, PaymentProcessorClient, PaymentStatus};
use ed25519_dalek::{Signer, SigningKey};
use soroban_sdk::{
    testutils::{Address as _, Events as _, Ledger as _},
    xdr::{ContractEventBody, FromXdr, ScVal, ToXdr},
    Address, Bytes, BytesN, Env, String, Symbol, TryIntoVal,
};

const NOW: u64 = 1_700_000_000;

fn merchant_key() -> SigningKey {
    SigningKey::from_bytes(&[
        1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25,
        26, 27, 28, 29, 30, 31, 32,
    ])
}

fn account_address(env: &Env, pubkey: &[u8; 32]) -> Address {
    let mut xdr = [0u8; 44];
    xdr[3] = 18;
    xdr[12..44].copy_from_slice(pubkey);
    Address::from_xdr(env, &Bytes::from_array(env, &xdr)).unwrap()
}

fn setup(env: &Env) -> (Address, Address, PaymentProcessorClient<'_>) {
    env.ledger().with_mut(|li| {
        li.timestamp = NOW;
    });
    env.mock_all_auths();
    let contract_id = env.register(PaymentProcessor, ());
    let client = PaymentProcessorClient::new(env, &contract_id);
    let admin = Address::generate(env);
    client.initialize_payment_processor(&admin);
    (contract_id, admin, client)
}

fn signed_payload(
    env: &Env,
    key: &SigningKey,
    merchant: &Address,
    domain: &BytesN<32>,
    function: &str,
    call_data: Bytes,
    nonce: u64,
    deadline: u64,
) -> (MetaTransactionPayload, BytesN<64>) {
    let payload = MetaTransactionPayload {
        merchant_id: merchant.clone(),
        target_function: Symbol::new(env, function),
        call_data,
        nonce,
        deadline,
        domain_separator: domain.clone(),
    };
    let digest = meta_transaction::signing_digest(env, &payload).unwrap();
    let signature = key.sign(digest.to_array().as_slice());
    (payload, BytesN::from_array(env, &signature.to_bytes()))
}

fn has_meta_event(env: &Env, action: &str) -> bool {
    env.events().all().events().iter().any(|event| {
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
                if namespace == Symbol::new(env, "META_TX")
                    && name == Symbol::new(env, action)
        )
    })
}

fn charge_args(env: &Env, merchant: &Address, payment_id: &str) -> CreatePaymentArgs {
    CreatePaymentArgs {
        payment_id: String::from_str(env, payment_id),
        merchant_id: merchant.clone(),
        payer: None,
        amount: 1_000,
        currency: Symbol::new(env, "USDC"),
        deposit_address: Address::generate(env),
        expires_at: Some(NOW + 3_600),
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
    }
}

#[test]
fn test_relayer_executes_merchant_actions() {
    let env = Env::default();
    let (_contract_id, admin, client) = setup(&env);
    let key = merchant_key();
    let merchant = account_address(&env, &key.verifying_key().to_bytes());
    let relayer = Address::generate(&env);
    client.grant_role(&admin, &role_merchant(&env), &merchant);
    let domain = client.get_domain_separator();

    let charge = charge_args(&env, &merchant, "pay_meta_charge");
    let (create_payload, create_sig) = signed_payload(
        &env,
        &key,
        &merchant,
        &domain,
        FN_CREATE_CHARGE,
        charge.to_xdr(&env),
        0,
        NOW + 600,
    );
    let created = client.execute_meta_transaction(&relayer, &create_payload, &create_sig);
    assert!(created.success);
    assert_eq!(created.nonce, 0);
    assert!(created.gas.resource_fee_stroops > 0);
    assert_eq!(created.gas.instructions, 65);
    assert!(has_meta_event(&env, "EXECUTED"));
    let auths = env.auths();
    assert_eq!(auths.len(), 1);
    assert_eq!(auths[0].0, relayer);
    let payment = client.get_payment(&String::from_str(&env, "pay_meta_charge"));
    assert_eq!(payment.merchant_id, merchant);
    assert_eq!(payment.status, PaymentStatus::Pending);
    assert_eq!(payment.amount, 1_000);

    let refund = RefundAuthRequest {
        payment_id: String::from_str(&env, "pay_meta_charge"),
        refund_amount: 400,
        reason: String::from_str(&env, "customer returned item"),
    };
    let (refund_payload, refund_sig) = signed_payload(
        &env,
        &key,
        &merchant,
        &domain,
        FN_AUTHORIZE_REFUND,
        refund.to_xdr(&env),
        1,
        NOW + 600,
    );
    let refunded = client.execute_meta_transaction(&relayer, &refund_payload, &refund_sig);
    assert!(refunded.success);
    let stored = client
        .get_refund_authorization(&merchant, &String::from_str(&env, "pay_meta_charge"))
        .unwrap();
    assert_eq!(stored.refund_amount, 400);

    let webhook_key = BytesN::from_array(&env, &[9u8; 32]);
    let (rotate_payload, rotate_sig) = signed_payload(
        &env,
        &key,
        &merchant,
        &domain,
        FN_ROTATE_WEBHOOK_KEY,
        webhook_key.clone().to_xdr(&env),
        2,
        NOW + 600,
    );
    assert!(
        client
            .execute_meta_transaction(&relayer, &rotate_payload, &rotate_sig)
            .success
    );
    assert_eq!(client.get_webhook_key(&merchant), Some(webhook_key));

    let recipient = Address::generate(&env);
    let (fee_payload, fee_sig) = signed_payload(
        &env,
        &key,
        &merchant,
        &domain,
        FN_SET_FEE_RECIPIENT,
        recipient.clone().to_xdr(&env),
        3,
        NOW + 600,
    );
    assert!(
        client
            .execute_meta_transaction(&relayer, &fee_payload, &fee_sig)
            .success
    );
    assert_eq!(client.get_fee_recipient(&merchant), Some(recipient));
    assert_eq!(client.get_merchant_nonce(&merchant), 4);
}

#[test]
fn test_signature_mismatch_and_forged_payload_are_rejected() {
    let env = Env::default();
    let (_contract_id, _admin, client) = setup(&env);
    let key = merchant_key();
    let merchant = account_address(&env, &key.verifying_key().to_bytes());
    let relayer = Address::generate(&env);
    let domain = client.get_domain_separator();
    let call_data = BytesN::from_array(&env, &[4u8; 32]).to_xdr(&env);
    let (payload, signature) = signed_payload(
        &env,
        &key,
        &merchant,
        &domain,
        FN_ROTATE_WEBHOOK_KEY,
        call_data,
        0,
        NOW + 600,
    );

    let mut forged_bytes = signature.to_array();
    forged_bytes[0] ^= 0xff;
    let forged_sig = BytesN::from_array(&env, &forged_bytes);
    let mismatch = client.try_execute_meta_transaction(&relayer, &payload, &forged_sig);
    assert!(mismatch.is_err());
    assert_eq!(client.get_merchant_nonce(&merchant), 0);

    let mut forged_payload = payload.clone();
    forged_payload.call_data = BytesN::from_array(&env, &[5u8; 32]).to_xdr(&env);
    let forged = client.try_execute_meta_transaction(&relayer, &forged_payload, &signature);
    assert!(forged.is_err());
    assert_eq!(client.get_merchant_nonce(&merchant), 0);
    assert!(client.get_webhook_key(&merchant).is_none());
}

#[test]
fn test_replayed_nonce_is_rejected() {
    let env = Env::default();
    let (_contract_id, _admin, client) = setup(&env);
    let key = merchant_key();
    let merchant = account_address(&env, &key.verifying_key().to_bytes());
    let relayer = Address::generate(&env);
    let domain = client.get_domain_separator();
    let (payload, signature) = signed_payload(
        &env,
        &key,
        &merchant,
        &domain,
        FN_ROTATE_WEBHOOK_KEY,
        BytesN::from_array(&env, &[6u8; 32]).to_xdr(&env),
        0,
        NOW + 600,
    );

    assert!(
        client
            .execute_meta_transaction(&relayer, &payload, &signature)
            .success
    );
    let replay = client.try_execute_meta_transaction(&relayer, &payload, &signature);
    assert_eq!(replay, Err(Ok(MetaTxError::InvalidNonce)));
    assert_eq!(client.get_merchant_nonce(&merchant), 1);
}

#[test]
fn test_expired_deadline_is_rejected() {
    let env = Env::default();
    let (_contract_id, _admin, client) = setup(&env);
    let key = merchant_key();
    let merchant = account_address(&env, &key.verifying_key().to_bytes());
    let relayer = Address::generate(&env);
    let domain = client.get_domain_separator();
    let (payload, signature) = signed_payload(
        &env,
        &key,
        &merchant,
        &domain,
        FN_ROTATE_WEBHOOK_KEY,
        BytesN::from_array(&env, &[7u8; 32]).to_xdr(&env),
        0,
        NOW - 1,
    );

    let result = client.try_execute_meta_transaction(&relayer, &payload, &signature);
    assert_eq!(result, Err(Ok(MetaTxError::Expired)));
    assert_eq!(client.get_merchant_nonce(&merchant), 0);
}

#[test]
fn test_wrong_domain_separator_is_rejected() {
    let env = Env::default();
    let (contract_id, _admin, client) = setup(&env);
    let key = merchant_key();
    let merchant = account_address(&env, &key.verifying_key().to_bytes());
    let relayer = Address::generate(&env);
    let other_contract = Address::generate(&env);
    let wrong_domain = env.as_contract(&contract_id, || {
        meta_transaction::domain_separator_for(&env, &other_contract, &env.ledger().network_id())
    });
    let (payload, signature) = signed_payload(
        &env,
        &key,
        &merchant,
        &wrong_domain,
        FN_ROTATE_WEBHOOK_KEY,
        BytesN::from_array(&env, &[8u8; 32]).to_xdr(&env),
        0,
        NOW + 600,
    );

    let result = client.try_execute_meta_transaction(&relayer, &payload, &signature);
    assert_eq!(result, Err(Ok(MetaTxError::InvalidDomain)));
    assert_eq!(client.get_merchant_nonce(&merchant), 0);
    assert!(client.get_webhook_key(&merchant).is_none());
}

#[test]
fn test_failed_inner_action_emits_failed_and_consumes_nonce() {
    let env = Env::default();
    let (_contract_id, _admin, client) = setup(&env);
    let key = merchant_key();
    let merchant = account_address(&env, &key.verifying_key().to_bytes());
    let relayer = Address::generate(&env);
    let domain = client.get_domain_separator();
    let refund = RefundAuthRequest {
        payment_id: String::from_str(&env, "pay_fail_1"),
        refund_amount: 0,
        reason: String::from_str(&env, "zero amount"),
    };
    let (payload, signature) = signed_payload(
        &env,
        &key,
        &merchant,
        &domain,
        FN_AUTHORIZE_REFUND,
        refund.to_xdr(&env),
        0,
        NOW + 600,
    );

    let receipt = client.execute_meta_transaction(&relayer, &payload, &signature);
    assert!(!receipt.success);
    assert!(has_meta_event(&env, "FAILED"));
    assert_eq!(client.get_merchant_nonce(&merchant), 1);
    assert!(client
        .get_refund_authorization(&merchant, &String::from_str(&env, "pay_fail_1"))
        .is_none());
}

#[test]
fn test_direct_merchant_call_still_requires_auth() {
    let env = Env::default();
    let (_contract_id, _admin, client) = setup(&env);
    let key = merchant_key();
    let merchant = account_address(&env, &key.verifying_key().to_bytes());
    let webhook_key = BytesN::from_array(&env, &[3u8; 32]);

    client.rotate_webhook_key(&merchant, &webhook_key);
    let auths = env.auths();
    assert_eq!(auths.len(), 1);
    assert_eq!(auths[0].0, merchant);

    assert_eq!(client.get_webhook_key(&merchant), Some(webhook_key));
}
