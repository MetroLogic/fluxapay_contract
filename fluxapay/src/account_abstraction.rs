use soroban_sdk::{
    contracterror, contracttype, xdr::FromXdr, Address, Bytes, BytesN, Env, Symbol, Vec,
};

/// Error types for account abstraction operations
#[contracterror]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AccountAbstractionError {
    Unauthorized = 1,
    SessionNotFound = 2,
    SessionExpired = 3,
    InvalidPayload = 4,
    SignerRevoked = 5,
    /// A single call moved more than `SessionPolicy::max_amount_per_tx`.
    SpendLimitExceeded = 6,
    /// The call would push the rolling window total past `max_amount_per_window`.
    WindowBudgetExceeded = 7,
    /// The payload targets a contract that is not on the session allowlist.
    ContractNotAllowed = 8,
    /// The payload names a function that is not on the session allowlist.
    FunctionNotAllowed = 9,
}

/// Session key metadata stored in persistent storage
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionKeyMetadata {
    pub account: Address,
    pub session_key: Address,
    pub expires_at: u64,
}

/// Event emitted when a session key executes a payload
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionExecutedEvent {
    pub account: Address,
    pub session_key: Address,
    pub payload_hash: BytesN<32>,
}

/// Limits and call allowlists attached to a session key.
///
/// `None` spend caps and empty allowlists mean "not configured".
/// `register_session_key` stores that unrestricted policy so existing
/// callers keep the previous behavior. A configured cap or a non-empty
/// allowlist is enforced by `execute_with_session`.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionPolicy {
    /// Maximum amount of one call. `None` disables the per-call cap.
    pub max_amount_per_tx: Option<i128>,
    /// Maximum cumulative amount inside one rolling window. `None` disables it.
    pub max_amount_per_window: Option<i128>,
    /// Window length in seconds. `0` keeps a single window for the life of the key
    /// when a cumulative cap is set. Ignored when the cumulative cap is `None`.
    pub window_secs: u64,
    /// Contracts this key may call. Empty allows every contract.
    pub allowed_contracts: Vec<Address>,
    /// Function selectors this key may call. Empty allows every function.
    pub allowed_functions: Vec<Symbol>,
}

/// Cumulative spend tracked for the current window of one session key.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionSpendState {
    /// Ledger timestamp when the current window opened.
    pub window_start: u64,
    /// Amount already accepted inside the current window.
    pub spent_in_window: i128,
}

/// Structured payload enforced when a session key has a spending policy
/// or a call allowlist. Encoded as Soroban ScVal XDR (`ToXdr` / `FromXdr`).
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionInvocation {
    pub target_contract: Address,
    pub function: Symbol,
    pub amount: i128,
}

/// Account abstraction data keys for persistent storage
#[contracttype]
pub enum AccountAbstractionDataKey {
    /// Maps (account, session_key) -> SessionKeyMetadata
    SessionKey(Address, Address),
    /// Maps (account, signer) -> bool; set when a delegated signer is revoked
    SignerRevoked(Address, Address),
    /// Maps (account, session_key) -> SessionPolicy
    SessionPolicy(Address, Address),
    /// Maps (account, session_key) -> SessionSpendState
    SessionSpend(Address, Address),
}

/// Register a new session key with an expiration timestamp for an account.
///
/// The key is stored with an unrestricted policy: no spend caps and no
/// contract or function allowlist. Call `register_session_key_with_policy`
/// to attach limits. Requires authorization from the account owner.
#[allow(deprecated)] // events::publish — migrate to #[contractevent] in a follow-up
pub fn register_session_key(
    env: Env,
    account: Address,
    session_key: Address,
    expires_at: u64,
) -> Result<(), AccountAbstractionError> {
    let policy = unrestricted_policy(&env);
    store_session(&env, account, session_key, expires_at, policy)
}

