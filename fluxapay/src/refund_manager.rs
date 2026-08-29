//! RefundManager contract implementation.

use crate::access_control::{
    role_admin, role_arbitrator, role_merchant, role_oracle, role_settlement_operator,
    AccessControl,
};
use crate::utils::{self, format_id, validate_ipfs_multihash};
use crate::*;
use soroban_sdk::{
    contract, contractimpl, map, token, vec, Address, BytesN, Env, Map, MuxedAddress, String, Symbol,
    Vec,
};

#[contract]
pub struct RefundManager;

#[cfg_attr(
    any(not(target_arch = "wasm32"), feature = "contract-refund-manager"),
    contractimpl
)]
#[allow(deprecated)] // events::publish — migrate to #[contractevent] in a follow-up
impl RefundManager {
    pub fn version() -> u32 {
        1
    }
    fn require_not_paused(env: &Env) -> Result<(), Error> {
        let pause_state: PauseState =
            env.storage()
                .persistent()
                .get(&DataKey::Paused)
                .unwrap_or(PauseState {
                    paused: false,
                    reason: String::from_str(env, ""),
                    admin: None,
                    timestamp: 0,
                });
        if pause_state.paused {
            return Err(Error::ContractPaused);
        }
        Ok(())
    }

    fn get_refund_fee_bps_internal(env: &Env) -> i128 {
        env.storage()
            .instance()
            .get::<DataKey, i128>(&DataKey::RefundFeeBps)
            .unwrap_or(REFUND_FEE_BPS)
    }

    fn get_refund_cooldown_secs(env: &Env) -> u64 {
        env.storage()
            .persistent()
            .get::<DataKey, u64>(&DataKey::RefundCooldownSecs)
            .unwrap_or(REFUND_COOLDOWN_SECS)
    }

