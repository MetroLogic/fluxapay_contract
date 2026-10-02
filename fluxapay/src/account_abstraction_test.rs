//! Spending policies and call allowlists for delegated session keys.
//!
//! Covers Issue #885: a per-transaction cap, a rolling window budget,
//! and contract/function allowlists. The unrestricted path registered by
//! `register_session_key` is covered alongside the existing module tests.

use crate::account_abstraction::{
    execute_with_session, register_session_key, register_session_key_with_policy,
    AccountAbstractionError, SessionInvocation, SessionPolicy,
};
use soroban_sdk::{
    contract, contractimpl,
    testutils::{Address as _, Events as _, Ledger as _},
    vec,
    xdr::ToXdr,
    Address, Bytes, Env, Symbol, Vec,
};

#[contract]
struct SessionContext;

#[contractimpl]
impl SessionContext {}

fn policy(
    env: &Env,
    max_amount_per_tx: Option<i128>,
    max_amount_per_window: Option<i128>,
    window_secs: u64,
    allowed_contracts: Vec<Address>,
    allowed_functions: Vec<Symbol>,
) -> SessionPolicy {
    SessionPolicy {
        max_amount_per_tx,
        max_amount_per_window,
        window_secs,
        allowed_contracts,
        allowed_functions,
    }
}

fn invocation(env: &Env, target: &Address, function: &str, amount: i128) -> Bytes {
    SessionInvocation {
        target_contract: target.clone(),
        function: Symbol::new(env, function),
        amount,
    }
    .to_xdr(env)
}

fn in_contract<T>(env: &Env, contract_id: &Address, body: impl FnOnce() -> T) -> T {
    env.as_contract(contract_id, body)
}

fn session_event_count(env: &Env, action: &str) -> usize {
    use soroban_sdk::xdr::ContractEventBody;
    env.events()
        .all()
        .events()
        .iter()
        .filter(|event| {
            let ContractEventBody::V0(body) = &event.body else {
                return false;
            };
            let mut topics = body.topics.iter();
            let Some(namespace) = topics.next() else {
                return false;
            };
            let Some(name) = topics.next() else {
                return false;
            };
            symbol_is(namespace, "SESSION") && symbol_is(name, action)
        })
        .count()
}

fn symbol_is(value: &soroban_sdk::xdr::ScVal, expected: &str) -> bool {
    match value {
        soroban_sdk::xdr::ScVal::Symbol(symbol) => symbol.as_slice() == expected.as_bytes(),
        _ => false,
    }
}

fn open(env: &Env) -> (Address, Address, Address) {
    env.mock_all_auths();
    let contract_id = env.register(SessionContext, ());
    let account = Address::generate(env);
    let session_key = Address::generate(env);
    (contract_id, account, session_key)
}

#[test]
fn test_default_registration_still_accepts_opaque_payload() {
    let env = Env::default();
    let (contract_id, account, session_key) = open(&env);
    let expires_at = env.ledger().timestamp() + 3_600;

    in_contract(&env, &contract_id, || {
        register_session_key(
            env.clone(),
            account.clone(),
            session_key.clone(),
            expires_at,
        )
        .unwrap();
        let payload = Bytes::from_slice(&env, b"test_payload");
        let res = execute_with_session(env.clone(), account, session_key, payload);
        assert!(res.is_ok());
    });
}

#[test]
fn test_spend_limit_exceeded_on_single_tx() {
    let env = Env::default();
    env.ledger().set_timestamp(1_000);
    let (contract_id, account, session_key) = open(&env);
    let target = Address::generate(&env);
    let limits = policy(&env, Some(100), None, 0, vec![&env], vec![&env]);

    in_contract(&env, &contract_id, || {
        register_session_key_with_policy(
            env.clone(),
            account.clone(),
            session_key.clone(),
            100_000,
            limits,
        )
        .unwrap();
    });
    assert_eq!(session_event_count(&env, "POLICY_REGISTERED"), 1);

    let over = invocation(&env, &target, "charge", 101);
    let res = in_contract(&env, &contract_id, || {
        execute_with_session(env.clone(), account.clone(), session_key.clone(), over)
    });
    assert_eq!(res, Err(AccountAbstractionError::SpendLimitExceeded));
    assert_eq!(session_event_count(&env, "POLICY_VIOLATION"), 1);

    let at_cap = invocation(&env, &target, "charge", 100);
    let res = in_contract(&env, &contract_id, || {
        execute_with_session(env.clone(), account, session_key, at_cap)
    });
    assert!(res.is_ok());
}

