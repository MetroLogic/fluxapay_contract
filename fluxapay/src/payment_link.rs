use soroban_sdk::{
    contract, contractimpl, contracttype, token, vec, Address, BytesN, Env, Map, MuxedAddress, String,
    Symbol, Vec,
};

use crate::utils;
use crate::{format_id, PaymentCharge, PaymentStatus};

/// Multi-currency fiat configuration for payment links (issue #413).
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FiatConfig {
    pub amount: i128,
    pub currency: Symbol,
    pub oracle: Address,
}

/// Nullable wrapper for FiatConfig.
///
/// Soroban's `#[contracttype]` macro does not support `Option<T>` when `T`
/// is itself a `#[contracttype]` struct (because structs implement `TryFrom`
/// rather than `From` for `ScVal`). Using an enum is the idiomatic pattern.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MaybeFiatConfig {
    None,
    Some(FiatConfig),
}

impl MaybeFiatConfig {
    pub fn as_option(&self) -> Option<&FiatConfig> {
        match self {
            MaybeFiatConfig::Some(ref c) => Some(c),
            MaybeFiatConfig::None => None,
        }
    }

    pub fn into_option(self) -> Option<FiatConfig> {
        match self {
            MaybeFiatConfig::Some(c) => Some(c),
            MaybeFiatConfig::None => None,
        }
    }
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaymentLink {
    pub link_id: String,
    pub merchant_id: Address,
    pub amount: Option<i128>,
    pub currency: Symbol,
    pub description: String,
    pub expires_at: Option<u64>,
    pub max_uses: Option<u32>,
    pub use_count: u32,
    /// Number of times this link has been viewed (incremented by `record_link_view`).
    pub view_count: u32,
    /// Total revenue (in USDC stroops) accumulated from successful `use_link` calls.
    pub total_revenue: i128,
    pub active: bool,
    /// If true, funds are transferred directly to the merchant wallet on use_link,
    /// bypassing the escrow/platform wallet (issue #111).
    pub direct_transfer: bool,
    /// Optional metadata attached to this payment link.
    pub metadata: Option<Map<String, String>>,
    /// Fiat configuration for multi-currency invoicing (issue #413).
    pub fiat: MaybeFiatConfig,
    /// Canonical shareable checkout URL: `{base_url}/pay/{link_id}`.
    pub shareable_url: Option<String>,
    /// Issue #663: Optional per-link fee override (basis points, 0-10_000).
    /// When `None`, `use_link` falls back to the contract-wide default set
    /// via `set_payment_link_fee_bps(admin, None, bps)`. Only settable by
    /// the admin via `set_payment_link_fee_bps`.
    pub fee_bps: Option<i128>,
}

/// Analytics summary for a payment link.
///
/// Returned by `get_link_analytics`. `conversion_rate` is expressed in
/// basis points (bps): `(use_count * 10_000) / view_count`, or `0` when
/// the link has not been viewed yet.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LinkAnalytics {
    pub view_count: u32,
    pub use_count: u32,
    pub total_revenue: i128,
    /// Conversion rate in basis points (bps). 100 bps = 1%.
    pub conversion_rate: u32,
}

#[contracttype]
pub enum LinkDataKey {
    Link(String),
    LinkAdmin,
    /// List of payment IDs generated from a link
    LinkPayments(String),
    /// Individual payment charge created from a link
    LinkPayment(String),
    /// Admin-configured default base URL for shareable payment links.
    PaymentBaseUrl,
    /// Issue #663: Contract-wide default fee (basis points) applied by
    /// `use_link` to links that don't have their own `fee_bps` override.
    GlobalFeeBps,
}

#[contract]
pub struct PaymentLinkManager;

#[cfg_attr(
    any(not(target_arch = "wasm32"), feature = "contract-payment-link"),
    contractimpl
)]
#[allow(deprecated)] // events::publish — migrate to #[contractevent] in a follow-up
impl PaymentLinkManager {
    pub fn version() -> u32 {
        1
    }

    /// Initialize the contract with an admin address.
    pub fn initialize(env: Env, admin: Address) {
        env.storage()
            .persistent()
            .set(&LinkDataKey::LinkAdmin, &admin);
    }

