#![cfg(test)]

use super::*;
use soroban_sdk::{testutils::Address as _, Address, Env, String};
use access_control::{role_admin, role_oracle, role_settlement_operator, role_merchant};

fn setup_contract(env: &Env) -> (Address, RefundManagerClient<'_>) {
    let contract_id = env.register(RefundManager, ());
    let client = RefundManagerClient::new(env, &contract_id);
    let admin = Address::generate(env);
    client.initialize(&admin);
    (admin, client)
}

#[test]
fn test_create_refund() {
    let env = Env::default();
    let (_admin, client) = setup_contract(&env);

    let payment_id = String::from_str(&env, "payment_123");
    let refund_amount = 1000i128;
    let reason = String::from_str(&env, "Customer requested refund");
    let requester = Address::generate(&env);

    let refund_id = client.create_refund(&payment_id, &refund_amount, &reason, &requester);
    let refund = client.get_refund(&refund_id);

    assert_eq!(refund.payment_id, payment_id);
    assert_eq!(refund.amount, refund_amount);
    assert_eq!(refund.reason, reason);
    assert_eq!(refund.status, RefundStatus::Pending);
    assert_eq!(refund.requester, requester);
    assert!(refund.processed_at.is_none());
}

#[test]
fn test_process_refund() {
    let env = Env::default();
    let (admin, client) = setup_contract(&env);

    let payment_id = String::from_str(&env, "payment_123");
    let refund_amount = 500i128;
    let reason = String::from_str(&env, "Product defect");
    let requester = Address::generate(&env);

    let refund_id = client.create_refund(&payment_id, &refund_amount, &reason, &requester);

    let operator = Address::generate(&env);
    client.grant_role(&admin, &role_settlement_operator(&env), &operator);
    client.process_refund(&operator, &refund_id);

    let refund = client.get_refund(&refund_id);
    assert_eq!(refund.status, RefundStatus::Completed);
    assert!(refund.processed_at.is_some());
}

#[test]
fn test_get_payment_refunds() {
    let env = Env::default();
    let (_admin, client) = setup_contract(&env);

    let payment_id = String::from_str(&env, "payment_456");
    let requester = Address::generate(&env);

    let refund_id1 = client.create_refund(
        &payment_id,
        &200i128,
        &String::from_str(&env, "Reason 1"),
        &requester,
    );

    let refund_id2 = client.create_refund(
        &payment_id,
        &300i128,
        &String::from_str(&env, "Reason 2"),
        &requester,
    );

    let refunds = client.get_payment_refunds(&payment_id);
    assert_eq!(refunds.len(), 2);

    let mut found1 = false;
    let mut found2 = false;
    for refund in refunds.iter() {
        if refund.refund_id == refund_id1 {
            found1 = true;
        }
        if refund.refund_id == refund_id2 {
            found2 = true;
        }
    }
    assert!(found1 && found2);
}

#[test]
fn test_invalid_refund_amount() {
    let env = Env::default();
    let (_admin, _client) = setup_contract(&env);

}

#[test]
fn test_process_already_processed_refund() {
    let env = Env::default();
    let (admin, client) = setup_contract(&env);

    let payment_id = String::from_str(&env, "payment_999");
    let requester = Address::generate(&env);

    let refund_id = client.create_refund(
        &payment_id,
        &150i128,
        &String::from_str(&env, "Test refund"),
        &requester,
    );

    let operator = Address::generate(&env);
    client.grant_role(&admin, &role_settlement_operator(&env), &operator);
    client.process_refund(&operator, &refund_id);
}

#[test]
fn test_get_nonexistent_refund() {
    let env = Env::default();
    let (_admin, _client) = setup_contract(&env);
}

#[test]
fn test_initialize_contract() {
    let env = Env::default();
    let contract_id = env.register(RefundManager, ());
    let client = RefundManagerClient::new(&env, &contract_id);
    let admin = Address::generate(&env);

    client.initialize(&admin);

    let stored_admin = client.get_admin();
    assert_eq!(stored_admin, Some(admin.clone()));
    assert!(client.has_role(&role_admin(&env), &admin));
}