/// Register a session key and attach a spending policy and call allowlist.
///
/// Re-registering the same key replaces the policy and clears spend already
/// counted against the previous window. Requires authorization from the account owner.
#[allow(deprecated)] // events::publish — migrate to #[contractevent] in a follow-up
pub fn register_session_key_with_policy(
    env: Env,
    account: Address,
    session_key: Address,
    expires_at: u64,
    policy: SessionPolicy,
) -> Result<(), AccountAbstractionError> {
    store_session(
        &env,
        account.clone(),
        session_key.clone(),
        expires_at,
        policy.clone(),
    )?;

    env.events().publish(
        (
            Symbol::new(&env, "SESSION"),
            Symbol::new(&env, "POLICY_REGISTERED"),
            account,
        ),
        (session_key, policy),
    );

    Ok(())
}

/// Persist session metadata, replace any previous policy, and clear revocation.
#[allow(deprecated)] // events::publish — migrate to #[contractevent] in a follow-up
fn store_session(
    env: &Env,
    account: Address,
    session_key: Address,
    expires_at: u64,
    policy: SessionPolicy,
) -> Result<(), AccountAbstractionError> {
    account.require_auth();

    if policy.max_amount_per_tx.is_some_and(|max| max < 0)
        || policy.max_amount_per_window.is_some_and(|max| max < 0)
    {
        return Err(AccountAbstractionError::InvalidPayload);
    }

    let meta = SessionKeyMetadata {
        account: account.clone(),
        session_key: session_key.clone(),
        expires_at,
    };

    env.storage().persistent().set(
        &AccountAbstractionDataKey::SessionKey(account.clone(), session_key.clone()),
        &meta,
    );
    env.storage().persistent().set(
        &AccountAbstractionDataKey::SessionPolicy(account.clone(), session_key.clone()),
        &policy,
    );
    // A replacement policy starts a fresh window. Drop the previous counter.
    env.storage()
        .persistent()
        .remove(&AccountAbstractionDataKey::SessionSpend(
            account.clone(),
            session_key.clone(),
        ));

    // Re-registering a session key clears any prior revocation flag so the
    // signer is active again.
    env.storage()
        .persistent()
        .remove(&AccountAbstractionDataKey::SignerRevoked(
            account.clone(),
            session_key.clone(),
        ));

    env.events().publish(
        (
            Symbol::new(&env, "SESSION"),
            Symbol::new(&env, "REGISTERED"),
            account,
        ),
        (session_key, expires_at),
    );

    Ok(())
}

fn unrestricted_policy(env: &Env) -> SessionPolicy {
    SessionPolicy {
        max_amount_per_tx: None,
        max_amount_per_window: None,
        window_secs: 0,
        allowed_contracts: Vec::new(env),
        allowed_functions: Vec::new(env),
    }
}

/// Revoke an existing session key for an account.
/// Requires authorization from the account owner.
#[allow(deprecated)] // events::publish — migrate to #[contractevent] in a follow-up
pub fn revoke_session_key(
    env: Env,
    account: Address,
    session_key: Address,
) -> Result<(), AccountAbstractionError> {
    account.require_auth();

    let key = AccountAbstractionDataKey::SessionKey(account.clone(), session_key.clone());
    if !env.storage().persistent().has(&key) {
        return Err(AccountAbstractionError::SessionNotFound);
    }

    env.storage().persistent().remove(&key);
    env.storage()
        .persistent()
        .remove(&AccountAbstractionDataKey::SessionPolicy(
            account.clone(),
            session_key.clone(),
        ));
    env.storage()
        .persistent()
        .remove(&AccountAbstractionDataKey::SessionSpend(
            account.clone(),
            session_key.clone(),
        ));

    // Mark the signer as revoked so any in-flight, pre-signed payloads are
    // rejected at execution time even after the session metadata is removed.
    env.storage().persistent().set(
        &AccountAbstractionDataKey::SignerRevoked(account.clone(), session_key.clone()),
        &true,
    );

    env.events().publish(
        (
            Symbol::new(&env, "SESSION"),
            Symbol::new(&env, "REVOKED"),
            account,
        ),
        session_key,
    );

    Ok(())
}