    /// Upgrade the contract WASM.
    ///
    /// Only the admin can call this. Emits a `CONTRACT/UPGRADED` event on success.
    pub fn upgrade(env: Env, admin: Address, new_wasm_hash: BytesN<32>) -> Result<(), crate::Error> {
        admin.require_auth();

        let stored_admin: Address = env
            .storage()
            .persistent()
            .get(&LinkDataKey::LinkAdmin)
            .ok_or(crate::Error::Unauthorized)?;

        if admin != stored_admin {
            return Err(crate::Error::Unauthorized);
        }

        let old_version = String::from_str(&env, "1.0.0");
        env.deployer().update_current_contract_wasm(new_wasm_hash);

        env.events().publish(
            (Symbol::new(&env, "CONTRACT"), Symbol::new(&env, "UPGRADED")),
            (old_version.clone(), String::from_str(&env, "1.0.1")),
        );

        Ok(())
    }

    /// Set the default base URL used when `create_link` omits `base_url`.
    pub fn set_payment_base_url(
        env: Env,
        admin: Address,
        url: String,
    ) -> Result<(), crate::Error> {
        admin.require_auth();

        let stored_admin: Address = env
            .storage()
            .persistent()
            .get(&LinkDataKey::LinkAdmin)
            .ok_or(crate::Error::Unauthorized)?;

        if admin != stored_admin {
            return Err(crate::Error::Unauthorized);
        }

        env.storage()
            .persistent()
            .set(&LinkDataKey::PaymentBaseUrl, &url);
        Ok(())
    }

    /// Return the admin-configured default payment base URL, if any.
    pub fn get_payment_base_url(env: Env) -> Option<String> {
        env.storage()
            .persistent()
            .get(&LinkDataKey::PaymentBaseUrl)
    }

    /// Issue #663: Set a per-link or contract-wide default fee override
    /// (basis points) for payment links, following the admin-gated
    /// setter pattern used by `set_fee_rate`/`set_refund_fee_bps`.
    ///
    /// * `link_id = Some(id)` — sets (or clears, when `fee_bps` is `None`)
    ///   a fee override on that specific link. Only the admin may call
    ///   this, so custom per-link fees cannot be self-assigned by a
    ///   merchant at link-creation time.
    /// * `link_id = None` — sets (or clears) the contract-wide default fee
    ///   applied by `use_link` to any link without its own override.
    ///
    /// `fee_bps`, when `Some`, must be in `0..=10_000` (0-100%).
    pub fn set_payment_link_fee_bps(
        env: Env,
        admin: Address,
        link_id: Option<String>,
        fee_bps: Option<i128>,
    ) -> Result<(), crate::Error> {
        admin.require_auth();

        let stored_admin: Address = env
            .storage()
            .persistent()
            .get(&LinkDataKey::LinkAdmin)
            .ok_or(crate::Error::Unauthorized)?;

        if admin != stored_admin {
            return Err(crate::Error::Unauthorized);
        }

        if let Some(bps) = fee_bps {
            if !(0..=10_000).contains(&bps) {
                return Err(crate::Error::InvalidAmount);
            }
        }

        match link_id {
            Some(id) => {
                let mut link = Self::get_link_internal(&env, &id)?;
                link.fee_bps = fee_bps;
                env.storage()
                    .persistent()
                    .set(&LinkDataKey::Link(id.clone()), &link);

                env.events().publish(
                    (Symbol::new(&env, "LINK"), Symbol::new(&env, "FEE_BPS_SET")),
                    (id, fee_bps),
                );
            }
            None => {
                env.storage()
                    .persistent()
                    .set(&LinkDataKey::GlobalFeeBps, &fee_bps);

                env.events().publish(
                    (
                        Symbol::new(&env, "LINK"),
                        Symbol::new(&env, "GLOBAL_FEE_BPS_SET"),
                    ),
                    fee_bps,
                );
            }
        }

        Ok(())
    }

    /// Issue #663: Returns the fee (basis points) that `use_link` would
    /// apply to `link_id` right now: the link's own override if set,
    /// otherwise the contract-wide default, otherwise `None` (no fee).
    pub fn get_effective_fee_bps(env: Env, link_id: String) -> Result<Option<i128>, crate::Error> {
        let link = Self::get_link_internal(&env, &link_id)?;
        if link.fee_bps.is_some() {
            return Ok(link.fee_bps);
        }
        Ok(env
            .storage()
            .persistent()
            .get(&LinkDataKey::GlobalFeeBps)
            .unwrap_or(None))
    }

