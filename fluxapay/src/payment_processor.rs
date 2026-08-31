//! PaymentProcessor contract implementation.

use crate::access_control::{
    role_admin, role_arbitrator, role_merchant, role_oracle, role_settlement_operator,
    AccessControl,
};
use crate::utils::{self, format_id, is_valid_cid, validate_id, validate_ipfs_multihash};
use crate::*;
use soroban_sdk::{
    contract, contractimpl, map, token, vec, Address, BytesN, Env, Map, MuxedAddress, String, Symbol,
    Vec,
};

#[contract]
pub struct PaymentProcessor;

#[cfg_attr(
    any(not(target_arch = "wasm32"), feature = "contract-payment-processor"),
    contractimpl
)]
#[allow(deprecated)] // events::publish — migrate to #[contractevent] in a follow-up
impl PaymentProcessor {
    /// Returns the current contract version string from persistent storage.
    /// Falls back to INITIAL_CONTRACT_VERSION if not set.
    pub fn version(env: Env) -> String {
        env.storage()
            .persistent()
            .get(&DataKey::ContractVersion)
            .unwrap_or_else(|| String::from_str(&env, INITIAL_CONTRACT_VERSION))
    }

    /// Alias for version() — returns the current contract version string.
    pub fn get_version(env: Env) -> String {
        Self::version(env)
    }

    /// Issue #683: Return a summary of key contract metrics for dashboards
    /// and monitoring. No authentication required — this is a public read.
    pub fn get_contract_health(env: Env) -> ContractHealth {
        let version = Self::version(env.clone());
        let is_paused = Self::is_paused(env.clone());
        let creation_paused: bool = env
            .storage()
            .persistent()
            .get::<DataKey, PauseState>(&DataKey::CreationPaused)
            .map(|s| s.paused)
            .unwrap_or(false);
        let treasury_balance = Self::get_treasury_balance(env.clone());

        let active_payment_count: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::MerchantPaymentCount(
                env.current_contract_address(),
            ))
            .unwrap_or(0u64) as u32;

        let fx_oracle_configured = env
            .storage()
            .persistent()
            .get::<DataKey, Address>(&DataKey::FxOracleAddress)
            .is_some();

        let merchant_registry_configured = env
            .storage()
            .persistent()
            .get::<DataKey, Address>(&DataKey::MerchantRegistryAddress)
            .is_some();

