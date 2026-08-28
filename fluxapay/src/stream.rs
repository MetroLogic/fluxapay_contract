use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, token, Address, Env, String, Symbol, Vec,
};

use crate::PaymentProcessor;

// ─── Data types ───────────────────────────────────────────────────────────────

/// Pure accrual computation (lazy formula with saturating arithmetic).
///
/// `total = min(deposit, checkpoint + (now - last_checkpoint) * rate)`
pub fn compute_total_accrued(
    accrued_at_checkpoint: i128,
    last_checkpoint_at: u64,
    now: u64,
    rate_per_second: i128,
    remaining_deposit: i128,
) -> i128 {
    let rate = rate_per_second.max(0);
    let deposit = remaining_deposit.max(0);
    let elapsed = now.saturating_sub(last_checkpoint_at);
    let newly_accrued = (elapsed as i128).saturating_mul(rate);
    accrued_at_checkpoint
        .saturating_add(newly_accrued)
        .min(deposit)
        .max(0)
}

/// Remaining deposit after a withdrawal; never negative.
pub fn compute_remaining_after_withdraw(remaining_deposit: i128, withdraw_amount: i128) -> i128 {
    remaining_deposit
        .saturating_sub(withdraw_amount.max(0))
        .max(0)
}

/// Status of a payment stream.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StreamStatus {
    /// Stream is actively draining the deposit at `rate_per_second`.
    Active,
    /// Stream was cancelled by the sender; residual deposit already refunded.
    Cancelled,
    /// Deposit was fully drained; stream reached its natural end.
    Exhausted,
    /// Stream is temporarily paused; accrual is frozen until resumed.
    Paused,
}

/// A continuous payment stream from `sender` to `receiver`.
///
/// Tokens flow at `rate_per_second` until either the deposit is exhausted or
/// the stream is cancelled. The sender may call [`decrease_rate_per_second`] at
/// any time to slow the flow; accrued amounts are check-pointed before the new
/// rate takes effect and any surplus deposit is refunded.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaymentStream {
    /// Unique identifier for this stream.
    pub stream_id: String,
    /// Address that funded the stream (payer).
    pub sender: Address,
    /// Address receiving the streamed tokens.
    pub receiver: Address,
    /// Optional fixed destination for withdrawals.
    pub destination: Option<Address>,
    /// Token contract address (e.g. USDC).
    pub token: Address,
    /// Current flow rate in the smallest token unit per second.
    pub rate_per_second: i128,
    /// Minimum allowed rate per second for this stream; prevents rate from falling below this.
    pub min_rate_per_second: i128,
    /// Total deposit locked in this contract on behalf of the stream.
    pub remaining_deposit: i128,
    /// Ledger timestamp of the last checkpoint.
    pub last_checkpoint_at: u64,
    /// Cumulative tokens accrued **up to** `last_checkpoint_at`.
    ///
    /// Accrual since the last checkpoint is calculated lazily:
    /// `total_accrued = accrued_at_checkpoint + (now - last_checkpoint_at) * rate_per_second`
    pub accrued_at_checkpoint: i128,
    /// Stream lifecycle state.
    pub status: StreamStatus,
    /// When false, distributions (withdrawals) are locked until the sender
    /// explicitly approves milestones for this stream.
    pub milestones_approved: bool,
}

/// Storage key for a [`PaymentStream`].
#[contracttype]
pub enum StreamDataKey {
    Stream(String),
    StreamFeeBps,
}

#[contracttype]
pub enum StreamIndexKey {
    SenderStream(Address, u32),
    SenderStreamCount(Address),
    RecipientStream(Address, u32),
    RecipientStreamCount(Address),
}

// ─── Errors ───────────────────────────────────────────────────────────────────

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum StreamError {
    /// No stream exists with the given ID.
    StreamNotFound = 1,
    /// Caller is not the sender of the stream.
    Unauthorized = 2,
    /// The new rate must be strictly less than the current rate.
    RateNotDecreased = 3,
    /// Rate cannot be zero or negative.
    InvalidRate = 4,
    /// A stream with that ID already exists.
    StreamAlreadyExists = 5,
    /// Deposit must be positive.
    InvalidDeposit = 6,
    /// Stream is not active.
    StreamNotActive = 7,
    /// A stream destination has not been configured for a permissionless withdrawal.
    DestinationNotSet = 8,
    /// The contract is currently globally paused.
    ContractPaused = 9,
    /// Stream distributions are locked until the sender approves milestones.
    MilestoneNotApproved = 10,
    /// Withdrawal already in progress (reentrancy guard).
    WithdrawalInProgress = 11,
    /// The new rate is below the minimum allowed rate for this stream.
    RateBelowMinimum = 12,
    /// Stream is not paused.
    StreamNotPaused = 13,
    /// Receiver address cannot be the same as sender address.
    InvalidReceiver = 14,
    /// Issue #627: A bulk operation was passed more stream IDs than the per-call cap.
    BatchTooLarge = 15,
}

/// Issue #627: Maximum number of stream IDs accepted by `bulk_bump_stream_ttls`
/// in a single call. Mirrors the 50-item cap on
/// `PaymentProcessor::bulk_bump_payment_ttls`.
pub const MAX_BULK_TTL_BUMP: u32 = 50;

/// Issue #627: TTL (in ledgers) that `bulk_bump_stream_ttls` extends each stream
/// entry to — ~3 years at 5s/ledger, matching `LONG_LIVE_TTL` in the payment
/// processor. The bump only fires when the remaining TTL drops below
/// `STREAM_TTL_BUMP / 5`.
pub const STREAM_TTL_BUMP: u32 = 18_921_600;

/// Storage key for the per-stream withdrawal reentrancy lock.
#[contracttype]
enum WithdrawLockKey {
    Lock(String),
}

fn acquire_lock(env: &Env, stream_id: &String) -> Result<(), StreamError> {
    let key = WithdrawLockKey::Lock(stream_id.clone());
    if env.storage().temporary().has(&key) {
        return Err(StreamError::WithdrawalInProgress);
    }
    env.storage().temporary().set(&key, &true);
    Ok(())
}

fn release_lock(env: &Env, stream_id: &String) {
    env.storage()
        .temporary()
        .remove(&WithdrawLockKey::Lock(stream_id.clone()));
}