    #[allow(clippy::too_many_arguments)]
    pub fn create_link(
        env: Env,
        merchant: Address,
        link_id: String,
        amount: Option<i128>,
        currency: Symbol,
        description: String,
        expires_at: Option<u64>,
        max_uses: Option<u32>,
        direct_transfer: bool,
        metadata: Option<Map<String, String>>,
        fiat: MaybeFiatConfig,
        base_url: Option<String>,
    ) -> Result<String, crate::Error> {
        merchant.require_auth();

        if !crate::utils::validate_id(&link_id) {
            return Err(crate::Error::InvalidPaymentId);
        }
        if let Some(ref meta_map) = metadata {
            utils::validate_metadata(meta_map)?;
        }

        let resolved_base = base_url.or_else(|| {
            env.storage()
                .persistent()
                .get(&LinkDataKey::PaymentBaseUrl)
        });

        let shareable_url = resolved_base.map(|base| {
            utils::concat_strings(
                &env,
                &[
                    base,
                    String::from_str(&env, "/pay/"),
                    link_id.clone(),
                ],
            )
        });

        let link = PaymentLink {
            link_id: link_id.clone(),
            merchant_id: merchant.clone(),
            amount,
            currency,
            description,
            expires_at,
            max_uses,
            use_count: 0,
            view_count: 0,
            total_revenue: 0,
            active: true,
            direct_transfer,
            metadata,
            fiat,
            shareable_url,
            fee_bps: None,
        };

        env.storage()
            .persistent()
            .set(&LinkDataKey::Link(link_id.clone()), &link);

        // Emit LINK/CREATED event
        env.events().publish(
            (Symbol::new(&env, "LINK"), Symbol::new(&env, "CREATED")),
            (link_id.clone(), merchant),
        );

        Ok(link_id)
    }

    /// Return the shareable URL for a link, if one was stored.
    pub fn get_link_url(env: Env, link_id: String) -> Option<String> {
        Self::get_link_internal(&env, &link_id)
            .ok()
            .and_then(|link| link.shareable_url)
    }

    /// Record a view of a payment link.
    ///
    /// This is a permissionless entry point — any caller may increment the
    /// `view_count` for an active link. Merchants use this (typically from
    /// their storefront or checkout page) to track how many people viewed
    /// the link versus how many actually paid, enabling conversion-rate
    /// analytics via `get_link_analytics`.
    pub fn record_link_view(env: Env, link_id: String) -> Result<(), crate::Error> {
        let mut link = Self::get_link_internal(&env, &link_id)?;

        if !link.active {
            return Err(crate::Error::Unauthorized);
        }

        link.view_count = link.view_count.saturating_add(1);
        env.storage()
            .persistent()
            .set(&LinkDataKey::Link(link_id.clone()), &link);

        // Emit LINK/VIEWED event
        env.events().publish(
            (Symbol::new(&env, "LINK"), Symbol::new(&env, "VIEWED")),
            link_id,
        );

        Ok(())
    }

