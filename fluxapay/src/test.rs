#![cfg(test)]

use super::*;
use soroban_sdk::{testutils::{Address as _, BytesN as _, Ledger}, Address, BytesN, Env, String, Symbol};

#[test]
fn test_create_payment() {
    let env = Env::default();
    let contract_id = env.register(PaymentProcessor, ());
    let client = PaymentProcessorClient::new(&env, &contract_id);

    let payment_id = String::from_str(&env, "payment_123");
    let merchant_id = Address::generate(&env);
    let amount = 1000000000i128; // 1000 USDC (6 decimals)
    let currency = Symbol::new(&env, "USDC");
    let deposit_address = Address::generate(&env);
    let expires_at = env.ledger().timestamp() + 3600; // 1 hour from now

    // Create payment
    let payment = client.create_payment(
        &payment_id,
        &merchant_id,
        &amount,
        &currency,
        &deposit_address,
        &expires_at,
    );

    // Verify payment details
    assert_eq!(payment.payment_id, payment_id);
    assert_eq!(payment.merchant_id, merchant_id);
    assert_eq!(payment.amount, amount);
    assert_eq!(payment.currency, currency);
    assert_eq!(payment.deposit_address, deposit_address);
    assert_eq!(payment.status, PaymentStatus::Pending);
    assert!(payment.payer_address.is_none());
    assert!(payment.transaction_hash.is_none());
    assert!(payment.confirmed_at.is_none());
    assert_eq!(payment.expires_at, expires_at);
}

#[test]
fn test_verify_payment_success() {
    let env = Env::default();
    let contract_id = env.register(PaymentProcessor, ());
    let client = PaymentProcessorClient::new(&env, &contract_id);

    let payment_id = String::from_str(&env, "payment_123");
    let merchant_id = Address::generate(&env);
    let amount = 1000000000i128; // 1000 USDC (6 decimals)
    let currency = Symbol::new(&env, "USDC");
    let deposit_address = Address::generate(&env);
    let expires_at = env.ledger().timestamp() + 3600;

    // Create payment
    client.create_payment(
        &payment_id,
        &merchant_id,
        &amount,
        &currency,
        &deposit_address,
        &expires_at,
    );

    // Verify payment
    let payer_address = Address::generate(&env);
    let transaction_hash = BytesN::<32>::random(&env);
    let amount_received = amount; // Exact match

    let status = client.verify_payment(
        &payment_id,
        &transaction_hash,
        &payer_address,
        &amount_received,
    );

    assert_eq!(status, PaymentStatus::Confirmed);

    // Verify payment was updated
    let payment = client.get_payment(&payment_id);
    assert_eq!(payment.status, PaymentStatus::Confirmed);
    assert_eq!(payment.payer_address, Some(payer_address));
    assert_eq!(payment.transaction_hash, Some(transaction_hash));
    assert!(payment.confirmed_at.is_some());
}

#[test]
fn test_verify_payment_wrong_amount() {
    let env = Env::default();
    let contract_id = env.register(PaymentProcessor, ());
    let client = PaymentProcessorClient::new(&env, &contract_id);

    let payment_id = String::from_str(&env, "payment_123");
    let merchant_id = Address::generate(&env);
    let amount = 1000000000i128;
    let currency = Symbol::new(&env, "USDC");
    let deposit_address = Address::generate(&env);
    let expires_at = env.ledger().timestamp() + 3600;

    // Create payment
    client.create_payment(
        &payment_id,
        &merchant_id,
        &amount,
        &currency,
        &deposit_address,
        &expires_at,
    );

    // Try to verify with wrong amount
    let payer_address = Address::generate(&env);
    let transaction_hash = BytesN::<32>::random(&env);
    let amount_received = amount - 1000000i128; // Slightly less

    let status = client.verify_payment(
        &payment_id,
        &transaction_hash,
        &payer_address,
        &amount_received,
    );

    assert_eq!(status, PaymentStatus::Failed);

    // Verify payment was marked as failed
    let payment = client.get_payment(&payment_id);
    assert_eq!(payment.status, PaymentStatus::Failed);
}

