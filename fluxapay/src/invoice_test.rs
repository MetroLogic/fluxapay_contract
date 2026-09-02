//! Unit tests for the invoice lifecycle: `create_invoice`,
//! `mark_invoice_paid`, `get_invoice`, and `get_merchant_invoices`, including
//! overdue detection.
//!
//! All tests run against the Soroban test environment (`Env::default()`); no
//! network calls are made.

use crate::{Error, InvoiceStatus, LineItem, PaymentProcessor, PaymentProcessorClient};
use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    vec, Address, Env, String, Symbol,
};

/// Register a fresh PaymentProcessor with an admin.
fn setup(env: &Env) -> PaymentProcessorClient<'_> {
    let contract_id = env.register(PaymentProcessor, ());
    let client = PaymentProcessorClient::new(env, &contract_id);
    let admin = Address::generate(env);
    client.initialize_payment_processor(&admin);
    client
}

fn line_item(env: &Env, description: &str, amount: i128, quantity: u32) -> LineItem {
    LineItem {
        description: String::from_str(env, description),
        amount,
        quantity,
    }
}

/// Create an invoice for `merchant` with two line items totaling
/// `total_amount` and due at `due_date`.
fn create_demo_invoice(
    env: &Env,
    client: &PaymentProcessorClient<'_>,
    merchant: &Address,
    due_date: u64,
    total_amount: i128,
) -> String {
    let items = vec![
        &env,
        line_item(env, "Consulting", 400i128, 2),
        line_item(env, "Setup fee", 200i128, 1),
    ];
    client.create_invoice(
        merchant,
        &String::from_str(env, "customer@example.com"),
        &items,
        &total_amount,
        &Symbol::new(env, "USDC"),
        &due_date,
    )
}

#[test]
fn test_create_invoice_success() {
    let env = Env::default();
    env.mock_all_auths();
    let client = setup(&env);
    let merchant = Address::generate(&env);
    let due_date = env.ledger().timestamp() + 100;

    let invoice_id = create_demo_invoice(&env, &client, &merchant, due_date, 1_000i128);

    let invoice = client.get_invoice(&invoice_id);
    assert_eq!(invoice.merchant_id, merchant);
    assert_eq!(
        invoice.customer_email,
        String::from_str(&env, "customer@example.com")
    );
    assert_eq!(invoice.line_items.len(), 2);
    assert_eq!(invoice.total_amount, 1_000i128);
    assert_eq!(invoice.currency, Symbol::new(&env, "USDC"));
    assert_eq!(invoice.due_date, due_date);
    assert_eq!(invoice.status, InvoiceStatus::Created);
}

#[test]
fn test_invoice_total_matches_line_items() {
    let env = Env::default();
    env.mock_all_auths();
    let client = setup(&env);
    let merchant = Address::generate(&env);

    let items = vec![
        &env,
        line_item(&env, "A", 100i128, 3),
        line_item(&env, "B", 50i128, 2),
        line_item(&env, "C", 25i128, 4),
    ];
    let expected_total = 100 * 3 + 50 * 2 + 25 * 4; // 550

    let invoice_id = client.create_invoice(
        &merchant,
        &String::from_str(&env, "customer@example.com"),
        &items,
        &expected_total,
        &Symbol::new(&env, "USDC"),
        &(env.ledger().timestamp() + 100),
    );

    let invoice = client.get_invoice(&invoice_id);
    let mut computed: i128 = 0;
    for li in invoice.line_items.iter() {
        computed += li.amount * li.quantity as i128;
    }
    assert_eq!(computed, expected_total);
    assert_eq!(invoice.total_amount, expected_total);
}

#[test]
fn test_mark_invoice_paid_transitions_status() {
    let env = Env::default();
    env.mock_all_auths();
    let client = setup(&env);
    let merchant = Address::generate(&env);

    let invoice_id = create_demo_invoice(&env, &client, &merchant, 1_000_000u64, 1_000i128);
    assert_eq!(
        client.get_invoice(&invoice_id).status,
        InvoiceStatus::Created
    );

    client.mark_invoice_paid(&invoice_id);
    assert_eq!(
        client.get_invoice(&invoice_id).status,
        InvoiceStatus::Paid
    );
}

#[test]
fn test_mark_invoice_paid_idempotency() {
    let env = Env::default();
    env.mock_all_auths();
    let client = setup(&env);
    let merchant = Address::generate(&env);

    let invoice_id = create_demo_invoice(&env, &client, &merchant, 1_000_000u64, 1_000i128);

    client.mark_invoice_paid(&invoice_id);
    // Calling a second time must not error and must not reset the status.
    client.mark_invoice_paid(&invoice_id);

    let invoice = client.get_invoice(&invoice_id);
    assert_eq!(invoice.status, InvoiceStatus::Paid);
    assert_eq!(invoice.total_amount, 1_000i128);
    assert_eq!(invoice.line_items.len(), 2);
}

#[test]
fn test_get_invoice_not_found() {
    let env = Env::default();
    env.mock_all_auths();
    let client = setup(&env);

    let result = client.try_get_invoice(&String::from_str(&env, "invoice_999"));
    assert_eq!(result, Err(Ok(Error::PaymentNotFound)));
}

#[test]
fn test_get_merchant_invoices_pagination() {
    let env = Env::default();
    env.mock_all_auths();
    let client = setup(&env);
    let merchant = Address::generate(&env);

    let mut created = vec![&env];
    for i in 0..5 {
        let id = client.create_invoice(
            &merchant,
            &String::from_str(&env, "customer@example.com"),
            &vec![
                &env,
                line_item(&env, "Item", 100i128, (i + 1) as u32),
            ],
            &100i128,
            &Symbol::new(&env, "USDC"),
            &1_000_000u64,
        );
        created.push_back(id);
    }

    let merchant_invoices = client.get_merchant_invoices(&merchant);
    assert_eq!(merchant_invoices.len(), 5);
    for i in 0..5 {
        assert_eq!(
            merchant_invoices.get(i).unwrap(),
            created.get(i).unwrap()
        );
    }
}

#[test]
fn test_overdue_invoice_detection() {
    let env = Env::default();
    env.mock_all_auths();
    let client = setup(&env);
    let merchant = Address::generate(&env);

    let now = 1_000_000u64;
    env.ledger().set_timestamp(now);
    let due_date = now + 100;
    let invoice_id = create_demo_invoice(&env, &client, &merchant, due_date, 1_000i128);

    // Before the due date the invoice is still Created.
    env.ledger().set_timestamp(due_date - 1);
    assert_eq!(
        client.get_invoice(&invoice_id).status,
        InvoiceStatus::Created
    );

    // At/after the due date (default grace period 0) it transitions to Overdue.
    env.ledger().set_timestamp(due_date);
    assert_eq!(
        client.get_invoice(&invoice_id).status,
        InvoiceStatus::Overdue
    );

    // Well past the due date it stays Overdue.
    env.ledger().set_timestamp(due_date + 10_000);
    assert_eq!(
        client.get_invoice(&invoice_id).status,
        InvoiceStatus::Overdue
    );
}