fn require_not_paused(env: &Env) -> Result<(), StreamError> {
    if PaymentProcessor::is_paused(env.clone()) {
        return Err(StreamError::ContractPaused);
    }
    Ok(())
}

fn get_stream_fee_bps(env: &Env) -> i128 {
    env.storage()
        .persistent()
        .get(&StreamDataKey::StreamFeeBps)
        .unwrap_or(0i128)
}

/// Compute fee and net amounts: returns (fee, net).
fn apply_fee(amount: i128, fee_bps: i128) -> (i128, i128) {
    if fee_bps <= 0 {
        return (0, amount);
    }
    let fee = amount * fee_bps / 10_000;
    (fee, amount - fee)
}

fn get_sender_stream_count(env: &Env, sender: &Address) -> u32 {
    env.storage()
        .persistent()
        .get(&StreamIndexKey::SenderStreamCount(sender.clone()))
        .unwrap_or(0)
}

fn get_recipient_stream_count(env: &Env, recipient: &Address) -> u32 {
    env.storage()
        .persistent()
        .get(&StreamIndexKey::RecipientStreamCount(recipient.clone()))
        .unwrap_or(0)
}

fn append_sender_stream(env: &Env, sender: &Address, stream_id: &String) {
    let count = get_sender_stream_count(env, sender);
    env.storage().persistent().set(
        &StreamIndexKey::SenderStream(sender.clone(), count),
        stream_id,
    );
    env.storage().persistent().set(
        &StreamIndexKey::SenderStreamCount(sender.clone()),
        &(count + 1),
    );
}

fn append_recipient_stream(env: &Env, recipient: &Address, stream_id: &String) {
    let count = get_recipient_stream_count(env, recipient);
    env.storage().persistent().set(
        &StreamIndexKey::RecipientStream(recipient.clone(), count),
        stream_id,
    );
    env.storage().persistent().set(
        &StreamIndexKey::RecipientStreamCount(recipient.clone()),
        &(count + 1),
    );
}

fn get_sender_stream_id(env: &Env, sender: &Address, idx: u32) -> Option<String> {
    env.storage()
        .persistent()
        .get(&StreamIndexKey::SenderStream(sender.clone(), idx))
}

fn get_recipient_stream_id(env: &Env, recipient: &Address, idx: u32) -> Option<String> {
    env.storage()
        .persistent()
        .get(&StreamIndexKey::RecipientStream(recipient.clone(), idx))
}

// ─── Implementation ───────────────────────────────────────────────────────────
// PaymentStreaming is an internal helper called by PaymentProcessor.
// #[contract] marks it so testutils can register it; #[contractimpl] is
// conditional to avoid duplicate exported symbols in the PaymentProcessor WASM.

#[contract]
pub struct PaymentStreaming;

#[cfg_attr(
    any(not(target_arch = "wasm32"), feature = "contract-payment-streaming"),
    contractimpl
)]
#[allow(deprecated)] // events::publish — migrate to #[contractevent] in a follow-up
impl PaymentStreaming {
    /// Contract version bump helper.
    pub fn version() -> u32 {
        1
    }

    /// Set the platform fee in basis points applied to stream withdrawals.
    /// Admin auth is enforced by the caller (PaymentProcessor).
    pub fn set_stream_fee_bps(env: Env, fee_bps: i128) {
        env.storage()
            .persistent()
            .set(&StreamDataKey::StreamFeeBps, &fee_bps);
    }

    pub fn get_stream_fee_bps(env: Env) -> i128 {
        get_stream_fee_bps(&env)
    }

    // ─── Stream creation ──────────────────────────────────────────────────────

    /// Create a new payment stream.
    ///
    /// The caller (sender) transfers `deposit` tokens from their account into
    /// this contract. Streaming begins immediately at `rate_per_second`.
    ///
    /// # Parameters
    /// * `sender`         – Account funding the stream; must sign the transaction.
    /// * `receiver`       – Account that will receive streamed tokens.
    /// * `token`          – Token contract address.
    /// * `rate_per_second`– Tokens per second to stream.
    /// * `deposit`        – Total tokens deposited upfront.
    /// * `stream_id`      – Caller-supplied unique identifier.
    pub fn create_stream(
        env: Env,
        sender: Address,
        receiver: Address,
        token: Address,
        rate_per_second: i128,
        deposit: i128,
        stream_id: String,
        min_rate: Option<i128>,
    ) -> Result<PaymentStream, StreamError> {
        sender.require_auth();
        if sender == receiver {
            return Err(StreamError::InvalidReceiver);
        }

        if rate_per_second <= 0 {
            return Err(StreamError::InvalidRate);
        }
        if deposit <= 0 {
            return Err(StreamError::InvalidDeposit);
        }
        if env
            .storage()
            .persistent()
            .has(&StreamDataKey::Stream(stream_id.clone()))
        {
            return Err(StreamError::StreamAlreadyExists);
        }

        let now = env.ledger().timestamp();
        let min_rate_per_second = min_rate.unwrap_or(1);
        let stream = PaymentStream {
            stream_id: stream_id.clone(),
            sender,
            receiver: receiver.clone(),
            destination: None,
            token: token.clone(),
            rate_per_second,
            min_rate_per_second,
            remaining_deposit: deposit,
            last_checkpoint_at: now,
            accrued_at_checkpoint: 0,
            status: StreamStatus::Active,
            milestones_approved: false,
        };

        // Persist state before interaction (reentrancy protection)
        env.storage()
            .persistent()
            .set(&StreamDataKey::Stream(stream_id.clone()), &stream);
        append_sender_stream(&env, &stream.sender, &stream_id);
        append_recipient_stream(&env, &stream.receiver, &stream_id);

        // Interaction: Transfer deposit from sender into this contract.
        let token_client = token::Client::new(&env, &token);
        token_client.transfer(&stream.sender, env.current_contract_address(), &deposit);

        env.events().publish(
            (Symbol::new(&env, "STREAM"), Symbol::new(&env, "CREATED")),
            (
                stream_id.clone(),
                stream.sender.clone(),
                stream.receiver.clone(),
                deposit,
            ),
        );

        Ok(stream)
    }