    pub fn use_link(
        env: Env,
        payer: Address,
        link_id: String,
        amount: i128,
        usdc_token: Option<Address>,
    ) -> Result<String, crate::Error> {
        payer.require_auth();

        let mut link = Self::get_link_internal(&env, &link_id)?;

        if !link.active {
            return Err(crate::Error::Unauthorized);
        }

        if let Some(expires_at) = link.expires_at {
            if env.ledger().timestamp() > expires_at {
                return Err(crate::Error::LinkExpired);
            }
        }

        if let Some(max_uses) = link.max_uses {
            if link.use_count >= max_uses {
                return Err(crate::Error::LinkMaxUsesReached);
            }
        }

        // Resolve the effective USDC amount:
        // - If fiat config is set, compute USDC equivalent via the FX oracle
        // - Otherwise use the caller-supplied amount (validated against link.amount if fixed)
        let resolved_amount = if let MaybeFiatConfig::Some(ref fiat_cfg) = link.fiat {
            let oracle_client = crate::fx_oracle::FXOracleClient::new(&env, &fiat_cfg.oracle);
            let rate_data = oracle_client
                .try_get_rate(&fiat_cfg.currency)
                .map_err(|_| crate::Error::StaleOracleRate)?
                .map_err(|_| crate::Error::StaleOracleRate)?;

            // Oracle rate represents X units of fiat per 1 USDC at the given decimals.
            // USDC amount = fiat_amount * 10^decimals / rate
            let mut divisor = 1i128;
            for _ in 0..rate_data.decimals {
                divisor = divisor.saturating_mul(10);
            }
            let usdc_equivalent = fiat_cfg.amount.saturating_mul(divisor) / rate_data.rate;

            // If the link also has a fixed USDC amount, validate against it
            if let Some(fixed_amount) = link.amount {
                if usdc_equivalent != fixed_amount {
                    return Err(crate::Error::InvalidAmount);
                }
            }

            // Validate that the payer sent the correct USDC amount
            if amount != usdc_equivalent {
                return Err(crate::Error::InvalidAmount);
            }

            usdc_equivalent
        } else {
            // Standard USDC-denominated link: validate against fixed amount if set
            if let Some(fixed_amount) = link.amount {
                if amount != fixed_amount {
                    return Err(crate::Error::InvalidAmount);
                }
            } else if amount <= 0 {
                return Err(crate::Error::InvalidAmount);
            }
            amount
        };

        // Atomic read-check-increment: use_count was checked above against the
        // in-memory snapshot; the single storage write below commits the bump
        // in the same transaction (Soroban tx isolation prevents mid-tx races).
        link.use_count = link.use_count.saturating_add(1);
        let hit_max_uses = link
            .max_uses
            .map(|m| link.use_count == m)
            .unwrap_or(false);

        // Accumulate revenue from this payment.
        link.total_revenue = link.total_revenue.saturating_add(resolved_amount);
        env.storage()
            .persistent()
            .set(&LinkDataKey::Link(link_id.clone()), &link);

        if hit_max_uses {
            env.events().publish(
                (
                    Symbol::new(&env, "LINK"),
                    Symbol::new(&env, "MAX_USES_REACHED"),
                ),
                (link_id.clone(), link.use_count, link.max_uses),
            );
        }

        // Issue #663: Resolve the effective fee (link override, else the
        // contract-wide default, else no fee) and compute the fee amount.
        let effective_fee_bps: Option<i128> = if link.fee_bps.is_some() {
            link.fee_bps
        } else {
            env.storage()
                .persistent()
                .get(&LinkDataKey::GlobalFeeBps)
                .unwrap_or(None)
        };
        let fee_amount: i128 = match effective_fee_bps {
            Some(bps) if bps > 0 => resolved_amount.saturating_mul(bps) / 10_000,
            _ => 0,
        };

        // Issue #111: If direct_transfer is true, transfer funds directly to the merchant,
        // bypassing the escrow/platform wallet. Issue #663: the configured link fee (if
        // any) is deducted here and routed to the link admin.
        if link.direct_transfer {
            let token_address = usdc_token.clone().ok_or(crate::Error::Unauthorized)?;
            let token_client = token::TokenClient::new(&env, &token_address);
            let merchant_muxed: MuxedAddress = (&link.merchant_id).into();
            let net_amount = resolved_amount.saturating_sub(fee_amount);
            token_client.transfer(&payer, &merchant_muxed, &net_amount);

            if fee_amount > 0 {
                if let Some(fee_admin) = env
                    .storage()
                    .persistent()
                    .get::<LinkDataKey, Address>(&LinkDataKey::LinkAdmin)
                {
                    token_client.transfer(&payer, &fee_admin, &fee_amount);
                }

                env.events().publish(
                    (Symbol::new(&env, "LINK"), Symbol::new(&env, "FEE_APPLIED")),
                    (link_id.clone(), fee_amount, effective_fee_bps),
                );
            }
        }
        // Emit LINK/DIRECT_TRANSFER_USED event for audit trail when direct transfer is used
        if link.direct_transfer {
            env.events().publish(
                (Symbol::new(&env, "LINK"), Symbol::new(&env, "DIRECT_TRANSFER_USED")),
                (link_id.clone(), payer.clone(), resolved_amount),
            );
        }

        // Generate a globally unique payment ID: combine ledger timestamp with the
        // post-increment use_count so multiple use_link calls within the same ledger
        // (same timestamp) never collide.
        let unique_id = (env.ledger().timestamp() as u128)
            .saturating_mul(1_000_000)
            .saturating_add(link.use_count as u128) as u64;
        let payment_id = format_id(&env, "lnk_pay_", unique_id);

        // Create and store a PaymentCharge record for this payment
        let now = env.ledger().timestamp();
        let payment = PaymentCharge {
            payment_id: payment_id.clone(),
            merchant_id: link.merchant_id.clone(),
            amount: resolved_amount,
            currency: link.currency.clone(),
            deposit_address: env.current_contract_address(),
            status: PaymentStatus::Pending,
            payer_address: Some(payer.clone()),
            transaction_hash: None,
            created_at: now,
            confirmed_at: None,
            expires_at: now.saturating_add(crate::DEFAULT_PAYMENT_DURATION_SECS),
            amount_received: None,
            memo: None,
            memo_type: None,
            token_address: usdc_token,
            metadata_hash: None,
            original_token: None,
            swap_path: None,
            fx_rate: None,
            fx_rate_at: None,
            metadata: link.metadata.clone(),
            fee_waiver_code: None,
            retry_of_payment_id: None,
            payer_muxed_id: None,
            // Issue #668: trace this payment back to the link that created it.
            payment_link_id: Some(link_id.clone()),
        };

        // Store the payment charge
        env.storage()
            .persistent()
            .set(&LinkDataKey::LinkPayment(payment_id.clone()), &payment);
        // If this is a direct transfer payment, mark it in the main contract storage
        // to prevent future disputes (issue #485)
        if link.direct_transfer {
            env.storage()
                .persistent()
                .set(&crate::DataKey::DirectTransferPayment(payment_id.clone()), &true);
        }

        // Track payment ID in the link's payment list
        let mut payment_ids: Vec<String> = env
            .storage()
            .persistent()
            .get(&LinkDataKey::LinkPayments(link_id.clone()))
            .unwrap_or_else(|| vec![&env]);
        payment_ids.push_back(payment_id.clone());
        env.storage()
            .persistent()
            .set(&LinkDataKey::LinkPayments(link_id.clone()), &payment_ids);

        // Emit LINK/USED event with the resolved USDC amount and metadata
        env.events().publish(
            (Symbol::new(&env, "LINK"), Symbol::new(&env, "USED")),
            (link_id, payer, resolved_amount, payment_id.clone(), link.metadata.clone()),
        );

        Ok(payment_id)
    }

