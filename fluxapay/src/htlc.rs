//! Hash Time-Locked Contract (HTLC) escrow module.
//!
//! Locks a token amount from a `sender` into on-chain escrow that can only be
//! released to a `receiver` by revealing the SHA-256 preimage of a
//! `hashlock`, or returned to `sender` once `timeout_ledger` has passed. This
//! is the building block for trustless conditional payments and cross-chain
//! atomic swaps. See `docs/htlc-guide.md` for the full design writeup.
//!
//! ## Issue #1021: permissionless timeout sweep
//!
//! `refund_htlc` already lets the original `sender` reclaim an expired HTLC,
//! but that requires the sender to notice the timeout and sign a
//! transaction. `claim_htlc_timeout` is the automation-friendly counterpart:
//! *anyone* may call it once `timeout_ledger` has passed, and it always
//! returns the escrowed funds to the original sender (never to the caller),
//! so a third-party sweep bot (see `scripts/htlc-sweep.js`) can clean up
//! expired HTLCs without holding any keys for the sender.

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, token, Address, Bytes, BytesN, Env,
    String, Symbol,
};

/// TTL threshold for persistent HTLC entries: once remaining TTL dips below
/// this many ledgers, the next write extends it back out to
/// `HTLC_TTL_EXTEND_TO` (~30 days at ~5s/ledger).
const HTLC_TTL_THRESHOLD: u32 = 86_400; // ~5 days
const HTLC_TTL_EXTEND_TO: u32 = 518_400; // ~30 days

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HtlcStatus {
    /// Funds are locked in escrow, awaiting claim or refund.
    Active,
    /// Receiver claimed the funds by revealing a valid preimage.
    Claimed,
    /// Sender (or anyone, via `claim_htlc_timeout`) reclaimed the funds after expiry.
    Refunded,
}

/// Persistent HTLC record.
///
/// `status` is updated in storage *before* any external token transfer, so a
/// reentrant call or a replay cannot drain the same escrow twice.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HtlcRecord {
    pub id: String,
    pub sender: Address,
    pub receiver: Address,
    pub token: Address,
    pub amount: i128,
    pub hashlock: BytesN<32>,
    /// Ledger sequence after which the escrow may be refunded (by `sender`
    /// via `refund_htlc`, or by anyone via `claim_htlc_timeout`).
    pub timeout_ledger: u32,
    pub status: HtlcStatus,
    pub created_at: u64,
    /// Ledger timestamp at which the HTLC was claimed or refunded, if resolved.
    pub resolved_at: Option<u64>,
}

#[contracttype]
pub enum HtlcDataKey {
    /// Monotonically increasing counter used to mint new HTLC ids.
    Counter,
    Htlc(String),
}

#[contracterror]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HtlcError {
    /// `amount` must be strictly positive.
    InvalidAmount = 1,
    /// `timeout_ledger` must be strictly greater than the current ledger sequence.
    InvalidTimeout = 2,
    /// No HTLC exists for the given id.
    NotFound = 3,
    /// The HTLC has already been claimed or refunded.
    AlreadyClaimed = 4,
    /// The caller is not authorized to perform this action on this HTLC.
    Unauthorized = 5,
    /// The supplied preimage does not hash to `hashlock`.
    PreimageMismatch = 6,
    /// The HTLC timelock has passed; claims must happen before `timeout_ledger`.
    Expired = 7,
    /// `timeout_ledger` has not yet passed; refunds are not yet allowed.
    NotExpired = 8,
}

#[contract]
pub struct Htlc;

#[cfg_attr(any(not(target_arch = "wasm32"), feature = "contract-htlc"), contractimpl)]
impl Htlc {
    /// Locks `amount` of `token` from `sender` into escrow, payable to
    /// `receiver` on reveal of the preimage of `hashlock`, or refundable to
    /// `sender` after `timeout_ledger`. Returns the new HTLC id.
    pub fn create_htlc(
        env: Env,
        sender: Address,
        receiver: Address,
        token: Address,
        amount: i128,
        hashlock: BytesN<32>,
        timeout_ledger: u32,
    ) -> Result<String, HtlcError> {
        sender.require_auth();

        if amount <= 0 {
            return Err(HtlcError::InvalidAmount);
        }
        if timeout_ledger <= env.ledger().sequence() {
            return Err(HtlcError::InvalidTimeout);
        }

        let next_id: u64 = env
            .storage()
            .instance()
            .get(&HtlcDataKey::Counter)
            .unwrap_or(0u64)
            + 1;
        env.storage()
            .instance()
            .set(&HtlcDataKey::Counter, &next_id);

        let id = crate::format_id(&env, "htlc_", next_id);

        let record = HtlcRecord {
            id: id.clone(),
            sender: sender.clone(),
            receiver: receiver.clone(),
            token: token.clone(),
            amount,
            hashlock,
            timeout_ledger,
            status: HtlcStatus::Active,
            created_at: env.ledger().timestamp(),
            resolved_at: None,
        };
        env.storage()
            .persistent()
            .set(&HtlcDataKey::Htlc(id.clone()), &record);
        Self::bump_htlc_ttl(&env, &id);

        // Interaction last: pull the deposit into this contract's custody.
        let token_client = token::Client::new(&env, &token);
        token_client.transfer(&sender, &env.current_contract_address(), &amount);

        env.events().publish(
            (Symbol::new(&env, "HTLC"), Symbol::new(&env, "CREATED")),
            (id.clone(), sender, receiver, token, amount, timeout_ledger),
        );

        Ok(id)
    }