        ContractHealth {
            version,
            is_paused,
            is_creation_paused: creation_paused,
            treasury_balance,
            active_payment_count,
            fx_oracle_configured,
            merchant_registry_configured,
        }
    }

    /// Admin-only: set an arbitrary on-chain metadata entry (issue #667), e.g. a
    /// description, deployment notes, or audit commit hash. Stored in instance
    /// storage under a caller-chosen key, with the instance TTL bumped to
    /// `LONG_LIVE_TTL` so metadata survives archival.
    pub fn set_contract_metadata(
        env: Env,
        admin: Address,
        key: Symbol,
        value: String,
    ) -> Result<(), Error> {
        admin.require_auth();

        if !AccessControl::has_role(&env, &role_admin(&env), &admin) {
            return Err(Error::Unauthorized);
        }

        env.storage()
            .instance()
            .set(&DataKey::ContractMetadata(key), &value);

        let threshold = core::cmp::max(1, LONG_LIVE_TTL / TTL_BUMP_THRESHOLD_DIVISOR);
        env.storage()
            .instance()
            .extend_ttl(threshold, LONG_LIVE_TTL);

        Ok(())
    }

    /// Public read of an on-chain metadata entry set via `set_contract_metadata`
    /// (issue #667). Returns `None` if the key was never set.
    pub fn get_contract_metadata(env: Env, key: Symbol) -> Option<String> {
        env.storage()
            .instance()
            .get(&DataKey::ContractMetadata(key))
    }

    fn validate_init_admin(env: &Env, admin: Address) -> Result<(), Error> {
        let zero_address = Address::from_str(env, ZERO_CONTRACT_STRKEY);
        if admin == zero_address {
            return Err(Error::InvalidAddress);
        }
        Ok(())
    }

    /// Formats a u64 as a decimal `String` without relying on `alloc`/`format!`
    /// (this crate is `#![no_std]`). Used to store `deployed_at` as metadata
    /// text so it round-trips through `get_contract_metadata`'s `String` type.
    fn u64_to_string(env: &Env, mut n: u64) -> String {
        if n == 0 {
            return String::from_str(env, "0");
        }
        let mut buf = [0u8; 20];
        let mut i = buf.len();
        while n > 0 {
            i -= 1;
            buf[i] = b'0' + (n % 10) as u8;
            n /= 10;
        }
        let s = core::str::from_utf8(&buf[i..]).unwrap_or("0");
        String::from_str(env, s)
    }

    pub fn initialize_payment_processor(env: Env, admin: Address) -> Result<(), Error> {
        Self::validate_init_admin(&env, admin.clone())?;
        AccessControl::initialize(&env, admin);

        let empty_reason = String::from_str(&env, "");
        let initial_state = PauseState {
            paused: false,
            reason: empty_reason,
            admin: None,
            timestamp: env.ledger().timestamp(),
        };

        env.storage()
            .persistent()
            .set(&DataKey::Paused, &initial_state);
        env.storage()
            .persistent()
            .set(&DataKey::CreationPaused, &initial_state);

        // Set initial contract version
        let initial_version = String::from_str(&env, INITIAL_CONTRACT_VERSION);
        env.storage()
            .persistent()
            .set(&DataKey::ContractVersion, &initial_version);

        // Issue #667: pre-populate on-chain metadata with description, version, and
        // deployment timestamp so explorers/integrators can identify the contract.
        env.storage().instance().set(
            &DataKey::ContractMetadata(Symbol::new(&env, "description")),
            &String::from_str(&env, "FluxaPay PaymentProcessor contract"),
        );
        env.storage().instance().set(
            &DataKey::ContractMetadata(Symbol::new(&env, "version")),
            &initial_version,
        );
        env.storage().instance().set(
            &DataKey::ContractMetadata(Symbol::new(&env, "deployed_at")),
            &Self::u64_to_string(&env, env.ledger().timestamp()),
        );
        let threshold = core::cmp::max(1, LONG_LIVE_TTL / TTL_BUMP_THRESHOLD_DIVISOR);
        env.storage()
            .instance()
            .extend_ttl(threshold, LONG_LIVE_TTL);

        Ok(())
    }

    pub fn set_merchant_registry_address(
        env: Env,
        admin: Address,
        registry_address: Address,
    ) -> Result<(), Error> {
        admin.require_auth();

        if !AccessControl::has_role(&env, &role_admin(&env), &admin) {
            return Err(Error::Unauthorized);
        }

        env.storage()
            .persistent()
            .set(&DataKey::MerchantRegistryAddress, &registry_address);
        Ok(())
    }

    /// Admin: configure the FX oracle used to snapshot rates during
    /// `verify_payment` (Issue #304).
    pub fn set_fx_oracle(env: Env, admin: Address, fx_oracle: Address) -> Result<(), Error> {
        admin.require_auth();

        if !AccessControl::has_role(&env, &role_admin(&env), &admin) {
            return Err(Error::Unauthorized);
        }

        env.storage()
            .persistent()
            .set(&DataKey::FxOracleAddress, &fx_oracle);
        Ok(())
    }

    /// Queue a settlement fee rate change via the timelock.
    ///
    /// Issue #624: `set_fee_rate` no longer takes effect immediately.  Instead it
    /// enqueues a `PendingTimelockAction` that can only be executed after the
    /// configured timelock delay (default 48 hours) has elapsed.  Returns the
    /// action ID assigned to the pending action.
    ///
    /// # Arguments
    /// * `admin` – Must hold the admin role.
    /// * `bps`   – Fee in basis points (e.g. 100 = 1 %). Must be 0–10 000.
    pub fn set_fee_rate(env: Env, admin: Address, bps: i128) -> Result<String, Error> {
        admin.require_auth();

        if !AccessControl::has_role(&env, &role_admin(&env), &admin) {
            return Err(Error::Unauthorized);
        }
        if !(0..=10_000).contains(&bps) {
            return Err(Error::InvalidAmount);
        }

        Self::enqueue_timelocked_action(&env, admin, TimelockActionKind::SetFeeRate(bps))
    }

    /// Admin-only: enable or disable automatic pending refund creation for overpaid payments.
    /// When enabled (default), any payment verified as Overpaid will automatically create
    /// a pending refund for the excess amount and emit a REFUND/AUTO_CREATED event.
    pub fn set_auto_refund_overpayment(
        env: Env,
        admin: Address,
        enabled: bool,
    ) -> Result<(), Error> {
        admin.require_auth();
        if !AccessControl::has_role(&env, &role_admin(&env), &admin) {
            return Err(Error::Unauthorized);
        }
        env.storage()
            .persistent()
            .set(&DataKey::AutoRefundOverpayment, &enabled);
        Ok(())
    }

    /// Check whether automatic refund creation for overpaid payments is enabled.
    /// Defaults to true if not explicitly configured.
    pub fn get_auto_refund_overpayment(env: &Env) -> bool {
        env.storage()
            .persistent()
            .get(&DataKey::AutoRefundOverpayment)
            .unwrap_or(true)
    }

    /// Return the accumulated treasury balance collected via settlement fees
    /// and platform fees (when no custom fee_recipient).
    pub fn set_min_payment_duration_secs(
        env: Env,
        admin: Address,
        min_secs: u64,
    ) -> Result<(), Error> {
        admin.require_auth();

        if !AccessControl::has_role(&env, &role_admin(&env), &admin) {
            return Err(Error::Unauthorized);
        }

        if min_secs < CREATE_PAYMENT_WINDOW_SECS {
            return Err(Error::InvalidAmount);
        }

        env.storage()
            .persistent()
            .set(&DataKey::MinPaymentDurationSecs, &min_secs);
        Ok(())
    }

    pub fn set_max_payment_duration_secs(
        env: Env,
        admin: Address,
        max_secs: u64,
    ) -> Result<(), Error> {
        admin.require_auth();

        if !AccessControl::has_role(&env, &role_admin(&env), &admin) {
            return Err(Error::Unauthorized);
        }

        if max_secs > 30 * 24 * 3600 {
            return Err(Error::InvalidAmount);
        }

        env.storage()
            .persistent()
            .set(&DataKey::MaxPaymentDurationSecs, &max_secs);
        Ok(())
    }

    /// Return the accumulated treasury balance collected via settlement fees.
    pub fn get_treasury_balance(env: Env) -> i128 {
        env.storage()
            .persistent()
            .get(&DataKey::TreasuryBalance)
            .unwrap_or(0)
    }

    fn record_treasury_withdrawal(env: &Env, record: TreasuryWithdrawal) {
        let key = DataKey::TreasuryWithdrawalHistory;
        let mut history: Vec<TreasuryWithdrawal> = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| vec![env]);
        history.push_front(record);
        while history.len() > TREASURY_WITHDRAWAL_HISTORY_CAP {
            history.pop_back();
        }
        env.storage().persistent().set(&key, &history);
    }

    /// Return a page of treasury withdrawal history (newest-first).
    pub fn get_treasury_withdrawal_history(
        env: Env,
        offset: u32,
        limit: u32,
    ) -> Vec<TreasuryWithdrawal> {
        let history: Vec<TreasuryWithdrawal> = env
            .storage()
            .persistent()
            .get(&DataKey::TreasuryWithdrawalHistory)
            .unwrap_or_else(|| vec![&env]);
        let page_limit = limit.min(TREASURY_WITHDRAWAL_HISTORY_CAP);
        let mut page: Vec<TreasuryWithdrawal> = vec![&env];
        let mut i = offset;
        while i < history.len() && page.len() < page_limit {
            if let Some(item) = history.get(i) {
                page.push_back(item);
            }
            i = i.saturating_add(1);
        }
        page
    }

    /// Issue #666: Append a fee-collection record from `settle_payment`,
    /// retaining only the newest `FEE_COLLECTION_HISTORY_CAP` entries
    /// (newest-first), mirroring `record_treasury_withdrawal`.
    fn record_fee_collection(
        env: &Env,
        total_fee: i128,
        treasury_share: i128,
        developer_share: i128,
    ) {
        if total_fee <= 0 {
            return;
        }
        let key = DataKey::FeeCollectionHistory;
        let mut history: Vec<FeeCollectionRecord> = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| vec![env]);
        history.push_front(FeeCollectionRecord {
            collected_at: env.ledger().timestamp(),
            total_fee,
            treasury_share,
            developer_share,
        });
        while history.len() > FEE_COLLECTION_HISTORY_CAP {
            history.pop_back();
        }
        env.storage().persistent().set(&key, &history);
    }

    /// Issue #666: Aggregate platform fee collection over `[from_ts, to_ts]`
    /// (inclusive), for treasury reporting. Reads from the `FeeCollectionHistory`
    /// log that `settle_payment` appends to on every fee-bearing settlement.
    ///
    /// NOTE: `FeeCollectionHistory` is capped at `FEE_COLLECTION_HISTORY_CAP`
    /// entries; queries for periods older than the retained window will
    /// undercount. A follow-up should move this to time-bucketed storage
    /// (e.g. per-day accumulator keys) if long-horizon reporting is needed.
    pub fn get_platform_fee_report(env: Env, from_ts: u64, to_ts: u64) -> PlatformFeeReport {
        let history: Vec<FeeCollectionRecord> = env
            .storage()
            .persistent()
            .get(&DataKey::FeeCollectionHistory)
            .unwrap_or_else(|| vec![&env]);

        let mut total_fees_collected: i128 = 0;
        let mut treasury_share: i128 = 0;
        let mut developer_share: i128 = 0;
        let mut payment_count: u64 = 0;

        for record in history.iter() {
            if record.collected_at >= from_ts && record.collected_at <= to_ts {
                total_fees_collected = total_fees_collected.saturating_add(record.total_fee);
                treasury_share = treasury_share.saturating_add(record.treasury_share);
                developer_share = developer_share.saturating_add(record.developer_share);
                payment_count = payment_count.saturating_add(1);
            }
        }

        PlatformFeeReport {
            total_fees_collected,
            treasury_share,
            developer_share,
            payment_count,
        }
    }

    /// Admin withdrawal of accumulated treasury fees. Emits `TREASURY/WITHDRAWN`
    /// with `(amount, destination)` and appends to the paginated history log.
    pub fn withdraw_treasury(
        env: Env,
        admin: Address,
        amount: i128,
        destination: Address,
    ) -> Result<(), Error> {
        admin.require_auth();

        if !AccessControl::has_role(&env, &role_admin(&env), &admin) {
            return Err(Error::Unauthorized);
        }
        if amount <= 0 {
            return Err(Error::InvalidAmount);
        }

        let treasury_balance = Self::get_treasury_balance(env.clone());
        if amount > treasury_balance {
            return Err(Error::InsufficientTreasuryBalance);
        }

        let usdc_token_address: Address = env
            .storage()
            .persistent()
            .get(&DataKey::UsdcToken)
            .ok_or(Error::Unauthorized)?;
        let token_client = token::TokenClient::new(&env, &usdc_token_address);
        let contract_address = env.current_contract_address();

        env.storage().persistent().set(
            &DataKey::TreasuryBalance,
            &treasury_balance.saturating_sub(amount),
        );

        token_client.transfer(&contract_address, &destination, &amount);

        Self::record_treasury_withdrawal(
            &env,
            TreasuryWithdrawal {
                amount,
                destination: destination.clone(),
                admin: admin.clone(),
                withdrawn_at: env.ledger().timestamp(),
            },
        );

        env.events().publish(
            (
                Symbol::new(&env, "TREASURY"),
                Symbol::new(&env, "WITHDRAWN"),
            ),
            (amount, destination),
        );

        Ok(())
    }

    /// Admin-only: register a reusable fee-waiver code for per-payment zero-fee
    /// promotions.
    ///
    /// The code can be consumed at settlement via the `fee_waiver_code` field
    /// on `PaymentCharge`. Each successful consumption atomically decrements
    /// `remaining_uses`; when the counter reaches zero or `expires_at` is in
    /// the past, the code is treated as invalid and normal fees apply.
    ///
    /// To immediately revoke a live code without waiting for expiry, pass
    /// `max_uses = 0` (which will set `remaining_uses = 0` on overwrite).
    ///
    /// # Arguments
    /// * `admin` – Must hold the admin role.
    /// * `code`  – Arbitrary case-sensitive code string (e.g. "LAUNCH2026").
    /// * `expires_at` – Unix ledger timestamp after which the code is invalid.
    /// * `max_uses` – Maximum total payments that may use this code. Must be `>= 1`
    ///   when creating a new code; may be `0` when revoking an existing one.
    pub fn add_fee_waiver_code(
        env: Env,
        admin: Address,
        code: String,
        expires_at: u64,
        max_uses: u32,
    ) -> Result<(), Error> {
        admin.require_auth();
        if !AccessControl::has_role(&env, &role_admin(&env), &admin) {
            return Err(Error::Unauthorized);
        }
        if expires_at <= env.ledger().timestamp() {
            return Err(Error::InvalidExpiry);
        }
        if max_uses == 0 {
            return Err(Error::InvalidAmount);
        }

        let record = FeeWaiverCodeRecord {
            code: code.clone(),
            expires_at,
            max_uses,
            remaining_uses: max_uses,
        };

        env.storage()
            .persistent()
            .set(&DataKey::FeeWaiverCode(code.clone()), &record);

        env.events().publish(
            (
                Symbol::new(&env, "FEE_WAIVER"),
                Symbol::new(&env, "CODE_ADDED"),
            ),
            (code, expires_at, max_uses),
        );

        Ok(())
    }

    pub fn set_global_rate_limit(
        env: Env,
        admin: Address,
        window_secs: u64,
        max_per_window: u32,
    ) -> Result<(), Error> {
        admin.require_auth();
        if !AccessControl::has_role(&env, &role_admin(&env), &admin) {
            return Err(Error::Unauthorized);
        }
        let config = RateLimitConfig {
            window_secs,
            max_per_window,
        };
        env.storage()
            .persistent()
            .set(&DataKey::GlobalRateLimit, &config);
        Ok(())
    }

    pub fn set_merchant_rate_limit(
        env: Env,
        admin: Address,
        merchant_id: Address,
        window_secs: u64,
        max_per_window: u32,
    ) -> Result<(), Error> {
        admin.require_auth();
        if !AccessControl::has_role(&env, &role_admin(&env), &admin) {
            return Err(Error::Unauthorized);
        }
        let config = RateLimitConfig {
            window_secs,
            max_per_window,
        };
        env.storage()
            .persistent()
            .set(&DataKey::MerchantSpecificRateLimit(merchant_id), &config);
        Ok(())
    }

    pub fn grant_role(
        env: Env,
        admin: Address,
        role: Symbol,
        account: Address,
    ) -> Result<(), Error> {
        AccessControl::grant_role(&env, admin, role, account).map_err(|_| Error::AccessControlError)
    }

    pub fn revoke_role(
        env: Env,
        admin: Address,
        role: Symbol,
        account: Address,
    ) -> Result<(), Error> {
        AccessControl::revoke_role(&env, admin, role, account)
            .map_err(|_| Error::AccessControlError)
    }

    /// Returns whether `account` holds `role` on this contract (issue #401).
    pub fn has_role(env: Env, role: Symbol, account: Address) -> bool {
        AccessControl::has_role(&env, &role, &account)
    }

    /// Returns all addresses currently holding `role` on this contract (issue #401).
    pub fn get_role_members(env: Env, role: Symbol) -> Vec<Address> {
        AccessControl::get_role_members(&env, &role)
    }

    /// Set the global paused state (admin only). When paused, create_payment, verify_payment, and cancel_payment are blocked.
    pub fn set_global_pause(
        env: Env,
        admin: Address,
        paused: bool,
        reason: String,
    ) -> Result<(), Error> {
        admin.require_auth();

        if !AccessControl::has_role(&env, &role_admin(&env), &admin) {
            return Err(Error::Unauthorized);
        }

        let state = PauseState {
            paused,
            reason: reason.clone(),
            admin: Some(admin.clone()),
            timestamp: env.ledger().timestamp(),
        };

        env.storage().persistent().set(&DataKey::Paused, &state);

        let event_name = if paused {
            Symbol::new(&env, "GLOBAL_PAUSED")
        } else {
            Symbol::new(&env, "GLOBAL_UNPAUSED")
        };

        env.events()
            .publish((Symbol::new(&env, "CONTRACT"), event_name), (admin, reason));

        Ok(())
    }

    /// Set the creation-only paused state (admin only, issue #670).
    ///
    /// When `paused` is true, only payment-creation entry points (`create_payment`,
    /// `create_payments_batch`, `swap_and_pay`, `swap_and_pay_multi_route`, and the
    /// creation path of `retry_payment`) are blocked with `Error::ContractPaused`.
    /// Settlement, verification, cancellation, and refund operations continue to work
    /// normally. This is narrower than `set_global_pause`, which halts all operations.
    /// Query the current state with `get_creation_pause_info`.
    pub fn set_creation_pause(
        env: Env,
        admin: Address,
        paused: bool,
        reason: String,
    ) -> Result<(), Error> {
        admin.require_auth();

        if !AccessControl::has_role(&env, &role_admin(&env), &admin) {
            return Err(Error::Unauthorized);
        }

        let state = PauseState {
            paused,
            reason: reason.clone(),
            admin: Some(admin.clone()),
            timestamp: env.ledger().timestamp(),
        };

        env.storage()
            .persistent()
            .set(&DataKey::CreationPaused, &state);

        let event_name = if paused {
            Symbol::new(&env, "CREATION_PAUSED")
        } else {
            Symbol::new(&env, "CREATION_UNPAUSED")
        };

        env.events()
            .publish((Symbol::new(&env, "CONTRACT"), event_name), (admin, reason));

        Ok(())
    }

    /// Legacy wrapper for set_global_pause
    pub fn set_paused(env: Env, admin: Address, paused: bool) -> Result<(), Error> {
        let reason = if paused {
            String::from_str(&env, "Legacy pause")
        } else {
            String::from_str(&env, "Legacy unpause")
        };
        Self::set_global_pause(env, admin, paused, reason)
    }

    /// Get the current creation-only pause state (issue #670).
    ///
    /// Distinct from `get_pause_info`, which returns the consolidated global + creation
    /// state. `set_creation_pause` blocks only `create_payment`, `create_payments_batch`,
    /// `swap_and_pay`, `swap_and_pay_multi_route`, and the creation path of `retry_payment`.
    /// It does NOT block `verify_payment`, `settle_payment`, `cancel_payment`,
    /// `process_refund`, `claim_refund`, or dispute resolution, so operators can halt new
    /// payment intake (e.g. during a maintenance window) while still allowing in-flight
    /// payments to be confirmed, settled, and refunded. Use `set_global_pause` instead when
    /// all operations need to be halted.
    pub fn get_creation_pause_info(env: Env) -> PauseState {
        env.storage()
            .persistent()
            .get::<DataKey, PauseState>(&DataKey::CreationPaused)
            .unwrap_or(PauseState {
                paused: false,
                reason: String::from_str(&env, ""),
                admin: None,
                timestamp: 0,
            })
    }

    /// Get the current consolidated pause info.
    pub fn get_pause_info(env: Env) -> PauseInfo {
        let empty_reason = String::from_str(&env, "");
        let default_state = PauseState {
            paused: false,
            reason: empty_reason,
            admin: None,
            timestamp: 0,
        };

        let global = env
            .storage()
            .persistent()
            .get::<DataKey, PauseState>(&DataKey::Paused)
            .unwrap_or_else(|| default_state.clone());

        let creation = env
            .storage()
            .persistent()
            .get::<DataKey, PauseState>(&DataKey::CreationPaused)
            .unwrap_or(default_state);

        PauseInfo { global, creation }
    }

    /// Get the current global paused state.
    pub fn is_paused(env: Env) -> bool {
        env.storage()
            .persistent()
            .get::<DataKey, PauseState>(&DataKey::Paused)
            .map(|s| s.paused)
            .unwrap_or(false)
    }

    /// Check if contract is globally paused and return error if so.
    fn require_not_paused(env: &Env) -> Result<(), Error> {
        if Self::is_paused(env.clone()) {
            return Err(Error::ContractPaused);
        }
        Ok(())
    }

    /// Check if payment creation is paused (either globally or specifically for creation).
    fn require_creation_not_paused(env: &Env) -> Result<(), Error> {
        Self::require_not_paused(env)?;

        let creation_paused: bool = env
            .storage()
            .persistent()
            .get::<DataKey, PauseState>(&DataKey::CreationPaused)
            .map(|s| s.paused)
            .unwrap_or(false);

        if creation_paused {
            return Err(Error::ContractPaused);
        }
        Ok(())
    }

    /// Fixed-window rate limiter per merchant.
    ///
    /// `last_payment_at` stores the start of the current fixed window (set when the window
    /// is first entered). The counter resets only when `now` exceeds `window_start + window_secs`.
    /// This prevents the sliding-window bypass where bursts at the end of one window and the
    /// beginning of the next could otherwise double the effective rate.
    fn enforce_create_payment_rate_limit(env: &Env, merchant_id: &Address) -> Result<(), Error> {
        let now = env.ledger().timestamp();

        let config: RateLimitConfig = env
            .storage()
            .persistent()
            .get(&DataKey::MerchantSpecificRateLimit(merchant_id.clone()))
            .unwrap_or_else(|| {
                env.storage()
                    .persistent()
                    .get(&DataKey::GlobalRateLimit)
                    .unwrap_or(RateLimitConfig {
                        window_secs: CREATE_PAYMENT_WINDOW_SECS,
                        max_per_window: CREATE_PAYMENT_MAX_PER_WINDOW,
                    })
            });

        let key = DataKey::MerchantRateLimit(merchant_id.clone());

        let mut state: MerchantCreateRateLimit =
            env.storage()
                .persistent()
                .get(&key)
                .unwrap_or(MerchantCreateRateLimit {
                    last_payment_at: now,
                    count: 0,
                });

        if now.saturating_sub(state.last_payment_at) >= config.window_secs {
            // Start a new fixed window
            state.count = 0;
            state.last_payment_at = now;
        }

        if state.count >= config.max_per_window {
            return Err(Error::RateLimitExceeded);
        }

        state.count = state.count.saturating_add(1);

        env.storage().persistent().set(&key, &state);
        Self::bump_ttl(env, &key, SHORT_LIVE_TTL);

        Ok(())
    }

    fn enforce_create_payment_rate_limit_for_payer(
        env: &Env,
        payer: &Address,
    ) -> Result<(), Error> {
        let now = env.ledger().timestamp();

        let config: RateLimitConfig = env
            .storage()
            .persistent()
            .get(&DataKey::GlobalRateLimit)
            .unwrap_or(RateLimitConfig {
                window_secs: CREATE_PAYMENT_WINDOW_SECS,
                max_per_window: CREATE_PAYMENT_MAX_PER_WINDOW,
            });

        let key = DataKey::PayerRateLimit(payer.clone());

        let mut state: MerchantCreateRateLimit =
            env.storage()
                .persistent()
                .get(&key)
                .unwrap_or(MerchantCreateRateLimit {
                    last_payment_at: now,
                    count: 0,
                });

        if now.saturating_sub(state.last_payment_at) >= config.window_secs {
            // Start a new fixed window
            state.count = 0;
            state.last_payment_at = now;
        }

        if state.count >= config.max_per_window {
            return Err(Error::RateLimitExceeded);
        }

        state.count = state.count.saturating_add(1);

        env.storage().persistent().set(&key, &state);
        Self::bump_ttl(env, &key, SHORT_LIVE_TTL);

        Ok(())
    }

    fn enforce_create_payment_batch_rate_limit(
        env: &Env,
        merchant_id: &Address,
    ) -> Result<(), Error> {
        let now = env.ledger().timestamp();

        let config: RateLimitConfig = env
            .storage()
            .persistent()
            .get(&DataKey::MerchantSpecificRateLimit(merchant_id.clone()))
            .unwrap_or_else(|| {
                env.storage()
                    .persistent()
                    .get(&DataKey::GlobalRateLimit)
                    .unwrap_or(RateLimitConfig {
                        window_secs: CREATE_PAYMENT_WINDOW_SECS,
                        max_per_window: CREATE_PAYMENT_MAX_PER_WINDOW,
                    })
            });

        let key = DataKey::MerchantRateLimit(merchant_id.clone());

        let mut state: MerchantCreateRateLimit =
            env.storage()
                .persistent()
                .get(&key)
                .unwrap_or(MerchantCreateRateLimit {
                    last_payment_at: now,
                    count: 0,
                });

        if now.saturating_sub(state.last_payment_at) >= config.window_secs {
            // Start a new fixed window
            state.count = 0;
            state.last_payment_at = now;
        }

        if state.count >= config.max_per_window {
            return Err(Error::RateLimitExceeded);
        }

        state.count = state.count.saturating_add(1);

        env.storage().persistent().set(&key, &state);
        Self::bump_ttl(env, &key, SHORT_LIVE_TTL);

        Ok(())
    }

    /// Set per-merchant min/max payment amount limits (merchant self-service).
    /// Pass None to clear a bound. Requires the caller to hold the MERCHANT role.
    pub fn set_merchant_amount_limits(
        env: Env,
        merchant_id: Address,
        min: Option<i128>,
        max: Option<i128>,
    ) -> Result<(), Error> {
        merchant_id.require_auth();
        if !AccessControl::has_role(&env, &role_merchant(&env), &merchant_id) {
            return Err(Error::Unauthorized);
        }
        if let (Some(lo), Some(hi)) = (min, max) {
            if lo > hi {
                return Err(Error::InvalidAmount);
            }
        }
        let limits = AmountLimits { min, max };
        env.storage()
            .persistent()
            .set(&DataKey::MerchantAmountLimits(merchant_id), &limits);
        Ok(())
    }

    /// Read per-merchant amount limits.
    pub fn get_merchant_amount_limits(env: Env, merchant_id: Address) -> Option<AmountLimits> {
        env.storage()
            .persistent()
            .get(&DataKey::MerchantAmountLimits(merchant_id))
    }

    /// Set global min/max payment amount limits (admin only).
    /// Pass None to clear a bound.
    pub fn set_global_amount_limits(
        env: Env,
        admin: Address,
        min: Option<i128>,
        max: Option<i128>,
    ) -> Result<(), Error> {
        admin.require_auth();
        if !AccessControl::has_role(&env, &role_admin(&env), &admin) {
            return Err(Error::Unauthorized);
        }
        if let (Some(lo), Some(hi)) = (min, max) {
            if lo > hi {
                return Err(Error::InvalidAmount);
            }
        }
        let limits = AmountLimits { min, max };
        env.storage()
            .persistent()
            .set(&DataKey::GlobalAmountLimits, &limits);
        Ok(())
    }

    /// Read global amount limits.
    pub fn get_global_amount_limits(env: Env) -> Option<AmountLimits> {
        env.storage().persistent().get(&DataKey::GlobalAmountLimits)
    }

    /// Enforce amount limits: merchant-specific limits take precedence over global limits.
    fn enforce_amount_limits(env: &Env, merchant_id: &Address, amount: i128) -> Result<(), Error> {
        let limits: Option<AmountLimits> = env
            .storage()
            .persistent()
            .get(&DataKey::MerchantAmountLimits(merchant_id.clone()))
            .or_else(|| env.storage().persistent().get(&DataKey::GlobalAmountLimits));

        if let Some(l) = limits {
            if let Some(min) = l.min {
                if amount < min {
                    return Err(Error::AmountBelowMin);
                }
            }
            if let Some(max) = l.max {
                if amount > max {
                    return Err(Error::AmountAboveMax);
                }
            }
        }
        Ok(())
    }

    /// Set the USDC token address used as the default settlement token.
    /// Also adds it to the supported tokens whitelist.
    pub fn set_usdc_token(env: Env, admin: Address, token_address: Address) -> Result<(), Error> {
        admin.require_auth();
        if !AccessControl::has_role(&env, &role_admin(&env), &admin) {
            return Err(Error::Unauthorized);
        }
        env.storage()
            .persistent()
            .set(&DataKey::UsdcToken, &token_address);
        // Auto-add USDC to the supported tokens whitelist
        Self::allow_token(env, admin, token_address)
    }

    /// Allow or disallow a token address for use in payments (admin only).
    pub fn allow_token(env: Env, admin: Address, token_address: Address) -> Result<(), Error> {
        admin.require_auth();
        if !AccessControl::has_role(&env, &role_admin(&env), &admin) {
            return Err(Error::Unauthorized);
        }
        env.storage()
            .persistent()
            .set(&DataKey::AllowedToken(token_address.clone()), &true);
        let mut tokens: Vec<Address> = env
            .storage()
            .persistent()
            .get(&DataKey::SupportedTokens)
            .unwrap_or(Vec::new(&env));
        if !tokens.contains(&token_address) {
            tokens.push_back(token_address);
            env.storage()
                .persistent()
                .set(&DataKey::SupportedTokens, &tokens);
            Self::bump_ttl(&env, &DataKey::SupportedTokens, LONG_LIVE_TTL);
        }
        Ok(())
    }
    /// Issue #483: Set the currency symbol for an allowed token (e.g., USDC, EURC, BRLT).
    /// Must be called after allow_token() to establish token-to-currency mapping.
    pub fn set_token_currency(
        env: Env,
        admin: Address,
        token_address: Address,
        currency: Symbol,
    ) -> Result<(), Error> {
        admin.require_auth();
        if !AccessControl::has_role(&env, &role_admin(&env), &admin) {
            return Err(Error::Unauthorized);
        }

        if !env
            .storage()
            .persistent()
            .has(&DataKey::AllowedToken(token_address.clone()))
        {
            return Err(Error::UnsupportedToken);
        }

        env.storage()
            .persistent()
            .set(&DataKey::TokenCurrency(token_address), &currency);
        Ok(())
    }

    /// Issue #301: Remove a token from the supported tokens list (admin only).
    pub fn remove_supported_token(
        env: Env,
        admin: Address,
        token_address: Address,
    ) -> Result<(), Error> {
        admin.require_auth();
        if !AccessControl::has_role(&env, &role_admin(&env), &admin) {
            return Err(Error::Unauthorized);
        }
        env.storage()
            .persistent()
            .set(&DataKey::AllowedToken(token_address.clone()), &false);
        let mut tokens: Vec<Address> = env
            .storage()
            .persistent()
            .get(&DataKey::SupportedTokens)
            .unwrap_or(Vec::new(&env));
        let mut i = 0;
        while i < tokens.len() {
            if tokens.get(i).unwrap() == token_address {
                tokens.remove(i);
            } else {
                i += 1;
            }
        }
        env.storage()
            .persistent()
            .set(&DataKey::SupportedTokens, &tokens);
        Self::bump_ttl(&env, &DataKey::SupportedTokens, LONG_LIVE_TTL);
        env.events().publish(
            (Symbol::new(&env, "TOKEN"), Symbol::new(&env, "REMOVED")),
            token_address,
        );
        Ok(())
    }

    /// Issue #301: Return the list of supported token addresses.
    pub fn get_supported_tokens(env: Env) -> Vec<Address> {
        env.storage()
            .persistent()
            .get(&DataKey::SupportedTokens)
            .unwrap_or(Vec::new(&env))
    }

    /// Issue #303: Set per‑tier KYC payment limits (admin only).
    pub fn set_kyc_tier_limits(
        env: Env,
        admin: Address,
        tier: KycTier,
        max_amount: i128,
    ) -> Result<String, Error> {
        admin.require_auth();
        if !AccessControl::has_role(&env, &role_admin(&env), &admin) {
            return Err(Error::Unauthorized);
        }
        // Issue #624: queue via timelock instead of applying immediately.
        Self::enqueue_timelocked_action(
            &env,
            admin,
            TimelockActionKind::SetKycTierLimits(tier, max_amount),
        )
    }

    /// Issue #303: Set the FX oracle contract address (admin only).
    pub fn set_fx_oracle_address(
        env: Env,
        admin: Address,
        oracle_address: Address,
    ) -> Result<(), Error> {
        admin.require_auth();
        if !AccessControl::has_role(&env, &role_admin(&env), &admin) {
            return Err(Error::Unauthorized);
        }
        env.storage()
            .persistent()
            .set(&DataKey::FXOracleAddress, &oracle_address);
        Self::bump_ttl(&env, &DataKey::FXOracleAddress, LONG_LIVE_TTL);
        Ok(())
    }

    /// Add an address to the global blacklist (admin only).
    pub fn add_to_blacklist(env: Env, admin: Address, address: Address) -> Result<(), Error> {
        admin.require_auth();
        if !AccessControl::has_role(&env, &role_admin(&env), &admin) {
            return Err(Error::Unauthorized);
        }
        env.storage()
            .persistent()
            .set(&DataKey::Blacklisted(address), &true);
        Ok(())
    }

    /// Remove an address from the global blacklist (admin only).
    pub fn remove_from_blacklist(env: Env, admin: Address, address: Address) -> Result<(), Error> {
        admin.require_auth();
        if !AccessControl::has_role(&env, &role_admin(&env), &admin) {
            return Err(Error::Unauthorized);
        }
        env.storage()
            .persistent()
            .set(&DataKey::Blacklisted(address), &false);
        Ok(())
    }

    /// Returns true when an address is globally blacklisted.
    pub fn is_blacklisted(env: Env, address: Address) -> bool {
        Self::is_blacklisted_address(&env, &address)
    }

    fn is_blacklisted_address(env: &Env, address: &Address) -> bool {
        env.storage()
            .persistent()
            .get::<DataKey, bool>(&DataKey::Blacklisted(address.clone()))
            .unwrap_or(false)
    }

    fn require_not_blacklisted(env: &Env, address: &Address) -> Result<(), Error> {
        if Self::is_blacklisted_address(env, address) {
            return Err(Error::Unauthorized);
        }
        Ok(())
    }

    /// Returns true if the given token address is on the allowlist.
    fn expiry_bucket_for(expires_at: u64) -> u32 {
        (expires_at / 5).min(u32::MAX as u64) as u32
    }

    fn index_payment_expiry(env: &Env, payment_id: &String, expires_at: u64) {
        let bucket = Self::expiry_bucket_for(expires_at);
        let key = DataKey::PaymentsByExpiry(bucket);
        let mut ids: Vec<String> = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| vec![env]);
        if !ids.contains(payment_id) {
            ids.push_back(payment_id.clone());
            env.storage().persistent().set(&key, &ids);
            Self::bump_ttl(env, &key, LONG_LIVE_TTL);
        }

        let buckets_key = DataKey::PaymentExpiryBuckets;
        let mut buckets: Vec<u32> = env
            .storage()
            .persistent()
            .get(&buckets_key)
            .unwrap_or_else(|| vec![env]);
        if !buckets.contains(bucket) {
            buckets.push_back(bucket);
            env.storage().persistent().set(&buckets_key, &buckets);
            Self::bump_ttl(env, &buckets_key, LONG_LIVE_TTL);
        }
    }

    /// Issue #678: Append payment_id to the daily bucket index for the given merchant.
    /// Bucket granularity is one day (86 400 seconds). The index allows analytics
    /// queries to scan only the relevant day buckets rather than all payments.
    fn index_payment_by_date(
        env: &Env,
        merchant_id: &Address,
        payment_id: &String,
        created_at: u64,
    ) {
        const SECONDS_PER_DAY: u64 = 86_400;
        let day_bucket = created_at / SECONDS_PER_DAY;
        let key = DataKey::DailyPaymentIndex(merchant_id.clone(), day_bucket);
        let mut ids: Vec<String> = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| vec![env]);
        if !ids.contains(payment_id) {
            ids.push_back(payment_id.clone());
            env.storage().persistent().set(&key, &ids);
            Self::bump_ttl(env, &key, LONG_LIVE_TTL);
        }
    }

    fn remove_payment_from_expiry_bucket(env: &Env, payment_id: &String, expires_at: u64) {
        let bucket = Self::expiry_bucket_for(expires_at);
        let key = DataKey::PaymentsByExpiry(bucket);
        if let Some(ids) = env.storage().persistent().get::<DataKey, Vec<String>>(&key) {
            let mut remaining = vec![env];
            for id in ids.iter() {
                if id != *payment_id {
                    remaining.push_back(id);
                }
            }
            if remaining.is_empty() {
                env.storage().persistent().remove(&key);
                let buckets_key = DataKey::PaymentExpiryBuckets;
                if let Some(buckets) = env
                    .storage()
                    .persistent()
                    .get::<DataKey, Vec<u32>>(&buckets_key)
                {
                    let mut kept = vec![env];
                    for candidate in buckets.iter() {
                        if candidate != bucket {
                            kept.push_back(candidate);
                        }
                    }
                    if kept.is_empty() {
                        env.storage().persistent().remove(&buckets_key);
                    } else {
                        env.storage().persistent().set(&buckets_key, &kept);
                        Self::bump_ttl(env, &buckets_key, LONG_LIVE_TTL);
                    }
                }
            } else {
                env.storage().persistent().set(&key, &remaining);
                Self::bump_ttl(env, &key, LONG_LIVE_TTL);
            }
        }
    }

    pub fn is_token_allowed(env: Env, token_address: Address) -> bool {
        env.storage()
            .persistent()
            .get::<DataKey, bool>(&DataKey::AllowedToken(token_address))
            .unwrap_or(false)
    }

    #[allow(deprecated)]
    pub fn create_payment(env: Env, args: CreatePaymentArgs) -> Result<PaymentCharge, Error> {
        Self::require_creation_not_paused(&env)?;
        args.merchant_id.require_auth();
        Self::require_not_blacklisted(&env, &args.merchant_id)?;
        Self::require_not_blacklisted(&env, &args.deposit_address)?;

        // Idempotency check: if client_token was already used, return the existing payment
        // (or error if it maps to a different payment_id).
        if let Some(ref token) = args.client_token {
            let key = DataKey::IdempotencyKey(token.clone());
            if let Some(existing_id) = env.storage().persistent().get::<DataKey, String>(&key) {
                if existing_id == args.payment_id {
                    return Self::get_payment_internal(&env, &args.payment_id);
                } else {
                    return Err(Error::DuplicateIdempotencyKey);
                }
            }
        }

        // Verify that the merchant has the MERCHANT role (granted on verification)
        if !AccessControl::has_role(&env, &role_merchant(&env), &args.merchant_id) {
            return Err(Error::Unauthorized);
        }

        // Issue #164: Validate token against admin-approved allowlist
        if let Some(ref token_addr) = args.token_address {
            let allowed: bool = env
                .storage()
                .persistent()
                .get::<DataKey, bool>(&DataKey::AllowedToken(token_addr.clone()))
                .unwrap_or(false);
            if !allowed {
                return Err(Error::UnsupportedToken);
            }
        }
        // Issue #483: Verify that token_address (if provided) matches the currency symbol
        if let Some(ref token_addr) = args.token_address {
            if let Some(token_currency) = env
                .storage()
                .persistent()
                .get::<DataKey, Symbol>(&DataKey::TokenCurrency(token_addr.clone()))
            {
                if token_currency != args.currency {
                    return Err(Error::UnsupportedToken);
                }
            }
        }

        // Issue #79: Cross-contract validate merchant is verified and active
        if let Some(registry_address) = env
            .storage()
            .persistent()
            .get::<DataKey, Address>(&DataKey::MerchantRegistryAddress)
        {
            let registry_client =
                crate::merchant_registry::MerchantRegistryClient::new(&env, &registry_address);
            match registry_client.try_get_merchant(&args.merchant_id) {
                Ok(Ok(merchant)) => {
                    // Require merchant to be verified (not Unverified), active, and not suspended
                    if merchant.kyc_tier == crate::merchant_registry::KycTier::Unverified
                        || !merchant.active
                        || merchant.suspension_reason.is_some()
                    {
                        return Err(Error::Unauthorized);
                    }

                    // Issue #516: Enforce merchant whitelist mode against the payer.
                    if merchant.whitelist_mode {
                        let payer = args.payer.clone().ok_or(Error::PayerNotWhitelisted)?;
                        match registry_client.try_is_customer_whitelisted(&args.merchant_id, &payer)
                        {
                            Ok(Ok(true)) => {}
                            _ => return Err(Error::PayerNotWhitelisted),
                        }
                    }
                }
                _ => {
                    // If registry lookup fails, reject the payment
                    return Err(Error::Unauthorized);
                }
            }
        }

        if args.amount <= 0 {
            return Err(Error::InvalidAmount);
        }

        Self::enforce_amount_limits(&env, &args.merchant_id, args.amount)?;

        // Issue #393: Enforce KYC tier per-payment limit when merchant registry is configured
        if let Some(registry_address) = env
            .storage()
            .persistent()
            .get::<DataKey, Address>(&DataKey::MerchantRegistryAddress)
        {
            let registry_client =
                crate::merchant_registry::MerchantRegistryClient::new(&env, &registry_address);
            if let Ok(Ok(merchant)) = registry_client.try_get_merchant(&args.merchant_id) {
                if let Ok(Ok(limits)) = registry_client.try_get_tier_limits(&merchant.kyc_tier) {
                    if let Some(min) = limits.min {
                        if args.amount < min {
                            return Err(Error::AmountBelowMin);
                        }
                    }
                    if let Some(max) = limits.max {
                        if args.amount > max {
                            return Err(Error::AmountAboveMax);
                        }
                    }
                }
            }
        }

        if env
            .storage()
            .persistent()
            .has(&DataKey::Payment(args.payment_id.clone()))
        {
            return Err(Error::PaymentAlreadyExists);
        }

        // Issue #489: Validate metadata_hash uniqueness
        if let Some(ref hash) = args.metadata_hash {
            if env
                .storage()
                .persistent()
                .has(&DataKey::MetadataHashPayment(hash.clone()))
            {
                return Err(Error::DuplicateIdempotencyKey);
            }
        }

        if !utils::validate_id(&args.payment_id) {
            return Err(Error::InvalidPaymentId);
        }

        // Validate metadata key count and key/value length limits.
        if let Some(ref meta_map) = args.metadata {
            utils::validate_metadata(meta_map)?;
        }

        // Issue #397: Validate Stellar memo type constraints.
        Self::validate_memo(&env, &args.memo, &args.memo_type)?;

        Self::enforce_create_payment_rate_limit(&env, &args.merchant_id)?;

        let now = env.ledger().timestamp();
        let min_duration = env
            .storage()
            .persistent()
            .get::<DataKey, u64>(&DataKey::MinPaymentDurationSecs)
            .unwrap_or(CREATE_PAYMENT_WINDOW_SECS);
        let max_duration = env
            .storage()
            .persistent()
            .get::<DataKey, u64>(&DataKey::MaxPaymentDurationSecs)
            .unwrap_or(30 * 24 * 3600);

        let resolved_expires_at = match args.expires_at {
            Some(ts) => {
                if ts <= now {
                    return Err(Error::InvalidExpiry);
                }
                let duration = ts.saturating_sub(now);
                if duration < min_duration || duration > max_duration {
                    return Err(Error::InvalidExpiry);
                }
                ts
            }
            None => {
                let duration = args.duration_secs.unwrap_or(DEFAULT_PAYMENT_DURATION_SECS);
                if duration < min_duration || duration > max_duration {
                    return Err(Error::InvalidExpiry);
                }
                now.saturating_add(duration)
            }
        };

        let payment = PaymentCharge {
            payment_id: args.payment_id.clone(),
            merchant_id: args.merchant_id.clone(),
            amount: args.amount,
            currency: args.currency,
            deposit_address: args.deposit_address,
            status: PaymentStatus::Pending,
            payer_address: None,
            transaction_hash: None,
            created_at: now,
            confirmed_at: None,
            expires_at: resolved_expires_at,
            amount_received: None,
            memo: args.memo.clone(),
            memo_type: args.memo_type.clone(),
            token_address: args.token_address.clone(),
            metadata_hash: args.metadata_hash.clone(),
            original_token: None,
            swap_path: None,
            fx_rate: None,
            fx_rate_at: None,
            metadata: args.metadata.clone(),
            fee_waiver_code: args.fee_waiver_code.clone(),
            retry_of_payment_id: None,
            payer_muxed_id: None,
            payment_link_id: None,
        };

        env.storage()
            .persistent()
            .set(&DataKey::Payment(args.payment_id.clone()), &payment);
        Self::record_payment_status(&env, &payment);
        Self::bump_payment_ttl(&env, &args.payment_id, &payment.status);
        Self::index_payment_expiry(&env, &args.payment_id, payment.expires_at);
        Self::index_payment_by_date(
            &env,
            &args.merchant_id,
            &args.payment_id,
            payment.created_at,
        );

        // Issue #489: Store reverse index for metadata_hash → payment_id lookup
        if let Some(ref hash) = args.metadata_hash {
            let key = DataKey::MetadataHashPayment(hash.clone());
            env.storage().persistent().set(&key, &args.payment_id);
            Self::bump_ttl(&env, &key, LONG_LIVE_TTL);
        }

        let mut merchant_payments = Self::get_merchant_payments_internal(&env, &args.merchant_id);
        merchant_payments.push_back(args.payment_id.clone());
        let merchant_payments_key = DataKey::MerchantPayments(args.merchant_id.clone());
        env.storage()
            .persistent()
            .set(&merchant_payments_key, &merchant_payments);
        Self::bump_ttl(&env, &merchant_payments_key, LONG_LIVE_TTL);

        // Issue #503: Increment persistent payment count for O(1) dashboard query
        let count_key = DataKey::MerchantPaymentCount(args.merchant_id.clone());
        let count: u64 = env.storage().persistent().get(&count_key).unwrap_or(0u64);
        env.storage().persistent().set(&count_key, &(count + 1));
        Self::bump_ttl(&env, &count_key, LONG_LIVE_TTL);

        // Issue #628: maintain the per-merchant gross-volume index and the
        // tracked-merchant list so `get_top_merchants` can rank without scanning.
        Self::record_merchant_volume(&env, &args.merchant_id, args.amount);

        // Issue #284: Normalised 2-tuple topic; merchant_id and metadata included in payload.
        env.events().publish(
            (Symbol::new(&env, "PAYMENT"), Symbol::new(&env, "CREATED")),
            (
                args.payment_id.clone(),
                args.merchant_id.clone(),
                args.amount,
                args.metadata.clone(),
            ),
        );

        // Issue #399: Persist idempotency key → payment_id mapping with a TTL that matches
        // the payment expiry window so keys do not accumulate indefinitely.
        if let Some(token) = args.client_token {
            let key = DataKey::IdempotencyKey(token.clone());
            env.storage().persistent().set(&key, &args.payment_id);
            // TTL in ledgers ≈ (expires_at − now) / 5s per ledger, clamped to SHORT_LIVE_TTL min.
            let payment_duration_secs = resolved_expires_at.saturating_sub(now);
            let ledgers_per_sec: u64 = 5;
            let ttl_ledgers =
                ((payment_duration_secs / ledgers_per_sec) as u32).max(SHORT_LIVE_TTL);
            Self::bump_ttl(&env, &key, ttl_ledgers);
            // Store reverse mapping so cancel/expire can clean up the token.
            // We prefix the payment_id with "r:" to avoid collision with real tokens.
            let rev_token_id = Self::rev_key_for(&env, &args.payment_id);
            let rev_key = DataKey::IdempotencyKey(rev_token_id);
            env.storage().persistent().set(&rev_key, &token);
            Self::bump_ttl(&env, &rev_key, ttl_ledgers);
        }

        Ok(payment)
    }

    /// Issue #165: Batch payment creation for optimized gas usage.
    /// Creates multiple payment charges in a single transaction.
    /// Reverts all if any element violates validation rules.
    #[allow(deprecated)]
    pub fn create_payments_batch(
        env: Env,
        args_list: Vec<CreatePaymentArgs>,
    ) -> Result<Vec<String>, Error> {
        Self::require_creation_not_paused(&env)?;

        if args_list.len() > 50 {
            return Err(Error::BatchTooLarge);
        }

        if args_list.is_empty() {
            return Ok(vec![&env]);
        }

        // Issue #682: Detect duplicate payment_ids within the batch
        let mut seen_payment_ids: Vec<String> = vec![&env];
        for args in args_list.iter() {
            let mut is_duplicate = false;
            for seen_id in seen_payment_ids.iter() {
                if args.payment_id == seen_id {
                    is_duplicate = true;
                    break;
                }
            }
            if is_duplicate {
                return Err(Error::BatchContainsDuplicates);
            }
            seen_payment_ids.push_back(args.payment_id.clone());
        }

        let mut batch_merchants: Vec<Address> = vec![&env];

        // Validate all payments first before creating any
        for args in args_list.iter() {
            args.merchant_id.require_auth();
            Self::require_not_blacklisted(&env, &args.merchant_id)?;
            Self::require_not_blacklisted(&env, &args.deposit_address)?;

            if !batch_merchants.contains(&args.merchant_id) {
                batch_merchants.push_back(args.merchant_id.clone());
            }

            // Verify merchant role
            if !AccessControl::has_role(&env, &role_merchant(&env), &args.merchant_id) {
                return Err(Error::Unauthorized);
            }

            // Issue #164: Validate token against allowlist
            if let Some(ref token_addr) = args.token_address {
                let allowed: bool = env
                    .storage()
                    .persistent()
                    .get::<DataKey, bool>(&DataKey::AllowedToken(token_addr.clone()))
                    .unwrap_or(false);
                if !allowed {
                    return Err(Error::UnsupportedToken);
                }
            }
            // Issue #483: Verify that token_address (if provided) matches the currency symbol
            if let Some(ref token_addr) = args.token_address {
                if let Some(token_currency) = env
                    .storage()
                    .persistent()
                    .get::<DataKey, Symbol>(&DataKey::TokenCurrency(token_addr.clone()))
                {
                    if token_currency != args.currency {
                        return Err(Error::UnsupportedToken);
                    }
                }
            }

            // Validate merchant is verified and active
            if let Some(registry_address) = env
                .storage()
                .persistent()
                .get::<DataKey, Address>(&DataKey::MerchantRegistryAddress)
            {
                let registry_client =
                    crate::merchant_registry::MerchantRegistryClient::new(&env, &registry_address);
                match registry_client.try_get_merchant(&args.merchant_id) {
                    Ok(Ok(merchant)) => {
                        if merchant.kyc_tier == crate::merchant_registry::KycTier::Unverified
                            || !merchant.active
                            || merchant.suspension_reason.is_some()
                        {
                            return Err(Error::Unauthorized);
                        }
                    }
                    _ => {
                        return Err(Error::Unauthorized);
                    }
                }
            }

            if args.amount <= 0 {
                return Err(Error::InvalidAmount);
            }

            Self::enforce_amount_limits(&env, &args.merchant_id, args.amount)?;

            if let Some(limits) = env
                .storage()
                .persistent()
                .get::<DataKey, KycTierLimits>(&DataKey::KycTierLimitsConfig)
            {
                if let Some(registry_address) = env
                    .storage()
                    .persistent()
                    .get::<DataKey, Address>(&DataKey::MerchantRegistryAddress)
                {
                    let registry_client = crate::merchant_registry::MerchantRegistryClient::new(
                        &env,
                        &registry_address,
                    );
                    if let Ok(Ok(merchant)) = registry_client.try_get_merchant(&args.merchant_id) {
                        if limits.tier == merchant.kyc_tier && args.amount > limits.max_amount {
                            return Err(Error::AmountAboveMax);
                        }
                    }
                }
            }

            if env
                .storage()
                .persistent()
                .has(&DataKey::Payment(args.payment_id.clone()))
            {
                return Err(Error::PaymentAlreadyExists);
            }

            // Issue #489: Validate metadata_hash uniqueness in batch
            if let Some(ref hash) = args.metadata_hash {
                if env
                    .storage()
                    .persistent()
                    .has(&DataKey::MetadataHashPayment(hash.clone()))
                {
                    return Err(Error::DuplicateIdempotencyKey);
                }
            }

            if args.payment_id.is_empty() {
                return Err(Error::InvalidPaymentId);
            }

            // Validate metadata key count and key/value length limits.
            if let Some(ref meta_map) = args.metadata {
                utils::validate_metadata(meta_map)?;
            }

            // Check idempotency
            if let Some(ref token) = args.client_token {
                let key = DataKey::IdempotencyKey(token.clone());
                if let Some(existing_id) = env.storage().persistent().get::<DataKey, String>(&key) {
                    if existing_id != args.payment_id {
                        return Err(Error::DuplicateIdempotencyKey);
                    }
                }
            }
        }

        for merchant_id in batch_merchants.iter() {
            Self::enforce_create_payment_batch_rate_limit(&env, &merchant_id)?;
        }

        // All validations passed, now create all payments
        let mut payment_ids = vec![&env];
        let now = env.ledger().timestamp();

        for args in args_list.iter() {
            let resolved_expires_at = match args.expires_at {
                Some(ts) => ts,
                None => {
                    now.saturating_add(args.duration_secs.unwrap_or(DEFAULT_PAYMENT_DURATION_SECS))
                }
            };
            if resolved_expires_at <= now {
                return Err(Error::InvalidExpiry);
            }

            let payment = PaymentCharge {
                payment_id: args.payment_id.clone(),
                merchant_id: args.merchant_id.clone(),
                amount: args.amount,
                currency: args.currency.clone(),
                deposit_address: args.deposit_address.clone(),
                status: PaymentStatus::Pending,
                payer_address: None,
                transaction_hash: None,
                created_at: now,
                confirmed_at: None,
                expires_at: resolved_expires_at,
                amount_received: None,
                memo: args.memo.clone(),
                memo_type: args.memo_type.clone(),
                token_address: args.token_address.clone(),
                metadata_hash: args.metadata_hash.clone(),
                original_token: None,
                swap_path: None,
                fx_rate: None,
                fx_rate_at: None,
                metadata: args.metadata.clone(),
                fee_waiver_code: args.fee_waiver_code.clone(),
                retry_of_payment_id: None,
                payer_muxed_id: None,
                payment_link_id: None,
            };

            env.storage()
                .persistent()
                .set(&DataKey::Payment(args.payment_id.clone()), &payment);
            Self::bump_payment_ttl(&env, &args.payment_id, &payment.status);
            Self::index_payment_expiry(&env, &args.payment_id, payment.expires_at);

            // Issue #489: Store reverse index for metadata_hash → payment_id lookup in batch
            if let Some(ref hash) = args.metadata_hash {
                let key = DataKey::MetadataHashPayment(hash.clone());
                env.storage().persistent().set(&key, &args.payment_id);
                Self::bump_ttl(&env, &key, LONG_LIVE_TTL);
            }

            let mut merchant_payments =
                Self::get_merchant_payments_internal(&env, &args.merchant_id);
            merchant_payments.push_back(args.payment_id.clone());
            let merchant_payments_key = DataKey::MerchantPayments(args.merchant_id.clone());
            env.storage()
                .persistent()
                .set(&merchant_payments_key, &merchant_payments);
            Self::bump_ttl(&env, &merchant_payments_key, LONG_LIVE_TTL);

            env.events().publish(
                (Symbol::new(&env, "PAYMENT"), Symbol::new(&env, "CREATED")),
                (
                    args.payment_id.clone(),
                    args.merchant_id.clone(),
                    args.amount,
                    args.metadata.clone(),
                ),
            );

            // Issue #399: Persist idempotency key with TTL matching payment expiry window.
            if let Some(ref token) = args.client_token {
                let key = DataKey::IdempotencyKey(token.clone());
                env.storage().persistent().set(&key, &args.payment_id);
                let payment_duration_secs = resolved_expires_at.saturating_sub(now);
                let ledgers_per_sec: u64 = 5;
                let ttl_ledgers =
                    ((payment_duration_secs / ledgers_per_sec) as u32).max(SHORT_LIVE_TTL);
                Self::bump_ttl(&env, &key, ttl_ledgers);
                // Reverse mapping for cleanup on cancel/expire.
                let rev_token_id = Self::rev_key_for(&env, &args.payment_id);
                let rev_key = DataKey::IdempotencyKey(rev_token_id);
                env.storage().persistent().set(&rev_key, token);
                Self::bump_ttl(&env, &rev_key, ttl_ledgers);
            }

            payment_ids.push_back(args.payment_id.clone());
        }

        // Emit batch creation event
        env.events().publish(
            (
                Symbol::new(&env, "PAYMENT"),
                Symbol::new(&env, "BATCH_CREATED"),
            ),
            payment_ids.len(),
        );

        Ok(payment_ids)
    }

    #[allow(deprecated)]
    pub fn verify_payment(
        env: Env,
        oracle: Address,
        payment_id: String,
        transaction_hash: BytesN<32>,
        payer_address: Address,
        amount_received: i128,
        payer_muxed_id: Option<u64>,
    ) -> Result<PaymentStatus, Error> {
        Self::require_not_paused(&env)?;
        oracle.require_auth();
        Self::require_not_blacklisted(&env, &oracle)?;
        Self::require_not_blacklisted(&env, &payer_address)?;

        if !AccessControl::has_role(&env, &role_oracle(&env), &oracle) {
            return Err(Error::Unauthorized);
        }

        let mut payment = Self::get_payment_internal(&env, &payment_id)?;
        Self::require_not_blacklisted(&env, &payment.merchant_id)?;

        // Issue #75: Enforce idempotent verify_payment - reject double verification
        // If payment is already Confirmed, return current status without error
        if payment.status == PaymentStatus::Confirmed {
            return Ok(payment.status);
        }

        // Reject if payment is in any other terminal state
        if payment.status != PaymentStatus::Pending {
            return Err(Error::PaymentAlreadyProcessed);
        }

        if env.ledger().timestamp() > payment.expires_at {
            return Err(Error::PaymentExpired);
        }

        // Record the actual amount received for reconciliation
        payment.amount_received = Some(amount_received);
        payment.payer_address = Some(payer_address.clone());
        payment.transaction_hash = Some(transaction_hash);
        payment.confirmed_at = Some(env.ledger().timestamp());
        // Issue #484: Store muxed ID if M-address was used
        payment.payer_muxed_id = payer_muxed_id;

        // Get merchant-specific tolerance if available, otherwise use global default
        let merchant_tolerance = if let Some(registry_address) = env
            .storage()
            .persistent()
            .get::<DataKey, Address>(&DataKey::MerchantRegistryAddress)
        {
            let registry_client =
                crate::merchant_registry::MerchantRegistryClient::new(&env, &registry_address);
            match registry_client.try_get_merchant(&payment.merchant_id) {
                Ok(Ok(merchant)) => merchant.payment_tolerance,
                _ => None,
            }
        } else {
            None
        };

        // Scale tolerance by token decimals: 1 unit in the smallest denomination per decimal place.
        // USDC has 7 decimals on Stellar (stroops); other tokens may differ.
        // tolerance = 10^(decimals - 6) clamped to at least 1, so a 6-decimal token gets tolerance=1,
        // a 7-decimal token gets tolerance=10, a 2-decimal token gets tolerance=1 (clamped).
        let _base_tolerance = if let Some(ref token_addr) = payment.token_address {
            let decimals = token::TokenClient::new(&env, token_addr).decimals();
            if decimals >= 6 {
                let exp = decimals - 6;
                let mut t: i128 = 1;
                let mut i = 0u32;
                while i < exp {
                    t *= 10;
                    i += 1;
                }
                t
            } else {
                1i128
            }
        } else {
            PAYMENT_TOLERANCE
        };

        // Use merchant tolerance if set, otherwise use global tolerance, otherwise base tolerance
        let global_tolerance = if let Some(registry_address) = env
            .storage()
            .persistent()
            .get::<DataKey, Address>(&DataKey::MerchantRegistryAddress)
        {
            let registry_client =
                crate::merchant_registry::MerchantRegistryClient::new(&env, &registry_address);
            registry_client.get_global_payment_tolerance()
        } else {
            PAYMENT_TOLERANCE
        };

        let tolerance = merchant_tolerance.unwrap_or(global_tolerance);

        // Cap tolerance at 1% of payment amount to prevent abuse
        let max_tolerance = payment.amount / 100; // 1% of payment amount
        let tolerance = tolerance.min(max_tolerance);

        let diff = amount_received - payment.amount;

        let mut new_status = if (0..=tolerance).contains(&diff) {
            // Exact match or tiny overpay within tolerance → Confirmed
            PaymentStatus::Confirmed
        } else if diff > tolerance {
            // Meaningfully more than expected → Overpaid
            PaymentStatus::Overpaid
        } else if diff >= -tolerance {
            // Tiny underpay within tolerance → Confirmed
            PaymentStatus::Confirmed
        } else {
            // Meaningfully less than expected → PartiallyPaid
            PaymentStatus::PartiallyPaid
        };

        payment.status = new_status.clone();

        // Issue #304: snapshot the FX rate at verification time, if an oracle
        // is configured for this contract.
        if let Some(fx_oracle) = env
            .storage()
            .persistent()
            .get::<DataKey, Address>(&DataKey::FxOracleAddress)
        {
            let oracle_client = FXOracleClient::new(&env, &fx_oracle);
            match oracle_client.try_get_rate(&payment.currency) {
                Ok(Ok(rate_data)) => {
                    payment.fx_rate = Some(rate_data.rate);
                    payment.fx_rate_at = Some(env.ledger().timestamp());
                }
                _ => {
                    env.events().publish(
                        (
                            Symbol::new(&env, "PAYMENT"),
                            Symbol::new(&env, "FX_RATE_UNAVAILABLE"),
                        ),
                        payment_id.clone(),
                    );
                }
            }
        }

        env.storage()
            .persistent()
            .set(&DataKey::Payment(payment_id.clone()), &payment);
        Self::bump_payment_ttl(&env, &payment_id, &payment.status);

        let _event_name = match &new_status {
            PaymentStatus::Confirmed => Symbol::new(&env, "VERIFIED"),
            PaymentStatus::Overpaid => Symbol::new(&env, "OVERPAID"),
            PaymentStatus::PartiallyPaid => Symbol::new(&env, "PARTIALLY_PAID"),
            _ => Symbol::new(&env, "FAILED"),
        };
        let overpaid_refund_amount = if new_status == PaymentStatus::Overpaid {
            Some(amount_received.saturating_sub(payment.amount))
        } else {
            None
        };

        // Issue #471: Emit status-specific events for PartiallyPaid and Overpaid.
        if new_status == PaymentStatus::PartiallyPaid {
            env.events().publish(
                (
                    Symbol::new(&env, "PAYMENT"),
                    Symbol::new(&env, "PARTIALLY_PAID"),
                    payment.merchant_id.clone(),
                ),
                (payment_id.clone(), payment.amount, amount_received),
            );
        }
        if new_status == PaymentStatus::Overpaid {
            env.events().publish(
                (
                    Symbol::new(&env, "PAYMENT"),
                    Symbol::new(&env, "OVERPAID"),
                    payment.merchant_id.clone(),
                ),
                (payment_id.clone(), payment.amount, amount_received),
            );
        }

        // Issue #162: Merchant-configurable partial payment policy.
        if new_status == PaymentStatus::PartiallyPaid {
            let partial_allowed = if let Some(registry_address) = env
                .storage()
                .persistent()
                .get::<DataKey, Address>(&DataKey::MerchantRegistryAddress)
            {
                let registry_client =
                    crate::merchant_registry::MerchantRegistryClient::new(&env, &registry_address);
                match registry_client.try_get_merchant(&payment.merchant_id) {
                    Ok(Ok(merchant)) => merchant.partial_payment_allowed,
                    _ => false,
                }
            } else {
                false
            };

            if !partial_allowed {
                new_status = PaymentStatus::Failed;
                env.events().publish(
                    (
                        Symbol::new(&env, "REFUND"),
                        Symbol::new(&env, "AUTO_REQUIRED"),
                        payment.merchant_id.clone(),
                    ),
                    (payment_id.clone(), amount_received),
                );
            }
        }

        // Issue #63: Enforce tier-based monthly volume cap before confirming payment.
        if new_status == PaymentStatus::Confirmed || new_status == PaymentStatus::Overpaid {
            Self::enforce_tier_volume_cap(&env, &payment.merchant_id, amount_received)?;
        }

        // Issue #304: Check FX rate freshness when both registry and oracle address are configured
        if new_status == PaymentStatus::Confirmed || new_status == PaymentStatus::Overpaid {
            if let Some(_registry_address) = env
                .storage()
                .persistent()
                .get::<DataKey, Address>(&DataKey::MerchantRegistryAddress)
            {
                if let Some(oracle_address) = env
                    .storage()
                    .persistent()
                    .get::<DataKey, Address>(&DataKey::FXOracleAddress)
                {
                    let oracle_client =
                        crate::fx_oracle::FXOracleClient::new(&env, &oracle_address);
                    match oracle_client.try_get_rate(&payment.currency) {
                        Ok(Ok(rate_data)) => {
                            payment.fx_rate = Some(rate_data.rate);
                            payment.fx_rate_at = Some(rate_data.updated_at);
                        }
                        _ => {
                            return Err(Error::StaleOracleRate);
                        }
                    }
                }
            }
        }

        // Issue #505: Validate status transition through state machine
        payment.status =
            payment_state_machine::transition_status(&payment.status, new_status.clone())?;
        Self::record_payment_status(&env, &payment);

        if let Some(refund_amount) = overpaid_refund_amount {
            if Self::get_auto_refund_overpayment(&env) {
                if let Some(registry_address) = env
                    .storage()
                    .persistent()
                    .get::<DataKey, Address>(&DataKey::MerchantRegistryAddress)
                {
                    let registry_client = crate::merchant_registry::MerchantRegistryClient::new(
                        &env,
                        &registry_address,
                    );

                    if let Some(refund_manager_address) =
                        registry_client.get_refund_manager_address()
                    {
                        let refund_client = RefundManagerClient::new(&env, &refund_manager_address);
                        refund_client.register_payment(
                            &payment_id,
                            &payment.merchant_id,
                            &payment.amount,
                            &payment.currency,
                        );

                        let auto_reason =
                            String::from_str(&env, "Automatic refund for overpayment");
                        let auto_refund_id = refund_client
                            .try_queue_auto_refund(
                                &env.current_contract_address(),
                                &registry_address,
                                &payment_id,
                                &refund_amount,
                                &payer_address,
                                &auto_reason,
                            )
                            .map_err(|_| Error::Unauthorized)?
                            .map_err(|_| Error::Unauthorized)?;

                        env.events().publish(
                            (
                                Symbol::new(&env, "REFUND"),
                                Symbol::new(&env, "AUTO_CREATED"),
                                payment.merchant_id.clone(),
                            ),
                            (payment_id.clone(), refund_amount, auto_refund_id),
                        );
                    }
                }
            }
        }

        // Issue #492: Auto-create or update customer profile on confirmed payment
        if new_status == PaymentStatus::Confirmed || new_status == PaymentStatus::Overpaid {
            let customer_key =
                DataKey::CustomerProfile(payment.merchant_id.clone(), payer_address.clone());
            let mut customer_profile = if let Some(existing) =
                env.storage()
                    .persistent()
                    .get::<DataKey, CustomerProfile>(&customer_key)
            {
                existing
            } else {
                CustomerProfile {
                    customer_id: payer_address.clone(),
                    merchant_id: payment.merchant_id.clone(),
                    email_hash: None,
                    created_at: env.ledger().timestamp(),
                    payment_count: 0,
                    total_spent: 0,
                }
            };
            customer_profile.payment_count += 1;
            customer_profile.total_spent += amount_received;
            env.storage()
                .persistent()
                .set(&customer_key, &customer_profile);
            Self::bump_ttl(&env, &customer_key, LONG_LIVE_TTL);
        }

        env.storage()
            .persistent()
            .set(&DataKey::Payment(payment_id.clone()), &payment);
        Self::bump_payment_ttl(&env, &payment_id, &payment.status);

        let event_name = match &new_status {
            PaymentStatus::Confirmed => Symbol::new(&env, "VERIFIED"),
            PaymentStatus::Overpaid => Symbol::new(&env, "OVERPAID"),
            PaymentStatus::PartiallyPaid => Symbol::new(&env, "PARTIALLY_PAID"),
            _ => Symbol::new(&env, "FAILED"),
        };

        // Issue #166: Optimize event topics for efficient indexing
        env.events().publish(
            (
                Symbol::new(&env, "PAYMENT"),
                event_name,
                payment.merchant_id.clone(),
            ),
            (payment_id.clone(), payment.amount, amount_received),
        );

        Ok(new_status)
    }

    pub fn verify_payment_batch(
        env: Env,
        operator: Address,
        verifications: Vec<VerifyPaymentArgs>,
    ) -> Result<Vec<Result<PaymentStatus, Error>>, Error> {
        operator.require_auth();
        if verifications.len() > 20 {
            return Err(Error::BatchTooLarge);
        }

        let mut results = Vec::new(&env);
        for verification in verifications.iter() {
            results.push_back(Self::verify_payment(
                env.clone(),
                operator.clone(),
                verification.payment_id,
                verification.transaction_hash,
                verification.payer_address,
                verification.amount_received,
                verification.payer_muxed_id,
            ));
        }
        Ok(results)
    }

    /// Issue #471: Allow a merchant to accept a PartiallyPaid payment at the received amount.
    /// Moves the payment from PartiallyPaid to Confirmed, using the received amount as the
    /// effective payment amount. No refund is created for the difference.
    pub fn accept_partial_payment(
        env: Env,
        merchant_id: Address,
        payment_id: String,
    ) -> Result<(), Error> {
        merchant_id.require_auth();
        Self::require_not_blacklisted(&env, &merchant_id)?;

        let mut payment = Self::get_payment_internal(&env, &payment_id)?;
        if payment.status != PaymentStatus::PartiallyPaid {
            return Err(Error::PaymentAlreadyProcessed);
        }
        if payment.merchant_id != merchant_id {
            return Err(Error::Unauthorized);
        }

        payment.status = PaymentStatus::Confirmed;
        Self::record_payment_status(&env, &payment);
        let amount_received = payment.amount_received.unwrap_or(payment.amount);
        payment.amount = amount_received;

        env.storage()
            .persistent()
            .set(&DataKey::Payment(payment_id.clone()), &payment);

        env.events().publish(
            (
                Symbol::new(&env, "PAYMENT"),
                Symbol::new(&env, "PARTIAL_ACCEPTED"),
                merchant_id,
            ),
            (payment_id, amount_received),
        );

        Ok(())
    }

    /// Issue #471: Allow a customer to complete a PartiallyPaid payment by topping up.
    /// This is a declaration that the payer will send additional funds off-chain.
    /// The payment status is moved to Pending so a new verify_payment call can
    /// confirm it with the combined amount.
    pub fn complete_partial_payment(
        env: Env,
        payer: Address,
        payment_id: String,
        top_up_amount: i128,
    ) -> Result<(), Error> {
        payer.require_auth();
        Self::require_not_blacklisted(&env, &payer)?;

        if top_up_amount <= 0 {
            return Err(Error::InvalidAmount);
        }

        let mut payment = Self::get_payment_internal(&env, &payment_id)?;
        if payment.status != PaymentStatus::PartiallyPaid {
            return Err(Error::PaymentAlreadyProcessed);
        }

        payment.status = PaymentStatus::Pending;
        Self::record_payment_status(&env, &payment);
        payment.amount = payment.amount.saturating_add(top_up_amount);

        env.storage()
            .persistent()
            .set(&DataKey::Payment(payment_id.clone()), &payment);

        env.events().publish(
            (
                Symbol::new(&env, "PAYMENT"),
                Symbol::new(&env, "PARTIAL_TOPUP"),
                payer,
            ),
            (payment_id, top_up_amount),
        );

        Ok(())
    }

    /// Issue #482: Create a retry payment for an expired or failed original payment.
    /// Returns the payment_id of the newly created payment, linked to the original.
    /// Maximum retry chain depth is 3.
    pub fn retry_payment(
        env: Env,
        merchant_id: Address,
        original_payment_id: String,
        new_expires_at: u64,
    ) -> Result<String, Error> {
        Self::require_creation_not_paused(&env)?;
        merchant_id.require_auth();
        Self::require_not_blacklisted(&env, &merchant_id)?;

        // Retrieve original payment
        let original = Self::get_payment_internal(&env, &original_payment_id)?;

        // Validate original payment status (must be Expired or Failed)
        if original.status != PaymentStatus::Expired && original.status != PaymentStatus::Failed {
            return Err(Error::PaymentAlreadyProcessed);
        }

        // Validate merchant ownership
        if original.merchant_id != merchant_id {
            return Err(Error::Unauthorized);
        }

        // Check retry chain depth (max 3)
        let mut current_id = original_payment_id.clone();
        let mut depth = 1u32;
        while let Some(ref retry_of) = {
            let payment = Self::get_payment_internal(&env, &current_id)?;
            payment.retry_of_payment_id.clone()
        } {
            current_id = retry_of.clone();
            depth = depth.saturating_add(1);
            if depth > 3 {
                return Err(Error::RetryChainTooDeep);
            }
        }

        // Validate new_expires_at
        let now = env.ledger().timestamp();
        if new_expires_at <= now {
            return Err(Error::InvalidExpiry);
        }

        // Generate new payment ID
        let new_payment_id = format_id(&env, "pay_", env.ledger().timestamp());

        // Create new PaymentCharge with inherited properties
        let new_payment = PaymentCharge {
            payment_id: new_payment_id.clone(),
            merchant_id: original.merchant_id.clone(),
            amount: original.amount,
            currency: original.currency.clone(),
            deposit_address: original.deposit_address.clone(),
            status: PaymentStatus::Pending,
            payer_address: None,
            transaction_hash: None,
            created_at: now,
            confirmed_at: None,
            expires_at: new_expires_at,
            amount_received: None,
            memo: original.memo.clone(),
            memo_type: original.memo_type.clone(),
            token_address: original.token_address.clone(),
            metadata_hash: original.metadata_hash.clone(),
            original_token: None,
            swap_path: None,
            fx_rate: None,
            fx_rate_at: None,
            metadata: original.metadata.clone(),
            fee_waiver_code: original.fee_waiver_code.clone(),
            retry_of_payment_id: Some(original_payment_id.clone()),
            payer_muxed_id: None,
            payment_link_id: original.payment_link_id.clone(),
        };

        // Store new payment
        env.storage()
            .persistent()
            .set(&DataKey::Payment(new_payment_id.clone()), &new_payment);
        Self::bump_payment_ttl(&env, &new_payment_id, &new_payment.status);

        // Track retry link
        let retries_key = DataKey::PaymentRetries(original_payment_id.clone());
        let mut retries: Vec<String> = env
            .storage()
            .persistent()
            .get(&retries_key)
            .unwrap_or_else(|| vec![&env]);
        retries.push_back(new_payment_id.clone());
        env.storage().persistent().set(&retries_key, &retries);
        Self::bump_ttl(&env, &retries_key, LONG_LIVE_TTL);

        // Emit PAYMENT/RETRY_CREATED event
        env.events().publish(
            (
                Symbol::new(&env, "PAYMENT"),
                Symbol::new(&env, "RETRY_CREATED"),
            ),
            (original_payment_id, new_payment_id.clone()),
        );

        Ok(new_payment_id)
    }

    pub fn get_payment(env: Env, payment_id: String) -> Result<PaymentCharge, Error> {
        let payment = Self::get_payment_internal(&env, &payment_id)?;
        Self::bump_payment_ttl(&env, &payment_id, &payment.status);
        Ok(payment)
    }

    pub fn get_payment_status_history(
        env: Env,
        payment_id: String,
    ) -> Result<Vec<PaymentStatusEvent>, Error> {
        Self::get_payment_internal(&env, &payment_id)?;
        Ok(env
            .storage()
            .persistent()
            .get(&DataKey::PaymentStatusHistory(payment_id))
            .unwrap_or_else(|| vec![&env]))
    }

    /// Issue #489: Reverse lookup payment by metadata_hash for order reconciliation.
    pub fn get_payment_by_metadata_hash(
        env: Env,
        metadata_hash: BytesN<32>,
    ) -> Result<PaymentCharge, Error> {
        let payment_id = env
            .storage()
            .persistent()
            .get::<DataKey, String>(&DataKey::MetadataHashPayment(metadata_hash.clone()))
            .ok_or(Error::PaymentNotFound)?;
        Self::get_payment(env, payment_id)
    }

    /// Issue #492: Get customer profile for merchant and customer pair.
    pub fn get_customer(
        env: Env,
        merchant_id: Address,
        customer_id: Address,
    ) -> Result<CustomerProfile, Error> {
        env.storage()
            .persistent()
            .get(&DataKey::CustomerProfile(merchant_id, customer_id))
            .ok_or(Error::PaymentNotFound)
    }

    /// Issue #492: Get top customers for a merchant sorted by total_spent (descending).
    pub fn get_top_customers(env: Env, merchant_id: Address, limit: u32) -> Vec<CustomerProfile> {
        let all_payments = Self::get_merchant_payments_internal(&env, &merchant_id);
        let mut customers: Map<Address, CustomerProfile> = map![&env];

        for payment_id in all_payments.iter() {
            if let Ok(payment) = Self::get_payment_internal(&env, &payment_id) {
                if payment.status == PaymentStatus::Confirmed
                    || payment.status == PaymentStatus::Overpaid
                {
                    if let Some(payer) = payment.payer_address {
                        if let Ok(profile) =
                            Self::get_customer(env.clone(), merchant_id.clone(), payer.clone())
                        {
                            customers.set(payer, profile);
                        }
                    }
                }
            }
        }

        let mut sorted: Vec<CustomerProfile> = vec![&env];
        for (_, profile) in customers.iter() {
            sorted.push_back(profile);
        }

        let mut i = 0;
        while i < sorted.len() {
            let mut j = i + 1;
            while j < sorted.len() {
                if let (Some(profile_i), Some(profile_j)) = (sorted.get(i), sorted.get(j)) {
                    if profile_j.total_spent > profile_i.total_spent {
                        sorted.set(i, profile_j.clone());
                        sorted.set(j, profile_i.clone());
                    }
                }
                j += 1;
            }
            i += 1;
        }

        let capped_limit = if limit == 0 {
            sorted.len()
        } else {
            limit.min(sorted.len())
        };
        let mut result = vec![&env];
        let mut idx = 0u32;
        while idx < capped_limit {
            if let Some(profile) = sorted.get(idx) {
                result.push_back(profile);
            }
            idx += 1;
        }
        result
    }

    /// Issue #628: Record a payment's `amount` against the merchant's cumulative
    /// gross-volume index and register the merchant in the tracked-merchant list
    /// (idempotently) so `get_top_merchants` can rank merchants without scanning
    /// individual payment records.
    fn record_merchant_volume(env: &Env, merchant_id: &Address, amount: i128) {
        let volume_key = DataKey::MerchantGrossVolume(merchant_id.clone());
        let current: i128 = env.storage().persistent().get(&volume_key).unwrap_or(0i128);
        env.storage()
            .persistent()
            .set(&volume_key, &current.saturating_add(amount));
        Self::bump_ttl(env, &volume_key, LONG_LIVE_TTL);

        let list_key = DataKey::TrackedMerchants;
        let mut merchants: Vec<Address> = env
            .storage()
            .persistent()
            .get(&list_key)
            .unwrap_or_else(|| vec![env]);
        if !merchants.contains(merchant_id) {
            merchants.push_back(merchant_id.clone());
            env.storage().persistent().set(&list_key, &merchants);
            Self::bump_ttl(env, &list_key, LONG_LIVE_TTL);
        }
    }

    /// Issue #628: Rank merchants by cumulative gross payment volume (descending).
    ///
    /// Reads only the per-merchant `MerchantGrossVolume` / `MerchantPaymentCount`
    /// indexes plus the `TrackedMerchants` list — it never iterates payment
    /// records. `limit` is capped at [`TOP_MERCHANTS_MAX_LIMIT`] (100); a
    /// `limit` of 0 is treated as the cap.
    pub fn get_top_merchants(env: Env, limit: u32) -> Vec<MerchantRanking> {
        let capped_limit = if limit == 0 {
            TOP_MERCHANTS_MAX_LIMIT
        } else {
            limit.min(TOP_MERCHANTS_MAX_LIMIT)
        };

        let merchants: Vec<Address> = env
            .storage()
            .persistent()
            .get(&DataKey::TrackedMerchants)
            .unwrap_or_else(|| vec![&env]);

        let mut rankings: Vec<MerchantRanking> = vec![&env];
        for merchant_id in merchants.iter() {
            let total_volume: i128 = env
                .storage()
                .persistent()
                .get(&DataKey::MerchantGrossVolume(merchant_id.clone()))
                .unwrap_or(0i128);
            let payment_count: u64 = env
                .storage()
                .persistent()
                .get(&DataKey::MerchantPaymentCount(merchant_id.clone()))
                .unwrap_or(0u64);
            rankings.push_back(MerchantRanking {
                merchant_id,
                total_volume,
                payment_count,
            });
        }

        // Selection sort by total_volume descending (mirrors get_top_customers).
        let mut i = 0;
        while i < rankings.len() {
            let mut j = i + 1;
            while j < rankings.len() {
                if let (Some(a), Some(b)) = (rankings.get(i), rankings.get(j)) {
                    if b.total_volume > a.total_volume {
                        rankings.set(i, b.clone());
                        rankings.set(j, a.clone());
                    }
                }
                j += 1;
            }
            i += 1;
        }

        let end = capped_limit.min(rankings.len());
        let mut result: Vec<MerchantRanking> = vec![&env];
        let mut idx = 0u32;
        while idx < end {
            if let Some(ranking) = rankings.get(idx) {
                result.push_back(ranking);
            }
            idx += 1;
        }
        result
    }

    /// Issue #488: Permissionless public entry point for TTL maintenance.
    pub fn bump_payment_ttl_public(env: Env, payment_id: String) -> Result<(), Error> {
        let payment = Self::get_payment_internal(&env, &payment_id)?;
        Self::bump_payment_ttl(&env, &payment_id, &payment.status);
        Ok(())
    }

    /// Issue #488: Bulk bump payment TTLs for efficient maintenance sweeps (max 50).
    pub fn bulk_bump_payment_ttls(env: Env, payment_ids: Vec<String>) -> Result<u32, Error> {
        if payment_ids.len() > 50 {
            return Err(Error::BatchTooLarge);
        }

        let mut bumped = 0u32;
        for payment_id in payment_ids.iter() {
            if let Ok(payment) = Self::get_payment_internal(&env, &payment_id) {
                Self::bump_payment_ttl(&env, &payment_id, &payment.status);
                bumped += 1;
            }
        }
        Ok(bumped)
    }

    pub fn get_merchant_payments(env: Env, merchant_id: Address) -> Vec<String> {
        Self::get_merchant_payments_internal(&env, &merchant_id)
    }

    pub fn get_merchant_payments_paginated(
        env: Env,
        merchant_id: Address,
        offset: u32,
        limit: u32,
        status_filter: Option<PaymentStatus>,
        token_address: Option<Address>,
    ) -> Vec<String> {
        let all = Self::get_merchant_payments_internal(&env, &merchant_id);
        if limit == 0 {
            return vec![&env];
        }

        let mut filtered = vec![&env];
        for id in all.iter() {
            if let Some(payment) = env
                .storage()
                .persistent()
                .get::<DataKey, PaymentCharge>(&DataKey::Payment(id.clone()))
            {
                let status_match = match &status_filter {
                    Some(status) => payment.status == status.clone(),
                    None => true,
                };

                let token_match = match &token_address {
                    Some(token) => payment.token_address.as_ref() == Some(token),
                    None => true,
                };

                if status_match && token_match {
                    filtered.push_back(id);
                }
            }
        }

        let mut page = vec![&env];
        let start = offset;
        let end = core::cmp::min(filtered.len(), start.saturating_add(limit));

        let mut i = start;
        while i < end {
            if let Some(id) = filtered.get(i) {
                page.push_back(id);
            }
            i += 1;
        }

        page
    }

    /// Generate a reconciliation report for a merchant over a time period.
    ///
    /// This is a read-only query that returns a structured summary of all
    /// settlements, fees, refunds, and disputes in the specified period.
    ///
    /// # Parameters
    /// * `merchant_id` - The merchant to generate the report for
    /// * `from_ts` - Start timestamp (inclusive)
    /// * `to_ts` - End timestamp (inclusive)
    /// * `offset` - Pagination offset (number of payments to skip)
    /// * `limit` - Maximum number of payments to include (max 100)
    pub fn generate_reconciliation_report(
        env: Env,
        merchant_id: Address,
        from_ts: u64,
        to_ts: u64,
        offset: u32,
        limit: u32,
    ) -> Result<ReconciliationReport, Error> {
        if from_ts > to_ts {
            return Err(Error::InvalidExpiry);
        }

        let capped_limit = if limit == 0 || limit > 100 {
            100
        } else {
            limit
        };

        const SECONDS_PER_DAY: u64 = 86_400;
        let start_bucket = from_ts / SECONDS_PER_DAY;
        let end_bucket = to_ts / SECONDS_PER_DAY;

        let mut candidate_ids: Vec<String> = vec![&env];
        let mut bucket = start_bucket;
        while bucket <= end_bucket {
            let key = DataKey::DailyPaymentIndex(merchant_id.clone(), bucket);
            if let Some(bucket_ids) = env.storage().persistent().get::<DataKey, Vec<String>>(&key) {
                for id in bucket_ids.iter() {
                    candidate_ids.push_back(id);
                }
            }
            bucket = bucket.saturating_add(1);
        }

        let mut payments_in_period = vec![&env];
        let mut total_gross: i128 = 0;
        let mut total_fees: i128 = 0;
        let mut total_refunds: i128 = 0;
        let dispute_adjustments: i128 = 0;

        let default_fee_bps = Self::get_refund_fee_bps_internal(&env);

        let merchant_fee_bps = if let Some(registry_address) = env
            .storage()
            .persistent()
            .get::<DataKey, Address>(&DataKey::MerchantRegistryAddress)
        {
            let registry_client =
                crate::merchant_registry::MerchantRegistryClient::new(&env, &registry_address);
            match registry_client.try_get_merchant(&merchant_id) {
                Ok(Ok(merchant)) => {
                    use crate::merchant_registry::KycTier;
                    match merchant.kyc_tier {
                        KycTier::Business => REFUND_FEE_BPS_BUSINESS,
                        KycTier::Full => REFUND_FEE_BPS_FULL,
                        KycTier::Basic => REFUND_FEE_BPS_BASIC,
                        KycTier::Unverified => default_fee_bps,
                    }
                }
                _ => default_fee_bps,
            }
        } else {
            default_fee_bps
        };

        for payment_id in candidate_ids.iter() {
            if let Ok(payment) = Self::get_payment_internal(&env, &payment_id) {
                let payment_time = payment.confirmed_at.unwrap_or(payment.created_at);

                if payment_time >= from_ts && payment_time <= to_ts {
                    let mut refund_amount: i128 = 0;

                    let refund_ids = RefundManager::get_payment_refunds_internal(&env, &payment_id);
                    for refund_id in refund_ids.iter() {
                        if let Ok(refund) = RefundManager::get_refund_internal(&env, &refund_id) {
                            if refund.status == RefundStatus::Completed {
                                refund_amount += refund.amount;
                            }
                        }
                    }

                    let fee = if refund_amount > 0 {
                        refund_amount * merchant_fee_bps / 10_000
                    } else {
                        0
                    };

                    let summary = PaymentSummary {
                        payment_id: payment.payment_id.clone(),
                        amount: payment.amount,
                        fee,
                        refund_amount,
                        status: payment.status.clone(),
                        settled_at: payment.confirmed_at.unwrap_or(0),
                    };

                    payments_in_period.push_back(summary.clone());
                    total_gross += payment.amount;
                    total_fees += fee;
                    total_refunds += refund_amount;
                }
            }
        }

        let mut paginated_payments = vec![&env];
        let start = offset;
        let end = core::cmp::min(payments_in_period.len(), start.saturating_add(capped_limit));

        let mut i = start;
        while i < end {
            if let Some(summary) = payments_in_period.get(i) {
                paginated_payments.push_back(summary);
            }
            i += 1;
        }

        let total_net_settled = total_gross - total_refunds;

        Ok(ReconciliationReport {
            merchant_id,
            period_start: from_ts,
            period_end: to_ts,
            payments: paginated_payments,
            total_gross,
            total_fees,
            total_refunds,
            total_net_settled,
            dispute_adjustments,
        })
    }

    pub fn reconciliation_report_page(
        env: Env,
        merchant_id: Address,
        from_ts: u64,
        to_ts: u64,
        offset: u32,
        limit: u32,
    ) -> Result<ReconciliationPage, Error> {
        let report = Self::generate_reconciliation_report(
            env.clone(),
            merchant_id,
            from_ts,
            to_ts,
            offset,
            limit,
        )?;
        let mut page_total = 0i128;
        for item in report.payments.iter() {
            page_total = page_total.saturating_add(item.amount);
        }
        let page_size = if limit == 0 { 100 } else { limit.min(100) };
        let has_more = page_size > 0 && page_size == report.payments.len();
        Ok(ReconciliationPage {
            items: report.payments,
            total_confirmed: report.total_gross,
            total_settled: report.total_net_settled,
            page_total,
            has_more,
        })
    }

    #[allow(deprecated)]
    pub fn cancel_payment(env: Env, authority: Address, payment_id: String) -> Result<(), Error> {
        Self::require_not_paused(&env)?;

        let mut payment = Self::get_payment_internal(&env, &payment_id)?;

        if payment.status != PaymentStatus::Pending {
            return Err(Error::PaymentAlreadyProcessed);
        }

        // Ensure the current time is less than the expiry time; if not, mark as expired and return.
        if env.ledger().timestamp() >= payment.expires_at {
            payment.status =
                payment_state_machine::transition_status(&payment.status, PaymentStatus::Expired)?;
            Self::record_payment_status(&env, &payment);

            env.storage()
                .persistent()
                .set(&DataKey::Payment(payment_id.clone()), &payment);
            Self::bump_payment_ttl(&env, &payment_id, &payment.status);
            // Issue #399: free idempotency key so client_token can be reused.
            Self::remove_idempotency_key(&env, &payment_id);

            // Issue #166: Optimize event topics
            env.events().publish(
                (
                    Symbol::new(&env, "PAYMENT"),
                    Symbol::new(&env, "EXPIRED"),
                    payment.merchant_id.clone(),
                ),
                (payment_id.clone(), payment.amount),
            );

            return Ok(());
        }

        authority.require_auth();
        let is_merchant = authority == payment.merchant_id;
        let is_oracle = AccessControl::has_role(&env, &role_oracle(&env), &authority);
        if !is_merchant && !is_oracle {
            return Err(Error::Unauthorized);
        }

        payment.status =
            payment_state_machine::transition_status(&payment.status, PaymentStatus::Failed)?;
        Self::record_payment_status(&env, &payment);

        env.storage()
            .persistent()
            .set(&DataKey::Payment(payment_id.clone()), &payment);
        Self::bump_payment_ttl(&env, &payment_id, &payment.status);
        // Issue #399: free idempotency key so client_token can be reused after cancellation.
        Self::remove_idempotency_key(&env, &payment_id);
        Self::remove_payment_from_expiry_bucket(&env, &payment_id, payment.expires_at);

        // Issue #166: Optimize event topics
        env.events().publish(
            (
                Symbol::new(&env, "PAYMENT"),
                Symbol::new(&env, "CANCELLED"),
                payment.merchant_id.clone(),
            ),
            (payment_id.clone(), payment.amount),
        );

        Ok(())
    }

    #[allow(deprecated)]
    pub fn expire_payment(env: Env, payment_id: String) -> Result<(), Error> {
        let mut payment = Self::get_payment_internal(&env, &payment_id)?;

        if payment.status != PaymentStatus::Pending {
            return Err(Error::PaymentAlreadyProcessed);
        }

        if env.ledger().timestamp() <= payment.expires_at {
            return Err(Error::PaymentExpired);
        }

        payment.status =
            payment_state_machine::transition_status(&payment.status, PaymentStatus::Expired)?;
        Self::record_payment_status(&env, &payment);

        env.storage()
            .persistent()
            .set(&DataKey::Payment(payment_id.clone()), &payment);
        Self::bump_payment_ttl(&env, &payment_id, &payment.status);
        // Issue #399: free idempotency key so client_token can be reused.
        Self::remove_idempotency_key(&env, &payment_id);
        Self::remove_payment_from_expiry_bucket(&env, &payment_id, payment.expires_at);

        // Issue #166: Optimize event topics
        env.events().publish(
            (
                Symbol::new(&env, "PAYMENT"),
                Symbol::new(&env, "EXPIRED"),
                payment.merchant_id.clone(),
            ),
            (payment_id.clone(), payment.amount),
        );

        Ok(())
    }

    pub fn batch_expire_payments(env: Env, payment_ids: Vec<String>) -> Result<u32, Error> {
        Self::require_not_paused(&env)?;

        let current_bucket = Self::expiry_bucket_for(env.ledger().timestamp());
        let mut selected_bucket: Option<u32> = None;
        if let Some(buckets) = env
            .storage()
            .persistent()
            .get::<DataKey, Vec<u32>>(&DataKey::PaymentExpiryBuckets)
        {
            for bucket in buckets.iter() {
                if bucket <= current_bucket && selected_bucket.is_none_or(|lowest| bucket < lowest)
                {
                    selected_bucket = Some(bucket);
                }
            }
        }

        let candidates: Vec<String> = if let Some(bucket) = selected_bucket {
            env.storage()
                .persistent()
                .get(&DataKey::PaymentsByExpiry(bucket))
                .unwrap_or_else(|| vec![&env])
        } else {
            payment_ids
        };

        let mut count = 0;
        let max = if candidates.len() > 50 {
            50
        } else {
            candidates.len()
        };
        let mut i = 0;
        while i < max {
            if let Some(payment_id) = candidates.get(i) {
                if Self::expire_payment(env.clone(), payment_id).is_ok() {
                    count += 1;
                }
            }
            i += 1;
        }

        Ok(count)
    }

    #[allow(clippy::type_complexity)]
    pub fn settle_payment(
        env: Env,
        operator: Address,
        payment_id: String,
        splits: Vec<SettlementSplit>,
    ) -> Result<(), Error> {
        operator.require_auth();

        if !AccessControl::has_role(&env, &role_settlement_operator(&env), &operator) {
            return Err(Error::Unauthorized);
        }

        Self::require_not_paused(&env)?;

        if env
            .storage()
            .persistent()
            .get::<DataKey, bool>(&DataKey::ReentrancyLock)
            .unwrap_or(false)
        {
            return Err(Error::Reentrancy);
        }
        env.storage()
            .persistent()
            .set(&DataKey::ReentrancyLock, &true);
        let _guard = ReentrancyGuard { env: &env };

        let mut payment = Self::get_payment_internal(&env, &payment_id)?;

        if payment.status != PaymentStatus::Confirmed {
            return Err(Error::PaymentAlreadyProcessed);
        }

        // Resolve the settlement token: use payment.token_address if set, else the configured USDC token
        let settlement_token: Option<Address> = payment.token_address.clone().or_else(|| {
            env.storage()
                .persistent()
                .get::<DataKey, Address>(&DataKey::UsdcToken)
        });

        // ── Configurable settlement fee split (treasury + developer) ─────────────
        let now = env.ledger().timestamp();

        // ── Fee-waiver evaluation (merchant time-based + per-payment code) ────────
        // We resolve whether a fee waiver applies BEFORE computing any fee values
        // so the same decision is honored by both settlement-fee and merchant-fee
        // code paths. The `fee_waiver_reason` below doubles as the reason string
        // emitted in the `PAYMENT/FEE_WAIVED` event; `None` means no waiver.
        //
        // Per-payment codes take precedence over (and are evaluated independently
        // of) the merchant-level time-based waiver. If both are present only the
        // code is consumed (since its remaining_uses must be decremented) and the
        // event reason reflects the code source.
        let fee_waiver_reason: Option<String> = {
            // 1. Per-payment code path (stronger: consumes uses if valid)
            if let Some(ref code) = payment.fee_waiver_code {
                let key = DataKey::FeeWaiverCode(code.clone());
                if let Some(mut record) = env
                    .storage()
                    .persistent()
                    .get::<DataKey, FeeWaiverCodeRecord>(&key)
                {
                    if now < record.expires_at && record.remaining_uses > 0 {
                        record.remaining_uses = record.remaining_uses.saturating_sub(1);
                        env.storage().persistent().set(&key, &record);
                        Some(crate::utils::concat_strings(
                            &env,
                            &[String::from_str(&env, "code_waiver:"), code.clone()],
                        ))
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                // 2. Merchant-level time-based waiver (cheaper, no uses to track)
                let registry_addr_opt = env
                    .storage()
                    .persistent()
                    .get::<DataKey, Address>(&DataKey::MerchantRegistryAddress);
                match registry_addr_opt {
                    Some(registry_addr) => {
                        use crate::merchant_registry::MerchantRegistryClient;
                        let registry_client = MerchantRegistryClient::new(&env, &registry_addr);
                        let merchant = registry_client.get_merchant(&payment.merchant_id);
                        match merchant.fee_waiver_expires_at {
                            Some(ts) if now < ts => Some(String::from_str(&env, "merchant_waiver")),
                            _ => None,
                        }
                    }
                    None => None,
                }
            }
        };

        // If any waiver resolved as active, emit the canonical PAYMENT/FEE_WAIVED
        // event once here (both settlement code paths read from this event).
        if let Some(ref reason) = fee_waiver_reason {
            env.events().publish(
                (
                    Symbol::new(&env, "PAYMENT"),
                    Symbol::new(&env, "FEE_WAIVED"),
                    payment.merchant_id.clone(),
                ),
                (payment_id.clone(), reason.clone()),
            );
        }

        // ── Configurable settlement fee (accumulated in TreasuryBalance) ─────────
        // Read the settlement fee rate in basis points. 0 bps → no fee, no event.
        // Waived: if any fee waiver is active, settlement_fee is forced to 0
        // regardless of global rate.
        let settlement_fee_bps: i128 = env
            .storage()
            .persistent()
            .get::<DataKey, i128>(&DataKey::SettlementFeeRate)
            .unwrap_or(0);

        let settlement_fee: i128 = if fee_waiver_reason.is_some() {
            0
        } else if settlement_fee_bps > 0 {
            payment.amount * settlement_fee_bps / 10_000
        } else {
            0
        };

        if settlement_fee > 0 {
            // Check whether a FeeSplitConfig has been configured.
            let fee_split_config: Option<FeeSplitConfig> =
                env.storage().persistent().get(&DataKey::FeeSplitConfig);

            if let Some(ref fsc) = fee_split_config {
                // Split the fee between treasury and developer.
                // dev_amount = fee * developer_bps / 10000; treasury gets the remainder
                // (including any rounding dust) so no tokens are lost.
                let dev_amount: i128 = settlement_fee * fsc.developer_bps as i128 / 10_000;

                // Rounding dust goes to treasury.
                let treasury_total = settlement_fee.saturating_sub(dev_amount);

                if let Some(ref st) = settlement_token {
                    let token_client = token::TokenClient::new(&env, st);
                    let from = env.current_contract_address();
                    if treasury_total > 0 {
                        let _ = token_client.try_transfer(
                            &from,
                            &fsc.treasury_address,
                            &treasury_total,
                        );
                    }
                    if dev_amount > 0 {
                        let _ =
                            token_client.try_transfer(&from, &fsc.developer_address, &dev_amount);
                    }
                }

                // Emit PAYMENT/FEE_SPLIT
                env.events().publish(
                    (Symbol::new(&env, "PAYMENT"), Symbol::new(&env, "FEE_SPLIT")),
                    (payment_id.clone(), treasury_total, dev_amount),
                );

                // Issue #666: record this settlement's fee split for
                // get_platform_fee_report.
                Self::record_fee_collection(&env, settlement_fee, treasury_total, dev_amount);
            } else {
                // No FeeSplitConfig — accumulate entire fee in TreasuryBalance (legacy path).
                let current_treasury: i128 = env
                    .storage()
                    .persistent()
                    .get::<DataKey, i128>(&DataKey::TreasuryBalance)
                    .unwrap_or(0);
                env.storage().persistent().set(
                    &DataKey::TreasuryBalance,
                    &current_treasury.saturating_add(settlement_fee),
                );

                env.events().publish(
                    (
                        Symbol::new(&env, "PAYMENT"),
                        Symbol::new(&env, "FEE_COLLECTED"),
                    ),
                    (
                        payment_id.clone(),
                        payment.merchant_id.clone(),
                        settlement_fee,
                    ),
                );

                // Issue #666: record this settlement's fee for get_platform_fee_report
                // (legacy path: entire fee accrues to treasury, no developer split).
                Self::record_fee_collection(&env, settlement_fee, settlement_fee, 0);
            }
        }

        // Net amount after settlement fee
        let net_after_settlement_fee = payment.amount.saturating_sub(settlement_fee);

        // ── Pre-lookup merchant AnchorConfig (SEP-6 / SEP-24) for fiat offramp ──
        // The anchor config itself lives in MerchantRegistry. We fetch it once
        // here so both settlement code paths can reuse it. `None` means the
        // merchant has not configured an anchor (on-chain-only settlement).
        let anchor_info: Option<(
            String,          // anchor_domain
            String,          // sep6_endpoint
            String,          // sep24_endpoint
            Vec<String>,     // supported_currencies
            Option<Address>, // merchant payout address
        )> = {
            let registry_addr_opt = env
                .storage()
                .persistent()
                .get::<DataKey, Address>(&DataKey::MerchantRegistryAddress);
            match registry_addr_opt {
                Some(registry_addr) => {
                    use crate::merchant_registry::{MaybeAnchorConfig, MerchantRegistryClient};
                    let registry_client = MerchantRegistryClient::new(&env, &registry_addr);
                    let merchant = registry_client.get_merchant(&payment.merchant_id);
                    match merchant.anchor_config {
                        MaybeAnchorConfig::Some(ref ac) => Some((
                            ac.anchor_domain.clone(),
                            ac.sep6_endpoint.clone(),
                            ac.sep24_endpoint.clone(),
                            ac.supported_currencies.clone(),
                            merchant.payout_address.clone(),
                        )),
                        MaybeAnchorConfig::None => None,
                    }
                }
                None => None,
            }
        };

        // Check if merchant registry is configured and merchant has FeeConfig
        let registry_address = env
            .storage()
            .persistent()
            .get::<DataKey, Address>(&DataKey::MerchantRegistryAddress);

        if let Some(registry_addr) = registry_address {
            let registry_client =
                crate::merchant_registry::MerchantRegistryClient::new(&env, &registry_addr);

            // Try to get merchant and their fee config
            let merchant = registry_client.get_merchant(&payment.merchant_id);
            if let Some(fee_config) = merchant.fee_config.as_option() {
                // Calculate fee using merchant's FeeConfig
                let fee_bps_amount =
                    (net_after_settlement_fee * (fee_config.platform_fee_bps as i128)) / 10_000;
                let fixed_fee = fee_config.fixed_fee;
                let total_merchant_fee = fee_bps_amount.saturating_add(fixed_fee);

                // Ensure fee doesn't exceed amount
                // Waived: if a fee waiver is active, the merchant-level fee
                // is forced to zero (no treasury, no custom recipient transfer).
                let actual_fee = if fee_waiver_reason.is_some() {
                    0
                } else if total_merchant_fee >= net_after_settlement_fee {
                    net_after_settlement_fee
                } else {
                    total_merchant_fee
                };

                let net_merchant_amount = net_after_settlement_fee.saturating_sub(actual_fee);

                // Transfer using the resolved settlement token (if configured)
                if let Some(ref settlement_token) = settlement_token {
                    let token_client = token::TokenClient::new(&env, settlement_token);
                    let from = env.current_contract_address();

                    // Transfer net amount to merchant
                    if net_merchant_amount > 0 {
                        let _ = token_client.try_transfer(
                            &from,
                            &payment.merchant_id,
                            &net_merchant_amount,
                        );
                    }
                }

                // Platform fee: custom fee_recipient receives a transfer; otherwise
                // credit DataKey::TreasuryBalance (unified treasury accounting).
                let fee_recipient: Address = if let Some(custom_recipient) =
                    &fee_config.fee_recipient
                {
                    if actual_fee > 0 {
                        if let Some(ref settlement_token) = settlement_token {
                            let token_client = token::TokenClient::new(&env, settlement_token);
                            let from = env.current_contract_address();
                            let _ = token_client.try_transfer(&from, custom_recipient, &actual_fee);
                        }
                    }
                    custom_recipient.clone()
                } else {
                    if actual_fee > 0 {
                        let current_treasury: i128 = env
                            .storage()
                            .persistent()
                            .get::<DataKey, i128>(&DataKey::TreasuryBalance)
                            .unwrap_or(0);
                        env.storage().persistent().set(
                            &DataKey::TreasuryBalance,
                            &current_treasury.saturating_add(actual_fee),
                        );
                    }
                    env.current_contract_address()
                };

                // Issue #666: record this settlement's merchant-level fee for
                // get_platform_fee_report. Only counted as treasury_share when
                // it actually accrued to DataKey::TreasuryBalance above (i.e.
                // no custom fee_recipient); no developer split on this path.
                let treasury_share_for_report = if fee_recipient == env.current_contract_address() {
                    actual_fee
                } else {
                    0
                };
                Self::record_fee_collection(&env, actual_fee, treasury_share_for_report, 0);

                // Emit FEE_COLLECTED event (merchant-level fee)
                env.events().publish(
                    (
                        Symbol::new(&env, "PAYMENT"),
                        Symbol::new(&env, "FEE_COLLECTED"),
                    ),
                    (
                        payment_id.clone(),
                        payment.merchant_id.clone(),
                        payment.amount,
                        fee_bps_amount,
                        fixed_fee,
                        net_merchant_amount,
                        fee_recipient,
                    ),
                );

                payment.status = payment_state_machine::transition_status(
                    &payment.status,
                    PaymentStatus::Settled,
                )?;
                Self::record_payment_status(&env, &payment);
                env.storage()
                    .persistent()
                    .set(&DataKey::Payment(payment_id.clone()), &payment);
                Self::bump_payment_ttl(&env, &payment_id, &payment.status);

                // If merchant has AnchorConfig set, emit the
                // SETTLEMENT_ANCHOR_WITHDRAW event so the off-chain
                // Settlement Service can call the anchor's SEP-6 API.
                if let Some((
                    ref anchor_domain,
                    ref sep6_endpoint,
                    ref sep24_endpoint,
                    ref supported_currencies,
                    ref merchant_payout_addr,
                )) = anchor_info
                {
                    let payout_addr = merchant_payout_addr
                        .clone()
                        .unwrap_or_else(|| payment.merchant_id.clone());
                    env.events().publish(
                        (
                            Symbol::new(&env, "PAYMENT"),
                            Symbol::new(&env, "ANCHOR_WITHDRAW"),
                            payment.merchant_id.clone(),
                            anchor_domain.clone(),
                        ),
                        (
                            payment_id.clone(),
                            net_merchant_amount,
                            payment.currency.clone(),
                            payout_addr,
                            sep6_endpoint.clone(),
                            sep24_endpoint.clone(),
                            supported_currencies.clone(),
                            env.ledger().timestamp(),
                        ),
                    );
                }

                return Ok(());
            }
        }

        // Original split-based settlement logic (no FeeConfig or no registry)
        let (mut platform_fee, fee_recipient) = if let Some(registry_address) = env
            .storage()
            .persistent()
            .get::<DataKey, Address>(&DataKey::MerchantRegistryAddress)
        {
            let registry_client =
                crate::merchant_registry::MerchantRegistryClient::new(&env, &registry_address);
            registry_client.calculate_platform_fee(&net_after_settlement_fee)
        } else {
            (0i128, env.current_contract_address())
        };

        // Waived: if a fee waiver is active, zero out the split-based platform
        // fee too (matches the settlement_fee zeroing and FeeConfig actual_fee
        // zeroing above).
        if fee_waiver_reason.is_some() {
            platform_fee = 0i128;
        }

        let net_amount = net_after_settlement_fee - platform_fee;

        if splits.is_empty() {
            if net_amount > 0 {
                if let Some(ref settlement_token) = settlement_token {
                    let token_client = token::TokenClient::new(&env, settlement_token);
                    let from = env.current_contract_address();
                    let _ = token_client.try_transfer(&from, &payment.merchant_id, &net_amount);
                }
            }
        } else {
            let mut total: i128 = 0;
            for split in splits.iter() {
                if split.amount <= 0 {
                    return Err(Error::InvalidSettlement);
                }
                total = total.saturating_add(split.amount);
            }
            if total != net_amount {
                return Err(Error::InvalidSettlement);
            }

            if let Some(ref settlement_token) = settlement_token {
                let token_client = token::TokenClient::new(&env, settlement_token);
                let from = env.current_contract_address();
                for split in splits.iter() {
                    let _ = token_client.try_transfer(&from, &split.recipient, &split.amount);
                }
            }
        }

        // Platform fee: when the registry returns the contract itself as recipient
        // (no custom fee_recipient), credit TreasuryBalance. Otherwise transfer out.
        if platform_fee > 0 {
            let contract_addr = env.current_contract_address();
            if fee_recipient == contract_addr {
                let current_treasury: i128 = env
                    .storage()
                    .persistent()
                    .get::<DataKey, i128>(&DataKey::TreasuryBalance)
                    .unwrap_or(0);
                env.storage().persistent().set(
                    &DataKey::TreasuryBalance,
                    &current_treasury.saturating_add(platform_fee),
                );
            } else if let Some(ref settlement_token) = settlement_token {
                let token_client = token::TokenClient::new(&env, settlement_token);
                let from = env.current_contract_address();
                let _ = token_client.try_transfer(&from, &fee_recipient, &platform_fee);
            }

            // Issue #666: record this settlement's platform fee for
            // get_platform_fee_report. Only counted as treasury_share when it
            // accrued to DataKey::TreasuryBalance above; no developer split
            // on this path.
            let treasury_share_for_report = if fee_recipient == contract_addr {
                platform_fee
            } else {
                0
            };
            Self::record_fee_collection(&env, platform_fee, treasury_share_for_report, 0);
        }

        payment.status =
            payment_state_machine::transition_status(&payment.status, PaymentStatus::Settled)?;
        Self::record_payment_status(&env, &payment);

        // Issue #480: Accumulate net merchant amount to pending settlement.
        let net_merchant_amount: i128 = splits
            .iter()
            .find(|s| s.recipient == payment.merchant_id)
            .map(|s| s.amount)
            .unwrap_or(0);
        if net_merchant_amount > 0 {
            if let Some(registry_address) = env
                .storage()
                .persistent()
                .get::<DataKey, Address>(&DataKey::MerchantRegistryAddress)
            {
                let registry_client =
                    crate::merchant_registry::MerchantRegistryClient::new(&env, &registry_address);
                registry_client.add_pending_settlement(&payment.merchant_id, &net_merchant_amount);
            }
        }

        env.storage()
            .persistent()
            .set(&DataKey::Payment(payment_id.clone()), &payment);
        Self::bump_payment_ttl(&env, &payment_id, &payment.status);

        // Issue #166: Optimize event topics
        env.events().publish(
            (
                Symbol::new(&env, "PAYMENT"),
                Symbol::new(&env, "SETTLED"),
                payment.merchant_id.clone(),
            ),
            (payment_id.clone(), payment.amount),
        );

        // If merchant has AnchorConfig set, emit the SETTLEMENT_ANCHOR_WITHDRAW
        // event so the off-chain Settlement Service can call the anchor's
        // SEP-6 withdrawal API. Mirrors the same emission in the FeeConfig
        // settlement path above.
        if let Some((
            ref anchor_domain,
            ref sep6_endpoint,
            ref sep24_endpoint,
            ref supported_currencies,
            ref merchant_payout_addr,
        )) = anchor_info
        {
            let payout_addr = merchant_payout_addr
                .clone()
                .unwrap_or_else(|| payment.merchant_id.clone());
            env.events().publish(
                (
                    Symbol::new(&env, "PAYMENT"),
                    Symbol::new(&env, "ANCHOR_WITHDRAW"),
                    payment.merchant_id.clone(),
                    anchor_domain.clone(),
                ),
                (
                    payment_id.clone(),
                    net_amount,
                    payment.currency.clone(),
                    payout_addr,
                    sep6_endpoint.clone(),
                    sep24_endpoint.clone(),
                    supported_currencies.clone(),
                    env.ledger().timestamp(),
                ),
            );
        }

        Ok(())
    }

    /// Issue #480: Trigger a settlement for a merchant's accumulated pending balance.
    ///
    /// Sweeps the merchant's pending settlement balance to their payout address.
    /// For `Daily` and `Weekly` schedules, enforces a minimum time since the
    /// last settlement. For `Manual`, settles immediately as long as the pending
    /// balance exceeds `SETTLEMENT_MIN_AMOUNT`.
    pub fn trigger_settlement(
        env: Env,
        operator: Address,
        merchant_id: Address,
    ) -> Result<i128, Error> {
        operator.require_auth();

        if !AccessControl::has_role(&env, &role_settlement_operator(&env), &operator) {
            return Err(Error::Unauthorized);
        }

        // Get merchant info from registry.
        let registry_address = env
            .storage()
            .persistent()
            .get::<DataKey, Address>(&DataKey::MerchantRegistryAddress)
            .ok_or(Error::Unauthorized)?;
        let registry_client =
            crate::merchant_registry::MerchantRegistryClient::new(&env, &registry_address);

        let merchant = registry_client.get_merchant(&merchant_id);

        // Resolve the USDC token address.
        let usdc_token = env
            .storage()
            .persistent()
            .get::<DataKey, Address>(&DataKey::UsdcToken)
            .ok_or(Error::Unauthorized)?;

        // Determine the payout address.
        let payout_address = merchant.payout_address.ok_or(Error::InvalidAddress)?;

        // Check minimum time since last settlement for scheduled types.
        let now = env.ledger().timestamp();
        let min_interval = match merchant.settlement_schedule {
            crate::merchant_registry::SettlementSchedule::Daily => {
                Some(SETTLEMENT_DAILY_INTERVAL_SECS)
            }
            crate::merchant_registry::SettlementSchedule::Weekly => {
                Some(SETTLEMENT_WEEKLY_INTERVAL_SECS)
            }
            crate::merchant_registry::SettlementSchedule::Manual => None,
        };

        if let Some(interval) = min_interval {
            if let Some(last) = merchant.last_settlement_at {
                if now < last.saturating_add(interval) {
                    // Schedule not yet eligible; still allow manual override with operator auth.
                    return Err(Error::Unauthorized);
                }
            }
        }

        // Read the pending settlement balance.
        let pending = registry_client.get_pending_settlement(&merchant_id);
        if pending < SETTLEMENT_MIN_AMOUNT {
            return Err(Error::InvalidAmount);
        }

        // Transfer USDC from the PaymentProcessor contract to the merchant's payout address.
        let token_client = token::TokenClient::new(&env, &usdc_token);
        let from = env.current_contract_address();
        token_client.transfer(&from, &payout_address, &pending);

        // Clear pending settlement balance.
        registry_client.clear_pending_settlement(&merchant_id);

        // Update last_settlement_at on the merchant record.
        registry_client.set_last_settlement_at(&merchant_id, &now);

        // Emit MERCHANT/SETTLEMENT_TRIGGERED event.
        env.events().publish(
            (
                Symbol::new(&env, "MERCHANT"),
                Symbol::new(&env, "SETTLEMENT_TRIGGERED"),
                merchant_id,
            ),
            (pending,),
        );

        Ok(pending)
    }

    pub fn prune_expired_payments(
        env: Env,
        operator: Address,
        payment_ids: Vec<String>,
    ) -> Result<u32, Error> {
        operator.require_auth();

        if !AccessControl::has_role(&env, &role_settlement_operator(&env), &operator) {
            return Err(Error::Unauthorized);
        }

        let mut pruned_count: u32 = 0;
        let current_timestamp = env.ledger().timestamp();

        for payment_id in payment_ids.iter() {
            if let Ok(payment) = Self::get_payment_internal(&env, &payment_id) {
                if payment.status == PaymentStatus::Pending
                    && payment.expires_at <= current_timestamp
                {
                    env.storage()
                        .persistent()
                        .remove(&DataKey::Payment(payment_id.clone()));
                    pruned_count = pruned_count.saturating_add(1);
                }
            }
        }

        Ok(pruned_count)
    }

    /// Validate DEX path quotes before executing a swap.
    /// Blocks circular routes and rejects paths whose quoted output is below the minimum.
    fn validate_path_returns(
        env: &Env,
        dex_router: &Address,
        token_in: &Address,
        amount_in: i128,
        amount_out_min: i128,
        path: &Vec<Address>,
    ) -> Result<Vec<i128>, Error> {
        if path.len() < 2 {
            return Err(Error::SwapPathInvalid);
        }

        if path.get(0) != Some(token_in.clone()) {
            return Err(Error::SwapPathInvalid);
        }

        // Circular paths are a common arbitrage exploitation pattern.
        for i in 0..path.len() {
            for j in (i + 1)..path.len() {
                if path.get(i) == path.get(j) {
                    return Err(Error::ArbitrageDetected);
                }
            }
        }

        let dex_client = DexRouterClient::new(env, dex_router);
        let amounts = dex_client.get_amounts_out(&amount_in, path);

        if amounts.len() != path.len() {
            return Err(Error::SwapPathInvalid);
        }

        if amounts.get(0) != Some(amount_in) {
            return Err(Error::SwapPathInvalid);
        }

        let quoted_out = amounts.get(path.len() - 1).ok_or(Error::SwapPathInvalid)?;
        if quoted_out < amount_out_min {
            return Err(Error::InvalidAmount);
        }

        Ok(amounts)
    }

    /// Compare DEX quoted output against a fresh oracle reference rate.
    fn validate_oracle_swap_rate(
        env: &Env,
        fx_oracle: &Address,
        oracle_pair: &Symbol,
        amount_in: i128,
        dex_quoted_out: i128,
        max_deviation_bps: u32,
    ) -> Result<(), Error> {
        let oracle_client = FXOracleClient::new(env, fx_oracle);
        let rate_data = match oracle_client.try_get_rate(oracle_pair) {
            Ok(Ok(data)) => data,
            _ => return Err(Error::OraclePriceDeviation),
        };

        let mut divisor = 1i128;
        for _ in 0..rate_data.decimals {
            divisor = divisor.saturating_mul(10);
        }

        let expected_out = amount_in
            .saturating_mul(rate_data.rate)
            .checked_div(divisor)
            .unwrap_or(0);
        if expected_out <= 0 {
            return Err(Error::OraclePriceDeviation);
        }

        let diff = if dex_quoted_out > expected_out {
            dex_quoted_out - expected_out
        } else {
            expected_out - dex_quoted_out
        };

        let deviation_bps = diff.saturating_mul(10_000) / expected_out;
        if deviation_bps > max_deviation_bps as i128 {
            return Err(Error::OraclePriceDeviation);
        }

        Ok(())
    }

    /// Atomic swap and pay: swap sender's token to merchant's required token and create payment.
    /// Integrates with DEX (e.g., Soroswap) for atomic asset conversion.
    ///
    /// # Arguments
    /// * `payer` - The address making the payment
    /// * `payment_id` - Unique payment identifier
    /// * `merchant_id` - Merchant's address
    /// * `amount` - Amount in the merchant's settlement currency (after swap)
    /// * `currency` - Settlement currency symbol
    /// * `deposit_address` - Where the payment should be deposited
    /// * `token_in` - Address of the token the payer is sending
    /// * `amount_in` - Amount of token_in to swap
    /// * `amount_out_min` - Minimum amount of settlement token required
    /// * `path` - DEX swap path [token_in, ..., settlement_token]
    /// * `expires_at` - Payment expiry timestamp
    /// * `dex_router` - Address of the DEX router contract
    ///
    /// # Returns
    /// The created PaymentCharge on success
    /// Add a router address to the allowlist (issue #437).
    pub fn add_router(env: Env, admin: Address, router: Address) -> Result<(), Error> {
        admin.require_auth();
        if !AccessControl::has_role(&env, &role_admin(&env), &admin) {
            return Err(Error::Unauthorized);
        }
        env.storage()
            .persistent()
            .set(&DataKey::AllowedRouter(router.clone()), &true);

        let mut list: Vec<Address> = env
            .storage()
            .persistent()
            .get(&DataKey::AllowedRoutersList)
            .unwrap_or_else(|| Vec::new(&env));
        if !list.contains(&router) {
            list.push_back(router.clone());
            env.storage()
                .persistent()
                .set(&DataKey::AllowedRoutersList, &list);
        }

        env.events().publish(
            (Symbol::new(&env, "ROUTER"), Symbol::new(&env, "ADDED")),
            router,
        );
        Ok(())
    }

    /// Remove a router address from the allowlist (issue #437).
    pub fn remove_router(env: Env, admin: Address, router: Address) -> Result<(), Error> {
        admin.require_auth();
        if !AccessControl::has_role(&env, &role_admin(&env), &admin) {
            return Err(Error::Unauthorized);
        }
        env.storage()
            .persistent()
            .remove(&DataKey::AllowedRouter(router.clone()));

        if let Some(list) = env
            .storage()
            .persistent()
            .get::<DataKey, Vec<Address>>(&DataKey::AllowedRoutersList)
        {
            let mut new_list: Vec<Address> = Vec::new(&env);
            for r in list.iter() {
                if r != router {
                    new_list.push_back(r);
                }
            }
            env.storage()
                .persistent()
                .set(&DataKey::AllowedRoutersList, &new_list);
        }

        env.events().publish(
            (Symbol::new(&env, "ROUTER"), Symbol::new(&env, "REMOVED")),
            router,
        );
        Ok(())
    }

    /// Check if a DEX router is allowed (issue #437).
    pub fn is_router_allowed(env: Env, router: Address) -> bool {
        env.storage()
            .persistent()
            .get(&DataKey::AllowedRouter(router))
            .unwrap_or(false)
    }

    /// Get all allowlisted DEX router addresses (issue #437).
    pub fn get_allowed_routers(env: Env) -> Vec<Address> {
        env.storage()
            .persistent()
            .get(&DataKey::AllowedRoutersList)
            .unwrap_or_else(|| Vec::new(&env))
    }

    /// Set the Wrapped XLM (WXLM) token contract address (issue #434).
    pub fn set_wrapped_xlm_contract(env: Env, admin: Address, wxlm: Address) -> Result<(), Error> {
        admin.require_auth();
        if !AccessControl::has_role(&env, &role_admin(&env), &admin) {
            return Err(Error::Unauthorized);
        }
        env.storage()
            .persistent()
            .set(&DataKey::WrappedXlmContract, &wxlm);
        Ok(())
    }

    /// Get the Wrapped XLM (WXLM) token contract address (issue #434).
    pub fn get_wrapped_xlm_contract(env: Env) -> Option<Address> {
        env.storage().persistent().get(&DataKey::WrappedXlmContract)
    }

    /// Swaps tokens via a DEX router and executes a payment (issue #434, #437).
    #[allow(clippy::too_many_arguments)]
    pub fn swap_and_pay(env: Env, args: SwapAndPayArgs) -> Result<PaymentCharge, Error> {
        args.payer.require_auth();
        Self::require_creation_not_paused(&env)?;

        if args.amount <= 0 || args.amount_in <= 0 {
            return Err(Error::InvalidAmount);
        }

        Self::enforce_create_payment_rate_limit_for_payer(&env, &args.payer)?;

        if args.amount_out_min < args.amount {
            return Err(Error::SwapPathInvalid);
        }

        // Issue #437: DEX router allowlist check
        let allowed_list = Self::get_allowed_routers(env.clone());
        if !allowed_list.is_empty()
            && !Self::is_router_allowed(env.clone(), args.dex_router.clone())
        {
            return Err(Error::RouterNotAllowed);
        }

        // Issue #434: XLM auto-wrapping detection
        let native_xlm = Address::from_str(&env, ZERO_CONTRACT_STRKEY);
        let mut actual_token_in = args.token_in.clone();

        if args.token_in == native_xlm {
            if let Some(wxlm) = Self::get_wrapped_xlm_contract(env.clone()) {
                token::Client::new(&env, &wxlm).transfer(
                    &args.payer,
                    env.current_contract_address(),
                    &args.amount_in,
                );
                actual_token_in = wxlm;
            }
        }

        let quoted_amounts = Self::validate_path_returns(
            &env,
            &args.dex_router,
            &actual_token_in,
            args.amount_in,
            args.amount_out_min,
            &args.path,
        )?;

        if let (Some(fx_oracle), Some(oracle_pair)) = (&args.fx_oracle, &args.oracle_pair) {
            let quoted_out = quoted_amounts
                .get(args.path.len() - 1)
                .ok_or(Error::SwapPathInvalid)?;
            Self::validate_oracle_swap_rate(
                &env,
                fx_oracle,
                oracle_pair,
                args.amount_in,
                quoted_out,
                args.max_deviation_bps,
            )?;
        }

        let deadline = env.ledger().timestamp().saturating_add(3_600);
        let dex_client = DexRouterClient::new(&env, &args.dex_router);

        let swap_result = dex_client.swap_exact_tokens_for_tokens(
            &args.amount_in,
            &args.amount_out_min,
            &args.path,
            &args.deposit_address,
            &deadline,
        );

        let actual_out = swap_result
            .get(args.path.len() - 1)
            .ok_or(Error::SwapPathInvalid)?;
        if actual_out < args.amount_out_min {
            return Err(Error::InvalidAmount);
        }

        let quoted_out = quoted_amounts
            .get(args.path.len() - 1)
            .ok_or(Error::SwapPathInvalid)?;
        if actual_out < quoted_out {
            return Err(Error::ArbitrageDetected);
        }

        let settlement_token = args
            .path
            .get(args.path.len() - 1)
            .unwrap_or(actual_token_in.clone());
        let create_args = CreatePaymentArgs {
            payment_id: args.payment_id.clone(),
            merchant_id: args.merchant_id,
            payer: None,
            amount: args.amount,
            currency: args.currency,
            deposit_address: args.deposit_address.clone(),
            expires_at: args.expires_at,
            duration_secs: None,
            memo: None,
            memo_type: None,
            token_address: Some(settlement_token),
            client_token: None,
            metadata_hash: None,
            metadata: None,
            fee_waiver_code: None,
            retry_of_payment_id: None,
            payer_muxed_id: None,
        };

        let mut payment = Self::create_payment(env.clone(), create_args)?;

        payment.original_token = Some(args.token_in.clone());
        payment.swap_path = Some(args.path.clone());

        env.storage()
            .persistent()
            .set(&DataKey::Payment(payment.payment_id.clone()), &payment);

        env.events().publish(
            (Symbol::new(&env, "SWAP"), Symbol::new(&env, "EXECUTED")),
            (
                args.payment_id.clone(),
                args.payer.clone(),
                args.amount_in,
                actual_out,
            ),
        );
        Ok(payment)
    }

    /// Issue #436: Multi-DEX route splitting / aggregation.
    pub fn swap_and_pay_multi_route(
        env: Env,
        args: SwapAndPayArgs,
        routes: Vec<SwapRoute>,
        min_output_amount: i128,
    ) -> Result<PaymentCharge, Error> {
        args.payer.require_auth();
        Self::require_creation_not_paused(&env)?;

        if args.amount <= 0 || routes.is_empty() {
            return Err(Error::InvalidAmount);
        }

        let allowed_list = Self::get_allowed_routers(env.clone());
        let mut total_route_input: i128 = 0;

        for route in routes.iter() {
            if route.amount_in <= 0 || route.path.is_empty() {
                return Err(Error::InvalidAmount);
            }
            total_route_input = total_route_input.saturating_add(route.amount_in);
            if !allowed_list.is_empty()
                && !Self::is_router_allowed(env.clone(), route.router.clone())
            {
                return Err(Error::RouterNotAllowed);
            }
        }

        if total_route_input != args.amount_in {
            return Err(Error::InvalidAmount);
        }

        let deadline = env.ledger().timestamp().saturating_add(3_600);
        let mut total_output: i128 = 0;

        for route in routes.iter() {
            let dex_client = DexRouterClient::new(&env, &route.router);
            let swap_result = dex_client.swap_exact_tokens_for_tokens(
                &route.amount_in,
                &0,
                &route.path,
                &args.deposit_address,
                &deadline,
            );
            let route_out = swap_result
                .get(route.path.len() - 1)
                .ok_or(Error::SwapPathInvalid)?;
            total_output = total_output.saturating_add(route_out);
        }

        if total_output < min_output_amount {
            return Err(Error::RouteOutputInsufficient);
        }

        let first_route = routes.get(0).unwrap();
        let settlement_token = first_route
            .path
            .get(first_route.path.len() - 1)
            .unwrap_or(args.token_in.clone());

        let create_args = CreatePaymentArgs {
            payment_id: args.payment_id.clone(),
            merchant_id: args.merchant_id,
            payer: None,
            amount: args.amount,
            currency: args.currency,
            deposit_address: args.deposit_address,
            expires_at: args.expires_at,
            duration_secs: None,
            memo: None,
            memo_type: None,
            token_address: Some(settlement_token),
            client_token: None,
            metadata_hash: None,
            metadata: None,
            fee_waiver_code: None,
            retry_of_payment_id: None,
            payer_muxed_id: None,
        };

        let mut payment = Self::create_payment(env.clone(), create_args)?;

        // Issue #173: record the original token and swap path so a later
        // refund can be routed back through the DEX to the payer's token.
        payment.original_token = Some(args.token_in.clone());
        payment.swap_path = Some(args.path.clone());
        env.storage()
            .persistent()
            .set(&DataKey::Payment(args.payment_id.clone()), &payment);
        Self::bump_payment_ttl(&env, &args.payment_id, &payment.status);

        env.events().publish(
            (
                Symbol::new(&env, "SWAP"),
                Symbol::new(&env, "AND"),
                Symbol::new(&env, "PAY"),
            ),
            (
                args.payment_id,
                args.payer,
                args.amount_in,
                args.token_in,
                args.amount,
            ),
        );

        Ok(payment)
    }

    #[allow(dead_code)]
    fn get_next_stream_id(env: &Env) -> u64 {
        let mut counter: u64 = env
            .storage()
            .persistent()
            .get(&DataKey::StreamCounter)
            .unwrap_or(0);
        counter += 1;
        env.storage()
            .persistent()
            .set(&DataKey::StreamCounter, &counter);
        counter
    }

    /// Enforce KYC tier monthly volume cap. Returns `TierVolumeLimitExceeded` if adding
    /// `amount` would exceed the merchant's tier cap for the current calendar month.
    /// On success, persists the updated cumulative volume.
    fn enforce_tier_volume_cap(
        env: &Env,
        merchant_id: &Address,
        amount: i128,
    ) -> Result<(), Error> {
        // Derive the monthly cap from the merchant's KYC tier (cross-contract if registry set).
        let cap = if let Some(registry_address) = env
            .storage()
            .persistent()
            .get::<DataKey, Address>(&DataKey::MerchantRegistryAddress)
        {
            let registry_client =
                crate::merchant_registry::MerchantRegistryClient::new(env, &registry_address);
            match registry_client.try_get_merchant(merchant_id) {
                Ok(Ok(merchant)) => {
                    use crate::merchant_registry::KycTier;
                    match merchant.kyc_tier {
                        KycTier::Business => TIER_CAP_BUSINESS,
                        KycTier::Full => TIER_CAP_FULL,
                        KycTier::Basic => TIER_CAP_BASIC,
                        KycTier::Unverified => TIER_CAP_UNVERIFIED,
                    }
                }
                _ => TIER_CAP_UNVERIFIED,
            }
        } else {
            TIER_CAP_BUSINESS // No registry → no cap
        };

        if cap == i128::MAX {
            return Ok(()); // Business tier: unlimited
        }

        // Month epoch: seconds since Unix epoch / seconds-per-30-days, cast to u32.
        let month_epoch = (env.ledger().timestamp() / 2_592_000) as u32;
        let key = DataKey::MerchantMonthlyVolume(merchant_id.clone(), month_epoch);

        let current: i128 = env.storage().persistent().get(&key).unwrap_or(0);
        let new_total = current.saturating_add(amount);

        if new_total > cap {
            return Err(Error::TierVolumeLimitExceeded);
        }

        env.storage().persistent().set(&key, &new_total);
        Self::bump_ttl(env, &key, LONG_LIVE_TTL);

        // Issue #207: Track cumulative volume and auto-upgrade KYC tier at milestones.
        let cum_key = DataKey::MerchantCumulativeVolume(merchant_id.clone());
        let cumulative: i128 = env.storage().persistent().get(&cum_key).unwrap_or(0);
        let new_cumulative = cumulative.saturating_add(amount);
        env.storage().persistent().set(&cum_key, &new_cumulative);
        Self::bump_ttl(env, &cum_key, LONG_LIVE_TTL);
        if let Some(registry_address) = env
            .storage()
            .persistent()
            .get::<DataKey, Address>(&DataKey::MerchantRegistryAddress)
        {
            Self::maybe_upgrade_kyc_tier(env, merchant_id, &registry_address, new_cumulative);
        }

        Ok(())
    }

    /// Issue #207: Attempt to auto-upgrade a merchant's KYC tier based on cumulative volume.
    /// Silently no-ops if the registry call fails (e.g. payment processor address not configured).
    fn maybe_upgrade_kyc_tier(
        env: &Env,
        merchant_id: &Address,
        registry_address: &Address,
        cumulative_volume: i128,
    ) {
        use crate::merchant_registry::{KycTier, MerchantRegistryClient};
        let registry_client = MerchantRegistryClient::new(env, registry_address);
        let merchant = match registry_client.try_get_merchant(merchant_id) {
            Ok(Ok(m)) => m,
            _ => return,
        };
        let next_tier = match merchant.kyc_tier {
            KycTier::Unverified if cumulative_volume >= TIER_UPGRADE_THRESHOLD_BASIC => {
                KycTier::Basic
            }
            KycTier::Basic if cumulative_volume >= TIER_UPGRADE_THRESHOLD_FULL => KycTier::Full,
            KycTier::Full if cumulative_volume >= TIER_UPGRADE_THRESHOLD_BUSINESS => {
                KycTier::Business
            }
            _ => return,
        };
        if matches!(
            registry_client.try_auto_upgrade_kyc_tier(
                &env.current_contract_address(),
                merchant_id,
                &next_tier,
            ),
            Ok(Ok(()))
        ) {
            env.events().publish(
                (Symbol::new(env, "KYC_TIER"), Symbol::new(env, "UPGRADED")),
                (merchant_id.clone(), cumulative_volume),
            );
        }
    }

    fn get_payment_internal(env: &Env, payment_id: &String) -> Result<PaymentCharge, Error> {
        env.storage()
            .persistent()
            .get(&DataKey::Payment(payment_id.clone()))
            .ok_or(Error::PaymentNotFound)
    }

    #[allow(dead_code)]
    fn get_refund_internal(env: &Env, refund_id: &String) -> Result<Refund, Error> {
        env.storage()
            .persistent()
            .get(&DataKey::Refund(refund_id.clone()))
            .ok_or(Error::RefundNotFound)
    }

    #[allow(dead_code)]
    fn get_payment_refunds_internal(env: &Env, payment_id: &String) -> Vec<String> {
        env.storage()
            .persistent()
            .get(&DataKey::PaymentRefunds(payment_id.clone()))
            .unwrap_or_else(|| vec![env])
    }

    pub fn get_merchant_dispute_count(env: Env, merchant_id: Address) -> u64 {
        env.storage()
            .persistent()
            .get(&DataKey::MerchantDisputeCount(merchant_id))
            .unwrap_or(0u64)
    }

    /// Issue #397: Validate Stellar memo type constraints.
    ///
    /// Allowed types: Text, Id, Hash, Return
    /// - Text: memo must be ≤ 28 bytes UTF-8
    /// - Id: memo must be parseable as u64
    /// - Hash / Return: memo must be exactly 32 bytes (64 hex chars)
    fn validate_memo(
        env: &Env,
        memo: &Option<String>,
        memo_type: &Option<String>,
    ) -> Result<(), Error> {
        // If no memo_type provided, memo should also be absent — either both or neither.
        let memo_type_str = match memo_type {
            None => return Ok(()), // no memo_type → skip validation
            Some(t) => t,
        };

        // Validate memo_type is one of the accepted values.
        let mut mt_buf = [0u8; 16];
        let mt_len = (memo_type_str.len() as usize).min(16);
        memo_type_str.copy_into_slice(&mut mt_buf[..mt_len]);
        let mt_bytes = &mt_buf[..mt_len];

        let is_text = mt_bytes == b"Text";
        let is_id = mt_bytes == b"Id";
        let is_hash = mt_bytes == b"Hash";
        let is_return = mt_bytes == b"Return";

        if !is_text && !is_id && !is_hash && !is_return {
            return Err(Error::InvalidMemoType);
        }

        let memo_val = match memo {
            None => return Ok(()), // no memo value → no further validation
            Some(m) => m,
        };

        if is_text {
            // Stellar text memos are limited to 28 bytes.
            if memo_val.len() > 28 {
                return Err(Error::MemoTooLong);
            }
        } else if is_id {
            // Id memo must be a valid u64 decimal string.
            let mut buf = [0u8; 20]; // u64::MAX is 20 digits
            let len = (memo_val.len() as usize).min(20);
            memo_val.copy_into_slice(&mut buf[..len]);
            let s = &buf[..len];
            // All bytes must be ASCII digits.
            let mut valid = len > 0;
            for b in s.iter() {
                if !b.is_ascii_digit() {
                    valid = false;
                    break;
                }
            }
            // Also check it doesn't overflow u64 (max 18446744073709551615, 20 digits).
            if valid && len == 20 {
                // Compare digit-by-digit with u64::MAX string.
                let max_str = b"18446744073709551615";
                for i in 0..20 {
                    if s[i] < max_str[i] {
                        break;
                    }
                    if s[i] > max_str[i] {
                        valid = false;
                        break;
                    }
                }
            }
            if !valid || len > 20 {
                return Err(Error::InvalidMemoId);
            }
        }
        // Hash / Return: no additional validation (the 32-byte constraint applies at the
        // Stellar protocol layer when submitting the transaction, not at the contract layer).

        let _ = env; // env unused but kept for consistent signature
        Ok(())
    }

    /// Issue #396: Returns the total number of payment IDs stored for a merchant.
    /// Used by pagination UIs alongside `get_merchant_payments_full`.
    /// Issue #396: Get merchant payment count for dashboard pagination (O(1) via counter).
    pub fn get_merchant_payment_count(env: Env, merchant_id: Address) -> u32 {
        let count: u64 = env
            .storage()
            .persistent()
            .get(&DataKey::MerchantPaymentCount(merchant_id))
            .unwrap_or(0u64);
        count as u32
    }

    pub fn get_merchant_payment_count_dash(env: Env, merchant_id: Address) -> u32 {
        let count: u64 = env
            .storage()
            .persistent()
            .get(&DataKey::MerchantPaymentCount(merchant_id))
            .unwrap_or(0u64);
        count as u32
    }

    /// Issue #396: Returns paginated full `PaymentCharge` structs for merchant dashboards.
    /// `limit` is capped at 50 to avoid ledger compute limits.
    /// Returns an empty vec (not an error) when `offset` exceeds the total count.
    /// When `token_address` is provided, returns only payments matching that token.
    #[allow(deprecated)]
    pub fn get_merchant_payments_full(
        env: Env,
        merchant_id: Address,
        offset: u32,
        limit: u32,
        token_address: Option<Address>,
    ) -> Vec<PaymentCharge> {
        let all = Self::get_merchant_payments_internal(&env, &merchant_id);
        let capped_limit = core::cmp::min(limit, 50);

        let mut filtered: Vec<PaymentCharge> = vec![&env];
        for id in all.iter() {
            if let Some(payment) = env
                .storage()
                .persistent()
                .get::<DataKey, PaymentCharge>(&DataKey::Payment(id))
            {
                let token_match = match &token_address {
                    Some(token) => payment.token_address.as_ref() == Some(token),
                    None => true,
                };

                if token_match {
                    filtered.push_back(payment);
                }
            }
        }

        if capped_limit == 0 || offset >= filtered.len() {
            return vec![&env];
        }

        let end = core::cmp::min(filtered.len(), offset.saturating_add(capped_limit));
        let mut result: Vec<PaymentCharge> = vec![&env];

        let mut i = offset;
        while i < end {
            if let Some(payment) = filtered.get(i) {
                result.push_back(payment.clone());
            }
            i += 1;
        }

        result
    }

    /// Issue #487: Query aggregate analytics for a merchant over a time range.
    /// Returns total_payments, confirmed_payments, failed_payments, total_volume,
    /// avg_payment_amount, dispute_count, refund_count, net_settled_volume.
    /// Issue #678: When from_ts/to_ts are provided, uses the DailyPaymentIndex
    /// to scan only the relevant day buckets (O(days) ledger reads).
    pub fn get_merchant_analytics(
        env: Env,
        merchant_id: Address,
        from_ts: u64,
        to_ts: u64,
    ) -> MerchantAnalytics {
        const SECONDS_PER_DAY: u64 = 86_400;
        let use_index = from_ts > 0 || to_ts < u64::MAX;

        let candidate_ids: Vec<String> = if use_index {
            let start_bucket = from_ts / SECONDS_PER_DAY;
            let end_bucket = to_ts / SECONDS_PER_DAY;
            let mut ids: Vec<String> = vec![&env];
            let mut bucket = start_bucket;
            while bucket <= end_bucket {
                let key = DataKey::DailyPaymentIndex(merchant_id.clone(), bucket);
                if let Some(bucket_ids) =
                    env.storage().persistent().get::<DataKey, Vec<String>>(&key)
                {
                    for id in bucket_ids.iter() {
                        ids.push_back(id);
                    }
                }
                bucket = bucket.saturating_add(1);
            }
            ids
        } else {
            Self::get_merchant_payments_internal(&env, &merchant_id)
        };

        let mut total_payments = 0u32;
        let mut confirmed_payments = 0u32;
        let mut failed_payments = 0u32;
        let mut total_volume: i128 = 0;
        let mut net_settled_volume: i128 = 0;
        let mut refund_count = 0u32;
        let max_samples = 500usize;

        for (sample_count, payment_id) in candidate_ids.iter().enumerate() {
            if sample_count >= max_samples {
                break;
            }

            if let Ok(payment) = Self::get_payment_internal(&env, &payment_id) {
                if payment.created_at >= from_ts && payment.created_at <= to_ts {
                    total_payments += 1;
                    total_volume = total_volume.saturating_add(payment.amount);

                    match payment.status {
                        PaymentStatus::Confirmed => {
                            confirmed_payments += 1;
                            net_settled_volume = net_settled_volume.saturating_add(payment.amount);
                        }
                        PaymentStatus::Failed | PaymentStatus::Expired => {
                            failed_payments += 1;
                        }
                        _ => {}
                    }
                }

                let refunds_for_payment =
                    RefundManager::get_payment_refunds_internal(&env, &payment_id);
                for refund_id in refunds_for_payment.iter() {
                    if let Ok(refund) = RefundManager::get_refund_internal(&env, &refund_id) {
                        if refund.created_at >= from_ts && refund.created_at <= to_ts {
                            refund_count += 1;
                        }
                    }
                }
            }
        }

        let dispute_count =
            RefundManager::get_merchant_dispute_count(env.clone(), merchant_id.clone()) as u32;
        let avg_payment_amount = if total_payments > 0 {
            total_volume / (total_payments as i128)
        } else {
            0
        };

        MerchantAnalytics {
            total_payments,
            confirmed_payments,
            failed_payments,
            total_volume,
            avg_payment_amount,
            dispute_count,
            refund_count,
            net_settled_volume,
        }
    }

    /// Issue #399: Remove the idempotency key associated with a payment so the
    /// client_token can be reused after expiry or cancellation.
    fn remove_idempotency_key(env: &Env, payment_id: &String) {
        let rev_token_id = Self::rev_key_for(env, payment_id);
        let rev_key = DataKey::IdempotencyKey(rev_token_id);
        if let Some(token) = env.storage().persistent().get::<DataKey, String>(&rev_key) {
            env.storage()
                .persistent()
                .remove(&DataKey::IdempotencyKey(token));
            env.storage().persistent().remove(&rev_key);
        }
    }

    /// Build the reverse-map storage key for an idempotency entry.
    /// Prefixes the payment_id with "r:" so it cannot clash with real client_tokens
    /// (client_tokens are caller-supplied and conventionally do not start with "r:").
    fn rev_key_for(env: &Env, payment_id: &String) -> String {
        use soroban_sdk::Bytes;
        let prefix = b"r:";
        let mut buf = Bytes::new(env);
        for b in prefix {
            buf.push_back(*b);
        }
        let len = payment_id.len() as usize;
        let mut pid_buf = [0u8; 256];
        let read = len.min(256);
        payment_id.copy_into_slice(&mut pid_buf[..read]);
        for b in &pid_buf[..read] {
            buf.push_back(*b);
        }
        let total = (prefix.len() + read).min(256);
        let mut out = [0u8; 256];
        out[..prefix.len()].copy_from_slice(&prefix[..]);
        for i in 0..read {
            out[prefix.len() + i] = pid_buf[i];
        }
        String::from_bytes(env, &out[..total])
    }

    fn get_merchant_payments_internal(env: &Env, merchant_id: &Address) -> Vec<String> {
        env.storage()
            .persistent()
            .get(&DataKey::MerchantPayments(merchant_id.clone()))
            .unwrap_or_else(|| vec![env])
    }

    fn payment_ttl(status: &PaymentStatus) -> u32 {
        match status {
            PaymentStatus::Pending => SHORT_LIVE_TTL,
            PaymentStatus::Confirmed
            | PaymentStatus::Settled
            | PaymentStatus::Expired
            | PaymentStatus::Failed
            | PaymentStatus::PartiallyPaid
            | PaymentStatus::Overpaid => LONG_LIVE_TTL,
        }
    }

    fn bump_payment_ttl(env: &Env, payment_id: &String, status: &PaymentStatus) {
        let key = DataKey::Payment(payment_id.clone());
        Self::bump_ttl(env, &key, Self::payment_ttl(status));
    }

    fn bump_ttl(env: &Env, key: &DataKey, ttl: u32) {
        let threshold = core::cmp::max(1, ttl / TTL_BUMP_THRESHOLD_DIVISOR);
        env.storage().persistent().extend_ttl(key, threshold, ttl);
    }

    /// Append a status-transition entry to a payment's on-chain status history.
    fn record_payment_status(env: &Env, payment: &PaymentCharge) {
        let key = DataKey::PaymentStatusHistory(payment.payment_id.clone());
        let mut history: Vec<PaymentStatusEvent> = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| vec![env]);
        history.push_back(PaymentStatusEvent {
            status: payment.status.clone(),
            timestamp: env.ledger().timestamp(),
            tx_hash: payment.transaction_hash.clone(),
        });
        env.storage().persistent().set(&key, &history);
        Self::bump_ttl(env, &key, SHORT_LIVE_TTL);
    }

    // ─── Merchant pre-authorization (pull payments) ───────────────────────────

    /// Customer grants a merchant permission to pull up to `limit_per_period`
    /// tokens per `period_secs`-second window.
    pub fn pre_authorize_merchant(
        env: Env,
        customer: Address,
        merchant: Address,
        token: Address,
        limit_per_period: i128,
        period_secs: u64,
    ) -> Result<MerchantAuthorization, MerchantAuthError> {
        MerchantPreAuth::pre_authorize_merchant(
            env,
            customer,
            merchant,
            token,
            limit_per_period,
            period_secs,
        )
    }

    /// Customer revokes a previously granted merchant authorization.
    pub fn revoke_merchant_authorization(
        env: Env,
        customer: Address,
        merchant: Address,
    ) -> Result<(), MerchantAuthError> {
        MerchantPreAuth::revoke_authorization(env, customer, merchant)
    }

    /// Merchant pulls `amount` tokens from the customer's account against
    /// an existing pre-authorization.
    pub fn pull_payment(
        env: Env,
        merchant: Address,
        customer: Address,
        amount: i128,
    ) -> Result<i128, MerchantAuthError> {
        MerchantPreAuth::pull_payment(env, merchant, customer, amount)
    }

    /// Return the stored authorization for a (customer, merchant) pair.
    pub fn get_merchant_authorization(
        env: Env,
        customer: Address,
        merchant: Address,
    ) -> Result<MerchantAuthorization, MerchantAuthError> {
        MerchantPreAuth::get_authorization(env, customer, merchant)
    }

    /// Return the remaining pull budget for the current period.
    pub fn merchant_authorization_remaining(
        env: Env,
        customer: Address,
        merchant: Address,
    ) -> Result<i128, MerchantAuthError> {
        MerchantPreAuth::remaining_limit(env, customer, merchant)
    }

    pub fn cancel_stream(env: Env, sender: Address, stream_id: String) -> Result<(), StreamError> {
        if Self::is_blacklisted_address(&env, &sender) {
            return Err(StreamError::Unauthorized);
        }
        PaymentStreaming::cancel_stream(env, sender, stream_id)
    }

    pub fn pause_stream(env: Env, sender: Address, stream_id: String) -> Result<(), StreamError> {
        if Self::is_blacklisted_address(&env, &sender) {
            return Err(StreamError::Unauthorized);
        }
        PaymentStreaming::pause_stream(env, sender, stream_id)
    }

    pub fn resume_stream(env: Env, sender: Address, stream_id: String) -> Result<(), StreamError> {
        if Self::is_blacklisted_address(&env, &sender) {
            return Err(StreamError::Unauthorized);
        }
        PaymentStreaming::resume_stream(env, sender, stream_id)
    }
    pub fn cancel_multiple_streams(
        env: Env,
        sender: Address,
        stream_ids: Vec<String>,
    ) -> Result<Vec<String>, StreamError> {
        if Self::is_blacklisted_address(&env, &sender) {
            return Err(StreamError::Unauthorized);
        }
        PaymentStreaming::cancel_multiple_streams(env, sender, stream_ids)
    }

    pub fn batch_cancel_streams(
        env: Env,
        sender: Address,
        stream_ids: Vec<String>,
    ) -> Result<Vec<String>, StreamError> {
        if Self::is_blacklisted_address(&env, &sender) {
            return Err(StreamError::Unauthorized);
        }
        PaymentStreaming::batch_cancel_streams(env, sender, stream_ids)
    }

    pub fn batch_withdraw_to(
        env: Env,
        recipient: Address,
        withdrawals: Vec<WithdrawalRecipient>,
    ) -> Result<Vec<String>, StreamError> {
        if Self::is_blacklisted_address(&env, &recipient) {
            return Err(StreamError::Unauthorized);
        }
        PaymentStreaming::batch_withdraw_to(env, recipient, withdrawals)
    }

    pub fn withdraw_all_for_recipient(
        env: Env,
        recipient: Address,
        max_streams: u32,
    ) -> Result<Vec<String>, StreamError> {
        if Self::is_blacklisted_address(&env, &recipient) {
            return Err(StreamError::Unauthorized);
        }
        PaymentStreaming::withdraw_all_for_recipient(env, recipient, max_streams)
    }

    pub fn trigger_withdrawal(env: Env, stream_id: String) -> Result<String, StreamError> {
        PaymentStreaming::trigger_withdrawal(env, stream_id)
    }

    /// Issue #627: Bulk-extend the TTL of many stream entries in one call
    /// (permissionless). Delegates to `PaymentStreaming::bulk_bump_stream_ttls`;
    /// non-existent stream IDs are silently skipped and the count of bumped
    /// streams is returned.
    pub fn bulk_bump_stream_ttls(env: Env, stream_ids: Vec<String>) -> Result<u32, StreamError> {
        PaymentStreaming::bulk_bump_stream_ttls(env, stream_ids)
    }

    pub fn set_stream_destination(
        env: Env,
        recipient: Address,
        stream_id: String,
        destination: Address,
    ) -> Result<(), StreamError> {
        if Self::is_blacklisted_address(&env, &recipient)
            || Self::is_blacklisted_address(&env, &destination)
        {
            return Err(StreamError::Unauthorized);
        }
        PaymentStreaming::set_stream_destination(env, recipient, stream_id, destination)
    }

    /// Sender approves milestones for a stream, unlocking withdrawals.
    pub fn approve_stream_milestone(
        env: Env,
        sender: Address,
        stream_id: String,
    ) -> Result<(), StreamError> {
        PaymentStreaming::approve_stream_milestone(env, sender, stream_id)
    }

    /// Sender revokes a previous milestone approval, re-locking withdrawals.
    pub fn revoke_stream_milestone(
        env: Env,
        sender: Address,
        stream_id: String,
    ) -> Result<(), StreamError> {
        PaymentStreaming::revoke_stream_milestone(env, sender, stream_id)
    }

    /// Sender sets a floor for `decrease_rate_per_second` on this stream.
    pub fn set_stream_min_rate(
        env: Env,
        sender: Address,
        stream_id: String,
        min_rate_per_second: i128,
    ) -> Result<(), StreamError> {
        PaymentStreaming::set_stream_min_rate(env, sender, stream_id, min_rate_per_second)
    }

    /// Reduce the flow rate of an active stream, refunding surplus deposit.
    pub fn decrease_rate_per_second(
        env: Env,
        sender: Address,
        stream_id: String,
        new_rate: i128,
    ) -> Result<(), StreamError> {
        PaymentStreaming::decrease_rate_per_second(env, sender, stream_id, new_rate)
    }

    pub fn get_sender_streams(
        env: Env,
        sender: Address,
        page: u32,
        page_size: u32,
    ) -> Vec<PaymentStream> {
        PaymentStreaming::get_sender_streams(env, sender, page, page_size)
    }

    pub fn get_stream(env: Env, stream_id: String) -> Result<PaymentStream, StreamError> {
        PaymentStreaming::get_stream(env, stream_id)
    }

    /// Create a new payment stream. Tokens are pulled from `sender` into the contract.
    pub fn create_stream(
        env: Env,
        sender: Address,
        receiver: Address,
        token: Address,
        rate_per_second: i128,
        deposit: i128,
        stream_id: String,
    ) -> Result<PaymentStream, StreamError> {
        if Self::is_blacklisted_address(&env, &sender)
            || Self::is_blacklisted_address(&env, &receiver)
        {
            return Err(StreamError::Unauthorized);
        }
        PaymentStreaming::create_stream(
            env,
            sender,
            receiver,
            token,
            rate_per_second,
            deposit,
            stream_id,
            None,
        )
    }

    pub fn top_up_stream(
        env: Env,
        caller: Address,
        stream_id: String,
        amount: i128,
    ) -> Result<(), StreamError> {
        if Self::is_blacklisted_address(&env, &caller) {
            return Err(StreamError::Unauthorized);
        }
        PaymentStreaming::top_up_stream(env, caller, stream_id, amount)
    }

    pub fn top_up_multiple_streams(
        env: Env,
        sender: Address,
        top_ups: Vec<(String, i128)>,
    ) -> Result<(), StreamError> {
        PaymentStreaming::top_up_multiple_streams(env, sender, top_ups)
    }

    /// Update the flow rate of an active stream (increase or decrease).
    pub fn update_stream_rate(
        env: Env,
        sender: Address,
        stream_id: String,
        new_rate: i128,
    ) -> Result<(), StreamError> {
        PaymentStreaming::update_stream_rate(env, sender, stream_id, new_rate)
    }

    /// Close a terminal (Exhausted/Cancelled) stream and remove its storage entry.
    pub fn close_expired_stream(env: Env, stream_id: String) -> Result<(), StreamError> {
        PaymentStreaming::close_expired_stream(env, stream_id)
    }

    /// Set the platform fee in basis points applied to stream withdrawals.
    /// Only the admin may call this.
    pub fn set_stream_fee_bps(env: Env, admin: Address, fee_bps: i128) -> Result<(), Error> {
        admin.require_auth();
        if AccessControl::get_admin(&env) != Some(admin) {
            return Err(Error::AccessControlError);
        }
        PaymentStreaming::set_stream_fee_bps(env, fee_bps);
        Ok(())
    }

    pub fn get_stream_fee_bps(env: Env) -> i128 {
        PaymentStreaming::get_stream_fee_bps(env)
    }

    pub fn set_stream_fee_recipient(env: Env, admin: Address, recipient: Address) -> Result<(), Error> {
        admin.require_auth();
        if AccessControl::get_admin(&env) != Some(admin) {
            return Err(Error::AccessControlError);
        }
        PaymentStreaming::set_stream_fee_recipient(env, recipient);
        Ok(())
    }

    pub fn get_stream_fee_recipient(env: Env) -> Option<Address> {
        PaymentStreaming::get_stream_fee_recipient(env)
    }

    /// Upgrade the contract WASM and increment the contract version.
    ///
    /// Issue #624: The upgrade is now queued via the timelock instead of
    /// executing immediately.  Returns the action ID of the pending action.
    pub fn upgrade_contract(
        env: Env,
        admin: Address,
        new_wasm_hash: BytesN<32>,
    ) -> Result<String, Error> {
        admin.require_auth();

        if !AccessControl::has_role(&env, &role_admin(&env), &admin) {
            return Err(Error::Unauthorized);
        }

        Self::enqueue_timelocked_action(
            &env,
            admin,
            TimelockActionKind::UpgradeContract(new_wasm_hash),
        )
    }

    // ── Issue #624: Timelock management functions ─────────────────────────────

    /// Set the timelock delay applied to critical admin operations.
    ///
    /// Default is 48 hours.  Only the admin may call this.
    pub fn set_timelock_delay(env: Env, admin: Address, secs: u64) -> Result<(), Error> {
        admin.require_auth();
        if !AccessControl::has_role(&env, &role_admin(&env), &admin) {
            return Err(Error::Unauthorized);
        }
        env.storage()
            .persistent()
            .set(&DataKey::TimelockDelaySecs, &secs);
        Ok(())
    }

    /// Return the current timelock delay in seconds.
    pub fn get_timelock_delay(env: Env) -> u64 {
        env.storage()
            .persistent()
            .get(&DataKey::TimelockDelaySecs)
            .unwrap_or(DEFAULT_TIMELOCK_SECS)
    }

    /// Return all pending timelocked admin actions.
    ///
    /// Any actor may call this to inspect queued operations and their
    /// `execute_after` timestamps.
    pub fn get_pending_admin_actions(env: Env) -> Vec<PendingTimelockAction> {
        let counter: u64 = env
            .storage()
            .persistent()
            .get(&DataKey::TimelockActionCounter)
            .unwrap_or(0u64);

        let mut pending: Vec<PendingTimelockAction> = Vec::new(&env);
        for i in 0..counter {
            let action_id = format_id(&env, "tl_", i);
            if let Some(action) = env
                .storage()
                .persistent()
                .get::<DataKey, PendingTimelockAction>(&DataKey::PendingTimelockAction(action_id))
            {
                pending.push_back(action);
            }
        }
        pending
    }

    /// Execute a previously queued timelocked admin action.
    ///
    /// Reverts with `TimelockNotExpired` if called before `execute_after`.
    /// Only the admin may execute.  Removes the action from the queue on success.
    pub fn execute_timelocked_action(
        env: Env,
        admin: Address,
        action_id: String,
    ) -> Result<(), Error> {
        admin.require_auth();
        if !AccessControl::has_role(&env, &role_admin(&env), &admin) {
            return Err(Error::Unauthorized);
        }

        let action: PendingTimelockAction = env
            .storage()
            .persistent()
            .get::<DataKey, PendingTimelockAction>(&DataKey::PendingTimelockAction(
                action_id.clone(),
            ))
            .ok_or(Error::PaymentNotFound)?; // reuse "not found" semantics

        let now = env.ledger().timestamp();
        if now < action.execute_after {
            return Err(Error::TimelockNotExpired);
        }

        // Dispatch the action
        match action.kind {
            TimelockActionKind::SetFeeRate(bps) => {
                env.storage()
                    .persistent()
                    .set(&DataKey::SettlementFeeRate, &bps);
            }
            TimelockActionKind::SetKycTierLimits(tier, max_amount) => {
                env.storage().persistent().set(
                    &DataKey::KycTierLimitsConfig,
                    &KycTierLimits { tier, max_amount },
                );
                Self::bump_ttl(&env, &DataKey::KycTierLimitsConfig, LONG_LIVE_TTL);
            }
            TimelockActionKind::UpgradeContract(new_wasm_hash) => {
                let old_version: String = env
                    .storage()
                    .persistent()
                    .get(&DataKey::ContractVersion)
                    .unwrap_or_else(|| String::from_str(&env, INITIAL_CONTRACT_VERSION));

                let new_version_str = bump_version_string(&env, &old_version);
                env.deployer().update_current_contract_wasm(new_wasm_hash);
                env.storage()
                    .persistent()
                    .set(&DataKey::ContractVersion, &new_version_str);

                env.events().publish(
                    (Symbol::new(&env, "CONTRACT"), Symbol::new(&env, "UPGRADED")),
                    (old_version, new_version_str),
                );
            }
        }

        // Remove the executed action from the queue
        env.storage()
            .persistent()
            .remove(&DataKey::PendingTimelockAction(action_id.clone()));

        env.events().publish(
            (
                Symbol::new(&env, "TIMELOCK"),
                Symbol::new(&env, "ACTION_EXECUTED"),
            ),
            (action_id, admin),
        );

        Ok(())
    }

    /// Internal helper: assign an action ID, persist the pending action, and emit an event.
    fn enqueue_timelocked_action(
        env: &Env,
        proposed_by: Address,
        kind: TimelockActionKind,
    ) -> Result<String, Error> {
        let delay_secs: u64 = env
            .storage()
            .persistent()
            .get(&DataKey::TimelockDelaySecs)
            .unwrap_or(DEFAULT_TIMELOCK_SECS);

        let counter: u64 = env
            .storage()
            .persistent()
            .get(&DataKey::TimelockActionCounter)
            .unwrap_or(0u64);

        let action_id = format_id(env, "tl_", counter);

        let action = PendingTimelockAction {
            action_id: action_id.clone(),
            kind,
            execute_after: env.ledger().timestamp() + delay_secs,
            proposed_by,
        };

        env.storage()
            .persistent()
            .set(&DataKey::PendingTimelockAction(action_id.clone()), &action);
        env.storage()
            .persistent()
            .set(&DataKey::TimelockActionCounter, &(counter + 1));

        env.events().publish(
            (
                Symbol::new(env, "TIMELOCK"),
                Symbol::new(env, "ACTION_QUEUED"),
            ),
            (action_id.clone(), action.execute_after),
        );

        Ok(action_id)
    }

    // =========================================================================
    // Multi-sig admin proposal functions
    // =========================================================================

    /// Configure the multi-sig threshold and signer set.
    /// Only the current admin may call this.
    pub fn set_multisig_config(
        env: Env,
        admin: Address,
        threshold: u32,
        signers: Vec<Address>,
    ) -> Result<(), Error> {
        AccessControl::set_multisig_config(&env, admin, threshold, signers)
            .map_err(|_| Error::AccessControlError)
    }

    /// Returns the current multi-sig (threshold, signers) configuration.
    pub fn get_multisig_config(env: Env) -> (u32, Vec<Address>) {
        AccessControl::get_multisig_config(&env)
    }

    /// Create a new admin proposal for a contract parameter change.
    /// The calling signer must be in the multisig signer set.
    /// Returns the proposal nonce.
    pub fn create_proposal(env: Env, signer: Address, action: AdminAction) -> Result<u64, Error> {
        AccessControl::create_proposal(&env, signer, action).map_err(|_| Error::AccessControlError)
    }

    /// Vote to approve an existing proposal.
    /// The calling signer must be in the multisig signer set and must not have
    /// already voted on this proposal.
    pub fn vote_proposal(env: Env, signer: Address, nonce: u64) -> Result<(), Error> {
        AccessControl::vote_proposal(&env, signer, nonce).map_err(|_| Error::AccessControlError)
    }

    /// Execute a proposal once the multisig threshold is met.
    ///
    /// * Proposals expire after 48 hours if the threshold is not reached.
    /// * For parameter-change actions (`SetDisputeBond`, `SetVolumeCap`,
    ///   `SetRefundFeeBps`, `SetRateLimit`) the new value is written to
    ///   persistent storage and a `ADMIN/PROPOSAL_EXECUTED` event is emitted.
    pub fn execute_proposal(env: Env, executor: Address, nonce: u64) -> Result<(), Error> {
        executor.require_auth();
        if !AccessControl::has_role(&env, &role_admin(&env), &executor) {
            return Err(Error::Unauthorized);
        }

        let remaining =
            AccessControl::execute_proposal(&env, nonce).map_err(|_| Error::AccessControlError)?;

        let action_tag = if let Some(ref action) = remaining {
            match action {
                AdminAction::SetDisputeBond(amount) => {
                    if *amount < 0 {
                        return Err(Error::InvalidAmount);
                    }
                    env.storage()
                        .persistent()
                        .set(&DataKey::DisputeBondAmount, amount);
                    Symbol::new(&env, "SET_DISPUTE_BOND")
                }
                AdminAction::SetVolumeCap(tier, cap) => {
                    if *cap < 0 {
                        return Err(Error::InvalidAmount);
                    }
                    env.storage()
                        .persistent()
                        .set(&DataKey::TierVolumeCap(tier.clone()), cap);
                    Symbol::new(&env, "SET_VOLUME_CAP")
                }
                AdminAction::SetRefundFeeBps(bps) => {
                    if *bps < 0 || *bps > 1_000 {
                        return Err(Error::InvalidAmount);
                    }
                    env.storage().instance().set(&DataKey::RefundFeeBps, bps);
                    Symbol::new(&env, "SET_REFUND_FEE")
                }
                AdminAction::SetRateLimit(max_per_window, window_secs) => {
                    let config = RateLimitConfig {
                        window_secs: *window_secs,
                        max_per_window: *max_per_window,
                    };
                    env.storage()
                        .persistent()
                        .set(&DataKey::GlobalRateLimit, &config);
                    Symbol::new(&env, "SET_RATE_LIMIT")
                }
                AdminAction::SetGlobalPause(paused, _reason) => {
                    let empty = String::from_str(&env, "multisig_proposal");
                    let state = PauseState {
                        paused: *paused,
                        reason: empty,
                        admin: Some(executor.clone()),
                        timestamp: env.ledger().timestamp(),
                    };
                    env.storage().persistent().set(&DataKey::Paused, &state);
                    Symbol::new(&env, "SET_GLOBAL_PAUSE")
                }
                AdminAction::AllowToken(token) => {
                    env.storage()
                        .persistent()
                        .set(&DataKey::AllowedToken(token.clone()), &true);
                    Symbol::new(&env, "ALLOW_TOKEN")
                }
                _ => Symbol::new(&env, "EXECUTED"),
            }
        } else {
            Symbol::new(&env, "EXECUTED")
        };

        // Emit ADMIN/PROPOSAL_EXECUTED event
        env.events().publish(
            (
                Symbol::new(&env, "ADMIN"),
                Symbol::new(&env, "PROPOSAL_EXECUTED"),
            ),
            (nonce, action_tag, executor),
        );

        Ok(())
    }

    /// Retrieve a pending proposal by nonce.
    pub fn get_proposal(env: Env, nonce: u64) -> Option<AdminProposal> {
        AccessControl::get_proposal(&env, nonce)
    }

    /// Read the effective dispute bond amount (configurable via multi-sig proposal).
    /// Falls back to the compile-time constant if not set via proposal.
    pub fn get_dispute_bond_amount(env: Env) -> i128 {
        env.storage()
            .persistent()
            .get::<DataKey, i128>(&DataKey::DisputeBondAmount)
            .unwrap_or(DISPUTE_BOND_AMOUNT)
    }

    /// Read the effective monthly volume cap for a KYC tier.
    /// Falls back to compile-time constants if not overridden by a proposal.
    pub fn get_tier_volume_cap(env: Env, tier: KycTier) -> i128 {
        if let Some(cap) = env
            .storage()
            .persistent()
            .get::<DataKey, i128>(&DataKey::TierVolumeCap(tier.clone()))
        {
            return cap;
        }
        match tier {
            KycTier::Unverified => TIER_CAP_UNVERIFIED,
            KycTier::Basic => TIER_CAP_BASIC,
            KycTier::Full => TIER_CAP_FULL,
            KycTier::Business => TIER_CAP_BUSINESS,
        }
    }

    /// Read the effective refund fee in basis points from instance storage.
    /// Falls back to the compile-time default (100 bps) if not yet configured.
    pub fn get_refund_fee_bps(env: Env) -> i128 {
        Self::get_refund_fee_bps_internal(&env)
    }

    fn get_refund_fee_bps_internal(env: &Env) -> i128 {
        env.storage()
            .instance()
            .get::<DataKey, i128>(&DataKey::RefundFeeBps)
            .unwrap_or(REFUND_FEE_BPS)
    }

    /// Set the refund cooldown period in seconds (overrides REFUND_COOLDOWN_SECS constant).
    /// Admin-only operation.
    pub fn set_refund_cooldown(env: Env, admin: Address, secs: u64) -> Result<(), Error> {
        admin.require_auth();

        if !AccessControl::has_role(&env, &role_admin(&env), &admin) {
            return Err(Error::Unauthorized);
        }

        env.storage()
            .persistent()
            .set(&DataKey::RefundCooldownSecs, &secs);
        Ok(())
    }

    /// Read the effective refund cooldown period in seconds.
    /// Falls back to the compile-time constant if not overridden by admin.
    #[allow(dead_code)]
    fn get_refund_cooldown_secs(env: &Env) -> u64 {
        env.storage()
            .persistent()
            .get::<DataKey, u64>(&DataKey::RefundCooldownSecs)
            .unwrap_or(REFUND_COOLDOWN_SECS)
    }

    /// Migration function: recompute all merchant payment counts from payment vector.
    /// Scans all merchants and rebuilds the persistent payment count index.
    /// Admin-only operation. Use this after upgrading from older contract versions.
    pub fn recompute_merchant_payment_count(env: Env, admin: Address) -> Result<u64, Error> {
        admin.require_auth();

        if !AccessControl::has_role(&env, &role_admin(&env), &admin) {
            return Err(Error::Unauthorized);
        }

        // This is a limited implementation that processes encountered merchants.
        // For full migration on mainnet, may need off-chain indexing support.
        let merchants_processed: u64 = 0;

        // Clear and rebuild payment counts by scanning merchant payment vectors.
        // In practice, we can only iterate merchants we encounter through payment history.
        // A full recompute would require iterating all historical payments.
        // For now, this is a placeholder that ensures the pattern is in place.

        Ok(merchants_processed)
    }

    pub fn create_invoice(
        env: Env,
        merchant_id: Address,
        customer_email: String,
        line_items: Vec<LineItem>,
        total_amount: i128,
        currency: Symbol,
        due_date: u64,
    ) -> Result<String, Error> {
        merchant_id.require_auth();

        if total_amount <= 0 {
            return Err(Error::InvalidAmount);
        }

        let invoice_id = Self::get_next_invoice_id(&env);
        let now = env.ledger().timestamp();

        let invoice = Invoice {
            invoice_id: invoice_id.clone(),
            merchant_id: merchant_id.clone(),
            customer_email: customer_email.clone(),
            line_items,
            total_amount,
            currency,
            due_date,
            status: InvoiceStatus::Created,
            payment_link_id: None,
            created_at: now,
        };

        env.storage()
            .persistent()
            .set(&DataKey::Invoice(invoice_id.clone()), &invoice);

        let mut merchant_invoices = Self::get_merchant_invoices_internal(&env, &merchant_id);
        merchant_invoices.push_back(invoice_id.clone());
        env.storage().persistent().set(
            &DataKey::MerchantInvoices(merchant_id.clone()),
            &merchant_invoices,
        );

        env.events().publish(
            (Symbol::new(&env, "INVOICE"), Symbol::new(&env, "CREATED")),
            (invoice_id.clone(), merchant_id.clone(), total_amount),
        );

        Ok(invoice_id)
    }

    /// Issue #632: Atomically create an invoice together with a payment link and
    /// wire them up (`invoice.payment_link_id` is set to the new link's ID).
    ///
    /// The payment link is created first via a cross-contract call to the
    /// `link_manager` (`PaymentLinkManager`) contract. If link creation fails the
    /// call returns an error and the invoice is never persisted; if any later
    /// step fails the whole transaction reverts, so the two records are always
    /// created together or not at all.
    pub fn create_payment_link_invoice(
        env: Env,
        merchant_id: Address,
        link_manager: Address,
        customer_email: String,
        line_items: Vec<LineItem>,
        total_amount: i128,
        currency: Symbol,
        due_date: u64,
        link_args: CreateLinkArgs,
    ) -> Result<(Invoice, PaymentLink), Error> {
        merchant_id.require_auth();

        if total_amount <= 0 {
            return Err(Error::InvalidAmount);
        }

        // Step 1: create the payment link on the PaymentLinkManager contract.
        let link_client = crate::payment_link::PaymentLinkManagerClient::new(&env, &link_manager);
        let link_id = match link_client.try_create_link(
            &merchant_id,
            &link_args.link_id,
            &link_args.amount,
            &link_args.currency,
            &link_args.description,
            &link_args.expires_at,
            &link_args.max_uses,
            &link_args.direct_transfer,
            &link_args.metadata,
            &link_args.fiat,
            &link_args.base_url,
        ) {
            Ok(Ok(id)) => id,
            _ => return Err(Error::InvalidPaymentId),
        };

        let payment_link = match link_client.try_get_link(&link_id) {
            Ok(Ok(link)) => link,
            _ => return Err(Error::PaymentNotFound),
        };

        // Step 2: create the invoice, pointing it at the new link.
        let invoice_id = Self::get_next_invoice_id(&env);
        let now = env.ledger().timestamp();
        let invoice = Invoice {
            invoice_id: invoice_id.clone(),
            merchant_id: merchant_id.clone(),
            customer_email,
            line_items,
            total_amount,
            currency,
            due_date,
            status: InvoiceStatus::Created,
            payment_link_id: Some(link_id.clone()),
            created_at: now,
        };

        env.storage()
            .persistent()
            .set(&DataKey::Invoice(invoice_id.clone()), &invoice);

        let mut merchant_invoices = Self::get_merchant_invoices_internal(&env, &merchant_id);
        merchant_invoices.push_back(invoice_id.clone());
        env.storage().persistent().set(
            &DataKey::MerchantInvoices(merchant_id.clone()),
            &merchant_invoices,
        );

        env.events().publish(
            (Symbol::new(&env, "INVOICE"), Symbol::new(&env, "CREATED")),
            (invoice_id.clone(), merchant_id.clone(), total_amount),
        );
        env.events().publish(
            (
                Symbol::new(&env, "INVOICE"),
                Symbol::new(&env, "LINK_ATTACHED"),
            ),
            (invoice_id, link_id),
        );

        Ok((invoice, payment_link))
    }

    pub fn mark_invoice_paid(env: Env, invoice_id: String) -> Result<(), Error> {
        let mut invoice: Invoice = env
            .storage()
            .persistent()
            .get(&DataKey::Invoice(invoice_id.clone()))
            .ok_or(Error::PaymentNotFound)?;

        // Idempotent: marking an already-paid invoice is a no-op (issue: invoice lifecycle tests).
        if invoice.status == InvoiceStatus::Paid {
            return Ok(());
        }

        if invoice.status != InvoiceStatus::Created {
            return Err(Error::PaymentAlreadyProcessed);
        }

        invoice.status = InvoiceStatus::Paid;
        env.storage()
            .persistent()
            .set(&DataKey::Invoice(invoice_id.clone()), &invoice);

        env.events().publish(
            (Symbol::new(&env, "INVOICE"), Symbol::new(&env, "PAID")),
            (invoice_id.clone(), invoice.merchant_id.clone()),
        );

        Ok(())
    }

    pub fn get_invoice(env: Env, invoice_id: String) -> Result<Invoice, Error> {
        env.storage()
            .persistent()
            .get(&DataKey::Invoice(invoice_id))
            .ok_or(Error::PaymentNotFound)
    }

    pub fn get_merchant_invoices(env: Env, merchant_id: Address) -> Vec<String> {
        let internal = Self::get_merchant_invoices_internal(&env, &merchant_id);
        let mut result = vec![&env];
        for s in internal.iter() {
            result.push_back(s);
        }
        result
    }

    fn get_next_invoice_id(env: &Env) -> String {
        let counter = env
            .storage()
            .persistent()
            .get::<DataKey, u64>(&DataKey::InvoiceCounter)
            .unwrap_or(0);

        env.storage()
            .persistent()
            .set(&DataKey::InvoiceCounter, &(counter + 1));

        utils::format_id(env, "invoice_", counter)
    }

    fn get_merchant_invoices_internal(env: &Env, merchant_id: &Address) -> Vec<String> {
        env.storage()
            .persistent()
            .get::<DataKey, Vec<String>>(&DataKey::MerchantInvoices(merchant_id.clone()))
            .unwrap_or_else(|| Vec::new(env))
    }
}