/// Execute a transaction payload on behalf of an account using a delegated session key.
#[allow(deprecated)] // events::publish — migrate to #[contractevent] in a follow-up
pub fn execute_with_session(
    env: Env,
    account: Address,
    session_key: Address,
    payload: Bytes,
) -> Result<Bytes, AccountAbstractionError> {
    session_key.require_auth();

    // Atomically check revocation and permissions: the revocation flag is read
    // in the same critical section as the session metadata lookup so a revoked
    // signer cannot slip a pre-signed payload through between the two reads.
    let revoked_key =
        AccountAbstractionDataKey::SignerRevoked(account.clone(), session_key.clone());
    if env.storage().persistent().has(&revoked_key) {
        return Err(AccountAbstractionError::SignerRevoked);
    }

    let key = AccountAbstractionDataKey::SessionKey(account.clone(), session_key.clone());
    let session_meta: SessionKeyMetadata = env
        .storage()
        .persistent()
        .get(&key)
        .ok_or(AccountAbstractionError::SessionNotFound)?;

    if env.ledger().timestamp() > session_meta.expires_at {
        return Err(AccountAbstractionError::SessionExpired);
    }

    if payload.is_empty() {
        return Err(AccountAbstractionError::InvalidPayload);
    }

    let policy: SessionPolicy = env
        .storage()
        .persistent()
        .get(&AccountAbstractionDataKey::SessionPolicy(
            account.clone(),
            session_key.clone(),
        ))
        .unwrap_or_else(|| unrestricted_policy(&env));

    // Unrestricted keys still accept the legacy opaque payload. A key with a
    // spend cap or an allowlist must present a `SessionInvocation`.
    if policy_is_enforced(&policy) {
        enforce_policy(&env, &account, &session_key, &policy, &payload)?;
    }

    env.events().publish(
        (
            Symbol::new(&env, "SESSION"),
            Symbol::new(&env, "EXECUTED"),
            account,
        ),
        (session_key, env.crypto().sha256(&payload).to_bytes()),
    );

    Ok(Bytes::new(&env))
}

fn policy_is_enforced(policy: &SessionPolicy) -> bool {
    policy.max_amount_per_tx.is_some()
        || policy.max_amount_per_window.is_some()
        || !policy.allowed_contracts.is_empty()
        || !policy.allowed_functions.is_empty()
}

fn enforce_policy(
    env: &Env,
    account: &Address,
    session_key: &Address,
    policy: &SessionPolicy,
    payload: &Bytes,
) -> Result<(), AccountAbstractionError> {
    let call = SessionInvocation::from_xdr(env, payload)
        .map_err(|_| AccountAbstractionError::InvalidPayload)?;
    if call.amount < 0 {
        return Err(AccountAbstractionError::InvalidPayload);
    }

    if !policy.allowed_contracts.is_empty()
        && !contains_address(&policy.allowed_contracts, &call.target_contract)
    {
        return reject(
            env,
            account,
            session_key,
            "contract",
            AccountAbstractionError::ContractNotAllowed,
        );
    }

    if !policy.allowed_functions.is_empty()
        && !contains_symbol(&policy.allowed_functions, &call.function)
    {
        return reject(
            env,
            account,
            session_key,
            "function",
            AccountAbstractionError::FunctionNotAllowed,
        );
    }

    if let Some(max_per_tx) = policy.max_amount_per_tx {
        if call.amount > max_per_tx {
            return reject(
                env,
                account,
                session_key,
                "max_per_tx",
                AccountAbstractionError::SpendLimitExceeded,
            );
        }
    }

    if let Some(window_max) = policy.max_amount_per_window {
        apply_window_budget(env, account, session_key, policy, call.amount, window_max)?;
    }

    Ok(())
}