    /// Get a payment charge created from a payment link.
    /// Returns the PaymentCharge record for the given payment_id.
    pub fn get_payment(env: Env, payment_id: String) -> Result<PaymentCharge, crate::Error> {
        env.storage()
            .persistent()
            .get(&LinkDataKey::LinkPayment(payment_id))
            .ok_or(crate::Error::PaymentNotFound)
    }

    /// Get all payment IDs generated from a specific payment link.
    /// Returns a vector of payment IDs in chronological order.
    pub fn get_link_payments(env: Env, link_id: String) -> Result<Vec<String>, crate::Error> {
        Ok(env
            .storage()
            .persistent()
            .get(&LinkDataKey::LinkPayments(link_id))
            .unwrap_or_else(|| vec![&env]))
    }

    pub fn deactivate_link(
        env: Env,
        merchant: Address,
        link_id: String,
    ) -> Result<(), crate::Error> {
        merchant.require_auth();

        let mut link = Self::get_link_internal(&env, &link_id)?;

        if link.merchant_id != merchant {
            return Err(crate::Error::Unauthorized);
        }

        link.active = false;
        env.storage()
            .persistent()
            .set(&LinkDataKey::Link(link_id.clone()), &link);

        // Emit LINK/DEACTIVATED event
        env.events().publish(
            (Symbol::new(&env, "LINK"), Symbol::new(&env, "DEACTIVATED")),
            link_id,
        );

        Ok(())
    }