#[test]
fn test_cumulative_spend_limit_exceeded_across_calls() {
    let env = Env::default();
    env.ledger().set_timestamp(1_000);
    let (contract_id, account, session_key) = open(&env);
    let target = Address::generate(&env);
    // Per-call cap is above each individual call. The window is the binding limit.
    let limits = policy(&env, Some(80), Some(100), 10_000, vec![&env], vec![&env]);

    in_contract(&env, &contract_id, || {
        register_session_key_with_policy(
            env.clone(),
            account.clone(),
            session_key.clone(),
            100_000,
            limits,
        )
        .unwrap();
        assert!(execute_with_session(
            env.clone(),
            account.clone(),
            session_key.clone(),
            invocation(&env, &target, "charge", 60),
        )
        .is_ok());

        let res = execute_with_session(
            env.clone(),
            account.clone(),
            session_key.clone(),
            invocation(&env, &target, "charge", 50),
        );
        assert_eq!(res, Err(AccountAbstractionError::WindowBudgetExceeded));

        // 40 still fits in the remaining 40 of this window.
        assert!(execute_with_session(
            env.clone(),
            account,
            session_key,
            invocation(&env, &target, "charge", 40),
        )
        .is_ok());
    });
    assert_eq!(session_event_count(&env, "POLICY_VIOLATION"), 1);
}

#[test]
fn test_spend_window_expires_and_resets() {
    let env = Env::default();
    env.ledger().set_timestamp(1_000);
    let (contract_id, account, session_key) = open(&env);
    let target = Address::generate(&env);
    let limits = policy(&env, Some(100), Some(100), 100, vec![&env], vec![&env]);

    in_contract(&env, &contract_id, || {
        register_session_key_with_policy(
            env.clone(),
            account.clone(),
            session_key.clone(),
            100_000,
            limits,
        )
        .unwrap();
        assert!(execute_with_session(
            env.clone(),
            account.clone(),
            session_key.clone(),
            invocation(&env, &target, "charge", 80),
        )
        .is_ok());
        let res = execute_with_session(
            env.clone(),
            account.clone(),
            session_key.clone(),
            invocation(&env, &target, "charge", 30),
        );
        assert_eq!(res, Err(AccountAbstractionError::WindowBudgetExceeded));
    });
    assert_eq!(session_event_count(&env, "WINDOW_ROLLOVER"), 0);

    // The 100-second window opened at t=1000, so t=1100 starts a new one.
    env.ledger().set_timestamp(1_100);
    in_contract(&env, &contract_id, || {
        assert!(execute_with_session(
            env.clone(),
            account.clone(),
            session_key.clone(),
            invocation(&env, &target, "charge", 80),
        )
        .is_ok());
        let res = execute_with_session(
            env.clone(),
            account,
            session_key,
            invocation(&env, &target, "charge", 30),
        );
        assert_eq!(res, Err(AccountAbstractionError::WindowBudgetExceeded));
    });
    assert_eq!(session_event_count(&env, "WINDOW_ROLLOVER"), 1);
}

#[test]
fn test_unauthorized_contract_rejected() {
    let env = Env::default();
    let (contract_id, account, session_key) = open(&env);
    let allowed = Address::generate(&env);
    let other = Address::generate(&env);
    let limits = policy(&env, None, None, 0, vec![&env, allowed.clone()], vec![&env]);

    in_contract(&env, &contract_id, || {
        register_session_key_with_policy(
            env.clone(),
            account.clone(),
            session_key.clone(),
            100_000,
            limits,
        )
        .unwrap();
        let res = execute_with_session(
            env.clone(),
            account.clone(),
            session_key.clone(),
            invocation(&env, &other, "charge", 10),
        );
        assert_eq!(res, Err(AccountAbstractionError::ContractNotAllowed));
        assert!(execute_with_session(
            env.clone(),
            account,
            session_key,
            invocation(&env, &allowed, "charge", 10),
        )
        .is_ok());
    });
    assert_eq!(session_event_count(&env, "POLICY_VIOLATION"), 1);
}

#[test]
fn test_unauthorized_function_rejected() {
    let env = Env::default();
    let (contract_id, account, session_key) = open(&env);
    let target = Address::generate(&env);
    let limits = policy(
        &env,
        None,
        None,
        0,
        vec![&env],
        vec![&env, Symbol::new(&env, "charge")],
    );

    in_contract(&env, &contract_id, || {
        register_session_key_with_policy(
            env.clone(),
            account.clone(),
            session_key.clone(),
            100_000,
            limits,
        )
        .unwrap();
        let res = execute_with_session(
            env.clone(),
            account.clone(),
            session_key.clone(),
            invocation(&env, &target, "withdraw", 10),
        );
        assert_eq!(res, Err(AccountAbstractionError::FunctionNotAllowed));
        assert!(execute_with_session(
            env.clone(),
            account,
            session_key,
            invocation(&env, &target, "charge", 10),
        )
        .is_ok());
    });
    assert_eq!(session_event_count(&env, "POLICY_VIOLATION"), 1);
}

#[test]
fn test_negative_spend_cap_rejected_at_registration() {
    let env = Env::default();
    let (contract_id, account, session_key) = open(&env);
    let limits = policy(&env, Some(-1), None, 0, vec![&env], vec![&env]);

    let res = in_contract(&env, &contract_id, || {
        register_session_key_with_policy(env.clone(), account, session_key, 100_000, limits)
    });
    assert_eq!(res, Err(AccountAbstractionError::InvalidPayload));
    assert_eq!(session_event_count(&env, "POLICY_REGISTERED"), 0);
}
