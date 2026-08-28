use soroban_sdk::{contracterror, contracttype, Address, Bytes, BytesN, Env, Symbol};

/// Error types for account abstraction operations
#[contracterror]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AccountAbstractionError {
    Unauthorized = 1,
    SessionNotFound = 2,
    SessionExpired = 3,
    InvalidPayload = 4,
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

/// Account abstraction data keys for persistent storage
#[contracttype]
pub enum AccountAbstractionDataKey {
    /// Maps (account, session_key) -> SessionKeyMetadata
    SessionKey(Address, Address),
}

/// Register a new session key with an expiration timestamp for an account.
/// Requires authorization from the account owner.
pub fn register_session_key(
    env: Env,
    account: Address,
    session_key: Address,
    expires_at: u64,
) -> Result<(), AccountAbstractionError> {
    account.require_auth();

    let meta = SessionKeyMetadata {
        account: account.clone(),
        session_key: session_key.clone(),
        expires_at,
    };

    env.storage().persistent().set(
        &AccountAbstractionDataKey::SessionKey(account.clone(), session_key.clone()),
        &meta,
    );

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

/// Revoke an existing session key for an account.
/// Requires authorization from the account owner.
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
pub fn execute_with_session(
    env: Env,
    account: Address,
    session_key: Address,
    payload: Bytes,
) -> Result<Bytes, AccountAbstractionError> {
    session_key.require_auth();

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

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::{testutils::Address as _, Address, Bytes, Env};

    #[test]
    fn test_register_and_execute_with_session() {
        let env = Env::default();
        env.mock_all_auths();

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
        let res = execute_with_session(env.clone(), account.clone(), session_key.clone(), payload);
        assert!(res.is_ok());
    }

    #[test]
    fn test_execute_with_expired_session() {
        let env = Env::default();
        env.mock_all_auths();

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
        let res = execute_with_session(env, account, session_key, payload);
        assert_eq!(res, Err(AccountAbstractionError::SessionExpired));
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
        let res = execute_with_session(env, account, session_key, payload);
        assert_eq!(res, Err(AccountAbstractionError::SessionNotFound));
    }
}
