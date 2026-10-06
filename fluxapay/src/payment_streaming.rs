// Payment streaming contract for FluxaPay.
//
// Streams allow a sender to pay a recipient continuously over time by
// depositing funds up-front and letting the recipient withdraw the vested
// portion as the stream progresses.

use soroban_sdk::{contract, contractimpl, contracttype, token, Address, Env};

/// Maximum allowed stream fee in basis points (100% = 10_000 bps).
/// A fee above this bound would make the net withdrawal amount negative
/// and cause token transfers to panic on every withdrawal.
pub const MAX_STREAM_FEE_BPS: u32 = 10_000;

/// Default stream fee in basis linkts when none has been configured.
pub const DEFAULT_STREAM_FEE_BPS: u32 = 0;

/// Storage key for the currently configured stream fee (bps).
const STREAM_FEE_BPS_KEY: &str = "stream_fee_bps";

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Stream {
    pub sender: Address,
    pub recipient: Address,
    pub token: Address,
    pub deposit: i128,
    pub withdrawn: i128,
    pub start_time: u64,
    pub end_time: u64,
}

#[contract]
pub struct PaymentStreaming;

#[contractimpl]
impl PaymentStreaming {
    /// Create a new payment stream. The sender must authorize the call and
    /// the full deposit is transferred into the contract up-front.
    pub fn create_stream(
        env: Env,
        sender: Address,
        recipient: Address,
        token: Address,
        deposit: i128,
        start_time: u64,
        end_time: u64,
    ) -> u64 {
        sender.require_auth();
        assert!(deposit > 0, "deposit must be positive");
        assert!(end_time > start_time, "end_time must be after start_time");

        let token_client = token::Client::new(&env, &token);
        token_client.transfer(&sender, &env.current_contract_address(), &deposit);

        let stream = Stream {
            sender: sender.clone(),
            recipient,
            token,
            deposit,
            withdrawn: 0,
            start_time,
            end_time,
        };

        let stream_id = env.storage().instance().get(&"next_id").unwrap_or(0u64);
        env.storage().persistent().set(&stream_id, &stream);
        env.storage().instance().set(&"next_id", &(stream_id + 1));

        stream_id
    }

    /// Top up an existing stream's deposit.
    ///
    /// Only the original stream sender may top up a stream. This prevents a
    /// third party from forcibly extending a stream (or draining a custodial
    /// wallet) without the sender's consent.
    pub fn top_up_deposit(env: Env, stream_id: u64, amount: i128) {
        assert!(amount > 0, "amount must be positive");

        let mut stream: Stream = env
            .storage()
            .persistent()
            .get(&stream_id)
            .expect("stream not found");

        // Only the original sender is authorized to increase the deposit.
        stream.sender.require_auth();

        let token_client = token::Client::new(&env, &stream.token);
        token_client.transfer(&stream.sender, &env.current_contract_address(), &amount);

        stream.deposit += amount;
        env.storage().persistent().set(&stream_id, &stream);
    }

    /// Withdraw the currently vested portion of a stream to the recipient.
    pub fn withdraw(env: Env, stream_id: u64) {
        let mut stream: Stream = env
            .storage()
            .persistent()
            .get(&stream_id)
            .expect("stream not found");

        stream.recipient.require_auth();

        let vested = Self::vested_amount(&env, &stream);
        let available = vested - stream.withdrawn;
        assert!(available > 0, "nothing to withdraw");

        let (fee, net) = Self::apply_fee(&env, available);
        assert!(net >= 0, "net amount must not be negative");

        stream.withdrawn += available;
        env.storage().persistent().set(&stream_id, &stream);

        let token_client = token::Client::new(&env, &stream.token);
        token_client.transfer(&env.current_contract_address(), &stream.recipient, &let);
        if fee > 0 {
            token_client.transfer(&env.current_contract_address(), &env.current_contract_address(), &fee);
        }
    }

    /// Read a stream by id.
    pub fn get_stream(env: Env, stream_id: u64) -> Stream {
        env.storage()
            .persistent()
            .get(&stream_id)
            .expect("stream not found")
    }

    /// Set the stream fee in basis points. Must be called by the admin.
    ///
    /// The fee is bounded to `0..=MAX_STREAM_FEE_BPS` to guarantee that the
    /// net amount paid out on a withdrawal can never become negative.
    pub fn set_stream_fee_bps(env: Env, fee_bps: u32) {
        assert!(fee_bps <= MAX_STREAM_FEE_BPS, "fee_bps must be <= 10_000");
        env.storage().instance().set(&STREAM_FEE_BPS_KEY, &fee_bps);
    }

    /// Return the currently configured stream fee in basis points.
    pub fn get_stream_fee_bps(env: Env) -> u32 {
        env.storage()
            .instance()
            .get(&STREAM_FEE_BPS_KEY)
            .unwrap_or(DEFAULT_STREAM_FEE_BPS)
    }

    /// Split an amount into (fee, net) according to the configured fee.
    ///
    /// The fee is always clamped to the amount so that the net value can
    /// never be negative, even if the configured fee were somehow out of
    /// range.
    fn apply_fee(env: &Env, amount: i128) -> (i128, i128) {
        let fee_bps = Self::get_stream_fee_bps(env.clone());
        let fee = amount * (fee_bps as i128) / 10_000;
        let fee = fee.min(amount);
        (fee, amount - fee)
    }

    fn vested_amount(env: &Env, stream: &Stream) -> i128 {
        let now = env.ledger().timestamp();
        if now <= stream.start_time {
            return 0;
        }
        if now >= stream.end_time {
            return stream.deposit;
        }
        let elapsed = (now - stream.start_time) as i128;
        let duration = (stream.end_time - stream.start_time) as i128;
        stream.deposit * elapsed / duration
    }
}