    /// Configure a fixed destination address for a stream.
    ///
    /// This lets anyone trigger a withdrawal later, while the recipient
    /// retains control of where funds are routed.
    ///
    /// Auth: only the stream's `receiver` may call this (recipient.require_auth()
    /// + ownership check). Rejects if the stream is not Active.
    pub fn set_stream_destination(
        env: Env,
        recipient: Address,
        stream_id: String,
        destination: Address,
    ) -> Result<(), StreamError> {
        recipient.require_auth();

        let mut stream: PaymentStream = env
            .storage()
            .persistent()
            .get(&StreamDataKey::Stream(stream_id.clone()))
            .ok_or(StreamError::StreamNotFound)?;

        // Auth check: caller must be the stream's receiver
        if stream.receiver != recipient {
            return Err(StreamError::Unauthorized);
        }
        if stream.status != StreamStatus::Active {
            return Err(StreamError::StreamNotActive);
        }
        if stream.sender == destination {
            return Err(StreamError::InvalidReceiver);
        }

        stream.destination = Some(destination.clone());

        env.storage()
            .persistent()
            .set(&StreamDataKey::Stream(stream_id.clone()), &stream);

        env.events().publish(
            (
                Symbol::new(&env, "STREAM"),
                Symbol::new(&env, "DESTINATION_SET"),
            ),
            (stream_id, recipient, destination),
        );

        Ok(())
    }

    /// Sender approves milestones for a stream, unlocking distributions.
    pub fn approve_stream_milestone(
        env: Env,
        sender: Address,
        stream_id: String,
    ) -> Result<(), StreamError> {
        sender.require_auth();

        let mut stream: PaymentStream = env
            .storage()
            .persistent()
            .get(&StreamDataKey::Stream(stream_id.clone()))
            .ok_or(StreamError::StreamNotFound)?;

        if stream.sender != sender {
            return Err(StreamError::Unauthorized);
        }

        stream.milestones_approved = true;
        env.storage()
            .persistent()
            .set(&StreamDataKey::Stream(stream_id.clone()), &stream);

        env.events().publish(
            (
                Symbol::new(&env, "STREAM"),
                Symbol::new(&env, "MILESTONE_APPROVED"),
            ),
            (stream_id, sender),
        );

        Ok(())
    }

    /// Sender revokes a previous milestone approval, re-locking distributions
    /// until `approve_stream_milestone` is called again.
    pub fn revoke_stream_milestone(
        env: Env,
        sender: Address,
        stream_id: String,
    ) -> Result<(), StreamError> {
        sender.require_auth();

        let mut stream: PaymentStream = env
            .storage()
            .persistent()
            .get(&StreamDataKey::Stream(stream_id.clone()))
            .ok_or(StreamError::StreamNotFound)?;

        if stream.sender != sender {
            return Err(StreamError::Unauthorized);
        }

        stream.milestones_approved = false;
        env.storage()
            .persistent()
            .set(&StreamDataKey::Stream(stream_id.clone()), &stream);

        env.events().publish(
            (
                Symbol::new(&env, "STREAM"),
                Symbol::new(&env, "MILESTONE_REVOKED"),
                stream_id,
            ),
            (sender,),
        );

        Ok(())
    }

    /// Sender sets a floor below which [`Self::decrease_rate_per_second`] will
    /// refuse to lower the stream's rate.
    pub fn set_stream_min_rate(
        env: Env,
        sender: Address,
        stream_id: String,
        min_rate_per_second: i128,
    ) -> Result<(), StreamError> {
        sender.require_auth();

        if min_rate_per_second < 0 {
            return Err(StreamError::InvalidRate);
        }

        let mut stream: PaymentStream = env
            .storage()
            .persistent()
            .get(&StreamDataKey::Stream(stream_id.clone()))
            .ok_or(StreamError::StreamNotFound)?;

        if stream.sender != sender {
            return Err(StreamError::Unauthorized);
        }

        stream.min_rate_per_second = min_rate_per_second;
        env.storage()
            .persistent()
            .set(&StreamDataKey::Stream(stream_id.clone()), &stream);

        Ok(())
    }

    // ─── Rate decrease ────────────────────────────────────────────────────────

    /// Reduce the flow rate of an active stream.
    ///
    /// This function:
    /// 1. **Checkpoints** the `accrued_amount` and current timestamp so that
    ///    earnings up to this moment are locked in before the rate changes.
    /// 2. **Refunds** any portion of the deposit that exceeds the tokens still
    ///    accruing at the new (lower) rate:
    ///    `surplus = remaining_deposit - accrued_since_checkpoint - new_needed`
    ///    where `new_needed` is what the new rate needs to sustain for the same
    ///    remaining seconds the old rate would have run.
    /// 3. **Emits** a `RateDecreased` event.
    ///
    /// # Parameters
    /// * `sender`       – Must be the original stream sender; must sign.
    /// * `stream_id`    – Stream to update.
    /// * `new_rate`     – New rate (must be strictly less than the current rate).
    pub fn decrease_rate_per_second(
        env: Env,
        sender: Address,
        stream_id: String,
        new_rate: i128,
    ) -> Result<(), StreamError> {
        sender.require_auth();

        // ── Load stream ──────────────────────────────────────────────────────
        let mut stream: PaymentStream = env
            .storage()
            .persistent()
            .get(&StreamDataKey::Stream(stream_id.clone()))
            .ok_or(StreamError::StreamNotFound)?;

        // ── Authorization ────────────────────────────────────────────────────
        if stream.sender != sender {
            return Err(StreamError::Unauthorized);
        }

        // ── Validation ───────────────────────────────────────────────────────
        if stream.status != StreamStatus::Active {
            return Err(StreamError::StreamNotActive);
        }
        if new_rate <= 0 {
            return Err(StreamError::InvalidRate);
        }
        if new_rate >= stream.rate_per_second {
            return Err(StreamError::RateNotDecreased);
        }
        if new_rate < stream.min_rate_per_second {
            return Err(StreamError::RateBelowMinimum);
        }

        let now = env.ledger().timestamp();
        let old_rate = stream.rate_per_second;

        // ── Step 1: Checkpoint accrued_amount ─────────────────────────────────
        // Calculate how many tokens accrued since the last checkpoint.
        let elapsed = now.saturating_sub(stream.last_checkpoint_at);
        let newly_accrued = (elapsed as i128).saturating_mul(old_rate);

        // Clamp so we never accrue more than the remaining deposit.
        let newly_accrued =
            newly_accrued.min(stream.remaining_deposit - stream.accrued_at_checkpoint);

        stream.accrued_at_checkpoint = stream.accrued_at_checkpoint.saturating_add(newly_accrued);
        stream.last_checkpoint_at = now;

        // ── Step 2: Calculate surplus and refund ──────────────────────────────
        // Tokens not yet transferred to the receiver that are no longer needed
        // given the lower rate are returned to the sender.
        //
        // remaining_unlocked = remaining_deposit − accrued_at_checkpoint
        //   (i.e. the portion of deposit not yet "earned" by receiver)
        //
        // With the old rate those unlocked tokens would last:
        //   old_seconds_left = remaining_unlocked / old_rate
        //
        // At the new (lower) rate those same seconds need fewer tokens:
        //   new_needed = old_seconds_left * new_rate
        //
        // surplus = remaining_unlocked − new_needed
        let remaining_unlocked = stream
            .remaining_deposit
            .saturating_sub(stream.accrued_at_checkpoint);

        let surplus = if remaining_unlocked > 0 && old_rate > 0 {
            // integer division — gives floor of seconds left at old rate
            let old_seconds_left = remaining_unlocked / old_rate;
            let new_needed = old_seconds_left.saturating_mul(new_rate);
            remaining_unlocked.saturating_sub(new_needed).max(0)
        } else {
            0
        };

        stream.rate_per_second = new_rate;
        stream.remaining_deposit = stream.remaining_deposit.saturating_sub(surplus);

        // Persist state before interaction (reentrancy protection)
        env.storage()
            .persistent()
            .set(&StreamDataKey::Stream(stream_id.clone()), &stream);

        // Interaction: Transfer surplus back to sender.
        if surplus > 0 {
            let token_client = token::Client::new(&env, &stream.token);
            token_client.transfer(&env.current_contract_address(), &stream.sender, &surplus);
        }

        // ── Step 3: Emit RateDecreased event ─────────────────────────────────
        env.events().publish(
            (
                Symbol::new(&env, "STREAM"),
                Symbol::new(&env, "RATE_DECREASED"),
            ),
            (stream_id, sender, old_rate, new_rate, surplus),
        );

        Ok(())
    }