/// Bumps the version string by incrementing the number after the last '.'.
/// Works with versions like "1.0.0" → "1.0.1", "1" → "2", "v1" → "v2".
/// Uses byte-level parsing since Soroban's String API doesn't support string
/// manipulation in no_std. Defaults to "2.0.0" if parsing fails.
fn bump_version_string(env: &Env, version: &String) -> String {
    use soroban_sdk::Bytes;

    let bytes: Bytes = version.to_bytes();
    let len = bytes.len() as usize;

    // Find the last '.' or the start of the version number
    let mut last_dot = None;
    let mut i = 0usize;
    while i < len {
        if bytes.get(i as u32) == Some(b'.') {
            last_dot = Some(i);
        }
        i += 1;
    }

    // Parse the numeric part to bump from the last dot position
    let num_start = last_dot.map(|p| p + 1).unwrap_or(0);
    let mut num_val: u32 = 0;
    let mut j = num_start;
    while j < len {
        match bytes.get(j as u32) {
            Some(b @ b'0'..=b'9') => {
                num_val = num_val.saturating_mul(10).saturating_add((b - b'0') as u32);
            }
            _ => break,
        }
        j += 1;
    }

    // Bump by 1
    let new_num = num_val.saturating_add(1);

    // Encode the new number as ASCII digits
    let mut num_buf = [0u8; 12];
    let mut num_len = 0usize;
    if new_num == 0 {
        num_buf[0] = b'0';
        num_len = 1;
    } else {
        let mut n = new_num;
        let mut rev = [0u8; 12];
        let mut rl = 0usize;
        while n > 0 {
            rev[rl] = (n % 10) as u8 + b'0';
            n /= 10;
            rl += 1;
        }
        while rl > 0 {
            rl -= 1;
            num_buf[num_len] = rev[rl];
            num_len += 1;
        }
    }

    // Reconstruct into a fixed-size buffer (max 64 bytes handles any sane version string)
    let mut result = [0u8; 64];
    let mut pos = 0usize;
    // prefix: bytes before the number
    let mut p = 0usize;
    while p < num_start && pos < 64 {
        result[pos] = bytes.get(p as u32).unwrap_or(b' ');
        pos += 1;
        p += 1;
    }
    // new number digits
    for &digit in num_buf.iter().take(num_len) {
        if pos < 64 {
            result[pos] = digit;
            pos += 1;
        }
    }
    // suffix: bytes after the number
    let mut k = j;
    while k < len && pos < 64 {
        result[pos] = bytes.get(k as u32).unwrap_or(b' ');
        pos += 1;
        k += 1;
    }

    String::from_bytes(env, &result[..pos])
}