    /// Releases the escrow to `claimant`, who must be the HTLC's `receiver`
    /// and must supply a `preimage` whose SHA-256 hash matches `hashlock`.
    /// Must happen strictly before `timeout_ledger`.
    pub fn claim_htlc(
        env: Env,
        htlc_id: String,
        claimant: Address,
        preimage: Bytes,
    ) -> Result<(), HtlcError> {
        claimant.require_auth();

        let mut record = Self::load(&env, &htlc_id)?;

        if record.status != HtlcStatus::Active {
            return Err(HtlcError::AlreadyClaimed);
        }
        if claimant != record.receiver {
            return Err(HtlcError::Unauthorized);
        }
        if env.ledger().sequence() >= record.timeout_ledger {
            return Err(HtlcError::Expired);
        }
        if env.crypto().sha256(&preimage).to_bytes() != record.hashlock {
            return Err(HtlcError::PreimageMismatch);
        }

        // Effect before interaction: mark claimed first to prevent double-claim.
        record.status = HtlcStatus::Claimed;
        record.resolved_at = Some(env.ledger().timestamp());
        env.storage()
            .persistent()
            .set(&HtlcDataKey::Htlc(htlc_id.clone()), &record);
        Self::bump_htlc_ttl(&env, &htlc_id);

        let token_client = token::Client::new(&env, &record.token);
        token_client.transfer(
            &env.current_contract_address(),
            &record.receiver,
            &record.amount,
        );

        env.events().publish(
            (Symbol::new(&env, "HTLC"), Symbol::new(&env, "CLAIMED")),
            (htlc_id, record.receiver, record.amount),
        );

        Ok(())
    }

    /// Returns the escrowed tokens to the original `sender`. Only the
    /// sender may call this, and only after `timeout_ledger` has passed.
    pub fn refund_htlc(env: Env, htlc_id: String, caller: Address) -> Result<(), HtlcError> {
        caller.require_auth();

        let mut record = Self::load(&env, &htlc_id)?;

        if record.status != HtlcStatus::Active {
            return Err(HtlcError::AlreadyClaimed);
        }
        if caller != record.sender {
            return Err(HtlcError::Unauthorized);
        }
        if env.ledger().sequence() < record.timeout_ledger {
            return Err(HtlcError::NotExpired);
        }

        record.status = HtlcStatus::Refunded;
        record.resolved_at = Some(env.ledger().timestamp());
        env.storage()
            .persistent()
            .set(&HtlcDataKey::Htlc(htlc_id.clone()), &record);
        Self::bump_htlc_ttl(&env, &htlc_id);

        let token_client = token::Client::new(&env, &record.token);
        token_client.transfer(
            &env.current_contract_address(),
            &record.sender,
            &record.amount,
        );

        env.events().publish(
            (Symbol::new(&env, "HTLC"), Symbol::new(&env, "REFUNDED")),
            (htlc_id, record.sender, record.amount),
        );

        Ok(())
    }

    /// Issue #1021: permissionless timeout sweep. Callable by *any* account
    /// (no `require_auth` needed from the caller - the contract itself
    /// authorizes moving its own escrowed balance). Succeeds only once
    /// `env.ledger().sequence() >= htlc.timeout_ledger`, and always refunds
    /// to the original `sender`, never to the caller.
    pub fn claim_htlc_timeout(env: Env, htlc_id: String) -> Result<(), HtlcError> {
        let mut record = Self::load(&env, &htlc_id)?;

        if record.status != HtlcStatus::Active {
            return Err(HtlcError::AlreadyClaimed);
        }
        if env.ledger().sequence() < record.timeout_ledger {
            return Err(HtlcError::NotExpired);
        }

        // Effect before interaction: mark refunded first to prevent a second
        // sweep (or a racing `refund_htlc`) from double-spending the escrow.
        record.status = HtlcStatus::Refunded;
        record.resolved_at = Some(env.ledger().timestamp());
        env.storage()
            .persistent()
            .set(&HtlcDataKey::Htlc(htlc_id.clone()), &record);
        Self::bump_htlc_ttl(&env, &htlc_id);

        let token_client = token::Client::new(&env, &record.token);
        token_client.transfer(
            &env.current_contract_address(),
            &record.sender,
            &record.amount,
        );

        env.events().publish(
            (
                Symbol::new(&env, "HTLC"),
                Symbol::new(&env, "TIMEOUT_REFUNDED"),
            ),
            (htlc_id, record.sender, record.amount),
        );

        Ok(())
    }

    /// Read-only lookup. Returns `HtlcError::NotFound` if the id does not exist.
    pub fn get_htlc(env: Env, htlc_id: String) -> Result<HtlcRecord, HtlcError> {
        Self::load(&env, &htlc_id)
    }

    fn load(env: &Env, htlc_id: &String) -> Result<HtlcRecord, HtlcError> {
        env.storage()
            .persistent()
            .get(&HtlcDataKey::Htlc(htlc_id.clone()))
            .ok_or(HtlcError::NotFound)
    }

    fn bump_htlc_ttl(env: &Env, htlc_id: &String) {
        env.storage().persistent().extend_ttl(
            &HtlcDataKey::Htlc(htlc_id.clone()),
            HTLC_TTL_THRESHOLD,
            HTLC_TTL_EXTEND_TO,
        );
    }
}