    // ─── Read helpers ─────────────────────────────────────────────────────────

    /// Return the stored stream, or `StreamNotFound`.
    pub fn get_stream(env: Env, stream_id: String) -> Result<PaymentStream, StreamError> {
        env.storage()
            .persistent()
            .get(&StreamDataKey::Stream(stream_id))
            .ok_or(StreamError::StreamNotFound)
    }

    /// Compute the total tokens accrued at the current ledger timestamp.
    ///
    /// This is a **view-only** helper and does not modify state.
    pub fn get_accrued_amount(env: Env, stream_id: String) -> Result<i128, StreamError> {
        let stream: PaymentStream = env
            .storage()
            .persistent()
            .get(&StreamDataKey::Stream(stream_id))
            .ok_or(StreamError::StreamNotFound)?;

        if stream.status != StreamStatus::Active {
            return Ok(stream.accrued_at_checkpoint);
        }

        let now = env.ledger().timestamp();
        Ok(compute_total_accrued(
            stream.accrued_at_checkpoint,
            stream.last_checkpoint_at,
            now,
            stream.rate_per_second,
            stream.remaining_deposit,
        ))
    }

    /// Query streams created by the given sender.
    ///
    /// Results are paginated and capped at 100 entries per page.
    pub fn get_sender_streams(
        env: Env,
        sender: Address,
        page: u32,
        page_size: u32,
    ) -> Vec<PaymentStream> {
        let page_size = page_size.min(100);
        let count = get_sender_stream_count(&env, &sender);
        let start = page.saturating_mul(page_size);
        let end = core::cmp::min(start.saturating_add(page_size), count);

        let mut streams = Vec::new(&env);
        for idx in start..end {
            if let Some(stream_id) = get_sender_stream_id(&env, &sender, idx) {
                if let Some(stream) = env
                    .storage()
                    .persistent()
                    .get(&StreamDataKey::Stream(stream_id.clone()))
                {
                    streams.push_back(stream);
                }
            }
        }
        streams
    }

    /// Withdraw accrued funds from up to `max_streams` active streams for the recipient.
    ///
    /// This helper scans the recipient stream index and stops after processing the
    /// configured number of active streams to avoid excessive gas usage.
    pub fn withdraw_all_for_recipient(
        env: Env,
        recipient: Address,
        max_streams: u32,
    ) -> Result<Vec<String>, StreamError> {
        recipient.require_auth();
        require_not_paused(&env)?;

        let count = get_recipient_stream_count(&env, &recipient);
        let mut processed = Vec::new(&env);
        let mut processed_count = 0u32;

        for idx in 0..count {
            if processed_count >= max_streams {
                break;
            }

            if let Some(stream_id) = get_recipient_stream_id(&env, &recipient, idx) {
                let mut stream: PaymentStream = env
                    .storage()
                    .persistent()
                    .get(&StreamDataKey::Stream(stream_id.clone()))
                    .ok_or(StreamError::StreamNotFound)?;

                if stream.receiver != recipient || stream.status != StreamStatus::Active {
                    continue;
                }

                // Skip streams that are locked by unapproved milestones.
                if !stream.milestones_approved {
                    continue;
                }

                let now = env.ledger().timestamp();
                let elapsed = now.saturating_sub(stream.last_checkpoint_at);
                let newly_accrued = (elapsed as i128)
                    .saturating_mul(stream.rate_per_second)
                    .min(stream.remaining_deposit - stream.accrued_at_checkpoint);
                stream.accrued_at_checkpoint =
                    stream.accrued_at_checkpoint.saturating_add(newly_accrued);
                stream.last_checkpoint_at = now;

                let withdrawable = stream
                    .accrued_at_checkpoint
                    .min(stream.remaining_deposit)
                    .max(0);
                if withdrawable == 0 {
                    continue;
                }

                stream.accrued_at_checkpoint =
                    stream.accrued_at_checkpoint.saturating_sub(withdrawable);
                stream.remaining_deposit = stream.remaining_deposit.saturating_sub(withdrawable);
                if stream.remaining_deposit == 0 {
                    stream.status = StreamStatus::Exhausted;
                }

                env.storage()
                    .persistent()
                    .set(&StreamDataKey::Stream(stream_id.clone()), &stream);

                // Reentrancy guard: lock acquired after state is persisted
                acquire_lock(&env, &stream_id)?;
                let token_client = token::Client::new(&env, &stream.token);
                let fee_bps = get_stream_fee_bps(&env);
                let (fee, net) = apply_fee(withdrawable, fee_bps);
                if fee > 0 {
                    if let Some(admin) = crate::access_control::AccessControl::get_admin(&env) {
                        token_client.transfer(&env.current_contract_address(), &admin, &fee);
                    }
                }
                token_client.transfer(&env.current_contract_address(), &recipient, &net);
                release_lock(&env, &stream_id);

                env.events().publish(
                    (Symbol::new(&env, "STREAM"), Symbol::new(&env, "WITHDRAWN")),
                    (
                        stream_id.clone(),
                        recipient.clone(),
                        recipient.clone(),
                        withdrawable,
                        stream.remaining_deposit,
                    ),
                );

                processed.push_back(stream_id);
                processed_count = processed_count.saturating_add(1);
            }
        }

        Ok(processed)
    }