    /// Permissionless entry point that deactivates an expired payment link.
    /// If the link has not expired or is already inactive, the call is idempotent
    /// (succeeds without changing state). Emits LINK/EXPIRED on actual deactivation.
    pub fn expire_link(env: Env, link_id: String) -> Result<(), crate::Error> {
        let mut link = match Self::get_link_internal(&env, &link_id) {
            Ok(link) => link,
            Err(_) => return Ok(()), // Idempotent: missing link is not an error.
        };

        if !link.active {
            return Ok(()); // Already inactive — nothing to do.
        }

        let expired = link
            .expires_at
            .map_or(false, |exp| env.ledger().timestamp() > exp);

        if !expired {
            return Ok(()); // Not expired — nothing to do.
        }

        link.active = false;
        env.storage()
            .persistent()
            .set(&LinkDataKey::Link(link_id.clone()), &link);

        env.events().publish(
            (Symbol::new(&env, "LINK"), Symbol::new(&env, "EXPIRED")),
            link_id,
        );

        Ok(())
    }

    /// Batch deactivate expired payment links (max 20 per call).
    /// Iterates over the provided link IDs and calls expire_link for each.
    /// Returns the count of links that were actually deactivated.
    pub fn batch_expire_links(env: Env, link_ids: Vec<String>) -> Result<u32, crate::Error> {
        if link_ids.len() > 20 {
            return Err(crate::Error::BatchTooLarge);
        }

        let mut deactivated: u32 = 0;
        for link_id in link_ids.iter() {
            // Use expire_link directly to emit individual events.
            if let Ok(()) = Self::expire_link(env.clone(), link_id.clone()) {
                // Check if the link was actually deactivated by reading it back.
                if let Ok(link) = Self::get_link_internal(&env, &link_id) {
                    if !link.active && link.expires_at.map_or(false, |exp| env.ledger().timestamp() > exp) {
                        deactivated += 1;
                    }
                }
            }
        }

        Ok(deactivated)
    }

    pub fn get_link(env: Env, link_id: String) -> Result<PaymentLink, crate::Error> {
        Self::get_link_internal(&env, &link_id)
    }

    /// Retrieve a payment link, automatically deactivating it if expired.
    /// Returns the link with `active: false` when the expiry timestamp has passed,
    /// even if `expire_link` has not been called explicitly.
    fn get_link_internal(env: &Env, link_id: &String) -> Result<PaymentLink, crate::Error> {
        let mut link: PaymentLink = env
            .storage()
            .persistent()
            .get(&LinkDataKey::Link(link_id.clone()))
            .ok_or(crate::Error::PaymentNotFound)?;

        // Auto-deactivate expired links on read.
        if link.active {
            if let Some(expires_at) = link.expires_at {
                if env.ledger().timestamp() > expires_at {
                    link.active = false;
                    env.storage()
                        .persistent()
                        .set(&LinkDataKey::Link(link_id.clone()), &link);
                }
            }
        }

        Ok(link)
    }

    /// Retrieve analytics for a payment link.
    ///
    /// Returns view_count, use_count, total_revenue, and conversion_rate
    /// (in basis points: `use_count * 10_000 / view_count`, or `0` if
    /// the link has not been viewed).
    pub fn get_link_analytics(env: Env, link_id: String) -> Result<LinkAnalytics, crate::Error> {
        let link = Self::get_link_internal(&env, &link_id)?;

        let conversion_rate = if link.view_count > 0 {
            (link.use_count as u32).saturating_mul(10_000) / link.view_count
        } else {
            0
        };

        Ok(LinkAnalytics {
            view_count: link.view_count,
            use_count: link.use_count,
            total_revenue: link.total_revenue,
            conversion_rate,
        })
    }

    /// Verify the status of multiple payment links in a single call.
    /// Returns a vector of (link_id, is_active, use_count, max_uses) tuples.
    pub fn verify_batch(env: Env, link_ids: Vec<String>) -> Vec<(String, bool, u32, Option<u32>)> {
        let mut results = vec![&env];
        for link_id in link_ids.iter() {
            match Self::get_link_internal(&env, &link_id) {
                Ok(link) => {
                    results.push_back((
                        link_id.clone(),
                        link.active,
                        link.use_count,
                        link.max_uses,
                    ));
                }
                Err(_) => {
                    // Link not found - return inactive status
                    results.push_back((link_id.clone(), false, 0, None));
                }
            }
        }
        results
    }
}