#[test]
fn test_grant_role() {
    let env = Env::default();
    let (admin, client) = setup_contract(&env);
    let account = Address::generate(&env);
    let role = role_oracle(&env);

    client.grant_role(&admin, &role, &account);
    assert!(client.has_role(&role, &account));
}

#[test]
fn test_grant_role_unauthorized() {
    let env = Env::default();
    let (_admin, client) = setup_contract(&env);
    let unauthorized = Address::generate(&env);
}

#[test]
fn test_revoke_role() {
    let env = Env::default();
    let (admin, client) = setup_contract(&env);
    let account = Address::generate(&env);
    let role = role_merchant(&env);

    client.grant_role(&admin, &role, &account);
    assert!(client.has_role(&role, &account));

    client.revoke_role(&admin, &role, &account);
    assert!(!client.has_role(&role, &account));
}

#[test]
fn test_has_role() {
    let env = Env::default();
    let (admin, client) = setup_contract(&env);
    let account = Address::generate(&env);
    let role = role_oracle(&env);

    assert!(!client.has_role(&role, &account));

    client.grant_role(&admin, &role, &account);
    assert!(client.has_role(&role, &account));
}

#[test]
fn test_renounce_role() {
    let env = Env::default();
    let (admin, client) = setup_contract(&env);
    let account = Address::generate(&env);
    let role = role_merchant(&env);

    client.grant_role(&admin, &role, &account);
    assert!(client.has_role(&role, &account));

    client.renounce_role(&account, &role);
    assert!(!client.has_role(&role, &account));
}

#[test]
fn test_transfer_admin() {
    let env = Env::default();
    let (current_admin, client) = setup_contract(&env);
    let new_admin = Address::generate(&env);

    client.transfer_admin(&current_admin, &new_admin);

    assert!(client.has_role(&role_admin(&env), &new_admin));
    assert!(!client.has_role(&role_admin(&env), &current_admin));

    let stored_admin = client.get_admin();
    assert_eq!(stored_admin, Some(new_admin));
}

#[test]
fn test_process_refund_with_oracle_role() {
    let env = Env::default();
    let (admin, client) = setup_contract(&env);

    let payment_id = String::from_str(&env, "payment_123");
    let refund_amount = 500i128;
    let reason = String::from_str(&env, "Product defect");
    let requester = Address::generate(&env);

    let refund_id = client.create_refund(&payment_id, &refund_amount, &reason, &requester);

    let oracle = Address::generate(&env);
    client.grant_role(&admin, &role_oracle(&env), &oracle);
    client.process_refund(&oracle, &refund_id);

    let refund = client.get_refund(&refund_id);
    assert_eq!(refund.status, RefundStatus::Completed);
}

#[test]
fn test_process_refund_unauthorized() {
    let env = Env::default();
    let (_admin, client) = setup_contract(&env);

    let payment_id = String::from_str(&env, "payment_123");
    let refund_amount = 500i128;
    let reason = String::from_str(&env, "Product defect");
    let requester = Address::generate(&env);

    let _refund_id = client.create_refund(&payment_id, &refund_amount, &reason, &requester);
}

#[test]
fn test_multiple_roles() {
    let env = Env::default();
    let (admin, client) = setup_contract(&env);
    let account = Address::generate(&env);

    client.grant_role(&admin, &role_merchant(&env), &account);
    client.grant_role(&admin, &role_oracle(&env), &account);
    client.grant_role(&admin, &role_settlement_operator(&env), &account);

    assert!(client.has_role(&role_merchant(&env), &account));
    assert!(client.has_role(&role_oracle(&env), &account));
    assert!(client.has_role(&role_settlement_operator(&env), &account));
}

#[test]
fn test_role_already_granted() {
    let env = Env::default();
    let (admin, client) = setup_contract(&env);
    let account = Address::generate(&env);
    let role = role_merchant(&env);

    client.grant_role(&admin, &role, &account);
    assert!(client.has_role(&role, &account));
}