    /// Trigger a permissionless withdrawal for a stream with an explicit destination.
    ///
    /// Only streams that have a destination configured and are active may be
    /// processed through this entrypoint.
    pub fn trigger_withdrawal(env: Env, stream_id: String) -> Result<String, StreamError> {
        require_not_paused(&env)?;

        let mut stream: PaymentStream = env
            .storage()
            .persistent()
            .get(&StreamDataKey::Stream(stream_id.clone()))
            .ok_or(StreamError::StreamNotFound)?;

        if stream.status != StreamStatus::Active {
            return Err(StreamError::StreamNotActive);
        }

        if !stream.milestones_approved {
            return Err(StreamError::MilestoneNotApproved);
        }

        let destination = stream
            .destination
            .clone()
            .ok_or(StreamError::DestinationNotSet)?;

        // Reentrancy guard: acquire lock before any state mutation or transfer
        acquire_lock(&env, &stream_id)?;

        let now = env.ledger().timestamp();
        let elapsed = now.saturating_sub(stream.last_checkpoint_at);
        let newly_accrued = (elapsed as i128)
            .saturating_mul(stream.rate_per_second)
            .min(stream.remaining_deposit - stream.accrued_at_checkpoint);

        stream.accrued_at_checkpoint = stream.accrued_at_checkpoint.saturating_add(newly_accrued);
        stream.last_checkpoint_at = now;

        let withdrawable = stream
            .accrued_at_checkpoint
            .min(stream.remaining_deposit)
            .max(0);
        if withdrawable == 0 {
            env.storage()
                .persistent()
                .set(&StreamDataKey::Stream(stream_id.clone()), &stream);
            return Ok(stream_id);
        }

        stream.accrued_at_checkpoint = stream.accrued_at_checkpoint.saturating_sub(withdrawable);
        stream.remaining_deposit = stream.remaining_deposit.saturating_sub(withdrawable);
        if stream.remaining_deposit == 0 {
            stream.status = StreamStatus::Exhausted;
        }

        env.storage()
            .persistent()
            .set(&StreamDataKey::Stream(stream_id.clone()), &stream);

        let token_client = token::Client::new(&env, &stream.token);
        let fee_bps = get_stream_fee_bps(&env);
        let (fee, net) = apply_fee(withdrawable, fee_bps);
        if fee > 0 {
            if let Some(admin) = crate::access_control::AccessControl::get_admin(&env) {
                token_client.transfer(&env.current_contract_address(), &admin, &fee);
            }
        }
        token_client.transfer(&env.current_contract_address(), &destination, &net);

        release_lock(&env, &stream_id);

        env.events().publish(
            (Symbol::new(&env, "STREAM"), Symbol::new(&env, "WITHDRAWN")),
            (
                stream_id.clone(),
                stream.receiver.clone(),
                destination.clone(),
                withdrawable,
                stream.remaining_deposit,
            ),
        );

        Ok(stream_id)
    }

    /// Top up multiple streams in a single atomic transaction.
    ///
    /// The caller (sender) must be the sender of ALL specified streams.
    ///
    /// # Parameters
    /// * `sender`     – Must be the sender of all streams; must sign.
    /// * `top_ups`    – Vector of (stream_id, amount) tuples.
    pub fn top_up_multiple_streams(
        env: Env,
        sender: Address,
        top_ups: Vec<(String, i128)>,
    ) -> Result<(), StreamError> {
        sender.require_auth();

        for top_up in top_ups.iter() {
            let (stream_id, amount) = top_up;
            Self::top_up_stream_internal(&env, &sender, &stream_id, amount)?;
        }

        Ok(())
    }

    pub fn top_up_stream(
        env: Env,
        sender: Address,
        stream_id: String,
        amount: i128,
    ) -> Result<(), StreamError> {
        sender.require_auth();

        Self::top_up_stream_internal(&env, &sender, &stream_id, amount)
    }

    fn top_up_stream_internal(
        env: &Env,
        sender: &Address,
        stream_id: &String,
        amount: i128,
    ) -> Result<(), StreamError> {
        if amount <= 0 {
            return Err(StreamError::InvalidDeposit);
        }

        let mut stream: PaymentStream = env
            .storage()
            .persistent()
            .get(&StreamDataKey::Stream(stream_id.clone()))
            .ok_or(StreamError::StreamNotFound)?;

        if stream.sender != *sender {
            return Err(StreamError::Unauthorized);
        }
        if stream.status != StreamStatus::Active {
            return Err(StreamError::StreamNotActive);
        }

        stream.remaining_deposit = stream.remaining_deposit.saturating_add(amount);

        env.storage()
            .persistent()
            .set(&StreamDataKey::Stream(stream_id.clone()), &stream);

        let token_client = token::Client::new(env, &stream.token);
        token_client.transfer(sender, env.current_contract_address(), &amount);

        env.events().publish(
            (Symbol::new(env, "STREAM"), Symbol::new(env, "TOPPED_UP")),
            (stream_id.clone(), sender.clone(), amount),
        );

        Ok(())
    }

    // ─── Stream cancellation ──────────────────────────────────────────────────