#[allow(deprecated)] // events::publish — migrate to #[contractevent] in a follow-up
fn apply_window_budget(
    env: &Env,
    account: &Address,
    session_key: &Address,
    policy: &SessionPolicy,
    amount: i128,
    window_max: i128,
) -> Result<(), AccountAbstractionError> {
    let now = env.ledger().timestamp();
    let spend_key = AccountAbstractionDataKey::SessionSpend(account.clone(), session_key.clone());
    let mut state: SessionSpendState =
        env.storage()
            .persistent()
            .get(&spend_key)
            .unwrap_or(SessionSpendState {
                window_start: now,
                spent_in_window: 0,
            });

    let mut rolled_from = None;
    if policy.window_secs > 0 {
        let window_end = state.window_start.saturating_add(policy.window_secs);
        if now >= window_end {
            rolled_from = Some(state.window_start);
            state.window_start = now;
            state.spent_in_window = 0;
        }
    }

    let next = state
        .spent_in_window
        .checked_add(amount)
        .ok_or(AccountAbstractionError::InvalidPayload)?;
    if next > window_max {
        return reject(
            env,
            account,
            session_key,
            "window_budget",
            AccountAbstractionError::WindowBudgetExceeded,
        );
    }

    if let Some(previous_start) = rolled_from {
        env.events().publish(
            (
                Symbol::new(env, "SESSION"),
                Symbol::new(env, "WINDOW_ROLLOVER"),
                account.clone(),
            ),
            (session_key.clone(), previous_start, state.window_start),
        );
    }

    state.spent_in_window = next;
    env.storage().persistent().set(&spend_key, &state);
    Ok(())
}

#[allow(deprecated)] // events::publish — migrate to #[contractevent] in a follow-up
fn reject(
    env: &Env,
    account: &Address,
    session_key: &Address,
    reason: &str,
    error: AccountAbstractionError,
) -> Result<(), AccountAbstractionError> {
    env.events().publish(
        (
            Symbol::new(env, "SESSION"),
            Symbol::new(env, "POLICY_VIOLATION"),
            account.clone(),
        ),
        (session_key.clone(), Symbol::new(env, reason)),
    );
    Err(error)
}

fn contains_address(list: &Vec<Address>, value: &Address) -> bool {
    list.iter().any(|item| item == *value)
}