    pub fn get_dispute_bond_amount(env: Env) -> i128 {
        env.storage()
            .persistent()
            .get(&DataKey::DisputeBondAmount)
            .unwrap_or(DISPUTE_BOND_AMOUNT)
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

    fn validate_init_address(env: &Env, address: Address) -> Result<(), Error> {
        let zero_address = Address::from_str(env, ZERO_CONTRACT_STRKEY);
        if address == zero_address {
            return Err(Error::InvalidAddress);
        }
        Ok(())
    }

    fn validate_admin_and_token(
        env: &Env,
        admin: Address,
        token_address: Address,
    ) -> Result<(), Error> {
        if admin == token_address {
            return Err(Error::InvalidAddress);
        }
        Self::validate_init_address(env, admin)?;
        Self::validate_init_address(env, token_address)
    }

    pub fn initialize_refund_manager(
        env: Env,
        admin: Address,
        usdc_token_address: Address,
    ) -> Result<(), Error> {
        Self::validate_admin_and_token(&env, admin.clone(), usdc_token_address.clone())?;
        AccessControl::initialize(&env, admin);
        env.storage()
            .persistent()
            .set(&DataKey::UsdcToken, &usdc_token_address);
        env.storage()
            .instance()
            .set(&DataKey::RefundFeeBps, &REFUND_FEE_BPS);

        // Issue #667: pre-populate on-chain metadata with description, version, and
        // deployment timestamp so explorers/integrators can identify the contract.
        env.storage().instance().set(
            &DataKey::ContractMetadata(Symbol::new(&env, "description")),
            &String::from_str(&env, "FluxaPay RefundManager contract"),
        );
        env.storage().instance().set(
            &DataKey::ContractMetadata(Symbol::new(&env, "version")),
            &String::from_str(&env, "1"),
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

    /// Admin-only: set the refund processing fee in basis points (0–1000, max 10%).
    pub fn set_refund_fee_bps(env: Env, admin: Address, bps: i128) -> Result<(), Error> {
        admin.require_auth();

        if !AccessControl::has_role(&env, &role_admin(&env), &admin) {
            return Err(Error::Unauthorized);
        }

        if !(0..=1_000).contains(&bps) {
            return Err(Error::InvalidAmount);
        }

        env.storage().instance().set(&DataKey::RefundFeeBps, &bps);
        Ok(())
    }

    pub fn get_refund_fee_bps(env: Env) -> i128 {
        Self::get_refund_fee_bps_internal(&env)
    }

    /// Admin: configure the DEX router used to route swap_and_pay refunds
    /// back to the payer's original token (Issue #173).
    pub fn set_dex_router_address(
        env: Env,
        admin: Address,
        dex_router: Address,
    ) -> Result<(), Error> {
        admin.require_auth();
        if !AccessControl::has_role(&env, &role_admin(&env), &admin) {
            return Err(Error::Unauthorized);
        }
        env.storage()
            .persistent()
            .set(&DataKey::DexRouterAddress, &dex_router);
        Ok(())
    }

    /// Admin: require a `receipt_hash` on every refund before `process_refund`
    /// will execute it (Issue #176).
    pub fn set_refund_policy(
        env: Env,
        admin: Address,
        require_receipt_hash: bool,
    ) -> Result<(), Error> {
        admin.require_auth();
        if !AccessControl::has_role(&env, &role_admin(&env), &admin) {
            return Err(Error::Unauthorized);
        }
        env.storage()
            .persistent()
            .set(&DataKey::RequireReceiptHash, &require_receipt_hash);
        Ok(())
    }

    /// Issue #676: read-only view of all refund configuration in one call —
    /// `require_receipt_hash` (Issue #176), `refund_expiry_secs` (Issue #170),
    /// `refund_fee_bps`, and `cooldown_secs`. Lets integrators check policy
    /// before submitting a refund without having to know every individual
    /// storage key / setter.
    pub fn get_refund_policy(env: Env) -> RefundPolicy {
        let require_receipt_hash: bool = env
            .storage()
            .persistent()
            .get(&DataKey::RequireReceiptHash)
            .unwrap_or(false);

        RefundPolicy {
            require_receipt_hash,
            refund_expiry_secs: Self::get_refund_expiry_secs(&env),
            refund_fee_bps: Self::get_refund_fee_bps_internal(&env),
            cooldown_secs: Self::get_refund_cooldown_secs(&env),
        }
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

    /// Synchronize a role grant across PaymentProcessor and RefundManager.
    ///
    /// If either grant fails, the transaction aborts and no changes are persisted.
    pub fn sync_grant_role_with_processor(
        env: Env,
        admin: Address,
        payment_processor_address: Address,
        role: Symbol,
        account: Address,
    ) -> Result<(), Error> {
        admin.require_auth();
        if !AccessControl::has_role(&env, &role_admin(&env), &admin) {
            return Err(Error::Unauthorized);
        }

        AccessControl::grant_role(&env, admin.clone(), role.clone(), account.clone())
            .map_err(|_| Error::AccessControlError)?;

        let payment_client = crate::PaymentProcessorClient::new(&env, &payment_processor_address);
        payment_client
            .try_grant_role(&admin, &role, &account)
            .map_err(|_| Error::AccessControlError)?
            .map_err(|_| Error::AccessControlError)?;

        env.events().publish(
            (
                Symbol::new(&env, "ACCESS_CONTROL"),
                Symbol::new(&env, "SYNC_GRANT"),
            ),
            (role, account),
        );

        Ok(())
    }

    /// Synchronize a role revoke across PaymentProcessor and RefundManager.
    ///
    /// If either revoke fails, the transaction aborts and no changes are persisted.
    pub fn sync_revoke_role_with_processor(
        env: Env,
        admin: Address,
        payment_processor_address: Address,
        role: Symbol,
        account: Address,
    ) -> Result<(), Error> {
        admin.require_auth();
        if !AccessControl::has_role(&env, &role_admin(&env), &admin) {
            return Err(Error::Unauthorized);
        }

        AccessControl::revoke_role(&env, admin.clone(), role.clone(), account.clone())
            .map_err(|_| Error::AccessControlError)?;

        let payment_client = crate::PaymentProcessorClient::new(&env, &payment_processor_address);
        payment_client
            .try_revoke_role(&admin, &role, &account)
            .map_err(|_| Error::AccessControlError)?
            .map_err(|_| Error::AccessControlError)?;

        env.events().publish(
            (
                Symbol::new(&env, "ACCESS_CONTROL"),
                Symbol::new(&env, "SYNC_REVOKE"),
            ),
            (role, account),
        );

        Ok(())
    }

    pub fn has_role(env: Env, role: Symbol, account: Address) -> bool {
        AccessControl::has_role(&env, &role, &account)
    }

    pub fn renounce_role(env: Env, account: Address, role: Symbol) -> Result<(), Error> {
        AccessControl::renounce_role(&env, account, role).map_err(|_| Error::AccessControlError)
    }

    pub fn propose_admin(
        env: Env,
        current_admin: Address,
        new_admin: Address,
    ) -> Result<(), Error> {
        AccessControl::propose_admin(&env, current_admin, new_admin)
            .map_err(|_| Error::AccessControlError)
    }

    pub fn claim_admin(env: Env, new_admin: Address) -> Result<(), Error> {
        AccessControl::claim_admin(&env, new_admin).map_err(|_| Error::AccessControlError)
    }

    pub fn transfer_admin(
        env: Env,
        current_admin: Address,
        new_admin: Address,
    ) -> Result<(), Error> {
        Self::propose_admin(env, current_admin, new_admin)
    }

    pub fn accept_admin_transfer(env: Env, new_admin: Address) -> Result<(), Error> {
        Self::claim_admin(env, new_admin)
    }

    pub fn get_admin(env: Env) -> Option<Address> {
        AccessControl::get_admin(&env)
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
        env.storage()
            .persistent()
            .get::<DataKey, bool>(&DataKey::Blacklisted(address))
            .unwrap_or(false)
    }

    fn require_not_blacklisted(env: &Env, address: &Address) -> Result<(), Error> {
        if env
            .storage()
            .persistent()
            .get::<DataKey, bool>(&DataKey::Blacklisted(address.clone()))
            .unwrap_or(false)
        {
            return Err(Error::Unauthorized);
        }
        Ok(())
    }

    /// Like `register_payment`, but also records the original token and swap
    /// path used by a `swap_and_pay` payment, so `process_refund` can route
    /// the refund back through the DEX to the payer's original token
    /// (Issue #173).
    pub fn register_swap_payment(
        env: Env,
        payment_id: String,
        merchant_id: Address,
        amount: i128,
        currency: Symbol,
        original_token: Address,
        swap_path: Vec<Address>,
    ) -> Result<(), Error> {
        if !env
            .storage()
            .persistent()
            .has(&DataKey::Payment(payment_id.clone()))
        {
            let payment = PaymentCharge {
                payment_id: payment_id.clone(),
                merchant_id,
                amount,
                currency,
                deposit_address: env.current_contract_address(),
                status: PaymentStatus::Confirmed,
                payer_address: None,
                transaction_hash: None,
                created_at: env.ledger().timestamp(),
                confirmed_at: None,
                expires_at: 0,
                amount_received: None,
                memo: None,
                memo_type: None,
                token_address: None,
                metadata_hash: None,
                fx_rate: None,
                fx_rate_at: None,
                original_token: Some(original_token),
                swap_path: Some(swap_path),
                metadata: None,
                fee_waiver_code: None,
                retry_of_payment_id: None,
                payer_muxed_id: None,
                payment_link_id: None,
            };
            env.storage()
                .persistent()
                .set(&DataKey::Payment(payment_id.clone()), &payment);
            Self::bump_payment_ttl(&env, &payment_id, &payment.status);
        }
        Ok(())
    }

    /// Issue #168: Configure fee split destinations for platform fees.
    /// Admin can set allocation ratios and destination addresses.
    /// `treasury_bps + developer_bps` must be ≤ 10 000; any remainder goes to treasury.
    pub fn configure_fee_split(
        env: Env,
        admin: Address,
        treasury_bps: u32,
        developer_bps: u32,
        treasury_address: Address,
        developer_address: Address,
    ) -> Result<(), Error> {
        admin.require_auth();

        if !AccessControl::has_role(&env, &role_admin(&env), &admin) {
            return Err(Error::Unauthorized);
        }

        if treasury_bps.saturating_add(developer_bps) > 10_000 {
            return Err(Error::InvalidAmount);
        }

        let config = FeeSplitConfig {
            treasury_bps,
            developer_bps,
            treasury_address,
            developer_address,
        };

        env.storage()
            .persistent()
            .set(&DataKey::FeeSplitConfig, &config);

        env.events().publish(
            (
                Symbol::new(&env, "FEE_SPLIT"),
                Symbol::new(&env, "CONFIGURED"),
            ),
            (treasury_bps, developer_bps),
        );

        Ok(())
    }

    /// Struct-based setter for the platform fee split config (alias for `configure_fee_split`).
    /// Admin only. `config.treasury_bps + config.developer_bps` must be ≤ 10 000.
    pub fn set_fee_split_config(
        env: Env,
        admin: Address,
        config: FeeSplitConfig,
    ) -> Result<(), Error> {
        admin.require_auth();

        if !AccessControl::has_role(&env, &role_admin(&env), &admin) {
            return Err(Error::Unauthorized);
        }

        if config.treasury_bps.saturating_add(config.developer_bps) > 10_000 {
            return Err(Error::InvalidAmount);
        }

        env.storage()
            .persistent()
            .set(&DataKey::FeeSplitConfig, &config);

        env.events().publish(
            (
                Symbol::new(&env, "FEE_SPLIT"),
                Symbol::new(&env, "CONFIGURED"),
            ),
            (config.treasury_bps, config.developer_bps),
        );

        Ok(())
    }

    /// Get the current fee split configuration.
    pub fn get_fee_split_config(env: Env) -> Option<FeeSplitConfig> {
        env.storage().persistent().get(&DataKey::FeeSplitConfig)
    }

    /// Returns all addresses currently holding the given role (issue #37).
    pub fn get_role_members(env: Env, role: Symbol) -> Vec<Address> {
        AccessControl::get_role_members(&env, &role)
    }

    pub fn propose_fee_update(env: Env, admin: Address, new_fee: i128) -> Result<(), Error> {
        admin.require_auth();
        if Some(admin.clone()) != AccessControl::get_admin(&env) {
            return Err(Error::Unauthorized);
        }
        let proposal = FeeProposal {
            proposed_fee: new_fee,
            proposed_at: env.ledger().timestamp(),
        };
        env.storage()
            .persistent()
            .set(&DataKey::FeeProposal, &proposal);
        Ok(())
    }

    pub fn finalize_fee_update(env: Env, admin: Address) -> Result<(), Error> {
        admin.require_auth();
        if Some(admin.clone()) != AccessControl::get_admin(&env) {
            return Err(Error::Unauthorized);
        }
        let proposal: FeeProposal = env
            .storage()
            .persistent()
            .get(&DataKey::FeeProposal)
            .ok_or(Error::NoFeeProposal)?;

        let now = env.ledger().timestamp();
        let seven_days_secs: u64 = 7 * 24 * 60 * 60;
        if now < proposal.proposed_at + seven_days_secs {
            return Err(Error::FeeProposalNotReady);
        }

        env.storage()
            .persistent()
            .set(&DataKey::CurrentFee, &proposal.proposed_fee);
        env.storage().persistent().remove(&DataKey::FeeProposal);

        Ok(())
    }

    /// Register a payment with the refund manager so refund amounts can be validated.
    pub fn register_payment(
        env: Env,
        payment_id: String,
        merchant_id: Address,
        amount: i128,
        currency: Symbol,
    ) {
        if !env
            .storage()
            .persistent()
            .has(&DataKey::Payment(payment_id.clone()))
        {
            let payment = PaymentCharge {
                payment_id: payment_id.clone(),
                merchant_id: merchant_id.clone(),
                amount,
                currency,
                deposit_address: env.current_contract_address(),
                status: PaymentStatus::Confirmed,
                payer_address: None,
                transaction_hash: None,
                created_at: env.ledger().timestamp(),
                confirmed_at: Some(env.ledger().timestamp()),
                expires_at: 0,
                amount_received: None,
                memo: None,
                memo_type: None,
                token_address: None,
                metadata_hash: None,
                original_token: None,
                swap_path: None,
                fx_rate: None,
                fx_rate_at: None,
                metadata: None,
                fee_waiver_code: None,
                retry_of_payment_id: None,
                payer_muxed_id: None,
                payment_link_id: None,
            };
            env.storage()
                .persistent()
                .set(&DataKey::Payment(payment_id.clone()), &payment);
            Self::bump_payment_ttl(&env, &payment_id, &payment.status);

            // Issue #184: Track confirmed payment count per merchant for dispute rate calculation
            let count_key = DataKey::MerchantPaymentCount(merchant_id.clone());
            let count: u64 = env.storage().persistent().get(&count_key).unwrap_or(0u64);
            env.storage().persistent().set(&count_key, &(count + 1));
            Self::bump_ttl(&env, &count_key, LONG_LIVE_TTL);
        }
    }

    pub fn queue_auto_refund(
        env: Env,
        caller: Address,
        registry_address: Address,
        payment_id: String,
        refund_amount: i128,
        requester: Address,
        reason: String,
    ) -> Result<String, Error> {
        caller.require_auth();

        let registry_client =
            crate::merchant_registry::MerchantRegistryClient::new(&env, &registry_address);
        let expected_caller = registry_client
            .get_payment_processor_address()
            .ok_or(Error::Unauthorized)?;

        if caller != expected_caller {
            return Err(Error::Unauthorized);
        }

        Self::require_not_blacklisted(&env, &requester)?;
        Self::create_refund_internal(
            &env,
            payment_id,
            refund_amount,
            reason,
            requester,
            None,
            None,
        )
    }

    pub fn create_refund(
        env: Env,
        payment_id: String,
        refund_amount: i128,
        reason: String,
        requester: Address,
    ) -> Result<String, Error> {
        requester.require_auth();
        Self::require_not_blacklisted(&env, &requester)?;
        Self::create_refund_internal(
            &env,
            payment_id,
            refund_amount,
            reason,
            requester,
            None,
            None,
        )
    }

    /// Issue #638: Create a refund request with an optional idempotency key.
    ///
    /// When `idempotency_key` is `Some`, the key is persisted for 30 days:
    /// * Retrying with the same key **and** the same `(payment_id, refund_amount,
    ///   reason)` returns the original `refund_id` without creating a duplicate.
    /// * Reusing the key with different parameters returns
    ///   `Error::DuplicateIdempotencyKey`.
    ///
    /// Passing `None` is exactly equivalent to `create_refund` (backward compatible).
    pub fn create_refund_idempotent(
        env: Env,
        payment_id: String,
        refund_amount: i128,
        reason: String,
        requester: Address,
        idempotency_key: Option<String>,
    ) -> Result<String, Error> {
        requester.require_auth();
        Self::require_not_blacklisted(&env, &requester)?;
        Self::create_refund_internal(
            &env,
            payment_id,
            refund_amount,
            reason,
            requester,
            None,
            idempotency_key,
        )
    }

    /// Create a refund request with optional receipt hash metadata.
    ///
    /// Issue #638: also accepts an optional `idempotency_key` with the same
    /// semantics as `create_refund_idempotent`.
    pub fn create_refund_with_receipt(
        env: Env,
        payment_id: String,
        refund_amount: i128,
        reason: String,
        requester: Address,
        receipt_hash: Option<BytesN<32>>,
        idempotency_key: Option<String>,
    ) -> Result<String, Error> {
        requester.require_auth();
        Self::require_not_blacklisted(&env, &requester)?;
        Self::create_refund_internal(
            &env,
            payment_id,
            refund_amount,
            reason,
            requester,
            receipt_hash,
            idempotency_key,
        )
    }

    fn create_refund_internal(
        env: &Env,
        payment_id: String,
        refund_amount: i128,
        reason: String,
        requester: Address,
        receipt_hash: Option<BytesN<32>>,
        idempotency_key: Option<String>,
    ) -> Result<String, Error> {
        if refund_amount <= 0 {
            return Err(Error::InvalidAmount);
        }

        // Issue #625: Enforce maximum length on the reason field.
        if reason.len() as usize > MAX_REASON_LEN {
            return Err(Error::InputTooLong);
        }

        // Issue #638: Idempotency short-circuit. If this key was already used,
        // return the original refund_id for identical params, or reject a reuse
        // with different params.
        if let Some(ref key) = idempotency_key {
            let dk = DataKey::RefundIdempotencyKey(key.clone());
            if let Some(record) = env
                .storage()
                .persistent()
                .get::<DataKey, RefundIdempotencyRecord>(&dk)
            {
                if record.payment_id == payment_id
                    && record.amount == refund_amount
                    && record.reason == reason
                {
                    return Ok(record.refund_id);
                }
                return Err(Error::DuplicateIdempotencyKey);
            }
        }

        // Validate refund amount does not exceed original payment amount
        // First try to get payment from local storage
        let payment: PaymentCharge = if let Some(local_payment) =
            env.storage()
                .persistent()
                .get::<DataKey, PaymentCharge>(&DataKey::Payment(payment_id.clone()))
        {
            local_payment
        } else {
            return Err(Error::PaymentNotFound);
        };
        Self::require_not_blacklisted(env, &payment.merchant_id)?;
        Self::require_not_blacklisted(env, &requester)?;

        // Issue #76: Reject refunds unless payment.status == Confirmed or Overpaid
        if payment.status != PaymentStatus::Confirmed && payment.status != PaymentStatus::Overpaid {
            return Err(Error::PaymentAlreadyProcessed);
        }

        // Issue #174: Check cooldown period after payment confirmation
        let confirmed_at = payment.confirmed_at.ok_or(Error::PaymentAlreadyProcessed)?;
        let now = env.ledger().timestamp();
        let cooldown_secs = Self::get_refund_cooldown_secs(env);
        if now < confirmed_at + cooldown_secs {
            return Err(Error::RefundCooldownNotElapsed);
        }

        // Sum existing refund amounts for this payment
        let existing_refunds = Self::get_payment_refunds_internal(env, &payment_id);
        let mut total_refunded: i128 = 0;
        for id in existing_refunds.iter() {
            if let Ok(r) = Self::get_refund_internal(env, &id) {
                if r.status != RefundStatus::Rejected && r.status != RefundStatus::Cancelled {
                    total_refunded += r.amount;
                }
            }
        }

        if total_refunded + refund_amount > payment.amount {
            return Err(Error::RefundExceedsPayment);
        }

        let counter = Self::get_next_refund_id(env);

        // Build refund ID: "refund_" + counter
        // For simplicity and to avoid complex string manipulation in no_std,
        // we use a match statement for common cases
        let refund_id = format_id(env, "refund_", counter);

        let created_at = env.ledger().timestamp();
        // Issue #170: Set expiry timestamp (30 days from now)
        let _expiry_at = now + REFUND_EXPIRY_SECS;

        let refund = Refund {
            refund_id: refund_id.clone(),
            payment_id: payment_id.clone(),
            amount: refund_amount,
            reason: reason.clone(),
            status: RefundStatus::Pending,
            requester,
            created_at,
            processed_at: None,
            receipt_hash,
            approved: false,
            expiry_at: created_at.saturating_add(Self::get_refund_expiry_secs(env)),
        };

        env.storage()
            .persistent()
            .set(&DataKey::Refund(refund_id.clone()), &refund);

        let mut payment_refunds = Self::get_payment_refunds_internal(env, &payment_id);
        payment_refunds.push_back(refund_id.clone());
        env.storage().persistent().set(
            &DataKey::PaymentRefunds(payment_id.clone()),
            &payment_refunds,
        );
        Self::bump_ttl(
            env,
            &DataKey::PaymentRefunds(payment_id.clone()),
            LONG_LIVE_TTL,
        );

        Self::bump_refund_ttl(env, &refund_id, &refund.status);

        // Issue #638: Persist the idempotency key → refund mapping (30-day TTL) so a
        // retried call with the same key returns this refund_id instead of duplicating.
        if let Some(key) = idempotency_key {
            let dk = DataKey::RefundIdempotencyKey(key);
            env.storage().persistent().set(
                &dk,
                &RefundIdempotencyRecord {
                    refund_id: refund_id.clone(),
                    payment_id: payment_id.clone(),
                    amount: refund_amount,
                    reason,
                },
            );
            Self::bump_ttl(env, &dk, REFUND_IDEMPOTENCY_TTL_LEDGERS);
        }

        // Issue #27: emit REFUND/CREATED event
        env.events().publish(
            (Symbol::new(env, "REFUND"), Symbol::new(env, "CREATED")),
            (payment_id, refund_id.clone(), refund_amount),
        );

        Ok(refund_id)
    }

    pub fn process_refund(env: Env, operator: Address, refund_id: String) -> Result<(), Error> {
        operator.require_auth();
        Self::require_not_paused(&env)?;
        Self::require_not_blacklisted(&env, &operator)?;

        // Issue #171: Allow either operator OR customer (requester) to process approved refunds
        let refund = Self::get_refund_internal(&env, &refund_id)?;
        Self::require_not_blacklisted(&env, &refund.requester)?;

        let has_settlement =
            AccessControl::has_role(&env, &role_settlement_operator(&env), &operator);
        let has_oracle = AccessControl::has_role(&env, &role_oracle(&env), &operator);
        let is_requester = operator == refund.requester;

        // Operator can always process; customer can only process if approved
        if !(has_settlement || has_oracle || is_requester && refund.approved) {
            return Err(Error::Unauthorized);
        }

        Self::process_refund_internal(&env, &operator, refund_id)
    }

    pub fn get_treasury_balance(env: Env) -> i128 {
        env.storage()
            .persistent()
            .get(&DataKey::TreasuryBalance)
            .unwrap_or(0)
    }

    /// Append a withdrawal record, retaining only the newest
    /// `TREASURY_WITHDRAWAL_HISTORY_CAP` entries (newest-first).
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
    /// `offset` skips the first N records; `limit` caps the page size (max 100).
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
            (amount, destination.clone()),
        );

        Ok(())
    }

    fn process_refund_internal(
        env: &Env,
        _operator: &Address,
        refund_id: String,
    ) -> Result<(), Error> {
        if env
            .storage()
            .persistent()
            .get::<DataKey, bool>(&DataKey::ReentrancyLock)
            .unwrap_or(false)
        {
            return Err(Error::Reentrancy);
        }
        // Per-refund lock: reject concurrent/reentrant process_refund for the same ID.
        if env
            .storage()
            .persistent()
            .has(&DataKey::RefundLock(refund_id.clone()))
        {
            return Err(Error::Reentrancy);
        }
        env.storage()
            .persistent()
            .set(&DataKey::ReentrancyLock, &true);
        env.storage()
            .persistent()
            .set(&DataKey::RefundLock(refund_id.clone()), &true);
        let _guard = ReentrancyGuard { env };
        let _refund_lock = RefundLockGuard {
            env,
            refund_id: refund_id.clone(),
        };

        let mut refund = Self::get_refund_internal(env, &refund_id)?;

        if refund.status != RefundStatus::Pending {
            return Err(Error::RefundAlreadyProcessed);
        }

        let require_receipt_hash: bool = env
            .storage()
            .persistent()
            .get(&DataKey::RequireReceiptHash)
            .unwrap_or(false);
        if require_receipt_hash && refund.receipt_hash.is_none() {
            return Err(Error::MissingReceiptHash);
        }
        // Issue #170: Check refund expiration
        let now = env.ledger().timestamp();
        if now > refund.expiry_at {
            return Err(Error::RefundExpired);
        }

        let usdc_token_address: Address = env
            .storage()
            .persistent()
            .get(&DataKey::UsdcToken)
            .ok_or(Error::Unauthorized)?;
        let _token_client = token::TokenClient::new(env, &usdc_token_address);

        // Issue #167: Query merchant's KYC tier and apply tiered refund fee
        let payment: PaymentCharge = env
            .storage()
            .persistent()
            .get::<DataKey, PaymentCharge>(&DataKey::Payment(refund.payment_id.clone()))
            .ok_or(Error::PaymentNotFound)?;

        // Issue #167: Query merchant's KYC tier and apply tiered refund fee
        let default_fee_bps = Self::get_refund_fee_bps_internal(env);

        let fee_bps = if let Some(registry_address) = env
            .storage()
            .persistent()
            .get::<DataKey, Address>(&DataKey::MerchantRegistryAddress)
        {
            let registry_client =
                crate::merchant_registry::MerchantRegistryClient::new(env, &registry_address);
            match registry_client.try_get_merchant(&payment.merchant_id) {
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

        let fee = refund.amount * fee_bps / 10_000;
        let net_amount = refund.amount - fee;

        // Issue #173: Multi-token swap refund router
        let (refund_token, _refund_amount_final) = if let (Some(original_token), Some(swap_path)) =
            (&payment.original_token, &payment.swap_path)
        {
            // Payment was made via swap_and_pay, refund in original token
            if swap_path.len() >= 2 {
                // Reverse the swap path for refund
                let mut reverse_path = Vec::new(env);
                for i in 0..swap_path.len() {
                    reverse_path.push_back(swap_path.get(swap_path.len() - 1 - i).unwrap());
                }

                // Use DEX to swap back to original token
                // For now, we'll use a simple approach - in production this would call the DEX
                // Simplified: just return the original token and net amount
                // Real implementation would execute the reverse swap
                (original_token.clone(), net_amount)
            } else {
                // Fallback to settlement token if path is invalid
                let settlement_token = payment.token_address.clone().unwrap_or_else(|| {
                    env.storage().persistent().get(&DataKey::UsdcToken).unwrap()
                });
                (settlement_token, net_amount)
            }
        } else {
            // Regular payment, refund in settlement token
            let settlement_token = payment
                .token_address
                .clone()
                .unwrap_or_else(|| env.storage().persistent().get(&DataKey::UsdcToken).unwrap());
            (settlement_token, net_amount)
        };

        let token_client = token::TokenClient::new(env, &refund_token);
        let from = env.current_contract_address();
        let to: MuxedAddress = (&refund.requester).into();

        // Effects before interactions: mark Completed before any token transfer.
        refund.status = RefundStatus::Completed;
        refund.processed_at = Some(env.ledger().timestamp());

        // Persist state before interaction (reentrancy protection)
        env.storage()
            .persistent()
            .set(&DataKey::Refund(refund_id.clone()), &refund);
        Self::bump_refund_ttl(env, &refund_id, &refund.status);

        // Issue #173: route the refund back through the DEX to the payer's
        // original token when the payment was made via swap_and_pay.
        let mut routed_via_dex = false;
        if let (Some(original_token), Some(swap_path)) =
            (&payment.original_token, &payment.swap_path)
        {
            if let Some(dex_router) = env
                .storage()
                .persistent()
                .get::<DataKey, Address>(&DataKey::DexRouterAddress)
            {
                let mut reversed_path = Vec::new(env);
                let mut i = swap_path.len();
                while i > 0 {
                    i -= 1;
                    reversed_path.push_back(swap_path.get_unchecked(i));
                }
                if !reversed_path.is_empty() {
                    let dex_client = crate::dex_router::DexRouterClient::new(env, &dex_router);
                    let deadline = env.ledger().timestamp().saturating_add(3_600);
                    match dex_client.try_swap_exact_tokens_for_tokens(
                        &net_amount,
                        &1i128,
                        &reversed_path,
                        &refund.requester,
                        &deadline,
                    ) {
                        Ok(Ok(_amounts)) => {
                            routed_via_dex = true;
                            env.events().publish(
                                (Symbol::new(env, "REFUND"), Symbol::new(env, "SWAP_ROUTED")),
                                (
                                    refund.payment_id.clone(),
                                    refund_id.clone(),
                                    original_token.clone(),
                                ),
                            );
                        }
                        _ => {
                            env.events().publish(
                                (
                                    Symbol::new(env, "REFUND"),
                                    Symbol::new(env, "SWAP_FALLBACK"),
                                ),
                                (refund.payment_id.clone(), refund_id.clone()),
                            );
                        }
                    }
                }
            }
        }

        // Interaction: Transfer net amount to requester (in USDC, unless already
        // routed back to the original token via the DEX above).
        if !routed_via_dex && token_client.try_transfer(&from, &to, &net_amount).is_err() {
            return Ok(());
        }

        if fee > 0 {
            let current_treasury_balance = Self::get_treasury_balance(env.clone());
            env.storage().persistent().set(
                &DataKey::TreasuryBalance,
                &current_treasury_balance.saturating_add(fee),
            );
        }

        env.events().publish(
            (Symbol::new(env, "REFUND"), Symbol::new(env, "COMPLETED")),
            (refund.payment_id.clone(), refund_id.clone(), refund.amount),
        );

        if refund.receipt_hash.is_some() {
            env.events().publish(
                (
                    Symbol::new(env, "REFUND"),
                    Symbol::new(env, "HASH_VERIFIED"),
                ),
                (refund.payment_id, refund_id),
            );
        }

        Ok(())
    }

    /// Reject a pending refund (operator only). Emits REFUND/REJECTED (issue #27).
    pub fn reject_refund(env: Env, operator: Address, refund_id: String) -> Result<(), Error> {
        operator.require_auth();
        let has_settlement =
            AccessControl::has_role(&env, &role_settlement_operator(&env), &operator);
        let has_oracle = AccessControl::has_role(&env, &role_oracle(&env), &operator);

        if !has_settlement && !has_oracle {
            return Err(Error::Unauthorized);
        }

        let mut refund = Self::get_refund_internal(&env, &refund_id)?;

        if refund.status != RefundStatus::Pending {
            return Err(Error::RefundAlreadyProcessed);
        }

        refund.status = RefundStatus::Rejected;
        refund.processed_at = Some(env.ledger().timestamp());

        env.storage()
            .persistent()
            .set(&DataKey::Refund(refund_id.clone()), &refund);
        Self::bump_refund_ttl(&env, &refund_id, &refund.status);

        // Issue #27: emit REFUND/REJECTED event
        env.events().publish(
            (Symbol::new(&env, "REFUND"), Symbol::new(&env, "REJECTED")),
            (refund.payment_id, refund_id, refund.amount),
        );

        Ok(())
    }

    /// Clean up a pending refund that has passed its `expiry_at` deadline
    /// (Issue #170). Marks it `Rejected` so it no longer blocks the
    /// payment's refundable balance. Callable by the same roles as
    /// `process_refund`/`reject_refund`.
    pub fn expire_refund(env: Env, operator: Address, refund_id: String) -> Result<(), Error> {
        operator.require_auth();
        let has_settlement =
            AccessControl::has_role(&env, &role_settlement_operator(&env), &operator);
        let has_oracle = AccessControl::has_role(&env, &role_oracle(&env), &operator);

        if !has_settlement && !has_oracle {
            return Err(Error::Unauthorized);
        }

        let mut refund = Self::get_refund_internal(&env, &refund_id)?;

        if refund.status != RefundStatus::Pending {
            return Err(Error::RefundAlreadyProcessed);
        }

        if env.ledger().timestamp() <= refund.expiry_at {
            return Err(Error::RefundExpired);
        }

        refund.status = RefundStatus::Rejected;
        refund.processed_at = Some(env.ledger().timestamp());

        env.storage()
            .persistent()
            .set(&DataKey::Refund(refund_id.clone()), &refund);
        Self::bump_refund_ttl(&env, &refund_id, &refund.status);

        env.events().publish(
            (Symbol::new(&env, "REFUND"), Symbol::new(&env, "EXPIRED")),
            (refund.payment_id, refund_id, refund.amount),
        );

        Ok(())
    }

    /// Issue #171: Approve a pending refund, allowing customer to claim it.
    /// Operator marks the refund as approved without processing it immediately.
    /// Customer can then call process_refund to claim the approved refund.
    pub fn approve_refund(env: Env, operator: Address, refund_id: String) -> Result<(), Error> {
        operator.require_auth();
        let has_settlement =
            AccessControl::has_role(&env, &role_settlement_operator(&env), &operator);
        let has_oracle = AccessControl::has_role(&env, &role_oracle(&env), &operator);

        if !has_settlement && !has_oracle {
            return Err(Error::Unauthorized);
        }

        let mut refund = Self::get_refund_internal(&env, &refund_id)?;

        if refund.status != RefundStatus::Pending {
            return Err(Error::RefundAlreadyProcessed);
        }

        refund.approved = true;

        env.storage()
            .persistent()
            .set(&DataKey::Refund(refund_id.clone()), &refund);
        Self::bump_refund_ttl(&env, &refund_id, &refund.status);

        env.events().publish(
            (Symbol::new(&env, "REFUND"), Symbol::new(&env, "APPROVED")),
            (refund.payment_id, refund_id, refund.amount),
        );

        Ok(())
    }

    /// Issue #450: Customer self-serves an operator-approved refund.
    ///
    /// Callable only by the original refund requester, and only once an
    /// operator has called `approve_refund`. `process_refund` remains
    /// available for operators/oracles who need to execute a refund
    /// directly without waiting for the customer to claim it.
    pub fn claim_refund(env: Env, requester: Address, refund_id: String) -> Result<(), Error> {
        requester.require_auth();
        Self::require_not_paused(&env)?;
        Self::require_not_blacklisted(&env, &requester)?;

        let refund = Self::get_refund_internal(&env, &refund_id)?;

        if refund.requester != requester {
            return Err(Error::Unauthorized);
        }
        if refund.status != RefundStatus::Pending {
            return Err(Error::RefundAlreadyProcessed);
        }
        if !refund.approved {
            return Err(Error::RefundNotApproved);
        }

        Self::process_refund_internal(&env, &requester, refund_id)
    }
    /// Admin-configurable refund expiry window in seconds (Issue #170).
    /// Applies to refunds created after this call.
    pub fn set_refund_expiry(env: Env, admin: Address, secs: u64) -> Result<(), Error> {
        admin.require_auth();
        if !AccessControl::has_role(&env, &role_admin(&env), &admin) {
            return Err(Error::Unauthorized);
        }
        if secs == 0 {
            return Err(Error::InvalidAmount);
        }

        env.storage()
            .persistent()
            .set(&DataKey::RefundExpirySecs, &secs);

        Ok(())
    }

    fn get_refund_expiry_secs(env: &Env) -> u64 {
        env.storage()
            .persistent()
            .get(&DataKey::RefundExpirySecs)
            .unwrap_or(DEFAULT_REFUND_EXPIRY_SECS)
    }

    /// Cancel a pending refund. Caller must be the refund requester (merchant) or contract admin.
    /// Removes the refund from the payment's pending list and emits REFUND/CANCELLED.
    /// Instantly refund a payment without operator approval.
    ///
    /// Only merchants with KYC tier `Full` or `Business` may call this.
    /// The merchant must be the `merchant_id` on the original payment.
    /// Executes the USDC transfer immediately (no `Pending` state).
    pub fn refund_instantly(
        env: Env,
        merchant_id: Address,
        payment_id: String,
        refund_amount: i128,
        reason: String,
        registry_address: Address,
    ) -> Result<String, Error> {
        merchant_id.require_auth();
        Self::require_not_blacklisted(&env, &merchant_id)?;

        // Verify merchant KYC tier is Full or Business via cross-contract call
        let registry_client =
            crate::merchant_registry::MerchantRegistryClient::new(&env, &registry_address);
        let merchant = registry_client
            .try_get_merchant(&merchant_id)
            .map_err(|_| Error::Unauthorized)?
            .map_err(|_| Error::Unauthorized)?;

        let is_high_trust = merchant.kyc_tier == crate::merchant_registry::KycTier::Full
            || merchant.kyc_tier == crate::merchant_registry::KycTier::Business;
        if !is_high_trust {
            return Err(Error::Unauthorized);
        }

        // Validate payment belongs to this merchant and is Confirmed
        let payment: PaymentCharge = env
            .storage()
            .persistent()
            .get(&DataKey::Payment(payment_id.clone()))
            .ok_or(Error::PaymentNotFound)?;

        // Issue #485: Prevent disputes on direct transfer payments
        if env
            .storage()
            .persistent()
            .has(&DataKey::DirectTransferPayment(payment_id.clone()))
        {
            return Err(Error::DirectTransferNotDisputable);
        }

        if payment.merchant_id != merchant_id {
            return Err(Error::Unauthorized);
        }
        if let Some(ref payer) = payment.payer_address {
            Self::require_not_blacklisted(&env, payer)?;
        }
        if payment.status != PaymentStatus::Confirmed {
            return Err(Error::PaymentAlreadyProcessed);
        }

        // Create the refund record (validates amount, checks totals)
        let refund_id = Self::create_refund_internal(
            &env,
            payment_id,
            refund_amount,
            reason,
            payment.payer_address.clone().ok_or(Error::Unauthorized)?,
            None,
            None,
        )?;

        // Execute transfer immediately — no operator approval needed
        let usdc_token_address: Address = env
            .storage()
            .persistent()
            .get(&DataKey::UsdcToken)
            .ok_or(Error::Unauthorized)?;
        let token_client = token::TokenClient::new(&env, &usdc_token_address);

        let default_fee_bps = Self::get_refund_fee_bps_internal(&env);
        let fee = refund_amount * default_fee_bps / 10_000;
        let net_amount = refund_amount - fee;

        let mut refund = Self::get_refund_internal(&env, &refund_id)?;
        refund.status = RefundStatus::Completed;
        refund.processed_at = Some(env.ledger().timestamp());

        // Effects before interaction (CEI)
        env.storage()
            .persistent()
            .set(&DataKey::Refund(refund_id.clone()), &refund);
        Self::bump_refund_ttl(&env, &refund_id, &refund.status);

        let from = env.current_contract_address();
        let to: MuxedAddress = (&refund.requester).into();
        let _ = token_client.try_transfer(&from, &to, &net_amount);

        if fee > 0 {
            if let Some(admin) = AccessControl::get_admin(&env) {
                let admin_muxed: MuxedAddress = (&admin).into();
                let _ = token_client.try_transfer(&from, &admin_muxed, &fee);
            }
        }

        env.events().publish(
            (Symbol::new(&env, "REFUND"), Symbol::new(&env, "COMPLETED")),
            (refund.payment_id, refund_id.clone(), refund_amount),
        );

        Ok(refund_id)
    }

    pub fn cancel_refund(env: Env, caller: Address, refund_id: String) -> Result<(), Error> {
        caller.require_auth();

        let mut refund = Self::get_refund_internal(&env, &refund_id)?;

        match refund.status {
            RefundStatus::Pending => {}
            RefundStatus::Cancelled => return Err(Error::RefundCancelled),
            _ => return Err(Error::RefundAlreadyProcessed),
        }

        let is_requester = caller == refund.requester;
        let is_admin = AccessControl::has_role(&env, &role_admin(&env), &caller);
        if !is_requester && !is_admin {
            return Err(Error::Unauthorized);
        }

        refund.status = RefundStatus::Cancelled;
        refund.processed_at = Some(env.ledger().timestamp());

        env.storage()
            .persistent()
            .set(&DataKey::Refund(refund_id.clone()), &refund);
        Self::bump_refund_ttl(&env, &refund_id, &refund.status);

        env.events().publish(
            (Symbol::new(&env, "REFUND"), Symbol::new(&env, "CANCELLED")),
            (refund.payment_id, refund_id, refund.amount),
        );

        Ok(())
    }

    pub fn get_refund(env: Env, refund_id: String) -> Result<Refund, Error> {
        Self::get_refund_internal(&env, &refund_id)
    }

    pub fn get_payment_refunds(env: Env, payment_id: String) -> Result<Vec<Refund>, Error> {
        let refund_ids = RefundManager::get_payment_refunds_internal(&env, &payment_id);
        let mut refunds = vec![&env];
        for id in refund_ids.iter() {
            if let Ok(refund) = Self::get_refund_internal(&env, &id) {
                refunds.push_back(refund);
            }
        }
        Ok(refunds)
    }

    fn get_next_refund_id(env: &Env) -> u64 {
        let mut counter: u64 = env
            .storage()
            .persistent()
            .get(&DataKey::RefundCounter)
            .unwrap_or(0);
        counter += 1;
        env.storage()
            .persistent()
            .set(&DataKey::RefundCounter, &counter);
        counter
    }

    fn get_refund_internal(env: &Env, refund_id: &String) -> Result<Refund, Error> {
        env.storage()
            .persistent()
            .get(&DataKey::Refund(refund_id.clone()))
            .ok_or(Error::RefundNotFound)
    }

    fn get_payment_refunds_internal(env: &Env, payment_id: &String) -> Vec<String> {
        env.storage()
            .persistent()
            .get(&DataKey::PaymentRefunds(payment_id.clone()))
            .unwrap_or_else(|| vec![env])
    }

    // Dispute handling functions
    pub fn create_dispute(
        env: Env,
        payment_id: String,
        amount: i128,
        reason: String,
        evidence: String,
        disputer: Address,
        payout_splits: Vec<SettlementSplit>,
    ) -> Result<String, Error> {
        disputer.require_auth();
        Self::create_dispute_inner(
            &env,
            payment_id,
            amount,
            reason,
            evidence,
            disputer,
            payout_splits,
        )
    }

    /// Batch-create disputes for marketplace bulk filing.
    ///
    /// Processes up to `max_batch` items (hard cap 20). Each item is handled
    /// identically to `create_dispute`; failures do not revert successes.
    /// Bonds are deducted only for successful disputes.
    /// Emits `DISPUTE/BATCH_CREATED` with `(success_count, fail_count)`.
    pub fn batch_create_disputes(
        env: Env,
        disputes: Vec<CreateDisputeArgs>,
        max_batch: u32,
    ) -> Result<Vec<DisputeBatchItemResult>, Error> {
        let effective_max = max_batch.min(MAX_DISPUTE_BATCH);
        if max_batch > MAX_DISPUTE_BATCH || disputes.len() > effective_max {
            return Err(Error::BatchTooLarge);
        }

        let mut results: Vec<DisputeBatchItemResult> = vec![&env];
        let mut success_count: u32 = 0;
        let mut fail_count: u32 = 0;
        let mut total_bond_deducted: i128 = 0;

        for args in disputes.iter() {
            args.disputer.require_auth();
            match Self::create_dispute_inner(
                &env,
                args.payment_id.clone(),
                args.amount,
                args.reason.clone(),
                args.evidence.clone(),
                args.disputer.clone(),
                args.payout_splits.clone(),
            ) {
                Ok(dispute_id) => {
                    success_count = success_count.saturating_add(1);
                    // Each successful dispute locks 2x DISPUTE_BOND_AMOUNT (disputer + merchant).
                    total_bond_deducted =
                        total_bond_deducted.saturating_add(DISPUTE_BOND_AMOUNT.saturating_mul(2));
                    results.push_back(DisputeBatchItemResult::Ok(dispute_id));
                }
                Err(e) => {
                    fail_count = fail_count.saturating_add(1);
                    results.push_back(DisputeBatchItemResult::Err(e as u32));
                }
            }
        }

        env.events().publish(
            (
                Symbol::new(&env, "DISPUTE"),
                Symbol::new(&env, "BATCH_CREATED"),
            ),
            (success_count, fail_count, total_bond_deducted),
        );

        Ok(results)
    }

    fn create_dispute_inner(
        env: &Env,
        payment_id: String,
        amount: i128,
        reason: String,
        evidence: String,
        disputer: Address,
        _payout_splits: Vec<SettlementSplit>,
    ) -> Result<String, Error> {
        Self::require_not_paused(env)?;

        // Issue #404: Validate payment_id format
        if !utils::validate_id(&payment_id) {
            return Err(Error::InvalidPaymentId);
        }

        if amount <= 0 {
            return Err(Error::InvalidAmount);
        }

        // Issue #625: Enforce maximum length on the evidence field.
        if evidence.len() as usize > MAX_EVIDENCE_LEN {
            return Err(Error::InputTooLong);
        }

        // IPFS CID validation when require_evidence_cid is enabled (default: true).
        // Empty evidence is always allowed; non-empty must be CIDv0/CIDv1 when flag is on.
        let require_cid = env
            .storage()
            .persistent()
            .get::<DataKey, bool>(&DataKey::RequireEvidenceCid)
            .unwrap_or(true);
        if require_cid && !evidence.is_empty() && !validate_ipfs_multihash(&evidence) {
            return Err(Error::InvalidEvidenceCid);
        }

        // Rate limits: max open disputes per payer + global hourly creation cap.
        Self::enforce_dispute_rate_limits(env, &disputer)?;

        // Issue #77: Load payment and cap dispute amount to confirmed payment amount
        let payment: PaymentCharge = env
            .storage()
            .persistent()
            .get(&DataKey::Payment(payment_id.clone()))
            .ok_or(Error::PaymentNotFound)?;

        // Ensure payment is confirmed
        if payment.status != PaymentStatus::Confirmed {
            return Err(Error::PaymentAlreadyProcessed);
        }

        // Cap dispute amount to payment amount
        if amount > payment.amount {
            return Err(Error::InvalidAmount);
        }

        payment.merchant_id.require_auth();
        Self::require_not_blacklisted(env, &disputer)?;
        Self::require_not_blacklisted(env, &payment.merchant_id)?;

        let usdc_token_address = env
            .storage()
            .persistent()
            .get::<DataKey, Address>(&DataKey::UsdcToken)
            .ok_or(Error::Unauthorized)?;
        let token_client = token::TokenClient::new(env, &usdc_token_address);
        let contract_address = env.current_contract_address();

        let bond_amount = Self::get_dispute_bond_amount(env.clone());

        if token_client
            .try_transfer(&disputer, &contract_address, &bond_amount)
            .is_err()
        {
            return Err(Error::Unauthorized);
        }
        if token_client
            .try_transfer(&payment.merchant_id, &contract_address, &bond_amount)
            .is_err()
        {
            return Err(Error::Unauthorized);
        }

        env.events().publish(
            (
                Symbol::new(env, "DISPUTE"),
                Symbol::new(env, "BOND_COLLECTED"),
            ),
            (disputer.clone(), bond_amount),
        );

        // Sum open disputes + prior refunds for the same payment_id
        let existing_disputes = Self::get_payment_disputes_internal(env, &payment_id);
        let mut total_disputed: i128 = 0;
        for id in existing_disputes.iter() {
            if let Ok(d) = Self::get_dispute_internal(env, &id) {
                if d.status != DisputeStatus::Rejected {
                    total_disputed += d.amount;
                }
            }
        }

        let existing_refunds = Self::get_payment_refunds_internal(env, &payment_id);
        let mut total_refunded: i128 = 0;
        for id in existing_refunds.iter() {
            if let Ok(r) = Self::get_refund_internal(env, &id) {
                if r.status != RefundStatus::Rejected && r.status != RefundStatus::Cancelled {
                    total_refunded += r.amount;
                }
            }
        }

        // Ensure totals stay within payment.amount
        if total_disputed + total_refunded + amount > payment.amount {
            return Err(Error::RefundExceedsPayment);
        }

        let counter = Self::get_next_dispute_id(env);
        let dispute_id = Self::build_dispute_id(env, counter);

        // Issue #177: Compute dynamic deadline based on dispute amount.
        // Small disputes (<= configured threshold, default 100 USDC): 3 days; larger: 7 days.
        let deadline_secs = Self::computed_dispute_deadline_secs(env, amount);

        let dispute = Dispute {
            dispute_id: dispute_id.clone(),
            payment_id: payment_id.clone(),
            merchant_id: payment.merchant_id.clone(),
            refund_id: None,
            amount,
            reason,
            evidence,
            status: DisputeStatus::Open,
            disputer: disputer.clone(),
            created_at: env.ledger().timestamp(),
            resolved_at: None,
            resolution_notes: None,
            review_deadline: None,
            escalated: false,
            payout_splits: Vec::new(env),
            computed_deadline_secs: Some(deadline_secs),
        };

        env.storage()
            .persistent()
            .set(&DataKey::Dispute(dispute_id.clone()), &dispute);

        let mut payment_disputes = Self::get_payment_disputes_internal(env, &payment_id);
        payment_disputes.push_back(dispute_id.clone());
        env.storage().persistent().set(
            &DataKey::PaymentDisputes(payment_id.clone()),
            &payment_disputes,
        );

        // Record rate-limit counters after successful dispute creation.
        Self::record_dispute_creation(env, &disputer);
        Self::bump_ttl(
            env,
            &DataKey::PaymentDisputes(payment_id.clone()),
            LONG_LIVE_TTL,
        );

        Self::bump_dispute_ttl(env, &dispute_id, &dispute.status);

        // Issue #184: Track dispute count per merchant and auto-suspend if dispute rate is too high
        let merchant_id = payment.merchant_id.clone();
        let dispute_count_key = DataKey::MerchantDisputeCount(merchant_id.clone());
        let dispute_count: u64 = env
            .storage()
            .persistent()
            .get(&dispute_count_key)
            .unwrap_or(0u64);
        let new_dispute_count = dispute_count + 1;
        env.storage()
            .persistent()
            .set(&dispute_count_key, &new_dispute_count);
        Self::bump_ttl(env, &dispute_count_key, LONG_LIVE_TTL);

        // Check dispute rate: if >= 10% of payments have disputes, auto-suspend via registry
        let payment_count: u64 = env
            .storage()
            .persistent()
            .get(&DataKey::MerchantPaymentCount(merchant_id.clone()))
            .unwrap_or(0u64);

        // Only evaluate after at least 5 payments to avoid false positives on new merchants
        if payment_count >= 5 {
            // dispute_rate_bps = (dispute_count * 10_000) / payment_count
            let dispute_rate_bps = new_dispute_count
                .saturating_mul(10_000)
                .checked_div(payment_count)
                .unwrap_or(0);

            // Threshold: 1000 bps = 10%
            if dispute_rate_bps >= 1_000 {
                if let Some(registry_address) = env
                    .storage()
                    .persistent()
                    .get::<DataKey, Address>(&DataKey::MerchantRegistryAddress)
                {
                    let registry_client = crate::merchant_registry::MerchantRegistryClient::new(
                        env,
                        &registry_address,
                    );
                    // Auto-suspend for 30 days; ignore errors (registry may not have this merchant)
                    let suspension_reason = String::from_str(
                        env,
                        "Auto-suspended: dispute rate exceeded 10% threshold",
                    );
                    let thirty_days_secs: u64 = 30 * 24 * 60 * 60;
                    let _ = registry_client.try_suspend_merchant_by_system(
                        &merchant_id,
                        &suspension_reason,
                        &thirty_days_secs,
                    );

                    // Emit auto-suspension event for off-chain indexers
                    env.events().publish(
                        (
                            Symbol::new(env, "MERCHANT"),
                            Symbol::new(env, "AUTO_SUSPENDED"),
                        ),
                        (
                            merchant_id,
                            new_dispute_count,
                            payment_count,
                            dispute_rate_bps,
                        ),
                    );
                }
            }
        }

        // Issue #27: emit DISPUTE_CREATED event
        env.events().publish(
            (Symbol::new(env, "DISPUTE"), Symbol::new(env, "CREATED")),
            (dispute_id.clone(), payment_id),
        );

        Ok(dispute_id)
    }

    pub fn review_dispute(env: Env, operator: Address, dispute_id: String) -> Result<(), Error> {
        operator.require_auth();

        let has_settlement =
            AccessControl::has_role(&env, &role_settlement_operator(&env), &operator);
        let has_oracle = AccessControl::has_role(&env, &role_oracle(&env), &operator);

        if !has_settlement && !has_oracle {
            return Err(Error::Unauthorized);
        }

        let mut dispute = Self::get_dispute_internal(&env, &dispute_id)?;

        if dispute.status != DisputeStatus::Open {
            return Err(Error::DisputeAlreadyResolved);
        }

        dispute.status = DisputeStatus::UnderReview;

        env.storage()
            .persistent()
            .set(&DataKey::Dispute(dispute_id.clone()), &dispute);
        Self::bump_dispute_ttl(&env, &dispute_id, &dispute.status);

        // Issue #27: emit DISPUTE_REVIEWED event
        env.events().publish(
            (Symbol::new(&env, "DISPUTE"), Symbol::new(&env, "REVIEWED")),
            (dispute_id, dispute.payment_id),
        );

        Ok(())
    }

    /// Configure dispute creation rate limits (admin only).
    ///
    /// * `per_payer` — max concurrent open/under-review disputes per disputer
    /// * `global_per_hour` — max dispute creations per hour across all disputers
    pub fn set_dispute_rate_limits(
        env: Env,
        admin: Address,
        per_payer: u32,
        global_per_hour: u32,
    ) -> Result<(), Error> {
        admin.require_auth();
        if !AccessControl::has_role(&env, &role_admin(&env), &admin) {
            return Err(Error::Unauthorized);
        }
        let config = DisputeRateLimitConfig {
            per_payer_open: per_payer,
            global_per_hour,
        };
        env.storage()
            .persistent()
            .set(&DataKey::DisputeRateLimits, &config);
        Ok(())
    }

    /// Toggle whether non-empty dispute evidence must be a valid IPFS CID.
    /// Set `false` for testnet/dev to accept arbitrary evidence strings.
    pub fn set_require_evidence_cid(
        env: Env,
        admin: Address,
        require_cid: bool,
    ) -> Result<(), Error> {
        admin.require_auth();
        if !AccessControl::has_role(&env, &role_admin(&env), &admin) {
            return Err(Error::Unauthorized);
        }
        env.storage()
            .persistent()
            .set(&DataKey::RequireEvidenceCid, &require_cid);
        Ok(())
    }

    pub fn set_dispute_threshold(env: Env, admin: Address, amount: i128) -> Result<(), Error> {
        admin.require_auth();
        if !AccessControl::has_role(&env, &role_admin(&env), &admin) {
            return Err(Error::Unauthorized);
        }
        if amount <= 0 {
            return Err(Error::InvalidAmount);
        }
        env.storage()
            .persistent()
            .set(&DataKey::DisputeDeadlineThresholdAmount, &amount);
        env.events().publish(
            (
                Symbol::new(&env, "DISPUTE"),
                Symbol::new(&env, "THRESHOLD_SET"),
            ),
            amount,
        );
        Ok(())
    }

    fn get_dispute_deadline_threshold(env: &Env) -> i128 {
        env.storage()
            .persistent()
            .get(&DataKey::DisputeDeadlineThresholdAmount)
            .unwrap_or(DEFAULT_DISPUTE_DEADLINE_THRESHOLD_AMOUNT)
    }

    fn computed_dispute_deadline_secs(env: &Env, amount: i128) -> u64 {
        if amount <= Self::get_dispute_deadline_threshold(env) {
            SMALL_DISPUTE_DEADLINE_SECS
        } else {
            LARGE_DISPUTE_DEADLINE_SECS
        }
    }

    fn get_dispute_rate_limits(env: &Env) -> DisputeRateLimitConfig {
        env.storage()
            .persistent()
            .get(&DataKey::DisputeRateLimits)
            .unwrap_or(DisputeRateLimitConfig {
                per_payer_open: DEFAULT_DISPUTE_PER_PAYER_OPEN,
                global_per_hour: DEFAULT_DISPUTE_GLOBAL_PER_HOUR,
            })
    }

    fn enforce_dispute_rate_limits(env: &Env, disputer: &Address) -> Result<(), Error> {
        let limits = Self::get_dispute_rate_limits(env);

        let open: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::PayerOpenDisputeCount(disputer.clone()))
            .unwrap_or(0);
        if open >= limits.per_payer_open {
            return Err(Error::DisputeRateLimitExceeded);
        }

        let now = env.ledger().timestamp();
        let mut state: DisputeCreationRateState = env
            .storage()
            .persistent()
            .get(&DataKey::GlobalDisputeCreationRate)
            .unwrap_or(DisputeCreationRateState {
                window_started_at: now,
                count: 0,
            });

        if now.saturating_sub(state.window_started_at) >= DISPUTE_GLOBAL_WINDOW_SECS {
            state.window_started_at = now;
            state.count = 0;
        }

        if state.count >= limits.global_per_hour {
            return Err(Error::DisputeRateLimitExceeded);
        }

        Ok(())
    }

    fn record_dispute_creation(env: &Env, disputer: &Address) {
        let open_key = DataKey::PayerOpenDisputeCount(disputer.clone());
        let open: u32 = env.storage().persistent().get(&open_key).unwrap_or(0);
        env.storage()
            .persistent()
            .set(&open_key, &open.saturating_add(1));
        Self::bump_ttl(env, &open_key, SHORT_LIVE_TTL);

        let now = env.ledger().timestamp();
        let mut state: DisputeCreationRateState = env
            .storage()
            .persistent()
            .get(&DataKey::GlobalDisputeCreationRate)
            .unwrap_or(DisputeCreationRateState {
                window_started_at: now,
                count: 0,
            });

        if now.saturating_sub(state.window_started_at) >= DISPUTE_GLOBAL_WINDOW_SECS {
            state.window_started_at = now;
            state.count = 0;
        }
        state.count = state.count.saturating_add(1);
        env.storage()
            .persistent()
            .set(&DataKey::GlobalDisputeCreationRate, &state);
        Self::bump_ttl(env, &DataKey::GlobalDisputeCreationRate, SHORT_LIVE_TTL);
    }

    fn release_open_dispute_slot(env: &Env, disputer: &Address) {
        let open_key = DataKey::PayerOpenDisputeCount(disputer.clone());
        let open: u32 = env.storage().persistent().get(&open_key).unwrap_or(0);
        if open > 0 {
            env.storage().persistent().set(&open_key, &(open - 1));
        }
    }

    /// Operator-only: set or update the review deadline for an open or under-review dispute.
    /// Emits DISPUTE/DEADLINE_SET. If the current ledger time already exceeds the deadline,
    /// the dispute is also flagged as escalated and DISPUTE/ESCALATED is emitted.
    pub fn set_dispute_deadline(
        env: Env,
        operator: Address,
        dispute_id: String,
        deadline: u64,
    ) -> Result<(), Error> {
        operator.require_auth();

        let has_settlement =
            AccessControl::has_role(&env, &role_settlement_operator(&env), &operator);
        let has_oracle = AccessControl::has_role(&env, &role_oracle(&env), &operator);

        if !has_settlement && !has_oracle {
            return Err(Error::Unauthorized);
        }

        let mut dispute = Self::get_dispute_internal(&env, &dispute_id)?;

        if dispute.status == DisputeStatus::Resolved || dispute.status == DisputeStatus::Rejected {
            return Err(Error::DisputeAlreadyResolved);
        }

        dispute.review_deadline = Some(deadline);

        let now = env.ledger().timestamp();
        if now > deadline && !dispute.escalated {
            dispute.escalated = true;
            env.storage()
                .persistent()
                .set(&DataKey::Dispute(dispute_id.clone()), &dispute);
            Self::bump_dispute_ttl(&env, &dispute_id, &dispute.status);
            env.events().publish(
                (Symbol::new(&env, "DISPUTE"), Symbol::new(&env, "ESCALATED")),
                (
                    dispute.payment_id.clone(),
                    dispute_id.clone(),
                    dispute.amount,
                ),
            );
        } else {
            env.storage()
                .persistent()
                .set(&DataKey::Dispute(dispute_id.clone()), &dispute);
            Self::bump_dispute_ttl(&env, &dispute_id, &dispute.status);
        }

        env.events().publish(
            (
                Symbol::new(&env, "DISPUTE"),
                Symbol::new(&env, "DEADLINE_SET"),
            ),
            (dispute.payment_id, dispute_id, deadline),
        );

        Ok(())
    }

    /// Operator: configure multi-party payout splits for a marketplace dispute.
    /// Splits must sum to exactly `dispute.amount`; validated again at
    /// resolution time in case the dispute amount changes. (Issue #446)
    pub fn set_dispute_payout_splits(
        env: Env,
        operator: Address,
        dispute_id: String,
        splits: Vec<SettlementSplit>,
    ) -> Result<(), Error> {
        operator.require_auth();

        let has_settlement =
            AccessControl::has_role(&env, &role_settlement_operator(&env), &operator);
        let has_oracle = AccessControl::has_role(&env, &role_oracle(&env), &operator);
        if !has_settlement && !has_oracle {
            return Err(Error::Unauthorized);
        }

        let mut dispute = Self::get_dispute_internal(&env, &dispute_id)?;
        if dispute.status == DisputeStatus::Resolved || dispute.status == DisputeStatus::Rejected {
            return Err(Error::DisputeAlreadyResolved);
        }

        let mut total: i128 = 0;
        for split in splits.iter() {
            total = total.saturating_add(split.amount);
        }
        if total != dispute.amount {
            return Err(Error::InvalidSplitSum);
        }

        dispute.payout_splits = splits;
        env.storage()
            .persistent()
            .set(&DataKey::Dispute(dispute_id.clone()), &dispute);
        Self::bump_dispute_ttl(&env, &dispute_id, &dispute.status);

        Ok(())
    }

    fn maybe_escalate_dispute_due_to_deadline(
        env: &Env,
        dispute_id: &String,
        dispute: &mut Dispute,
    ) -> Result<bool, Error> {
        if dispute.status == DisputeStatus::Resolved || dispute.status == DisputeStatus::Rejected {
            return Ok(false);
        }

        let deadline = dispute.review_deadline.or_else(|| {
            dispute
                .computed_deadline_secs
                .map(|secs| dispute.created_at.saturating_add(secs))
        });
        let Some(deadline) = deadline else {
            return Ok(false);
        };

        let now = env.ledger().timestamp();
        if now <= deadline || dispute.escalated {
            return Ok(false);
        }

        dispute.escalated = true;
        env.storage()
            .persistent()
            .set(&DataKey::Dispute(dispute_id.clone()), &*dispute);
        Self::bump_dispute_ttl(env, dispute_id, &dispute.status);
        env.events().publish(
            (Symbol::new(env, "DISPUTE"), Symbol::new(env, "ESCALATED")),
            (
                dispute.payment_id.clone(),
                dispute_id.clone(),
                dispute.amount,
            ),
        );

        Ok(true)
    }

    /// Anyone may call this to trigger escalation after a dispute review deadline elapses.
    pub fn check_dispute_deadline(env: Env, dispute_id: String) -> Result<(), Error> {
        let mut dispute = Self::get_dispute_internal(&env, &dispute_id)?;
        let _ = Self::maybe_escalate_dispute_due_to_deadline(&env, &dispute_id, &mut dispute)?;
        Ok(())
    }

    pub fn escalate_expired_disputes(env: Env, dispute_ids: Vec<String>) -> u32 {
        let mut count = 0;
        let mut i = 0;
        let len = dispute_ids.len();
        let max = if len > 20 { 20 } else { len };

        while i < max {
            if let Some(dispute_id) = dispute_ids.get(i) {
                if let Ok(mut dispute) = Self::get_dispute_internal(&env, &dispute_id) {
                    if let Ok(true) = Self::maybe_escalate_dispute_due_to_deadline(
                        &env,
                        &dispute_id,
                        &mut dispute,
                    ) {
                        count += 1;
                    }
                }
            }
            i += 1;
        }
        count
    }

    pub fn resolve_dispute_with_refund(
        env: Env,
        operator: Address,
        dispute_id: String,
        resolution_notes: String,
        operator_signature: String,
    ) -> Result<String, Error> {
        operator.require_auth();
        Self::require_not_paused(&env)?;

        let has_settlement =
            AccessControl::has_role(&env, &role_settlement_operator(&env), &operator);
        let has_oracle = AccessControl::has_role(&env, &role_oracle(&env), &operator);

        if !has_settlement && !has_oracle {
            return Err(Error::Unauthorized);
        }

        let mut dispute = Self::get_dispute_internal(&env, &dispute_id)?;

        if dispute.status == DisputeStatus::Resolved || dispute.status == DisputeStatus::Rejected {
            return Err(Error::DisputeAlreadyResolved);
        }

        // Issue #446: if payout_splits are configured, distribute funds to
        // each recipient directly instead of issuing a single-recipient
        // refund.
        if !dispute.payout_splits.is_empty() {
            let mut total: i128 = 0;
            for split in dispute.payout_splits.iter() {
                total = total.saturating_add(split.amount);
            }
            if total != dispute.amount {
                return Err(Error::InvalidSplitSum);
            }

            let usdc_token_address: Address = env
                .storage()
                .persistent()
                .get(&DataKey::UsdcToken)
                .ok_or(Error::Unauthorized)?;
            let token_client = token::TokenClient::new(&env, &usdc_token_address);
            let from = env.current_contract_address();

            for split in dispute.payout_splits.iter() {
                token_client.transfer(&from, &split.recipient, &split.amount);
            }

            let now = env.ledger().timestamp();
            dispute.status = DisputeStatus::Resolved;
            dispute.resolved_at = Some(now);
            dispute.resolution_notes = Some(resolution_notes.clone());

            env.storage()
                .persistent()
                .set(&DataKey::Dispute(dispute_id.clone()), &dispute);
            Self::bump_dispute_ttl(&env, &dispute_id, &dispute.status);

            env.events().publish(
                (
                    Symbol::new(&env, "DISPUTE"),
                    Symbol::new(&env, "SPLIT_RESOLVED"),
                ),
                (
                    dispute_id.clone(),
                    dispute.payment_id.clone(),
                    dispute.payout_splits.len(),
                    dispute.amount,
                ),
            );

            return Ok(dispute_id);
        }

        // Create refund for the disputed amount
        let refund_reason = String::from_str(&env, "Refund issued due to dispute resolution");

        let refund_id = Self::create_refund_internal(
            &env,
            dispute.payment_id.clone(),
            dispute.amount,
            refund_reason,
            dispute.disputer.clone(),
            None,
            None,
        )?;

        // Process the refund immediately (CEI: status=Completed before token transfer)
        Self::process_refund_internal(&env, &operator, refund_id.clone())?;

        let now = env.ledger().timestamp();

        // Persist operator note on-chain for full transparency.
        let note = DisputeOperatorNote {
            dispute_id: dispute_id.clone(),
            operator: operator.clone(),
            resolution_notes: resolution_notes.clone(),
            operator_signature: operator_signature.clone(),
            recorded_at: now,
        };
        env.storage()
            .persistent()
            .set(&DataKey::DisputeOperatorNote(dispute_id.clone()), &note);
        Self::bump_ttl(
            &env,
            &DataKey::DisputeOperatorNote(dispute_id.clone()),
            LONG_LIVE_TTL,
        );

        // Emit full note + signature so off-chain indexers have the complete record.
        env.events().publish(
            (
                Symbol::new(&env, "DISPUTE"),
                Symbol::new(&env, "OPERATOR_NOTE"),
            ),
            (
                dispute_id.clone(),
                operator.clone(),
                resolution_notes.clone(),
                operator_signature,
            ),
        );

        // Effects before bond interactions: mark dispute resolved first.
        dispute.status = DisputeStatus::Resolved;
        dispute.refund_id = Some(refund_id.clone());
        dispute.resolved_at = Some(now);
        dispute.resolution_notes = Some(resolution_notes);

        env.storage()
            .persistent()
            .set(&DataKey::Dispute(dispute_id.clone()), &dispute);
        Self::bump_dispute_ttl(&env, &dispute_id, &dispute.status);
        Self::release_open_dispute_slot(&env, &dispute.disputer);

        // Interactions: return bonds after effects are persisted.
        let usdc_token_address = env
            .storage()
            .persistent()
            .get::<DataKey, Address>(&DataKey::UsdcToken)
            .ok_or(Error::Unauthorized)?;
        let token_client = token::TokenClient::new(&env, &usdc_token_address);
        let contract_address = env.current_contract_address();
        let collector = AccessControl::get_admin(&env).unwrap_or_else(|| contract_address.clone());
        let bond_amount = Self::get_dispute_bond_amount(env.clone());

        if token_client
            .try_transfer(&contract_address, &dispute.disputer, &bond_amount)
            .is_err()
        {
            return Err(Error::Unauthorized);
        }
        if token_client
            .try_transfer(&contract_address, &collector, &bond_amount)
            .is_err()
        {
            return Err(Error::Unauthorized);
        }

        // Issue #677: bond is released back to the disputer (winner) after
        // the dispute is resolved with a refund in their favor.
        events::emit_dispute_bond_returned(
            &env,
            dispute_id.clone(),
            dispute.disputer.clone(),
            bond_amount,
        );

        // Emit DISPUTE_RESOLVED event
        env.events().publish(
            (Symbol::new(&env, "DISPUTE"), Symbol::new(&env, "RESOLVED")),
            (dispute_id, dispute.payment_id),
        );

        Ok(refund_id)
    }

    pub fn reject_dispute(
        env: Env,
        operator: Address,
        dispute_id: String,
        resolution_notes: String,
        operator_signature: String,
    ) -> Result<(), Error> {
        operator.require_auth();

        // Issue #625: Enforce maximum length on the resolution_notes field.
        if resolution_notes.len() as usize > MAX_NOTES_LEN {
            return Err(Error::InputTooLong);
        }

        let has_settlement =
            AccessControl::has_role(&env, &role_settlement_operator(&env), &operator);
        let has_oracle = AccessControl::has_role(&env, &role_oracle(&env), &operator);

        if !has_settlement && !has_oracle {
            return Err(Error::Unauthorized);
        }

        let mut dispute = Self::get_dispute_internal(&env, &dispute_id)?;

        if dispute.status == DisputeStatus::Resolved || dispute.status == DisputeStatus::Rejected {
            return Err(Error::DisputeAlreadyResolved);
        }

        dispute.status = DisputeStatus::Rejected;
        dispute.resolved_at = Some(env.ledger().timestamp());
        dispute.resolution_notes = Some(resolution_notes.clone());

        env.storage()
            .persistent()
            .set(&DataKey::Dispute(dispute_id.clone()), &dispute);
        Self::bump_dispute_ttl(&env, &dispute_id, &dispute.status);

        // Store resolution note for record-keeping
        let note = DisputeOperatorNote {
            dispute_id: dispute_id.clone(),
            operator: operator.clone(),
            resolution_notes: resolution_notes.clone(),
            operator_signature,
            recorded_at: env.ledger().timestamp(),
        };
        env.storage()
            .persistent()
            .set(&DataKey::DisputeOperatorNote(dispute_id.clone()), &note);

        Self::release_open_dispute_slot(&env, &dispute.disputer);

        // Issue #626: Decrement the merchant's active dispute count when a dispute is
        // rejected, so the suspension threshold only tracks non-rejected disputes.
        let merchant_dispute_key = DataKey::MerchantDisputeCount(dispute.merchant_id.clone());
        let current_count: u64 = env
            .storage()
            .persistent()
            .get(&merchant_dispute_key)
            .unwrap_or(0u64);
        if current_count > 0 {
            env.storage()
                .persistent()
                .set(&merchant_dispute_key, &(current_count - 1));
        }

        let usdc_token_address = env
            .storage()
            .persistent()
            .get::<DataKey, Address>(&DataKey::UsdcToken)
            .ok_or(Error::Unauthorized)?;
        let token_client = token::TokenClient::new(&env, &usdc_token_address);
        let contract_address = env.current_contract_address();
        let collector = AccessControl::get_admin(&env).unwrap_or_else(|| contract_address.clone());

        let bond_amount = Self::get_dispute_bond_amount(env.clone());

        if token_client
            .try_transfer(&contract_address, &dispute.merchant_id, &bond_amount)
            .is_err()
        {
            return Err(Error::Unauthorized);
        }
        if token_client
            .try_transfer(&contract_address, &collector, &bond_amount)
            .is_err()
        {
            return Err(Error::Unauthorized);
        }

        // Issue #677: merchant's counter-bond is released back to them since
        // the dispute was rejected in their favor.
        events::emit_dispute_bond_returned(
            &env,
            dispute_id.clone(),
            dispute.merchant_id.clone(),
            bond_amount,
        );
        // Issue #677: disputer's bond is forfeited to the treasury/collector
        // when the dispute is rejected.
        events::emit_dispute_bond_forfeited(
            &env,
            dispute_id.clone(),
            collector.clone(),
            bond_amount,
        );

        // Emit DISPUTE_REJECTED event
        env.events().publish(
            (Symbol::new(&env, "DISPUTE"), Symbol::new(&env, "REJECTED")),
            (dispute_id, dispute.payment_id),
        );

        Ok(())
    }

    /// Retrieve the persisted operator note for a dispute.
    pub fn get_dispute_operator_note(
        env: Env,
        dispute_id: String,
    ) -> Result<DisputeOperatorNote, Error> {
        env.storage()
            .persistent()
            .get(&DataKey::DisputeOperatorNote(dispute_id))
            .ok_or(Error::DisputeNotFound)
    }

    // ─── Issue #185: Off-chain collaborative settlement ───────────────────────

    /// Close a dispute instantly when both the buyer and merchant have agreed
    /// on a settlement amount off-chain and submit their Ed25519 signatures.
    ///
    /// The message that both parties must sign is:
    ///   `SHA-256( dispute_id_bytes || settlement_amount_bytes )`
    /// where `settlement_amount_bytes` is the little-endian 16-byte encoding
    /// of the `i128` settlement amount.
    ///
    /// # Parameters
    /// * `dispute_id`         – The dispute to settle.
    /// * `settlement_amount`  – Agreed amount to refund to the buyer (≤ disputed amount).
    /// * `buyer_pubkey`       – Ed25519 public key of the buyer (32 bytes).
    /// * `signature_buyer`    – Ed25519 signature from the buyer (64 bytes).
    /// * `merchant_pubkey`    – Ed25519 public key of the merchant (32 bytes).
    /// * `signature_merchant` – Ed25519 signature from the merchant (64 bytes).
    pub fn settle_dispute_collaboratively(
        env: Env,
        dispute_id: String,
        settlement_amount: i128,
        buyer_pubkey: BytesN<32>,
        signature_buyer: BytesN<64>,
        merchant_pubkey: BytesN<32>,
        signature_merchant: BytesN<64>,
    ) -> Result<String, Error> {
        if settlement_amount <= 0 {
            return Err(Error::InvalidAmount);
        }

        let dispute = Self::get_dispute_internal(&env, &dispute_id)?;

        if dispute.status == DisputeStatus::Resolved || dispute.status == DisputeStatus::Rejected {
            return Err(Error::DisputeAlreadyResolved);
        }

        if settlement_amount > dispute.amount {
            return Err(Error::InvalidAmount);
        }

        // Build the message: SHA-256(dispute_id_bytes || settlement_amount_le16)
        // Both parties must have signed this exact message off-chain.
        let message = Self::build_settlement_message(&env, &dispute_id, settlement_amount);

        // Verify buyer signature
        env.crypto()
            .ed25519_verify(&buyer_pubkey, &message, &signature_buyer);

        // Verify merchant signature
        env.crypto()
            .ed25519_verify(&merchant_pubkey, &message, &signature_merchant);

        // Both signatures verified — create and process the refund
        let refund_reason = String::from_str(&env, "Collaborative off-chain settlement");

        let refund_id = Self::create_refund_internal(
            &env,
            dispute.payment_id.clone(),
            settlement_amount,
            refund_reason,
            dispute.disputer.clone(),
            None,
            None,
        )?;

        // Process the refund immediately (no operator approval needed)
        Self::process_refund_internal(&env, &env.current_contract_address(), refund_id.clone())?;

        let now = env.ledger().timestamp();

        // Persist the collaborative settlement record
        let settlement = CollaborativeSettlement {
            dispute_id: dispute_id.clone(),
            settlement_amount,
            buyer_pubkey,
            merchant_pubkey,
            settled_at: now,
        };
        env.storage().persistent().set(
            &DataKey::CollaborativeSettlement(dispute_id.clone()),
            &settlement,
        );
        Self::bump_ttl(
            &env,
            &DataKey::CollaborativeSettlement(dispute_id.clone()),
            LONG_LIVE_TTL,
        );

        // Update dispute to Resolved
        let mut dispute = Self::get_dispute_internal(&env, &dispute_id)?;
        dispute.status = DisputeStatus::Resolved;
        dispute.refund_id = Some(refund_id.clone());
        dispute.resolved_at = Some(now);
        dispute.resolution_notes = Some(String::from_str(
            &env,
            "Resolved via collaborative off-chain settlement",
        ));

        env.storage()
            .persistent()
            .set(&DataKey::Dispute(dispute_id.clone()), &dispute);
        Self::bump_dispute_ttl(&env, &dispute_id, &dispute.status);
        Self::release_open_dispute_slot(&env, &dispute.disputer);

        // Emit event
        env.events().publish(
            (
                Symbol::new(&env, "DISPUTE"),
                Symbol::new(&env, "COLLABORATIVE_SETTLED"),
            ),
            (dispute_id, dispute.payment_id, settlement_amount),
        );

        Ok(refund_id)
    }

    /// Retrieve the collaborative settlement record for a dispute.
    pub fn get_collaborative_settlement(
        env: Env,
        dispute_id: String,
    ) -> Result<CollaborativeSettlement, Error> {
        env.storage()
            .persistent()
            .get(&DataKey::CollaborativeSettlement(dispute_id))
            .ok_or(Error::DisputeNotFound)
    }

    /// Issue #184: Get the current dispute count for a merchant.
    pub fn get_merchant_dispute_count(env: Env, merchant_id: Address) -> u64 {
        env.storage()
            .persistent()
            .get(&DataKey::MerchantDisputeCount(merchant_id))
            .unwrap_or(0u64)
    }

    /// Issue #184: Get the current confirmed payment count for a merchant.
    pub fn get_merchant_payment_count(env: Env, merchant_id: Address) -> u64 {
        env.storage()
            .persistent()
            .get(&DataKey::MerchantPaymentCount(merchant_id))
            .unwrap_or(0u64)
    }

    /// Build the canonical settlement message for collaborative dispute resolution.
    ///
    /// Message = SHA-256( dispute_id_bytes || settlement_amount_le16 )
    fn build_settlement_message(
        env: &Env,
        dispute_id: &String,
        settlement_amount: i128,
    ) -> soroban_sdk::Bytes {
        use soroban_sdk::Bytes;

        let id_len = dispute_id.len() as usize;
        let mut raw = Bytes::new(env);

        // Append dispute_id bytes
        let mut id_buf = [0u8; 64];
        let read_len = id_len.min(64);
        dispute_id.copy_into_slice(&mut id_buf[..read_len]);
        for b in id_buf.iter().take(read_len) {
            raw.push_back(*b);
        }

        // Append settlement_amount as little-endian 16 bytes
        let amount_bytes = settlement_amount.to_le_bytes();
        for b in amount_bytes.iter() {
            raw.push_back(*b);
        }

        // Return SHA-256 hash as Bytes
        let hash = env.crypto().sha256(&raw).to_bytes();
        let mut result = Bytes::new(env);
        for i in 0..32u32 {
            result.push_back(hash.get(i).unwrap());
        }
        result
    }

    // ─── Stake-weighted dispute voting (issue #33) ────────────────────────────

    /// Lock a governance-token stake to participate in dispute voting.
    ///
    /// The arbitrator transfers `amount` tokens into the contract as a stake.
    /// The stake is slashed if the arbitrator votes against the majority.
    ///
    /// # Parameters
    /// * `arbitrator`  – Address locking the stake; must sign.
    /// * `dispute_id`  – Dispute to vote on.
    /// * `token`       – Governance token contract address.
    /// * `amount`      – Amount to lock (must be > 0).
    pub fn lock_stake(
        env: Env,
        arbitrator: Address,
        dispute_id: String,
        token: Address,
        amount: i128,
    ) -> Result<(), Error> {
        arbitrator.require_auth();

        if amount <= 0 {
            return Err(Error::InvalidAmount);
        }

        // Dispute must exist and be open / under review
        let dispute = Self::get_dispute_internal(&env, &dispute_id)?;
        if dispute.status == DisputeStatus::Resolved || dispute.status == DisputeStatus::Rejected {
            return Err(Error::DisputeAlreadyResolved);
        }

        // Prevent double-staking
        let stake_key = DataKey::DisputeStake(dispute_id.clone(), arbitrator.clone());
        if env.storage().persistent().has(&stake_key) {
            return Err(Error::Unauthorized);
        }

        // Effects: record stake before token transfer
        env.storage().persistent().set(&stake_key, &amount);
        Self::bump_ttl(&env, &stake_key, LONG_LIVE_TTL);

        // Interaction: pull stake from arbitrator
        let token_client = token::Client::new(&env, &token);
        token_client.transfer(&arbitrator, env.current_contract_address(), &amount);

        env.events().publish(
            (
                Symbol::new(&env, "DISPUTE"),
                Symbol::new(&env, "STAKE_LOCKED"),
            ),
            (dispute_id, arbitrator, amount),
        );

        Ok(())
    }

    /// Cast a stake-weighted vote on a dispute.
    ///
    /// The arbitrator must have locked a stake first via `lock_stake`.
    /// Each arbitrator may only vote once per dispute.
    ///
    /// # Parameters
    /// * `arbitrator` – Voting arbitrator; must sign.
    /// * `dispute_id` – Dispute to vote on.
    /// * `choice`     – `VoteChoice::Favour` or `VoteChoice::Against`.
    pub fn cast_vote(
        env: Env,
        arbitrator: Address,
        dispute_id: String,
        choice: VoteChoice,
    ) -> Result<(), Error> {
        arbitrator.require_auth();

        // Dispute must be open / under review
        let dispute = Self::get_dispute_internal(&env, &dispute_id)?;
        if dispute.status == DisputeStatus::Resolved || dispute.status == DisputeStatus::Rejected {
            return Err(Error::DisputeAlreadyResolved);
        }

        // Arbitrator must have a locked stake
        let stake_key = DataKey::DisputeStake(dispute_id.clone(), arbitrator.clone());
        let stake: i128 = env
            .storage()
            .persistent()
            .get(&stake_key)
            .ok_or(Error::Unauthorized)?;

        // Prevent double-voting
        let vote_key = DataKey::DisputeVote(dispute_id.clone(), arbitrator.clone());
        if env.storage().persistent().has(&vote_key) {
            return Err(Error::Unauthorized);
        }

        // Record vote
        env.storage().persistent().set(&vote_key, &choice);
        Self::bump_ttl(&env, &vote_key, LONG_LIVE_TTL);

        // Update tally
        let tally_key = DataKey::DisputeVoteTally(dispute_id.clone());
        let mut tally: VoteTally =
            env.storage()
                .persistent()
                .get(&tally_key)
                .unwrap_or(VoteTally {
                    favour_weight: 0,
                    against_weight: 0,
                    vote_count: 0,
                });

        match choice {
            VoteChoice::Favour => tally.favour_weight = tally.favour_weight.saturating_add(stake),
            VoteChoice::Against => {
                tally.against_weight = tally.against_weight.saturating_add(stake)
            }
        }
        tally.vote_count = tally.vote_count.saturating_add(1);

        env.storage().persistent().set(&tally_key, &tally);
        Self::bump_ttl(&env, &tally_key, LONG_LIVE_TTL);

        env.events().publish(
            (Symbol::new(&env, "DISPUTE"), Symbol::new(&env, "VOTE_CAST")),
            (dispute_id, arbitrator, stake),
        );

        Ok(())
    }

    /// Finalize a dispute based on stake-weighted votes.
    ///
    /// The majority side wins. Arbitrators who voted against the majority
    /// lose 10% of their stake (slashed to the contract admin). Winners
    /// receive their stake back.
    ///
    /// # Parameters
    /// * `operator`    – Settlement operator or oracle; must sign.
    /// * `dispute_id`  – Dispute to finalize.
    /// * `token`       – Governance token used for stakes.
    /// * `arbitrators` – List of all arbitrators who participated.
    pub fn finalize_dispute_vote(
        env: Env,
        operator: Address,
        dispute_id: String,
        token: Address,
        arbitrators: Vec<Address>,
    ) -> Result<(), Error> {
        operator.require_auth();

        let has_settlement =
            AccessControl::has_role(&env, &role_settlement_operator(&env), &operator);
        let has_oracle = AccessControl::has_role(&env, &role_oracle(&env), &operator);
        if !has_settlement && !has_oracle {
            return Err(Error::Unauthorized);
        }

        let dispute = Self::get_dispute_internal(&env, &dispute_id)?;
        if dispute.status == DisputeStatus::Resolved || dispute.status == DisputeStatus::Rejected {
            return Err(Error::DisputeAlreadyResolved);
        }

        let tally_key = DataKey::DisputeVoteTally(dispute_id.clone());
        let tally: VoteTally = env
            .storage()
            .persistent()
            .get(&tally_key)
            .unwrap_or(VoteTally {
                favour_weight: 0,
                against_weight: 0,
                vote_count: 0,
            });

        // Determine majority
        let favour_wins = tally.favour_weight >= tally.against_weight;
        let majority = if favour_wins {
            VoteChoice::Favour
        } else {
            VoteChoice::Against
        };

        let token_client = token::Client::new(&env, &token);
        let slash_bps: i128 = 1_000; // 10% slash

        // Return stakes; slash minority voters
        for arb in arbitrators.iter() {
            let stake_key = DataKey::DisputeStake(dispute_id.clone(), arb.clone());
            let stake: i128 = match env.storage().persistent().get(&stake_key) {
                Some(s) => s,
                None => continue,
            };

            let vote_key = DataKey::DisputeVote(dispute_id.clone(), arb.clone());
            let vote: VoteChoice = match env.storage().persistent().get(&vote_key) {
                Some(v) => v,
                None => continue,
            };

            let voted_with_majority = vote == majority;

            // Effects: remove stake record
            env.storage().persistent().remove(&stake_key);

            if voted_with_majority {
                // Return full stake
                token_client.transfer(&env.current_contract_address(), &arb, &stake);
            } else {
                // Slash 10%, return remainder
                let slash = stake * slash_bps / 10_000;
                let remainder = stake.saturating_sub(slash);
                if remainder > 0 {
                    token_client.transfer(&env.current_contract_address(), &arb, &remainder);
                }
                if slash > 0 {
                    if let Some(admin) = AccessControl::get_admin(&env) {
                        token_client.transfer(&env.current_contract_address(), &admin, &slash);
                    }
                }
            }
        }

        // Resolve or reject the dispute based on vote outcome
        if favour_wins {
            // Majority voted in favour — issue refund
            let refund_reason =
                String::from_str(&env, "Resolved by stake-weighted arbitration vote");
            if let Ok(refund_id) = Self::create_refund_internal(
                &env,
                dispute.payment_id.clone(),
                dispute.amount,
                refund_reason,
                dispute.disputer.clone(),
                None,
                None,
            ) {
                let _ = Self::process_refund_internal(&env, &operator, refund_id);
            }

            let mut d = Self::get_dispute_internal(&env, &dispute_id)?;
            d.status = DisputeStatus::Resolved;
            d.resolved_at = Some(env.ledger().timestamp());
            env.storage()
                .persistent()
                .set(&DataKey::Dispute(dispute_id.clone()), &d);
            Self::bump_dispute_ttl(&env, &dispute_id, &d.status);
            Self::release_open_dispute_slot(&env, &d.disputer);
        } else {
            let mut d = Self::get_dispute_internal(&env, &dispute_id)?;
            d.status = DisputeStatus::Rejected;
            d.resolved_at = Some(env.ledger().timestamp());
            env.storage()
                .persistent()
                .set(&DataKey::Dispute(dispute_id.clone()), &d);
            Self::bump_dispute_ttl(&env, &dispute_id, &d.status);
            Self::release_open_dispute_slot(&env, &d.disputer);
        }

        env.events().publish(
            (
                Symbol::new(&env, "DISPUTE"),
                Symbol::new(&env, "VOTE_FINALIZED"),
            ),
            (
                dispute_id,
                tally.favour_weight,
                tally.against_weight,
                favour_wins,
            ),
        );

        Ok(())
    }

    /// Cast a role-gated vote on a dispute. Unlike [`Self::cast_vote`] (which
    /// is stake-weighted), this flow simply counts one vote per
    /// `ARBITRATOR`-role address and auto-executes the resolution as soon as
    /// either side reaches [`ARBITRATOR_VOTING_THRESHOLD`].
    ///
    /// # Parameters
    /// * `arbitrator` – Must hold the `ARBITRATOR` role; must sign.
    /// * `dispute_id` – Dispute to vote on; must currently be `UnderReview`.
    /// * `choice`     – `ArbitratorVoteChoice::Approve` or `::Reject`.
    pub fn vote_dispute(
        env: Env,
        arbitrator: Address,
        dispute_id: String,
        choice: ArbitratorVoteChoice,
    ) -> Result<(), Error> {
        arbitrator.require_auth();

        if !AccessControl::has_role(&env, &role_arbitrator(&env), &arbitrator) {
            return Err(Error::Unauthorized);
        }

        let dispute = Self::get_dispute_internal(&env, &dispute_id)?;
        if dispute.status != DisputeStatus::UnderReview {
            return Err(Error::DisputeAlreadyResolved);
        }

        let vote_key = DataKey::ArbitratorVote(dispute_id.clone(), arbitrator.clone());
        if env.storage().persistent().has(&vote_key) {
            return Err(Error::AlreadyVoted);
        }

        env.storage().persistent().set(&vote_key, &choice);
        Self::bump_ttl(&env, &vote_key, LONG_LIVE_TTL);

        let tally_key = DataKey::ArbitratorVoteTally(dispute_id.clone());
        let mut tally: ArbitratorVoteTally =
            env.storage()
                .persistent()
                .get(&tally_key)
                .unwrap_or(ArbitratorVoteTally {
                    approve_count: 0,
                    reject_count: 0,
                });

        match choice {
            ArbitratorVoteChoice::Approve => {
                tally.approve_count = tally.approve_count.saturating_add(1)
            }
            ArbitratorVoteChoice::Reject => {
                tally.reject_count = tally.reject_count.saturating_add(1)
            }
        }

        env.storage().persistent().set(&tally_key, &tally);
        Self::bump_ttl(&env, &tally_key, LONG_LIVE_TTL);

        env.events().publish(
            (Symbol::new(&env, "DISPUTE"), Symbol::new(&env, "VOTE_CAST")),
            (dispute_id.clone(), arbitrator),
        );

        if tally.approve_count >= ARBITRATOR_VOTING_THRESHOLD {
            Self::auto_resolve_dispute(&env, &dispute_id, true)?;
        } else if tally.reject_count >= ARBITRATOR_VOTING_THRESHOLD {
            Self::auto_resolve_dispute(&env, &dispute_id, false)?;
        }

        Ok(())
    }

    /// Finalize a dispute once ARBITRATOR-role voting has reached
    /// [`ARBITRATOR_VOTING_THRESHOLD`] in either direction. `approved` issues
    /// a refund and marks the dispute `Resolved`; otherwise it's `Rejected`.
    fn auto_resolve_dispute(env: &Env, dispute_id: &String, approved: bool) -> Result<(), Error> {
        let mut dispute = Self::get_dispute_internal(env, dispute_id)?;
        if dispute.status == DisputeStatus::Resolved || dispute.status == DisputeStatus::Rejected {
            return Ok(());
        }

        if approved {
            let refund_reason = String::from_str(env, "Auto-resolved by arbitrator vote");
            if let Some(admin) = AccessControl::get_admin(env) {
                if let Ok(refund_id) = Self::create_refund_internal(
                    env,
                    dispute.payment_id.clone(),
                    dispute.amount,
                    refund_reason,
                    dispute.disputer.clone(),
                    None,
                    None,
                ) {
                    let _ = Self::process_refund_internal(env, &admin, refund_id);
                }
            }
            dispute.status = DisputeStatus::Resolved;
        } else {
            dispute.status = DisputeStatus::Rejected;
        }
        dispute.resolved_at = Some(env.ledger().timestamp());

        env.storage()
            .persistent()
            .set(&DataKey::Dispute(dispute_id.clone()), &dispute);
        Self::bump_dispute_ttl(env, dispute_id, &dispute.status);

        env.events().publish(
            (
                Symbol::new(env, "DISPUTE"),
                Symbol::new(env, "AUTO_RESOLVED"),
            ),
            (dispute_id.clone(), approved),
        );

        Ok(())
    }

    /// Get the current vote tally for a dispute.
    pub fn get_vote_tally(env: Env, dispute_id: String) -> VoteTally {
        env.storage()
            .persistent()
            .get(&DataKey::DisputeVoteTally(dispute_id))
            .unwrap_or(VoteTally {
                favour_weight: 0,
                against_weight: 0,
                vote_count: 0,
            })
    }

    pub fn get_dispute(env: Env, dispute_id: String) -> Result<Dispute, Error> {
        let mut dispute = Self::get_dispute_internal(&env, &dispute_id)?;
        let _ = Self::maybe_escalate_dispute_due_to_deadline(&env, &dispute_id, &mut dispute)?;
        Ok(dispute)
    }

    pub fn get_payment_disputes(env: Env, payment_id: String) -> Result<Vec<Dispute>, Error> {
        let dispute_ids = Self::get_payment_disputes_internal(&env, &payment_id);
        let mut disputes = vec![&env];
        for id in dispute_ids.iter() {
            if let Ok(dispute) = Self::get_dispute_internal(&env, &id) {
                disputes.push_back(dispute);
            }
        }
        Ok(disputes)
    }

    /// Issue #178: Submit an arbitrator vote on a dispute resolution.
    pub fn submit_arbitrator_vote(
        env: Env,
        dispute_id: String,
        arbitrator: Address,
        vote: ArbitratorVoteChoice,
    ) -> Result<(), Error> {
        arbitrator.require_auth();

        // Check if arbitrator has the ARBITRATOR role
        if !AccessControl::has_role(&env, &role_arbitrator(&env), &arbitrator) {
            return Err(Error::Unauthorized);
        }

        let dispute = Self::get_dispute_internal(&env, &dispute_id)?;

        // Only allow voting on Open disputes
        if dispute.status != DisputeStatus::Open {
            return Err(Error::DisputeAlreadyResolved);
        }

        // Check if arbitrator has already voted
        let vote_key = DataKey::ArbitratorVote(dispute_id.clone(), arbitrator.clone());
        if env.storage().persistent().has(&vote_key) {
            return Err(Error::InvalidAmount); // Reusing error code for "already voted"
        }

        // Record the vote
        let arbitrator_vote = ArbitratorVote {
            dispute_id: dispute_id.clone(),
            arbitrator: arbitrator.clone(),
            vote: vote.clone(),
            voted_at: env.ledger().timestamp(),
        };

        env.storage().persistent().set(&vote_key, &arbitrator_vote);

        // Add arbitrator to the voters list for this dispute
        let voters_key = DataKey::DisputeArbitratorVotes(dispute_id.clone());
        let mut voters: Vec<Address> = env
            .storage()
            .persistent()
            .get(&voters_key)
            .unwrap_or(vec![&env]);

        voters.push_back(arbitrator.clone());
        env.storage().persistent().set(&voters_key, &voters);

        // Emit arbitrator vote event
        env.events().publish(
            (
                Symbol::new(&env, "DISPUTE"),
                Symbol::new(&env, "ARBITRATOR_VOTE"),
            ),
            (dispute_id, arbitrator),
        );

        Ok(())
    }

    /// Issue #178: Check arbitrator voting threshold and auto-resolve if met.
    pub fn check_arbitration_threshold(env: Env, dispute_id: String) -> Result<bool, Error> {
        let dispute = Self::get_dispute_internal(&env, &dispute_id)?;

        if dispute.status != DisputeStatus::Open {
            return Ok(false);
        }

        let voters_key = DataKey::DisputeArbitratorVotes(dispute_id.clone());
        let voters: Vec<Address> = env
            .storage()
            .persistent()
            .get(&voters_key)
            .unwrap_or(vec![&env]);

        if voters.len() < ARBITRATOR_VOTING_THRESHOLD {
            return Err(Error::ArbitrationVotingThresholdNotMet);
        }

        // Count approvals
        let mut approvals: u32 = 0;
        for voter in voters.iter() {
            let vote_key = DataKey::ArbitratorVote(dispute_id.clone(), voter.clone());
            if let Some(vote) = env
                .storage()
                .persistent()
                .get::<DataKey, ArbitratorVote>(&vote_key)
            {
                if let ArbitratorVoteChoice::Approve = vote.vote {
                    approvals += 1;
                }
            }
        }

        // Threshold met if majority (>= threshold) approves
        Ok(approvals >= ARBITRATOR_VOTING_THRESHOLD)
    }

    fn get_next_dispute_id(env: &Env) -> u64 {
        let mut counter: u64 = env
            .storage()
            .persistent()
            .get(&DataKey::DisputeCounter)
            .unwrap_or(0);
        counter += 1;
        env.storage()
            .persistent()
            .set(&DataKey::DisputeCounter, &counter);
        counter
    }

    fn build_dispute_id(env: &Env, counter: u64) -> String {
        format_id(env, "dispute_", counter)
    }

    fn get_dispute_internal(env: &Env, dispute_id: &String) -> Result<Dispute, Error> {
        env.storage()
            .persistent()
            .get(&DataKey::Dispute(dispute_id.clone()))
            .ok_or(Error::DisputeNotFound)
    }

    fn get_payment_disputes_internal(env: &Env, payment_id: &String) -> Vec<String> {
        env.storage()
            .persistent()
            .get(&DataKey::PaymentDisputes(payment_id.clone()))
            .unwrap_or_else(|| vec![env])
    }

    // Subscription management functions
    pub fn create_subscription_plan(
        env: Env,
        merchant: Address,
        plan_id: String,
        name: String,
        description: String,
        amount: i128,
        currency: Symbol,
        billing_interval: BillingInterval,
    ) -> Result<(), Error> {
        merchant.require_auth();

        if !AccessControl::has_role(&env, &role_merchant(&env), &merchant) {
            return Err(Error::Unauthorized);
        }

        if amount <= 0 {
            return Err(Error::InvalidAmount);
        }

        let interval_secs = billing_interval.to_secs();

        let plan = SubscriptionPlan {
            plan_id: plan_id.clone(),
            merchant_id: merchant.clone(),
            name,
            description,
            amount,
            currency,
            interval_secs,
            billing_interval,
            active: true,
            payout_splits: Vec::new(&env),
        };

        env.storage()
            .persistent()
            .set(&DataKey::SubscriptionPlan(plan_id.clone()), &plan);

        // Issue #635: emit SUBSCRIPTION/PLAN_CREATED for indexer plan-level visibility.
        events::emit_subscription_plan_created(&env, &plan_id, &merchant, amount, interval_secs);

        Ok(())
    }

    /// Create a billing plan with an explicit interval in seconds.
    pub fn create_plan(
        env: Env,
        merchant: Address,
        plan_id: String,
        name: String,
        description: String,
        amount: i128,
        currency: Symbol,
        interval_secs: u64,
    ) -> Result<(), Error> {
        merchant.require_auth();

        if !AccessControl::has_role(&env, &role_merchant(&env), &merchant) {
            return Err(Error::Unauthorized);
        }

        if amount <= 0 || interval_secs == 0 {
            return Err(Error::InvalidAmount);
        }

        let plan = SubscriptionPlan {
            plan_id: plan_id.clone(),
            merchant_id: merchant,
            name,
            description,
            amount,
            currency,
            interval_secs,
            billing_interval: BillingInterval::Daily,
            active: true,
            payout_splits: Vec::new(&env),
        };

        env.storage()
            .persistent()
            .set(&DataKey::SubscriptionPlan(plan_id), &plan);

        Ok(())
    }

    pub fn get_subscription_plan(env: Env, plan_id: String) -> Result<SubscriptionPlan, Error> {
        env.storage()
            .persistent()
            .get(&DataKey::SubscriptionPlan(plan_id))
            .ok_or(Error::PaymentNotFound)
    }

    /// Alias for `get_subscription_plan`.
    pub fn get_plan(env: Env, plan_id: String) -> Result<SubscriptionPlan, Error> {
        Self::get_subscription_plan(env, plan_id)
    }

    pub fn deactivate_subscription_plan(
        env: Env,
        merchant: Address,
        plan_id: String,
    ) -> Result<(), Error> {
        merchant.require_auth();

        let mut plan: SubscriptionPlan = env
            .storage()
            .persistent()
            .get(&DataKey::SubscriptionPlan(plan_id.clone()))
            .ok_or(Error::PaymentNotFound)?;

        if plan.merchant_id != merchant {
            return Err(Error::Unauthorized);
        }

        plan.active = false;
        env.storage()
            .persistent()
            .set(&DataKey::SubscriptionPlan(plan_id.clone()), &plan);

        // Issue #635: emit SUBSCRIPTION/PLAN_DEACTIVATED for indexer plan-level visibility.
        events::emit_subscription_plan_deactivated(&env, &plan_id, &merchant);

        Ok(())
    }

    pub fn subscribe(
        env: Env,
        payer: Address,
        plan_id: String,
        max_payments: Option<u32>,
        affiliate: Option<Address>,
        affiliate_fee_bps: Option<u32>,
    ) -> Result<String, Error> {
        payer.require_auth();

        let plan: SubscriptionPlan = env
            .storage()
            .persistent()
            .get(&DataKey::SubscriptionPlan(plan_id.clone()))
            .ok_or(Error::PaymentNotFound)?;

        if !plan.active {
            return Err(Error::PaymentAlreadyProcessed);
        }

        let counter = Self::get_next_subscription_id(&env);
        let subscription_id = format_id(&env, "sub_", counter);

        let now = env.ledger().timestamp();
        let subscription = Subscription {
            subscription_id: subscription_id.clone(),
            merchant_id: plan.merchant_id.clone(),
            payer_address: payer.clone(),
            plan_id: plan_id.clone(),
            amount: plan.amount,
            currency: plan.currency,
            interval_secs: plan.interval_secs,
            next_payment_at: now.saturating_add(plan.interval_secs),
            status: SubscriptionStatus::Active,
            created_at: now,
            last_payment_at: None,
            total_payments: 0,
            max_payments,
            retry_count: 0,
            next_retry_at: None,
            resume_at: None,
            affiliate: affiliate.clone(),
            affiliate_fee_bps,
        };

        env.storage().persistent().set(
            &DataKey::Subscription(subscription_id.clone()),
            &subscription,
        );

        let mut payer_subscriptions = Self::get_payer_subscriptions_internal(&env, &payer);
        payer_subscriptions.push_back(subscription_id.clone());
        env.storage().persistent().set(
            &DataKey::PayerSubscriptions(payer.clone()),
            &payer_subscriptions,
        );

        // Issue #302: Track in ActiveSubscriptions index
        Self::add_active_subscription(&env, &subscription_id);

        // Issue #633: Track in the per-plan subscriber index
        Self::add_plan_subscriber(&env, &plan_id, &subscription_id);

        env.events().publish(
            (
                Symbol::new(&env, "SUBSCRIPTION"),
                Symbol::new(&env, "CREATED"),
            ),
            (subscription_id.clone(), payer, plan_id),
        );

        Ok(subscription_id)
    }

    /// Subscribe to a plan using a caller-supplied subscription identifier.
    pub fn subscribe_to_plan(
        env: Env,
        payer: Address,
        subscription_id: String,
        plan_id: String,
    ) -> Result<(), Error> {
        payer.require_auth();

        if env
            .storage()
            .persistent()
            .has(&DataKey::Subscription(subscription_id.clone()))
        {
            return Err(Error::PaymentAlreadyExists);
        }

        let plan: SubscriptionPlan = env
            .storage()
            .persistent()
            .get(&DataKey::SubscriptionPlan(plan_id.clone()))
            .ok_or(Error::PaymentNotFound)?;

        if !plan.active {
            return Err(Error::PaymentAlreadyProcessed);
        }

        let now = env.ledger().timestamp();
        let subscription = Subscription {
            subscription_id: subscription_id.clone(),
            merchant_id: plan.merchant_id.clone(),
            payer_address: payer.clone(),
            plan_id: plan_id.clone(),
            amount: plan.amount,
            currency: plan.currency,
            interval_secs: plan.interval_secs,
            next_payment_at: now.saturating_add(plan.interval_secs),
            status: SubscriptionStatus::Active,
            created_at: now,
            last_payment_at: None,
            total_payments: 0,
            max_payments: None,
            retry_count: 0,
            next_retry_at: None,
            resume_at: None,
            affiliate: None,
            affiliate_fee_bps: None,
        };

        env.storage().persistent().set(
            &DataKey::Subscription(subscription_id.clone()),
            &subscription,
        );

        let mut payer_subscriptions = Self::get_payer_subscriptions_internal(&env, &payer);
        payer_subscriptions.push_back(subscription_id.clone());
        env.storage().persistent().set(
            &DataKey::PayerSubscriptions(payer.clone()),
            &payer_subscriptions,
        );

        Self::add_active_subscription(&env, &subscription_id);

        // Issue #633: Track in the per-plan subscriber index
        Self::add_plan_subscriber(&env, &plan_id, &subscription_id);

        env.events().publish(
            (
                Symbol::new(&env, "SUBSCRIPTION"),
                Symbol::new(&env, "CREATED"),
            ),
            (subscription_id, payer, plan_id),
        );

        Ok(())
    }

    pub fn get_subscription(env: Env, subscription_id: String) -> Result<Subscription, Error> {
        Self::get_subscription_internal(&env, &subscription_id)
    }

    pub fn get_payer_subscriptions(env: Env, payer: Address) -> Vec<Subscription> {
        let subscription_ids = Self::get_payer_subscriptions_internal(&env, &payer);
        let mut subscriptions = vec![&env];
        for id in subscription_ids.iter() {
            if let Ok(sub) = Self::get_subscription_internal(&env, &id) {
                subscriptions.push_back(sub);
            }
        }
        subscriptions
    }

    /// Issue #633: List subscribers to a plan, paginated.
    ///
    /// Returns up to `limit` `Subscription` records (hard-capped at 100 per
    /// call) starting at `offset` within the plan's subscriber index, in
    /// subscription order. When `include_cancelled` is `false`, subscriptions
    /// with `status == Cancelled` are filtered out *before* pagination, so the
    /// page always contains `limit` live subscribers when that many remain.
    ///
    /// Used by merchants for plan-level analytics and bulk notifications.
    pub fn get_plan_subscribers(
        env: Env,
        plan_id: String,
        offset: u32,
        limit: u32,
        include_cancelled: bool,
    ) -> Vec<Subscription> {
        let subscription_ids: Vec<String> = env
            .storage()
            .persistent()
            .get(&DataKey::PlanSubscribers(plan_id))
            .unwrap_or_else(|| vec![&env]);

        let capped_limit = if limit == 0 || limit > 100 {
            100
        } else {
            limit
        };

        let mut result = vec![&env];
        let mut matched: u32 = 0;
        for id in subscription_ids.iter() {
            let sub = match Self::get_subscription_internal(&env, &id) {
                Ok(s) => s,
                Err(_) => continue,
            };
            if !include_cancelled && sub.status == SubscriptionStatus::Cancelled {
                continue;
            }
            if matched >= offset {
                result.push_back(sub);
                if result.len() >= capped_limit {
                    break;
                }
            }
            matched = matched.saturating_add(1);
        }
        result
    }

    pub fn pause_subscription(
        env: Env,
        payer: Address,
        subscription_id: String,
    ) -> Result<(), Error> {
        payer.require_auth();

        let mut subscription = Self::get_subscription_internal(&env, &subscription_id)?;

        if subscription.payer_address != payer {
            return Err(Error::Unauthorized);
        }

        if subscription.status != SubscriptionStatus::Active {
            return Err(Error::PaymentAlreadyProcessed);
        }

        subscription.status = SubscriptionStatus::Paused;
        env.storage().persistent().set(
            &DataKey::Subscription(subscription_id.clone()),
            &subscription,
        );

        // Issue #302: Remove from ActiveSubscriptions index
        Self::remove_active_subscription(&env, &subscription_id);

        Ok(())
    }

    pub fn pause_with_resume_date(
        env: Env,
        payer: Address,
        subscription_id: String,
        resume_timestamp: u64,
    ) -> Result<(), Error> {
        payer.require_auth();

        let now = env.ledger().timestamp();
        if resume_timestamp <= now {
            return Err(Error::InvalidResumeTimestamp);
        }

        let mut subscription = Self::get_subscription_internal(&env, &subscription_id)?;

        if subscription.payer_address != payer {
            return Err(Error::Unauthorized);
        }

        if subscription.status != SubscriptionStatus::Active {
            return Err(Error::PaymentAlreadyProcessed);
        }

        subscription.status = SubscriptionStatus::Paused;
        subscription.resume_at = Some(resume_timestamp);

        env.storage().persistent().set(
            &DataKey::Subscription(subscription_id.clone()),
            &subscription,
        );

        // Issue #302: Remove from ActiveSubscriptions index
        Self::remove_active_subscription(&env, &subscription_id);

        env.events().publish(
            (
                Symbol::new(&env, "SUBSCRIPTION"),
                Symbol::new(&env, "PAUSED"),
            ),
            (subscription_id, payer, resume_timestamp),
        );

        Ok(())
    }

    /// Attempt to charge a subscription.
    ///
    /// Handles the full lifecycle including:
    /// - Auto-resuming a paused subscription whose `resume_at` has passed.
    /// - Pulling the due amount via a pre-authorization (if one exists) or
    ///   directly via the token contract.
    /// - On insufficient balance: entering a grace period with up to
    ///   `SUBSCRIPTION_MAX_RETRIES` retries spaced `SUBSCRIPTION_RETRY_INTERVAL_SECS`
    ///   apart before marking the subscription as `Cancelled`.
    ///
    /// # Parameters
    /// * `operator`        – Oracle or settlement operator; must sign.
    /// * `subscription_id` – Subscription to charge.
    /// * `token`           – Token contract to pull payment from.
    pub fn charge_subscription(
        env: Env,
        operator: Address,
        subscription_id: String,
        token: Address,
    ) -> Result<SubscriptionStatus, Error> {
        operator.require_auth();

        if !AccessControl::has_role(&env, &role_oracle(&env), &operator)
            && !AccessControl::has_role(&env, &role_settlement_operator(&env), &operator)
        {
            return Err(Error::Unauthorized);
        }

        let mut subscription = Self::get_subscription_internal(&env, &subscription_id)?;
        let now = env.ledger().timestamp();

        // ── Auto-resume if the pause window has expired ───────────────────────
        if subscription.status == SubscriptionStatus::Paused {
            if let Some(resume_at) = subscription.resume_at {
                if now >= resume_at {
                    subscription.status = SubscriptionStatus::Active;
                    subscription.resume_at = None;
                    // Push next payment forward from the resume point.
                    subscription.next_payment_at =
                        resume_at.saturating_add(subscription.interval_secs);

                    // Issue #302: Add back to ActiveSubscriptions index
                    Self::add_active_subscription(&env, &subscription_id);

                    env.events().publish(
                        (
                            Symbol::new(&env, "SUBSCRIPTION"),
                            Symbol::new(&env, "RESUMED"),
                        ),
                        (subscription_id.clone(), subscription.payer_address.clone()),
                    );
                }
            }
        }

        // Only charge Active subscriptions.
        if subscription.status != SubscriptionStatus::Active {
            env.storage().persistent().set(
                &DataKey::Subscription(subscription_id.clone()),
                &subscription,
            );
            return Ok(subscription.status);
        }

        // Check whether we are in a retry window or a normal due-date window.
        let is_retry = subscription.next_retry_at.is_some();
        let due = if is_retry {
            subscription.next_retry_at.unwrap_or(0)
        } else {
            subscription.next_payment_at
        };

        if now < due {
            // Not yet due — nothing to do.
            env.storage().persistent().set(
                &DataKey::Subscription(subscription_id.clone()),
                &subscription,
            );
            return Ok(subscription.status);
        }

        // ── Attempt token transfer ────────────────────────────────────────────
        let token_client = token::Client::new(&env, &token);
        let payer = subscription.payer_address.clone();
        let merchant = subscription.merchant_id.clone();
        let amount = subscription.amount;

        // Pull the full amount into this contract so we can distribute splits/fees.
        let transfer_ok = token_client
            .try_transfer(&payer, env.current_contract_address(), &amount)
            .is_ok();

        if transfer_ok {
            // ── Success path ──────────────────────────────────────────────────
            // Distribute according to plan splits or affiliate settings.
            // First try to resolve the plan and its payout splits.
            if let Ok(plan) = Self::get_subscription_plan(env.clone(), subscription.plan_id.clone())
            {
                if !plan.payout_splits.is_empty() {
                    // If payout_splits configured, send each recipient their configured amount.
                    for s in plan.payout_splits.iter() {
                        let _ = token_client.try_transfer(
                            &env.current_contract_address(),
                            &s.recipient,
                            &s.amount,
                        );
                    }
                } else if let (Some(aff), Some(bps)) = (
                    subscription.affiliate.clone(),
                    subscription.affiliate_fee_bps,
                ) {
                    // Pay affiliate fee then merchant receives remainder.
                    let fee = amount.saturating_mul(bps as i128) / 10_000i128;
                    let merchant_amount = amount.saturating_sub(fee);
                    if fee > 0 {
                        let _ =
                            token_client.try_transfer(&env.current_contract_address(), &aff, &fee);
                    }
                    let _ = token_client.try_transfer(
                        &env.current_contract_address(),
                        &merchant,
                        &merchant_amount,
                    );
                } else {
                    // Default: send full amount to merchant.
                    let _ = token_client.try_transfer(
                        &env.current_contract_address(),
                        &merchant,
                        &amount,
                    );
                }
            } else {
                // If plan can't be loaded, fall back to sending full amount to merchant.
                let _ =
                    token_client.try_transfer(&env.current_contract_address(), &merchant, &amount);
            }

            subscription.last_payment_at = Some(now);
            subscription.total_payments = subscription.total_payments.saturating_add(1);
            subscription.retry_count = 0;
            subscription.next_retry_at = None;
            subscription.next_payment_at = now.saturating_add(subscription.interval_secs);

            // Check max_payments cap.
            if let Some(max) = subscription.max_payments {
                if subscription.total_payments >= max {
                    subscription.status = SubscriptionStatus::Expired;
                    // Issue #302: Remove from ActiveSubscriptions index
                    Self::remove_active_subscription(&env, &subscription_id);
                }
            }

            env.storage().persistent().set(
                &DataKey::Subscription(subscription_id.clone()),
                &subscription,
            );

            env.events().publish(
                (
                    Symbol::new(&env, "SUBSCRIPTION"),
                    Symbol::new(&env, "CHARGED"),
                ),
                (
                    subscription_id.clone(),
                    payer.clone(),
                    merchant.clone(),
                    amount,
                    subscription.total_payments,
                ),
            );

            // Emit explicit expired event when the subscription reached its cap.
            if subscription.status == SubscriptionStatus::Expired {
                env.events().publish(
                    (
                        Symbol::new(&env, "SUBSCRIPTION"),
                        Symbol::new(&env, "EXPIRED"),
                    ),
                    (subscription_id, payer),
                );
            }
        } else {
            // ── Failure path — grace period / retry logic ─────────────────────
            subscription.retry_count = subscription.retry_count.saturating_add(1);

            if subscription.retry_count >= SUBSCRIPTION_MAX_RETRIES {
                // Exhausted all retries — cancel the subscription.
                subscription.status = SubscriptionStatus::Cancelled;
                subscription.next_retry_at = None;

                // Issue #302: Remove from ActiveSubscriptions index
                Self::remove_active_subscription(&env, &subscription_id);

                env.storage().persistent().set(
                    &DataKey::Subscription(subscription_id.clone()),
                    &subscription,
                );

                env.events().publish(
                    (
                        Symbol::new(&env, "SUBSCRIPTION"),
                        Symbol::new(&env, "CANCELLED_MAX_RETRIES"),
                    ),
                    (
                        subscription_id.clone(),
                        payer.clone(),
                        subscription.retry_count,
                        SUBSCRIPTION_MAX_RETRIES,
                    ),
                );

                return Err(Error::SubscriptionRetryExhausted);
            } else {
                // Schedule the next retry attempt.
                let next_retry = now.saturating_add(SUBSCRIPTION_RETRY_INTERVAL_SECS);
                subscription.next_retry_at = Some(next_retry);

                env.storage().persistent().set(
                    &DataKey::Subscription(subscription_id.clone()),
                    &subscription,
                );

                env.events().publish(
                    (
                        Symbol::new(&env, "SUBSCRIPTION"),
                        Symbol::new(&env, "PAYMENT_FAILED"),
                    ),
                    (
                        subscription_id,
                        payer,
                        subscription.retry_count,
                        SUBSCRIPTION_MAX_RETRIES,
                        next_retry,
                    ),
                );

                return Err(Error::SubscriptionInGracePeriod);
            }
        }

        Ok(subscription.status)
    }

    /// Trigger a recurring subscription charge when the billing date is due.
    pub fn process_subscription(
        env: Env,
        operator: Address,
        subscription_id: String,
    ) -> Result<SubscriptionStatus, Error> {
        operator.require_auth();

        if !AccessControl::has_role(&env, &role_oracle(&env), &operator)
            && !AccessControl::has_role(&env, &role_settlement_operator(&env), &operator)
        {
            return Err(Error::Unauthorized);
        }

        let subscription = Self::get_subscription_internal(&env, &subscription_id)?;
        let now = env.ledger().timestamp();
        let due = subscription
            .next_retry_at
            .unwrap_or(subscription.next_payment_at);
        if now < due {
            return Err(Error::PaymentAlreadyProcessed);
        }

        let token: Address = env
            .storage()
            .persistent()
            .get(&DataKey::UsdcToken)
            .ok_or(Error::PaymentNotFound)?;

        Self::charge_subscription(env, operator, subscription_id, token)
    }

    pub fn resume_subscription(
        env: Env,
        payer: Address,
        subscription_id: String,
    ) -> Result<(), Error> {
        payer.require_auth();

        let mut subscription = Self::get_subscription_internal(&env, &subscription_id)?;

        if subscription.payer_address != payer {
            return Err(Error::Unauthorized);
        }

        if subscription.status != SubscriptionStatus::Paused {
            return Err(Error::PaymentAlreadyProcessed);
        }

        subscription.status = SubscriptionStatus::Active;
        subscription.next_payment_at = env
            .ledger()
            .timestamp()
            .saturating_add(subscription.interval_secs);
        env.storage().persistent().set(
            &DataKey::Subscription(subscription_id.clone()),
            &subscription,
        );

        // Issue #302: Add back to ActiveSubscriptions index
        Self::add_active_subscription(&env, &subscription_id);

        Ok(())
    }

    /// Cancel a subscription and optionally create a prorated pending refund.
    ///
    /// When `refund_remaining` is true, a payment was made in the current billing
    /// period, and the admin policy `allow_prorated_refunds` is enabled, a pending
    /// refund is created for the unused portion of the period (by whole days).
    pub fn cancel_subscription(
        env: Env,
        payer_or_merchant: Address,
        subscription_id: String,
        refund_remaining: bool,
    ) -> Result<(), Error> {
        payer_or_merchant.require_auth();

        let mut subscription = Self::get_subscription_internal(&env, &subscription_id)?;

        if subscription.payer_address != payer_or_merchant
            && subscription.merchant_id != payer_or_merchant
        {
            return Err(Error::Unauthorized);
        }

        if subscription.status == SubscriptionStatus::Cancelled {
            return Err(Error::PaymentAlreadyProcessed);
        }

        let now = env.ledger().timestamp();
        let allow_proration: bool = env
            .storage()
            .persistent()
            .get(&DataKey::AllowProratedRefunds)
            .unwrap_or(false);

        if refund_remaining && allow_proration {
            if let Some(last_paid) = subscription.last_payment_at {
                // Payment must fall within the current open period.
                if last_paid < subscription.next_payment_at && now < subscription.next_payment_at {
                    let secs_remaining = subscription.next_payment_at.saturating_sub(now);
                    let days_remaining = secs_remaining / 86_400;
                    let period_days = core::cmp::max(1, subscription.interval_secs / 86_400);

                    if days_remaining > 0 {
                        let prorated = subscription.amount.saturating_mul(days_remaining as i128)
                            / (period_days as i128);

                        if prorated > 0 {
                            Self::create_subscription_prorated_refund(
                                &env,
                                &subscription,
                                prorated,
                            )?;
                        }
                    }
                }
            }
        }

        subscription.status = SubscriptionStatus::Cancelled;
        env.storage().persistent().set(
            &DataKey::Subscription(subscription_id.clone()),
            &subscription,
        );

        // Stop future tick billing
        Self::remove_active_subscription(&env, &subscription_id);

        env.events().publish(
            (
                Symbol::new(&env, "SUBSCRIPTION"),
                Symbol::new(&env, "CANCELLED"),
            ),
            (subscription_id, payer_or_merchant),
        );

        Ok(())
    }

    /// Admin policy: enable or disable prorated refunds on subscription cancel.
    pub fn set_allow_prorated_refunds(env: Env, admin: Address, allow: bool) -> Result<(), Error> {
        admin.require_auth();
        if !AccessControl::has_role(&env, &role_admin(&env), &admin) {
            return Err(Error::Unauthorized);
        }
        env.storage()
            .persistent()
            .set(&DataKey::AllowProratedRefunds, &allow);
        Ok(())
    }

    /// Query whether prorated subscription refunds are allowed.
    pub fn get_allow_prorated_refunds(env: Env) -> bool {
        env.storage()
            .persistent()
            .get(&DataKey::AllowProratedRefunds)
            .unwrap_or(false)
    }

    /// Create a pending refund backed by a synthetic confirmed payment for the
    /// last subscription billing period. Emits `REFUND/AUTO_CREATED`.
    fn create_subscription_prorated_refund(
        env: &Env,
        subscription: &Subscription,
        refund_amount: i128,
    ) -> Result<String, Error> {
        let tick_id = Self::get_next_subscription_tick_id(env);
        let payment_id = format_id(env, "sub_pr_", tick_id);
        let now = env.ledger().timestamp();
        let last_paid = subscription.last_payment_at.unwrap_or(now);

        let payment = PaymentCharge {
            payment_id: payment_id.clone(),
            merchant_id: subscription.merchant_id.clone(),
            amount: subscription.amount,
            currency: subscription.currency.clone(),
            deposit_address: env.current_contract_address(),
            status: PaymentStatus::Confirmed,
            payer_address: Some(subscription.payer_address.clone()),
            transaction_hash: None,
            created_at: last_paid,
            confirmed_at: Some(last_paid),
            expires_at: subscription.next_payment_at,
            amount_received: Some(subscription.amount),
            memo: None,
            memo_type: None,
            token_address: None,
            metadata_hash: None,
            original_token: None,
            swap_path: None,
            fx_rate: None,
            fx_rate_at: None,
            metadata: None,
            fee_waiver_code: None,
            retry_of_payment_id: None,
            payer_muxed_id: None,
            payment_link_id: None,
        };

        env.storage()
            .persistent()
            .set(&DataKey::Payment(payment_id.clone()), &payment);
        Self::bump_payment_ttl(env, &payment_id, &payment.status);

        let counter = Self::get_next_refund_id(env);
        let refund_id = format_id(env, "refund_", counter);
        let reason = String::from_str(env, "Prorated subscription cancellation");

        let refund = Refund {
            refund_id: refund_id.clone(),
            payment_id: payment_id.clone(),
            amount: refund_amount,
            reason,
            status: RefundStatus::Pending,
            requester: subscription.payer_address.clone(),
            created_at: now,
            processed_at: None,
            approved: false,
            receipt_hash: None,
            expiry_at: now + REFUND_EXPIRY_SECS,
        };

        env.storage()
            .persistent()
            .set(&DataKey::Refund(refund_id.clone()), &refund);

        let mut payment_refunds = Self::get_payment_refunds_internal(env, &payment_id);
        payment_refunds.push_back(refund_id.clone());
        env.storage().persistent().set(
            &DataKey::PaymentRefunds(payment_id.clone()),
            &payment_refunds,
        );
        Self::bump_ttl(
            env,
            &DataKey::PaymentRefunds(payment_id.clone()),
            LONG_LIVE_TTL,
        );
        Self::bump_refund_ttl(env, &refund_id, &refund.status);

        env.events().publish(
            (Symbol::new(env, "REFUND"), Symbol::new(env, "AUTO_CREATED")),
            (
                payment_id,
                refund_id.clone(),
                refund_amount,
                subscription.subscription_id.clone(),
            ),
        );

        Ok(refund_id)
    }

    /// Admin override to reactivate a subscription that was cancelled due to max retries.
    /// Resets retry_count to 0 and reschedules the next payment.
    pub fn admin_reactivate_subscription(
        env: Env,
        admin: Address,
        subscription_id: String,
    ) -> Result<(), Error> {
        admin.require_auth();

        if !AccessControl::has_role(&env, &role_admin(&env), &admin) {
            return Err(Error::Unauthorized);
        }

        let mut subscription = Self::get_subscription_internal(&env, &subscription_id)?;

        if subscription.status != SubscriptionStatus::Cancelled {
            return Err(Error::PaymentAlreadyProcessed);
        }

        let now = env.ledger().timestamp();
        subscription.status = SubscriptionStatus::Active;
        subscription.retry_count = 0;
        subscription.next_retry_at = None;
        subscription.next_payment_at = now.saturating_add(subscription.interval_secs);

        env.storage().persistent().set(
            &DataKey::Subscription(subscription_id.clone()),
            &subscription,
        );

        // Issue #302: Add back to ActiveSubscriptions index
        Self::add_active_subscription(&env, &subscription_id);

        env.events().publish(
            (
                Symbol::new(&env, "SUBSCRIPTION"),
                Symbol::new(&env, "REACTIVATED"),
            ),
            (subscription_id, subscription.payer_address.clone()),
        );

        Ok(())
    }

    /// Submit usage metrics for a metered subscription.
    ///
    /// Operators call this to record usage units consumed since the last
    /// billing cycle. The subscription amount is scaled by
    /// `units_used * unit_price` and charged immediately via `charge_subscription`.
    ///
    /// # Parameters
    /// * `operator`         – Must hold oracle or settlement_operator role.
    /// * `subscription_id`  – Target subscription.
    /// * `units_used`       – Number of usage units consumed this period.
    /// * `unit_price`       – Price per unit in the subscription token's smallest unit.
    /// * `token`            – Token contract address used for the charge.
    pub fn submit_usage_metrics(
        env: Env,
        operator: Address,
        subscription_id: String,
        units_used: i128,
        unit_price: i128,
        token: Address,
    ) -> Result<SubscriptionStatus, Error> {
        operator.require_auth();

        if !AccessControl::has_role(&env, &role_oracle(&env), &operator)
            && !AccessControl::has_role(&env, &role_settlement_operator(&env), &operator)
        {
            return Err(Error::Unauthorized);
        }

        if units_used <= 0 || unit_price <= 0 {
            return Err(Error::InvalidAmount);
        }

        let mut subscription = Self::get_subscription_internal(&env, &subscription_id)?;

        // Issue #664: Usage metrics can only be submitted for a subscription
        // that is still billable (Active, or Paused-but-auto-resuming via
        // `charge_subscription` below). A Cancelled/Expired subscription
        // cannot be metered.
        if subscription.status == SubscriptionStatus::Cancelled
            || subscription.status == SubscriptionStatus::Expired
        {
            return Err(Error::InvalidStatusTransition);
        }

        // Override the subscription amount with the metered charge for this cycle.
        let metered_amount = units_used.saturating_mul(unit_price);
        subscription.amount = metered_amount;
        env.storage().persistent().set(
            &DataKey::Subscription(subscription_id.clone()),
            &subscription,
        );

        env.events().publish(
            (
                Symbol::new(&env, "SUBSCRIPTION"),
                Symbol::new(&env, "USAGE_RECORDED"),
            ),
            (
                subscription_id.clone(),
                units_used,
                unit_price,
                metered_amount,
            ),
        );

        // Issue #664: Append this usage record to the subscription's metrics
        // log so `get_usage_metrics` can return usage history over a range.
        let mut usage_log: Vec<UsageMetrics> = env
            .storage()
            .persistent()
            .get(&DataKey::UsageMetricsLog(subscription_id.clone()))
            .unwrap_or_else(|| vec![&env]);
        usage_log.push_back(UsageMetrics {
            subscription_id: subscription_id.clone(),
            units_used,
            unit_price,
            amount: metered_amount,
            recorded_at: env.ledger().timestamp(),
        });
        env.storage().persistent().set(
            &DataKey::UsageMetricsLog(subscription_id.clone()),
            &usage_log,
        );
        Self::bump_ttl(
            &env,
            &DataKey::UsageMetricsLog(subscription_id.clone()),
            LONG_LIVE_TTL,
        );

        // Trigger the charge at the updated metered amount.
        Self::charge_subscription(env, operator, subscription_id, token)
    }

    /// Issue #664: Return usage-metric records for `subscription_id`
    /// recorded within `[from_timestamp, to_timestamp]` (inclusive),
    /// oldest first. Returns an empty vector if none were recorded, or if
    /// the subscription itself doesn't exist.
    pub fn get_usage_metrics(
        env: Env,
        subscription_id: String,
        from_timestamp: u64,
        to_timestamp: u64,
    ) -> Vec<UsageMetrics> {
        let log: Vec<UsageMetrics> = env
            .storage()
            .persistent()
            .get(&DataKey::UsageMetricsLog(subscription_id))
            .unwrap_or_else(|| vec![&env]);

        let mut result = vec![&env];
        for record in log.iter() {
            if record.recorded_at >= from_timestamp && record.recorded_at <= to_timestamp {
                result.push_back(record.clone());
            }
        }
        result
    }

    /// Issue #633: Append a subscription ID to the per-plan subscriber index.
    /// Idempotent — a subscription ID already present is not added twice.
    fn add_plan_subscriber(env: &Env, plan_id: &String, subscription_id: &String) {
        let key = DataKey::PlanSubscribers(plan_id.clone());
        let mut subscribers: Vec<String> = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| vec![env]);
        for id in subscribers.iter() {
            if id == subscription_id.clone() {
                return;
            }
        }
        subscribers.push_back(subscription_id.clone());
        env.storage().persistent().set(&key, &subscribers);
        Self::bump_ttl(env, &key, LONG_LIVE_TTL);
    }

    /// Issue #302: Track subscription in the ActiveSubscriptions index
    fn add_active_subscription(env: &Env, subscription_id: &String) {
        let mut active: Vec<String> = env
            .storage()
            .persistent()
            .get(&DataKey::ActiveSubscriptions)
            .unwrap_or_else(|| vec![env]);
        let mut found = false;
        for id in active.iter() {
            if id == subscription_id.clone() {
                found = true;
                break;
            }
        }
        if !found {
            active.push_back(subscription_id.clone());
            env.storage()
                .persistent()
                .set(&DataKey::ActiveSubscriptions, &active);
            Self::bump_ttl(env, &DataKey::ActiveSubscriptions, LONG_LIVE_TTL);
        }
    }

    /// Issue #302: Remove subscription from the ActiveSubscriptions index
    fn remove_active_subscription(env: &Env, subscription_id: &String) {
        let active: Vec<String> = env
            .storage()
            .persistent()
            .get(&DataKey::ActiveSubscriptions)
            .unwrap_or_else(|| vec![env]);
        let mut updated = vec![env];
        for id in active.iter() {
            if id != subscription_id.clone() {
                updated.push_back(id);
            }
        }
        env.storage()
            .persistent()
            .set(&DataKey::ActiveSubscriptions, &updated);
        Self::bump_ttl(env, &DataKey::ActiveSubscriptions, LONG_LIVE_TTL);
    }

    /// Issue #302: Get the next subscription tick counter for payment IDs
    fn get_next_subscription_tick_id(env: &Env) -> u64 {
        let mut counter: u64 = env
            .storage()
            .persistent()
            .get(&DataKey::SubscriptionTickCounter)
            .unwrap_or(0);
        counter += 1;
        env.storage()
            .persistent()
            .set(&DataKey::SubscriptionTickCounter, &counter);
        counter
    }

    /// Issue #302: Process due subscriptions - iterate ActiveSubscriptions and
    /// create a payment record for each due subscription.
    pub fn process_due_subscriptions(env: Env, operator: Address) -> Result<u32, Error> {
        operator.require_auth();

        if !AccessControl::has_role(&env, &role_oracle(&env), &operator)
            && !AccessControl::has_role(&env, &role_settlement_operator(&env), &operator)
        {
            return Err(Error::Unauthorized);
        }

        let active: Vec<String> = env
            .storage()
            .persistent()
            .get(&DataKey::ActiveSubscriptions)
            .unwrap_or_else(|| vec![&env]);

        let now = env.ledger().timestamp();
        let mut total_payments: u32 = 0;

        for subscription_id in active.iter() {
            let mut subscription = match env
                .storage()
                .persistent()
                .get::<DataKey, Subscription>(&DataKey::Subscription(subscription_id.clone()))
            {
                Some(s) => s,
                None => continue,
            };

            if subscription.status != SubscriptionStatus::Active {
                continue;
            }

            if now < subscription.next_payment_at {
                continue;
            }

            let tick_id = Self::get_next_subscription_tick_id(&env);
            let payment_id = format_id(&env, "sub_tick_", tick_id);

            let payment = PaymentCharge {
                payment_id: payment_id.clone(),
                merchant_id: subscription.merchant_id.clone(),
                amount: subscription.amount,
                currency: subscription.currency.clone(),
                deposit_address: env.current_contract_address(),
                status: PaymentStatus::Pending,
                payer_address: Some(subscription.payer_address.clone()),
                transaction_hash: None,
                created_at: now,
                confirmed_at: None,
                expires_at: now.saturating_add(DEFAULT_PAYMENT_DURATION_SECS),
                amount_received: None,
                memo: None,
                memo_type: None,
                token_address: None,
                metadata_hash: None,
                original_token: None,
                swap_path: None,
                fx_rate: None,
                fx_rate_at: None,
                metadata: None,
                fee_waiver_code: None,
                retry_of_payment_id: None,
                payer_muxed_id: None,
                payment_link_id: None,
            };

            env.storage()
                .persistent()
                .set(&DataKey::Payment(payment_id.clone()), &payment);
            Self::bump_payment_ttl(&env, &payment_id, &payment.status);

            subscription.last_payment_at = Some(now);
            subscription.total_payments = subscription.total_payments.saturating_add(1);
            subscription.next_payment_at = now.saturating_add(subscription.interval_secs);

            env.events().publish(
                (
                    Symbol::new(&env, "SUBSCRIPTION"),
                    Symbol::new(&env, "TICKED"),
                ),
                (
                    subscription_id.clone(),
                    subscription.payer_address.clone(),
                    subscription.amount,
                    subscription.total_payments,
                ),
            );

            if let Some(max) = subscription.max_payments {
                if subscription.total_payments >= max {
                    subscription.status = SubscriptionStatus::Cancelled;
                    Self::remove_active_subscription(&env, &subscription_id);

                    env.events().publish(
                        (
                            Symbol::new(&env, "SUBSCRIPTION"),
                            Symbol::new(&env, "COMPLETED"),
                        ),
                        (
                            subscription_id.clone(),
                            subscription.payer_address.clone(),
                            subscription.total_payments,
                        ),
                    );

                    env.events().publish(
                        (
                            Symbol::new(&env, "SUBSCRIPTION"),
                            Symbol::new(&env, "CANCELLED"),
                        ),
                        (subscription_id.clone(), subscription.payer_address.clone()),
                    );
                }
            }

            env.storage().persistent().set(
                &DataKey::Subscription(subscription_id.clone()),
                &subscription,
            );

            env.events().publish(
                (Symbol::new(&env, "PAYMENT"), Symbol::new(&env, "CREATED")),
                (
                    payment_id.clone(),
                    subscription.merchant_id.clone(),
                    subscription.amount,
                    None::<Map<String, String>>,
                ),
            );

            total_payments = total_payments.saturating_add(1);
        }

        Ok(total_payments)
    }

    fn get_next_subscription_id(env: &Env) -> u64 {
        let mut counter: u64 = env
            .storage()
            .persistent()
            .get(&DataKey::SubscriptionCounter)
            .unwrap_or(0);
        counter += 1;
        env.storage()
            .persistent()
            .set(&DataKey::SubscriptionCounter, &counter);
        counter
    }

    fn get_subscription_internal(
        env: &Env,
        subscription_id: &String,
    ) -> Result<Subscription, Error> {
        env.storage()
            .persistent()
            .get(&DataKey::Subscription(subscription_id.clone()))
            .ok_or(Error::PaymentNotFound)
    }

    fn get_payer_subscriptions_internal(env: &Env, payer: &Address) -> Vec<String> {
        env.storage()
            .persistent()
            .get(&DataKey::PayerSubscriptions(payer.clone()))
            .unwrap_or_else(|| vec![env])
    }

    fn refund_ttl(status: &RefundStatus) -> u32 {
        match status {
            RefundStatus::Pending => SHORT_LIVE_TTL,
            RefundStatus::Completed | RefundStatus::Rejected | RefundStatus::Cancelled => {
                LONG_LIVE_TTL
            }
        }
    }

    fn bump_refund_ttl(env: &Env, refund_id: &String, status: &RefundStatus) {
        let key = DataKey::Refund(refund_id.clone());
        Self::bump_ttl(env, &key, Self::refund_ttl(status));
    }

    fn dispute_ttl(status: &DisputeStatus) -> u32 {
        match status {
            DisputeStatus::Open | DisputeStatus::UnderReview => SHORT_LIVE_TTL,
            DisputeStatus::Resolved | DisputeStatus::Rejected => LONG_LIVE_TTL,
        }
    }

    fn bump_dispute_ttl(env: &Env, dispute_id: &String, status: &DisputeStatus) {
        let key = DataKey::Dispute(dispute_id.clone());
        Self::bump_ttl(env, &key, Self::dispute_ttl(status));
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

    /// Queue a contract WASM upgrade via the timelock.
    ///
    /// Issue #624: `upgrade_contract` no longer takes effect immediately.  The
    /// upgrade is queued as a `PendingTimelockAction` and can only be executed
    /// after the configured delay (default 48 hours) via
    /// `execute_timelocked_action`.  Returns the action ID.
    pub fn upgrade_contract(
        env: Env,
        admin: Address,
        new_wasm_hash: BytesN<32>,
    ) -> Result<String, Error> {
        admin.require_auth();

        if !AccessControl::has_role(&env, &role_admin(&env), &admin) {
            return Err(Error::Unauthorized);
        }

        PaymentProcessor::enqueue_timelocked_action(
            &env,
            admin,
            TimelockActionKind::UpgradeContract(new_wasm_hash),
        )
    }
}