    /// Cancel an active stream and refund any remaining un-accrued deposit to
    /// the sender.
    ///
    /// # Parameters
    /// * `sender`    – Must be the original stream sender; must sign.
    /// * `stream_id` – Stream to cancel.
    pub fn cancel_stream(env: Env, sender: Address, stream_id: String) -> Result<(), StreamError> {
        sender.require_auth();

        let mut stream: PaymentStream = env
            .storage()
            .persistent()
            .get(&StreamDataKey::Stream(stream_id.clone()))
            .ok_or(StreamError::StreamNotFound)?;

        if stream.sender != sender {
            return Err(StreamError::Unauthorized);
        }
        if stream.status != StreamStatus::Active {
            return Err(StreamError::StreamNotActive);
        }

        let now = env.ledger().timestamp();

        // Checkpoint accrued amount up to now
        let elapsed = now.saturating_sub(stream.last_checkpoint_at);
        let newly_accrued = (elapsed as i128)
            .saturating_mul(stream.rate_per_second)
            .min(stream.remaining_deposit - stream.accrued_at_checkpoint);
        stream.accrued_at_checkpoint = stream.accrued_at_checkpoint.saturating_add(newly_accrued);
        stream.last_checkpoint_at = now;

        let accrued = stream.accrued_at_checkpoint;
        let refund = stream.remaining_deposit.saturating_sub(accrued);

        // Effects: mark cancelled
        stream.status = StreamStatus::Cancelled;
        stream.remaining_deposit = accrued; // only accrued portion remains

        // Persist before interaction (CEI pattern)
        env.storage()
            .persistent()
            .set(&StreamDataKey::Stream(stream_id.clone()), &stream);

        // Interaction: send accrued amount to receiver, refund to sender
        let token_client = token::Client::new(&env, &stream.token);
        if accrued > 0 {
            token_client.transfer(&env.current_contract_address(), &stream.receiver, &accrued);
        }
        if refund > 0 {
            token_client.transfer(&env.current_contract_address(), &stream.sender, &refund);
        }

        env.events().publish(
            (Symbol::new(&env, "STREAM"), Symbol::new(&env, "CANCELLED")),
            (stream_id, sender, accrued, refund),
        );

        Ok(())
    }

    /// Pause an active stream, freezing accrual until resumed.
    ///
    /// Accrued tokens are check-pointed at the current timestamp so that
    /// earnings up to this moment are preserved. Only the stream sender may
    /// pause.
    ///
    /// # Parameters
    /// * `sender`    – Must be the original stream sender; must sign.
    /// * `stream_id` – Stream to pause.
    pub fn pause_stream(env: Env, sender: Address, stream_id: String) -> Result<(), StreamError> {
        sender.require_auth();

        let mut stream: PaymentStream = env
            .storage()
            .persistent()
            .get(&StreamDataKey::Stream(stream_id.clone()))
            .ok_or(StreamError::StreamNotFound)?;

        if stream.sender != sender {
            return Err(StreamError::Unauthorized);
        }
        if stream.status != StreamStatus::Active {
            return Err(StreamError::StreamNotActive);
        }

        // Checkpoint accrued amount up to now before freezing.
        let now = env.ledger().timestamp();
        let elapsed = now.saturating_sub(stream.last_checkpoint_at);
        let newly_accrued = (elapsed as i128)
            .saturating_mul(stream.rate_per_second)
            .min(stream.remaining_deposit - stream.accrued_at_checkpoint);
        stream.accrued_at_checkpoint = stream.accrued_at_checkpoint.saturating_add(newly_accrued);
        stream.last_checkpoint_at = now;

        stream.status = StreamStatus::Paused;

        env.storage()
            .persistent()
            .set(&StreamDataKey::Stream(stream_id.clone()), &stream);

        env.events().publish(
            (Symbol::new(&env, "STREAM"), Symbol::new(&env, "PAUSED")),
            (stream_id, sender),
        );

        Ok(())
    }

    /// Resume a paused stream, restarting accrual from the current timestamp.
    ///
    /// Only the stream sender may resume.
    ///
    /// # Parameters
    /// * `sender`    – Must be the original stream sender; must sign.
    /// * `stream_id` – Stream to resume.
    pub fn resume_stream(env: Env, sender: Address, stream_id: String) -> Result<(), StreamError> {
        sender.require_auth();

        let mut stream: PaymentStream = env
            .storage()
            .persistent()
            .get(&StreamDataKey::Stream(stream_id.clone()))
            .ok_or(StreamError::StreamNotFound)?;

        if stream.sender != sender {
            return Err(StreamError::Unauthorized);
        }
        if stream.status != StreamStatus::Paused {
            return Err(StreamError::StreamNotPaused);
        }

        // Reset checkpoint to now so accrual restarts from this moment.
        stream.last_checkpoint_at = env.ledger().timestamp();
        stream.status = StreamStatus::Active;

        env.storage()
            .persistent()
            .set(&StreamDataKey::Stream(stream_id.clone()), &stream);

        env.events().publish(
            (Symbol::new(&env, "STREAM"), Symbol::new(&env, "RESUMED")),
            (stream_id, sender),
        );

        Ok(())
    }

    /// Cancel multiple streams atomically.
    ///
    /// Each stream is cancelled in order; if any stream fails the whole call
    /// reverts.  Returns the list of cancelled stream IDs.
    ///
    /// # Parameters
    /// * `sender`     – Must be the sender of all streams; must sign.
    /// * `stream_ids` – IDs of streams to cancel.
    pub fn cancel_multiple_streams(
        env: Env,
        sender: Address,
        stream_ids: Vec<String>,
    ) -> Result<Vec<String>, StreamError> {
        sender.require_auth();

        let mut cancelled = Vec::new(&env);
        for stream_id in stream_ids.iter() {
            let mut stream: PaymentStream = env
                .storage()
                .persistent()
                .get(&StreamDataKey::Stream(stream_id.clone()))
                .ok_or(StreamError::StreamNotFound)?;

            if stream.sender != sender {
                return Err(StreamError::Unauthorized);
            }
            if stream.status != StreamStatus::Active {
                return Err(StreamError::StreamNotActive);
            }

            let now = env.ledger().timestamp();
            let elapsed = now.saturating_sub(stream.last_checkpoint_at);
            let newly_accrued = (elapsed as i128)
                .saturating_mul(stream.rate_per_second)
                .min(stream.remaining_deposit - stream.accrued_at_checkpoint);
            stream.accrued_at_checkpoint =
                stream.accrued_at_checkpoint.saturating_add(newly_accrued);
            stream.last_checkpoint_at = now;

            let accrued = stream.accrued_at_checkpoint;
            let refund = stream.remaining_deposit.saturating_sub(accrued);

            // Effects
            stream.status = StreamStatus::Cancelled;
            stream.remaining_deposit = accrued;
            env.storage()
                .persistent()
                .set(&StreamDataKey::Stream(stream_id.clone()), &stream);

            // Interactions
            let token_client = token::Client::new(&env, &stream.token);
            if accrued > 0 {
                token_client.transfer(&env.current_contract_address(), &stream.receiver, &accrued);
            }
            if refund > 0 {
                token_client.transfer(&env.current_contract_address(), &stream.sender, &refund);
            }

            env.events().publish(
                (Symbol::new(&env, "STREAM"), Symbol::new(&env, "CANCELLED")),
                (stream_id.clone(), sender.clone(), accrued, refund),
            );

            cancelled.push_back(stream_id);
        }

        Ok(cancelled)
    }