fn contains_symbol(list: &Vec<Symbol>, value: &Symbol) -> bool {
    list.iter().any(|item| item == *value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::{
        contract, contractimpl,
        testutils::{Address as _, Ledger as _},
        Address, Bytes, Env,
    };

    /// Storage and events are scoped to the contract that is executing.
    /// These helpers are free functions, so the tests run them inside one contract.
    #[contract]
    struct SessionContext;

    #[contractimpl]
    impl SessionContext {}

    fn in_contract(env: &Env, body: impl FnOnce()) {
        let contract_id = env.register(SessionContext, ());
        env.as_contract(&contract_id, body);
    }

    #[test]
    fn test_register_and_execute_with_session() {
        let env = Env::default();
        env.mock_all_auths();
        in_contract(&env, || {
            let account = Address::generate(&env);
            let session_key = Address::generate(&env);
            let expires_at = env.ledger().timestamp() + 3600;

            assert!(register_session_key(
                env.clone(),
                account.clone(),
                session_key.clone(),
                expires_at
            )
            .is_ok());

            let payload = Bytes::from_slice(&env, b"test_payload");
            let res =
                execute_with_session(env.clone(), account.clone(), session_key.clone(), payload);
            assert!(res.is_ok());
        });
    }

    #[test]
    fn test_register_session_key_expired_ledger_rejected() {
        let env = Env::default();
        env.mock_all_auths();
        // Ledger time starts at 0, and saturating_sub would stay at 0.
        env.ledger().set_timestamp(1_000);
        in_contract(&env, || {
            let account = Address::generate(&env);
            let session_key = Address::generate(&env);
            // expires_at in the past — registration succeeds (storage is permissive)
            // but execution immediately fails with SessionExpired.
            let expires_at = env.ledger().timestamp().saturating_sub(1);

            let res = register_session_key(
                env.clone(),
                account.clone(),
                session_key.clone(),
                expires_at,
            );
            assert!(res.is_ok(), "registration itself should succeed");

            // Execute should immediately reject because the session is expired.
            let payload = Bytes::from_slice(&env, b"test_payload");
            let res = execute_with_session(env.clone(), account, session_key, payload);
            assert_eq!(res, Err(AccountAbstractionError::SessionExpired));
        });
    }

    #[test]
    fn test_execute_with_expired_session() {
        let env = Env::default();
        env.mock_all_auths();
        env.ledger().set_timestamp(1_000);
        in_contract(&env, || {
            let account = Address::generate(&env);
            let session_key = Address::generate(&env);
            let expires_at = env.ledger().timestamp().saturating_sub(10);

            let _ = register_session_key(
                env.clone(),
                account.clone(),
                session_key.clone(),
                expires_at,
            );

            let payload = Bytes::from_slice(&env, b"test_payload");
            let res = execute_with_session(env.clone(), account, session_key, payload);
            assert_eq!(res, Err(AccountAbstractionError::SessionExpired));
        });
    }

    #[test]
    fn test_session_payload_hash_is_deterministic_and_distinct() {
        let env = Env::default();
        let first = Bytes::from_slice(&env, b"same_payload");
        let second = Bytes::from_slice(&env, b"same_payload");
        let different = Bytes::from_slice(&env, b"different_payload");

        assert_eq!(
            env.crypto().sha256(&first).to_bytes(),
            env.crypto().sha256(&second).to_bytes()
        );
        assert_ne!(
            env.crypto().sha256(&first).to_bytes(),
            env.crypto().sha256(&different).to_bytes()
        );
    }

    #[test]
    fn test_revoke_session_key() {
        let env = Env::default();
        env.mock_all_auths();
        in_contract(&env, || {
            let account = Address::generate(&env);
            let session_key = Address::generate(&env);
            let expires_at = env.ledger().timestamp() + 3600;

            let _ = register_session_key(
                env.clone(),
                account.clone(),
                session_key.clone(),
                expires_at,
            );

            assert!(revoke_session_key(env.clone(), account.clone(), session_key.clone()).is_ok());

            let payload = Bytes::from_slice(&env, b"test_payload");
            let res = execute_with_session(env.clone(), account, session_key, payload);
            assert_eq!(res, Err(AccountAbstractionError::SignerRevoked));
        });
    }

    #[test]
    fn test_execute_with_session_revoked_key_rejected() {
        let env = Env::default();
        env.mock_all_auths();
        in_contract(&env, || {
            let account = Address::generate(&env);
            let session_key = Address::generate(&env);
            let expires_at = env.ledger().timestamp() + 3600;

            let _ = register_session_key(
                env.clone(),
                account.clone(),
                session_key.clone(),
                expires_at,
            );

            // Pre-sign a payload while the signer is still active.
            let payload = Bytes::from_slice(&env, b"pre_signed_payload");

            // Merchant revokes the delegated signer before the payload is submitted.
            assert!(revoke_session_key(env.clone(), account.clone(), session_key.clone()).is_ok());

            // The in-flight, pre-signed payload must now be rejected.
            let res = execute_with_session(env.clone(), account, session_key, payload);
            assert_eq!(res, Err(AccountAbstractionError::SignerRevoked));
        });
    }

    #[test]
    fn test_reregister_clears_revocation() {
        let env = Env::default();
        env.mock_all_auths();
        in_contract(&env, || {
            let account = Address::generate(&env);
            let session_key = Address::generate(&env);
            let expires_at = env.ledger().timestamp() + 3600;

            let _ = register_session_key(
                env.clone(),
                account.clone(),
                session_key.clone(),
                expires_at,
            );
            assert!(revoke_session_key(env.clone(), account.clone(), session_key.clone()).is_ok());

            // Re-registering the signer clears the revocation flag.
            assert!(register_session_key(
                env.clone(),
                account.clone(),
                session_key.clone(),
                expires_at
            )
            .is_ok());

            let payload = Bytes::from_slice(&env, b"test_payload");
            let res = execute_with_session(env.clone(), account, session_key, payload);
            assert!(res.is_ok());
        });
    }
}