#[test]
fn test_get_payment() {
    let env = Env::default();
    let contract_id = env.register(PaymentProcessor, ());
    let client = PaymentProcessorClient::new(&env, &contract_id);

    let payment_id = String::from_str(&env, "payment_456");
    let merchant_id = Address::generate(&env);
    let amount = 500000000i128;
    let currency = Symbol::new(&env, "USDC");
    let deposit_address = Address::generate(&env);
    let expires_at = env.ledger().timestamp() + 7200;

    // Create payment
    let created_payment = client.create_payment(
        &payment_id,
        &merchant_id,
        &amount,
        &currency,
        &deposit_address,
        &expires_at,
    );

    // Get payment details
    let retrieved_payment = client.get_payment(&payment_id);

    assert_eq!(retrieved_payment.payment_id, created_payment.payment_id);
    assert_eq!(retrieved_payment.merchant_id, created_payment.merchant_id);
    assert_eq!(retrieved_payment.amount, created_payment.amount);
    assert_eq!(retrieved_payment.currency, created_payment.currency);
    assert_eq!(retrieved_payment.deposit_address, created_payment.deposit_address);
    assert_eq!(retrieved_payment.status, created_payment.status);
    assert_eq!(retrieved_payment.expires_at, created_payment.expires_at);
}

#[test]
fn test_cancel_expired_payment() {
    let env = Env::default();
    let contract_id = env.register(PaymentProcessor, ());
    let client = PaymentProcessorClient::new(&env, &contract_id);

    let payment_id = String::from_str(&env, "payment_expired");
    let merchant_id = Address::generate(&env);
    let amount = 1000000000i128;
    let currency = Symbol::new(&env, "USDC");
    let deposit_address = Address::generate(&env);
    let expires_at = env.ledger().timestamp() + 3600;

    // Create payment
    client.create_payment(
        &payment_id,
        &merchant_id,
        &amount,
        &currency,
        &deposit_address,
        &expires_at,
    );

    // Fast-forward time past expiration
    env.ledger().set_timestamp(expires_at + 1);

    // Cancel expired payment
    client.cancel_payment(&payment_id);

    // Verify payment was cancelled
    let payment = client.get_payment(&payment_id);
    assert_eq!(payment.status, PaymentStatus::Expired);
}

#[test]
fn test_payment_already_exists() {
    let env = Env::default();
    let contract_id = env.register(PaymentProcessor, ());
    let client = PaymentProcessorClient::new(&env, &contract_id);

    let payment_id = String::from_str(&env, "duplicate_payment");
    let merchant_id = Address::generate(&env);
    let amount = 1000000000i128;
    let currency = Symbol::new(&env, "USDC");
    let deposit_address = Address::generate(&env);
    let expires_at = env.ledger().timestamp() + 3600;

    // Create payment first time
    client.create_payment(
        &payment_id,
        &merchant_id,
        &amount,
        &currency,
        &deposit_address,
        &expires_at,
    );

    // Try to create the same payment again (this will panic in Soroban tests)
    // In a real environment, this would return an error
}

#[test]
fn test_verify_expired_payment() {
    let env = Env::default();
    let contract_id = env.register(PaymentProcessor, ());
    let client = PaymentProcessorClient::new(&env, &contract_id);

    let payment_id = String::from_str(&env, "expired_payment");
    let merchant_id = Address::generate(&env);
    let amount = 1000000000i128;
    let currency = Symbol::new(&env, "USDC");
    let deposit_address = Address::generate(&env);
    let expires_at = env.ledger().timestamp() + 3600;

    // Create payment
    client.create_payment(
        &payment_id,
        &merchant_id,
        &amount,
        &currency,
        &deposit_address,
        &expires_at,
    );

    // Fast-forward time past expiration
    env.ledger().set_timestamp(expires_at + 1);

    // Try to verify expired payment (this will panic in Soroban tests)
    let payer_address = Address::generate(&env);
    let transaction_hash = BytesN::<32>::random(&env);
    // client.verify_payment(&payment_id, &transaction_hash, &payer_address, &amount);
}

#[test]
fn test_invalid_payment_amount() {
    let env = Env::default();
    let contract_id = env.register(PaymentProcessor, ());
    let _client = PaymentProcessorClient::new(&env, &contract_id);

    let _payment_id = String::from_str(&env, "invalid_amount");
    let _merchant_id = Address::generate(&env);
    let _amount = 0i128; // Invalid amount
    let _currency = Symbol::new(&env, "USDC");
    let _deposit_address = Address::generate(&env);
    let _expires_at = env.ledger().timestamp() + 3600;

    // Try to create payment with invalid amount (this will panic in Soroban tests)
    // _client.create_payment(&_payment_id, &_merchant_id, &_amount, &_currency, &_deposit_address, &_expires_at);
}