    // ─── Dynamic rate adjustment ──────────────────────────────────────────────

    /// Update the flow rate of an active stream (increase or decrease).
    ///
    /// Unlike `decrease_rate_per_second`, this function allows both increases
    /// and decreases. Accrued tokens are check-pointed at the current rate
    /// before the new rate takes effect.
    ///
    /// When the rate is **decreased**, any surplus deposit is refunded to the
    /// sender (same logic as `decrease_rate_per_second`).
    /// When the rate is **increased**, no additional deposit is required; the
    /// existing deposit simply drains faster.
    ///
    /// # Parameters
    /// * `sender`    – Must be the original stream sender; must sign.
    /// * `stream_id` – Stream to update.
    /// * `new_rate`  – New flow rate (must be > 0).
    pub fn update_stream_rate(
        env: Env,
        sender: Address,
        stream_id: String,
        new_rate: i128,
    ) -> Result<(), StreamError> {
        sender.require_auth();
        require_not_paused(&env)?;

        let mut stream: PaymentStream = env
            .storage()
            .persistent()
            .get(&StreamDataKey::Stream(stream_id.clone()))
            .ok_or(StreamError::StreamNotFound)?;

        if stream.sender != sender {
            return Err(StreamError::Unauthorized);
        }
        if stream.status != StreamStatus::Active {
            return Err(StreamError::StreamNotActive);
        }
        if new_rate <= 0 {
            return Err(StreamError::InvalidRate);
        }

        let now = env.ledger().timestamp();
        let old_rate = stream.rate_per_second;

        // Checkpoint accrued amount at the old rate.
        let elapsed = now.saturating_sub(stream.last_checkpoint_at);
        let newly_accrued = (elapsed as i128)
            .saturating_mul(old_rate)
            .min(stream.remaining_deposit - stream.accrued_at_checkpoint);
        stream.accrued_at_checkpoint = stream.accrued_at_checkpoint.saturating_add(newly_accrued);
        stream.last_checkpoint_at = now;

        // When decreasing: refund surplus deposit no longer needed at the new rate.
        let surplus = if new_rate < old_rate {
            let remaining_unlocked = stream
                .remaining_deposit
                .saturating_sub(stream.accrued_at_checkpoint);
            if remaining_unlocked > 0 && old_rate > 0 {
                let old_seconds_left = remaining_unlocked / old_rate;
                let new_needed = old_seconds_left.saturating_mul(new_rate);
                remaining_unlocked.saturating_sub(new_needed).max(0)
            } else {
                0
            }
        } else {
            0
        };

        stream.rate_per_second = new_rate;
        stream.remaining_deposit = stream.remaining_deposit.saturating_sub(surplus);

        // Persist before interaction (CEI pattern).
        env.storage()
            .persistent()
            .set(&StreamDataKey::Stream(stream_id.clone()), &stream);

        if surplus > 0 {
            let token_client = token::Client::new(&env, &stream.token);
            token_client.transfer(&env.current_contract_address(), &stream.sender, &surplus);
        }

        env.events().publish(
            (
                Symbol::new(&env, "STREAM"),
                Symbol::new(&env, "RATE_UPDATED"),
            ),
            (stream_id, sender, old_rate, new_rate, surplus),
        );

        Ok(())
    }

    // ─── Expired stream cleanup ───────────────────────────────────────────────

    /// Close an exhausted or cancelled stream and remove its storage entry.
    ///
    /// This releases the contract storage slot for streams that have already
    /// reached a terminal state (`Exhausted` or `Cancelled`). Any remaining
    /// accrued balance is paid out to the receiver before the key is removed.
    ///
    /// Can be called by anyone (permissionless) once the stream is terminal,
    /// incentivising keepers to clean up storage.
    ///
    /// # Parameters
    /// * `stream_id` – Stream to close and remove.
    pub fn close_expired_stream(env: Env, stream_id: String) -> Result<(), StreamError> {
        let stream: PaymentStream = env
            .storage()
            .persistent()
            .get(&StreamDataKey::Stream(stream_id.clone()))
            .ok_or(StreamError::StreamNotFound)?;

        // Only terminal streams may be cleaned up.
        if stream.status == StreamStatus::Active {
            return Err(StreamError::StreamNotActive);
        }

        // Pay out any residual accrued balance to the receiver.
        let residual = stream
            .accrued_at_checkpoint
            .min(stream.remaining_deposit)
            .max(0);
        if residual > 0 {
            let token_client = token::Client::new(&env, &stream.token);
            token_client.transfer(&env.current_contract_address(), &stream.receiver, &residual);
        }

        // Remove the storage entry to reclaim ledger space.
        env.storage()
            .persistent()
            .remove(&StreamDataKey::Stream(stream_id.clone()));

        env.events().publish(
            (Symbol::new(&env, "STREAM"), Symbol::new(&env, "CLOSED")),
            (stream_id, stream.sender, stream.receiver, residual),
        );

        Ok(())
    }

    /// Cancel up to `MAX_BATCH_CANCEL` streams in a single transaction, skipping
    /// streams that are not active or not owned by `sender` instead of aborting.
    /// Returns the list of successfully cancelled stream IDs.
    pub fn batch_cancel_streams(
        env: Env,
        sender: Address,
        stream_ids: Vec<String>,
    ) -> Result<Vec<String>, StreamError> {
        const MAX_BATCH_CANCEL: u32 = 20;
        sender.require_auth();

        let mut cancelled = Vec::new(&env);
        let now = env.ledger().timestamp();

        for (i, stream_id) in stream_ids.iter().enumerate() {
            if i as u32 >= MAX_BATCH_CANCEL {
                break;
            }
            let mut stream: PaymentStream = match env
                .storage()
                .persistent()
                .get(&StreamDataKey::Stream(stream_id.clone()))
            {
                Some(s) => s,
                None => continue,
            };

            if stream.sender != sender || stream.status != StreamStatus::Active {
                continue;
            }

            let elapsed = now.saturating_sub(stream.last_checkpoint_at);
            let newly_accrued = (elapsed as i128)
                .saturating_mul(stream.rate_per_second)
                .min(stream.remaining_deposit - stream.accrued_at_checkpoint);
            stream.accrued_at_checkpoint =
                stream.accrued_at_checkpoint.saturating_add(newly_accrued);
            stream.last_checkpoint_at = now;

            let accrued = stream.accrued_at_checkpoint;
            let refund = stream.remaining_deposit.saturating_sub(accrued);

            stream.status = StreamStatus::Cancelled;
            stream.remaining_deposit = accrued;
            env.storage()
                .persistent()
                .set(&StreamDataKey::Stream(stream_id.clone()), &stream);

            let token_client = token::Client::new(&env, &stream.token);
            if accrued > 0 {
                token_client.transfer(&env.current_contract_address(), &stream.receiver, &accrued);
            }
            if refund > 0 {
                token_client.transfer(&env.current_contract_address(), &stream.sender, &refund);
            }

            env.events().publish(
                (Symbol::new(&env, "STREAM"), Symbol::new(&env, "CANCELLED")),
                (stream_id.clone(), sender.clone(), accrued, refund),
            );

            cancelled.push_back(stream_id);
        }

        Ok(cancelled)
    }

    // ─── Batch withdrawal ─────────────────────────────────────────────────────

    /// Withdraw accrued amounts from multiple streams to specified destinations
    /// in a single transaction.
    ///
    /// The caller must be the receiver of each stream.  Returns the list of
    /// stream IDs that were processed.
    ///
    /// # Parameters
    /// * `recipient`   – Must be the receiver of all streams; must sign.
    /// * `withdrawals` – Vector of `WithdrawalRecipient` (stream_id, destination, amount).
    pub fn batch_withdraw_to(
        env: Env,
        recipient: Address,
        withdrawals: Vec<crate::WithdrawalRecipient>,
    ) -> Result<Vec<String>, StreamError> {
        recipient.require_auth();

        let mut processed = Vec::new(&env);
        for w in withdrawals.iter() {
            let mut stream: PaymentStream = env
                .storage()
                .persistent()
                .get(&StreamDataKey::Stream(w.stream_id.clone()))
                .ok_or(StreamError::StreamNotFound)?;

            if stream.receiver != recipient {
                return Err(StreamError::Unauthorized);
            }
            if stream.status != StreamStatus::Active {
                return Err(StreamError::StreamNotActive);
            }

            // Disallow withdrawals until sender has approved milestones.
            if !stream.milestones_approved {
                return Err(StreamError::MilestoneNotApproved);
            }

            // Recompute accrued up to now
            let now = env.ledger().timestamp();
            let elapsed = now.saturating_sub(stream.last_checkpoint_at);
            let newly_accrued = (elapsed as i128)
                .saturating_mul(stream.rate_per_second)
                .min(stream.remaining_deposit - stream.accrued_at_checkpoint);
            stream.accrued_at_checkpoint =
                stream.accrued_at_checkpoint.saturating_add(newly_accrued);
            stream.last_checkpoint_at = now;

            let withdrawable = stream.accrued_at_checkpoint.min(w.amount).max(0);
            if withdrawable == 0 {
                continue;
            }

            // Effects
            stream.accrued_at_checkpoint =
                stream.accrued_at_checkpoint.saturating_sub(withdrawable);
            stream.remaining_deposit = stream.remaining_deposit.saturating_sub(withdrawable);

            if stream.remaining_deposit == 0 {
                stream.status = StreamStatus::Exhausted;
            }

            env.storage()
                .persistent()
                .set(&StreamDataKey::Stream(w.stream_id.clone()), &stream);

            // Interaction
            let token_client = token::Client::new(&env, &stream.token);
            let fee_bps = get_stream_fee_bps(&env);
            let (fee, net) = apply_fee(withdrawable, fee_bps);
            if fee > 0 {
                if let Some(admin) = crate::access_control::AccessControl::get_admin(&env) {
                    token_client.transfer(&env.current_contract_address(), &admin, &fee);
                }
            }
            token_client.transfer(&env.current_contract_address(), &w.destination, &net);

            env.events().publish(
                (Symbol::new(&env, "STREAM"), Symbol::new(&env, "WITHDRAWN")),
                (
                    w.stream_id.clone(),
                    recipient.clone(),
                    w.destination.clone(),
                    withdrawable,
                    stream.remaining_deposit,
                ),
            );

            processed.push_back(w.stream_id.clone());
        }

        Ok(processed)
    }

    // ─── Bulk TTL maintenance ─────────────────────────────────────────────────

    /// Issue #627: Extend the persistent-storage TTL of many stream entries in a
    /// single call, so contracts with many active streams pay one batched set of
    /// TTL writes instead of one per stream interaction.
    ///
    /// Mirrors `PaymentProcessor::bulk_bump_payment_ttls`:
    /// * Permissionless — TTL bumps only ever extend an entry's lifetime.
    /// * Stream IDs that don't resolve to a stored stream are silently skipped.
    /// * Rejects batches larger than [`MAX_BULK_TTL_BUMP`] (50).
    ///
    /// Returns the number of streams whose TTL was actually bumped.
    pub fn bulk_bump_stream_ttls(env: Env, stream_ids: Vec<String>) -> Result<u32, StreamError> {
        if stream_ids.len() > MAX_BULK_TTL_BUMP {
            return Err(StreamError::BatchTooLarge);
        }

        let threshold = core::cmp::max(1, STREAM_TTL_BUMP / 5);
        let mut bumped = 0u32;
        for stream_id in stream_ids.iter() {
            let key = StreamDataKey::Stream(stream_id.clone());
            if env.storage().persistent().has(&key) {
                env.storage()
                    .persistent()
                    .extend_ttl(&key, threshold, STREAM_TTL_BUMP);
                bumped += 1;
            }
        }

        env.events().publish(
            (Symbol::new(&env, "STREAM"), Symbol::new(&env, "TTL_BUMPED")),
            bumped,
        );

        Ok(bumped)
    }
}
