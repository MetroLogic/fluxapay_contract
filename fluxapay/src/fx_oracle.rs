use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, Address, BytesN, Env, String, Symbol,
    SymbolStr, TryFromVal, Vec,
};

use crate::access_control::{role_admin, role_oracle, AccessControl};

/// Maximum allowed age of a rate in seconds, regardless of admin-configured threshold.
const MAX_RATE_AGE_SECS: u64 = 86_400; // 24 hours
const MIN_RATE_AGE_SECS: u64 = 60;

/// Maximum ledger sequence gap since last rate update (~24 h at ~5 s/ledger).
const MAX_LEDGER_GAP: u32 = 17_280;

/// Maximum number of currency pairs accepted by `set_rates_batch`.
const MAX_BATCH_RATES: u32 = 20;

/// Issue #636: Fixed-point precision of the reciprocal rate returned by
/// `get_rate_or_inverse` when it falls back to the inverse pair. 14 decimals
/// keeps ~7 significant digits of precision for reciprocals of rates that
/// themselves use up to 7 decimals.
const INVERSE_RATE_DECIMALS: u32 = 14;

/// Issue #636: Maximum length of a `BASE_QUOTE` pair symbol (matches the
/// Soroban `Symbol` limit of 32 characters).
const MAX_PAIR_SYMBOL_LEN: usize = 32;

/// Issue #811: Hard cap on the token allowlist.
///
/// Bounds the linear scan every `set_rate` performs. Without a cap, an admin
/// could grow the list until the scan alone exhausted the instruction budget —
/// which would brick rate publishing rather than merely slow it.
const MAX_ALLOWED_TOKENS: u32 = 50;

#[contract]
pub struct FXOracle;

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RateData {
    pub pair: Symbol,
    pub rate: i128,
    pub decimals: u32,
    pub updated_at: u64,
    pub updated_sequence: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OracleSubmission {
    pub operator: Address,
    pub rate: i128,
    pub decimals: u32,
}

#[contracterror]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FXOracleError {
    RateNotFound = 1,
    RateStale = 2,
    Unauthorized = 3,
    /// Batch rate update exceeds the maximum of 20 pairs.
    BatchTooLarge = 4,
    /// Issue #478: Rate deviation exceeds configured limit
    RateDeviationExceeded = 5,
    /// Issue #636: Neither the requested `BASE_QUOTE` pair nor its inverse
    /// `QUOTE_BASE` has a stored rate (or the pair symbol is malformed).
    PairNotFound = 6,
    InvalidStalenessThreshold = 7,
    /// Issue #851: `set_rates_batch` was called with an empty rate list.
    EmptyBatch = 8,
    /// Issue #811: Pair symbol is malformed, or a token is outside the allowlist.
    TokenNotAllowed = 9,
}

#[contracttype]
pub enum OracleDataKey {
    Rate(Symbol),
    StalenessThreshold,
    /// Issue #478: Maximum allowed rate deviation per pair in basis points
    MaxDeviation(Symbol),
    Quorum,
    Submissions(Symbol),
    /// Issue #811: `Vec<Symbol>` of tokens a rate may be published against.
    AllowedTokens,
}

#[cfg_attr(
    any(not(target_arch = "wasm32"), feature = "contract-fx-oracle"),
    contractimpl
)]
#[allow(deprecated)] // events::publish — migrate to #[contractevent] in a follow-up
impl FXOracle {
    pub fn version() -> u32 {
        1
    }

    pub fn oracle_initialize(env: Env, admin: Address, staleness_threshold: u64) {
        let empty = Vec::new(&env);
        Self::oracle_initialize_with_tokens(env, admin, staleness_threshold, empty);
    }

    /// Issue #811: Initialise with a populated token allowlist.
    ///
    /// Separate entry point rather than a changed signature on
    /// `oracle_initialize`, which is called from deployment scripts, tests and
    /// the SDK. Breaking all of them to add an argument most callers can supply
    /// afterwards via `add_allowed_token` would be a poor trade.
    ///
    /// An **empty** allowlist means "not yet configured", and rate publishing is
    /// allowed through — see `require_pair_allowed` for why that default is the
    /// safe one here rather than the reverse.
    pub fn oracle_initialize_with_tokens(
        env: Env,
        admin: Address,
        staleness_threshold: u64,
        allowed_tokens: Vec<Symbol>,
    ) {
        AccessControl::initialize(&env, admin);
        let threshold = staleness_threshold.clamp(MIN_RATE_AGE_SECS, MAX_RATE_AGE_SECS);
        env.storage()
            .instance()
            .set(&OracleDataKey::StalenessThreshold, &threshold);
        // An empty list stays "not configured" and permits every pair.
        env.storage()
            .instance()
            .set(&OracleDataKey::AllowedTokens, &allowed_tokens);
    }

    pub fn oracle_grant_role(
        env: Env,
        admin: Address,
        role: Symbol,
        account: Address,
    ) -> Result<(), FXOracleError> {
        AccessControl::grant_role(&env, admin, role, account)
            .map_err(|_| FXOracleError::Unauthorized)
    }

    pub fn oracle_has_role(env: Env, role: Symbol, account: Address) -> bool {
        AccessControl::has_role(&env, &role, &account)
    }

    pub fn get_fx_admin(env: Env) -> Option<Address> {
        AccessControl::get_admin(&env)
    }

    pub fn set_rate(
        env: Env,
        operator: Address,
        pair: Symbol,
        rate: i128,
        decimals: u32,
    ) -> Result<(), FXOracleError> {
        operator.require_auth();

        if !AccessControl::has_role(&env, &role_oracle(&env), &operator) {
            return Err(FXOracleError::Unauthorized);
        }

        // Issue #811: checked before the quorum bookkeeping, so a disallowed
        // pair cannot accumulate submissions and then be rejected only once
        // quorum is reached.
        Self::require_pair_allowed(&env, &pair)?;

        let quorum = env
            .storage()
            .instance()
            .get::<OracleDataKey, u32>(&OracleDataKey::Quorum)
            .unwrap_or(1);
        if quorum > 1 {
            let key = OracleDataKey::Submissions(pair.clone());
            let mut submissions: Vec<OracleSubmission> = env
                .storage()
                .persistent()
                .get(&key)
                .unwrap_or_else(|| Vec::new(&env));
            submissions.push_back(OracleSubmission {
                operator: operator.clone(),
                rate,
                decimals,
            });
            let mut matches = 0u32;
            for submission in submissions.iter() {
                if submission.rate == rate && submission.decimals == decimals {
                    matches += 1;
                }
            }
            if matches < quorum {
                env.storage().persistent().set(&key, &submissions);
                return Ok(());
            }
            env.storage().persistent().remove(&key);
            env.events().publish(
                (
                    Symbol::new(&env, "ORACLE"),
                    Symbol::new(&env, "RATE_QUORUM_REACHED"),
                ),
                (pair.clone(), rate, quorum),
            );
        }
        Self::store_rate(&env, pair.clone(), rate, decimals)?;

        // Emit event: (RATE, UPDATED), pair
        env.events().publish(
            (Symbol::new(&env, "RATE"), Symbol::new(&env, "UPDATED")),
            pair,
        );

        Ok(())
    }

    pub fn set_oracle_quorum(env: Env, admin: Address, quorum: u32) -> Result<(), FXOracleError> {
        admin.require_auth();
        if !AccessControl::has_role(&env, &role_admin(&env), &admin) || quorum == 0 {
            return Err(FXOracleError::Unauthorized);
        }
        env.storage()
            .instance()
            .set(&OracleDataKey::Quorum, &quorum);
        Ok(())
    }

    pub fn get_oracle_submissions(env: Env, pair: Symbol) -> Vec<OracleSubmission> {
        env.storage()
            .persistent()
            .get(&OracleDataKey::Submissions(pair))
            .unwrap_or_else(|| Vec::new(&env))
    }

    /// Atomically update up to 20 currency pairs. Requires the ORACLE role.
    ///
    /// Emits `RATE/BATCH_UPDATED` with the number of pairs updated.
    pub fn set_rates_batch(
        env: Env,
        operator: Address,
        rates: Vec<(Symbol, i128, u32)>,
    ) -> Result<u32, FXOracleError> {
        operator.require_auth();

        if !AccessControl::has_role(&env, &role_oracle(&env), &operator) {
            return Err(FXOracleError::Unauthorized);
        }

        // Issue #851: Reject empty batches immediately. Submitting an empty batch
        // wastes transaction fees and ledger bandwidth while performing no useful state
        // mutations. Enforcing this prevents silent no-ops when an operator submits zero rates.
        if rates.is_empty() {
            return Err(FXOracleError::EmptyBatch);
        }

        if rates.len() > MAX_BATCH_RATES {
            return Err(FXOracleError::BatchTooLarge);
        }

        // Issue #811: every pair is validated before any is written, so a batch
        // containing one disallowed pair does not half-apply. `store_rate` has
        // already mutated state by the time a later entry fails, and Soroban
        // gives us no partial rollback inside a call.
        for (pair, _, _) in rates.iter() {
            Self::require_pair_allowed(&env, &pair)?;
        }

        let count = rates.len();
        for (pair, rate, decimals) in rates.iter() {
            Self::store_rate(&env, pair, rate, decimals)?;
        }

        env.events().publish(
            (
                Symbol::new(&env, "RATE"),
                Symbol::new(&env, "BATCH_UPDATED"),
            ),
            count,
        );

        Ok(count)
    }

    /// Issue #811: Admin adds a token to the publish allowlist.
    ///
    /// Idempotent. An empty allowlist means enforcement is off; the first
    /// successful add turns it on.
    pub fn add_allowed_token(env: Env, admin: Address, token: Symbol) -> Result<(), FXOracleError> {
        admin.require_auth();
        if !AccessControl::has_role(&env, &role_admin(&env), &admin) {
            return Err(FXOracleError::Unauthorized);
        }
        let mut allowed = Self::allowed_tokens(&env);
        if Self::token_in_list(&allowed, &token) {
            return Ok(());
        }
        if allowed.len() >= MAX_ALLOWED_TOKENS {
            return Err(FXOracleError::Unauthorized);
        }
        allowed.push_back(token);
        env.storage()
            .instance()
            .set(&OracleDataKey::AllowedTokens, &allowed);
        Ok(())
    }

    /// Issue #811: Admin removes a token from the publish allowlist.
    ///
    /// Idempotent. Existing stored rates are left in place and age out through
    /// the normal staleness path.
    pub fn remove_allowed_token(
        env: Env,
        admin: Address,
        token: Symbol,
    ) -> Result<(), FXOracleError> {
        admin.require_auth();
        if !AccessControl::has_role(&env, &role_admin(&env), &admin) {
            return Err(FXOracleError::Unauthorized);
        }
        let current = Self::allowed_tokens(&env);
        let mut next = Vec::new(&env);
        for item in current.iter() {
            if item != token {
                next.push_back(item);
            }
        }
        env.storage()
            .instance()
            .set(&OracleDataKey::AllowedTokens, &next);
        Ok(())
    }

    /// Tokens a rate may be published against. Empty means enforcement is off.
    pub fn get_allowed_tokens(env: Env) -> Vec<Symbol> {
        Self::allowed_tokens(&env)
    }

    /// Whether `token` is on the allowlist. Returns false when the list is empty.
    pub fn is_token_allowed(env: Env, token: Symbol) -> bool {
        Self::token_in_list(&Self::allowed_tokens(&env), &token)
    }

    fn allowed_tokens(env: &Env) -> Vec<Symbol> {
        env.storage()
            .instance()
            .get(&OracleDataKey::AllowedTokens)
            .unwrap_or_else(|| Vec::new(env))
    }

    fn token_in_list(allowed: &Vec<Symbol>, token: &Symbol) -> bool {
        for item in allowed.iter() {
            if item == *token {
                return true;
            }
        }
        false
    }

    /// Reject a pair unless the allowlist is unset/empty, or both `BASE` and
    /// `QUOTE` of a single-underscore pair are on the list.
    fn require_pair_allowed(env: &Env, pair: &Symbol) -> Result<(), FXOracleError> {
        let allowed = Self::allowed_tokens(env);
        if allowed.is_empty() {
            return Ok(());
        }
        let (base, quote) = Self::split_pair(env, pair).ok_or(FXOracleError::TokenNotAllowed)?;
        if Self::token_in_list(&allowed, &base) && Self::token_in_list(&allowed, &quote) {
            Ok(())
        } else {
            Err(FXOracleError::TokenNotAllowed)
        }
    }

    /// Split `BASE_QUOTE` into two symbols. `None` when the underscore is
    /// missing, repeated, or either half is empty.
    fn split_pair(env: &Env, pair: &Symbol) -> Option<(Symbol, Symbol)> {
        let str_repr = SymbolStr::try_from_val(env, &pair.to_symbol_val()).ok()?;
        let text: &str = str_repr.as_ref();
        let bytes = text.as_bytes();

        let mut separator: Option<usize> = None;
        for (i, b) in bytes.iter().enumerate() {
            if *b == b'_' {
                if separator.is_some() {
                    return None;
                }
                separator = Some(i);
            }
        }
        let separator = separator?;
        let base = &bytes[..separator];
        let quote = &bytes[separator + 1..];
        if base.is_empty() || quote.is_empty() || base.len() > 32 || quote.len() > 32 {
            return None;
        }

        let mut base_buf = [0u8; 32];
        let mut quote_buf = [0u8; 32];
        base_buf[..base.len()].copy_from_slice(base);
        quote_buf[..quote.len()].copy_from_slice(quote);
        let base_len = base.len();
        let quote_len = quote.len();
        let base_sym = Symbol::new(env, core::str::from_utf8(&base_buf[..base_len]).ok()?);
        let quote_sym = Symbol::new(env, core::str::from_utf8(&quote_buf[..quote_len]).ok()?);
        Some((base_sym, quote_sym))
    }

    fn store_rate(env: &Env, pair: Symbol, rate: i128, decimals: u32) -> Result<(), FXOracleError> {
        // Reject non-positive rates before persisting. A stored `rate <= 0`
        // would cause a divide-by-zero panic in downstream consumers such as
        // `PaymentLinkManager.use_link`, permanently bricking affected links.
        if rate <= 0 {
            return Err(FXOracleError::InvalidRate);
        }

        // Issue #478: Check rate deviation against configured limit
        let max_deviation_bps = env
            .storage()
            .persistent()
            .get::<OracleDataKey, u32>(&OracleDataKey::MaxDeviation(pair.clone()))
            .unwrap_or(0);

        if max_deviation_bps > 0 {
            if let Ok(last_rate_data) = env
                .storage()
                .persistent()
                .get::<OracleDataKey, RateData>(&OracleDataKey::Rate(pair.clone()))
                .ok_or(FXOracleError::RateNotFound)
            {
                let last_rate = last_rate_data.rate;
                // Calculate deviation in basis points: (abs(new - old) / old) * 10_000
                let diff = if rate > last_rate {
                    rate - last_rate
                } else {
                    last_rate - rate
                };
                let deviation_bps = ((diff * 10_000) / last_rate.abs().max(1)).max(0) as u32;

                if deviation_bps > max_deviation_bps {
                    return Err(FXOracleError::RateDeviationExceeded);
                }

                // Emit warning if deviation is within 50% of limit
                if deviation_bps * 2 >= max_deviation_bps {
                    env.events().publish(
                        (
                            Symbol::new(env, "RATE"),
                            Symbol::new(env, "DEVIATION_WARNING"),
                        ),
                        (pair.clone(), last_rate, rate, deviation_bps),
                    );
                }
            }
            // On first rate set (no prior rate), skip deviation check
        }

        let rate_data = RateData {
            pair: pair.clone(),
            rate,
            decimals,
            updated_at: env.ledger().timestamp(),
            updated_sequence: env.ledger().sequence(),
        };

        env.storage()
            .persistent()
            .set(&OracleDataKey::Rate(pair), &rate_data);
        Ok(())
    }

    pub fn get_rate(env: Env, pair: Symbol) -> Result<RateData, FXOracleError> {
        let rate_data: RateData = env
            .storage()
            .persistent()
            .get(&OracleDataKey::Rate(pair.clone()))
            .ok_or(FXOracleError::RateNotFound)?;

        Self::check_rate_freshness(&env, &rate_data, &pair)?;

        Ok(rate_data)
    }

    /// Permissionless staleness probe. Returns `true` when the rate is stale
    /// (or missing) and emits `RATE/STALE_ALERT` when stale data is present.
    pub fn check_rate_staleness(env: Env, pair: Symbol) -> bool {
        let rate_data: RateData = match env
            .storage()
            .persistent()
            .get(&OracleDataKey::Rate(pair.clone()))
        {
            Some(data) => data,
            None => return true,
        };

        if Self::is_rate_stale(&env, &rate_data) {
            env.events().publish(
                (Symbol::new(&env, "RATE"), Symbol::new(&env, "STALE_ALERT")),
                pair,
            );
            true
        } else {
            false
        }
    }

    // SECURITY: Rate freshness relies on ledger wall-clock time (`env.ledger().timestamp()`),
    // which Stellar validators can influence within a small window (~±a few seconds).
    // A compromised oracle key or delayed off-chain feed could also leave stale rates in
    // storage. Mitigations enforced here:
    //   1. Hard cap (`MAX_RATE_AGE_SECS`) — rates older than 24 h are always rejected,
    //      even if the admin-configured threshold is higher.
    //   2. Ledger-sequence circuit breaker (`MAX_LEDGER_GAP`) — if no rate update has
    //      occurred within the last N ledgers, settlement is blocked and a STALE_ALERT
    //      event is emitted for off-chain monitoring.
    // Accepted residual risk: timestamp drift within the validator window may delay or
    // accelerate staleness by a few seconds. A dual timestamp+sequence AND-check (reject
    // only when both conditions hold) is tracked as a follow-up to reduce false positives.
    fn check_rate_freshness(
        env: &Env,
        rate_data: &RateData,
        pair: &Symbol,
    ) -> Result<(), FXOracleError> {
        if Self::is_rate_stale(env, rate_data) {
            let ledger_gap = env
                .ledger()
                .sequence()
                .saturating_sub(rate_data.updated_sequence);
            if ledger_gap > MAX_LEDGER_GAP {
                env.events().publish(
                    (Symbol::new(env, "RATE"), Symbol::new(env, "STALE_ALERT")),
                    pair.clone(),
                );
            }
            return Err(FXOracleError::RateStale);
        }
        Ok(())
    }

    fn is_rate_stale(env: &Env, rate_data: &RateData) -> bool {
        let configured_threshold: u64 = env
            .storage()
            .instance()
            .get(&OracleDataKey::StalenessThreshold)
            .unwrap_or(MAX_RATE_AGE_SECS);

        let effective_threshold = configured_threshold.min(MAX_RATE_AGE_SECS);

        let now = env.ledger().timestamp();
        if now > rate_data.updated_at.saturating_add(effective_threshold) {
            return true;
        }

        let ledger_gap = env
            .ledger()
            .sequence()
            .saturating_sub(rate_data.updated_sequence);
        ledger_gap > MAX_LEDGER_GAP
    }

    pub fn get_settlement_amount(
        env: Env,
        usdc_amount: i128,
        target_currency: Symbol,
    ) -> Result<i128, FXOracleError> {
        let rate_data = Self::get_rate(env.clone(), target_currency)?;

        let mut divisor = 1i128;
        for _ in 0..rate_data.decimals {
            divisor *= 10;
        }

        Ok(usdc_amount
            .checked_mul(rate_data.rate)
            .ok_or(FXOracleError::PairNotFound)?
            .checked_div(divisor)
            .unwrap_or(0))
    }

    /// Issue #636: Return the rate for `pair`, transparently falling back to the
    /// inverse pair when only the reciprocal is stored.
    ///
    /// Lookup order:
    /// 1. Direct: `pair` (e.g. `EUR_USD`). If a rate is stored, it is returned
    ///    as-is (subject to the usual staleness checks). A stored-but-stale
    ///    direct rate surfaces `RateStale` — it does *not* fall through.
    /// 2. Inverse: `pair` with its `BASE`/`QUOTE` halves swapped (e.g.
    ///    `USD_EUR`). If found, the returned `RateData` carries `1 / rate`
    ///    scaled to `INVERSE_RATE_DECIMALS` fixed-point precision, with `pair`
    ///    set to the originally-requested symbol and the timestamp/sequence
    ///    copied from the inverse rate.
    ///
    /// Returns [`FXOracleError::PairNotFound`] only when both the direct and
    /// inverse lookups find nothing (or `pair` is not a parseable
    /// `BASE_QUOTE` symbol).
    pub fn get_rate_or_inverse(env: Env, pair: Symbol) -> Result<RateData, FXOracleError> {
        match Self::get_rate(env.clone(), pair.clone()) {
            Ok(direct) => return Ok(direct),
            Err(FXOracleError::RateNotFound) => {}
            Err(other) => return Err(other),
        }

        let inverse_pair = Self::invert_pair(&env, &pair).ok_or(FXOracleError::PairNotFound)?;

        let inverse = match Self::get_rate(env.clone(), inverse_pair) {
            Ok(data) => data,
            Err(FXOracleError::RateNotFound) => return Err(FXOracleError::PairNotFound),
            Err(other) => return Err(other),
        };

        if inverse.rate <= 0 {
            return Err(FXOracleError::PairNotFound);
        }

        // reciprocal = 10^(inverse.decimals + INVERSE_RATE_DECIMALS) / inverse.rate
        let mut numerator: i128 = 1;
        for _ in 0..(inverse.decimals + INVERSE_RATE_DECIMALS) {
            numerator = numerator
                .checked_mul(10)
                .ok_or(FXOracleError::PairNotFound)?;
        }
        let reciprocal = numerator / inverse.rate;

        Ok(RateData {
            pair,
            rate: reciprocal,
            decimals: INVERSE_RATE_DECIMALS,
            updated_at: inverse.updated_at,
            updated_sequence: inverse.updated_sequence,
        })
    }

    /// Issue #636: Convert `amount` units of `from` into `to`, using the
    /// `FROM_TO` pair rate and automatically falling back to the inverse
    /// `TO_FROM` pair via [`Self::get_rate_or_inverse`].
    ///
    /// `result = amount * rate / 10^decimals`.
    pub fn get_settlement_amount_for_pair(
        env: Env,
        from: Symbol,
        to: Symbol,
        amount: i128,
    ) -> Result<i128, FXOracleError> {
        let pair = Self::join_pair(&env, &from, &to).ok_or(FXOracleError::PairNotFound)?;
        let rate_data = Self::get_rate_or_inverse(env.clone(), pair)?;

        let mut divisor: i128 = 1;
        for _ in 0..rate_data.decimals {
            divisor = divisor.checked_mul(10).ok_or(FXOracleError::PairNotFound)?;
        }

        let scaled = amount
            .checked_mul(rate_data.rate)
            .ok_or(FXOracleError::PairNotFound)?;
        Ok(scaled.checked_div(divisor).unwrap_or(0))
    }

    /// Derive the inverse of a `BASE_QUOTE` pair symbol, e.g. `EUR_USD` ->
    /// `USD_EUR`. Returns `None` when `pair` has no single `_` separator or
    /// either half is empty.
    fn invert_pair(env: &Env, pair: &Symbol) -> Option<Symbol> {
        let str_repr = SymbolStr::try_from_val(env, &pair.to_symbol_val()).ok()?;
        let text: &str = str_repr.as_ref();
        let bytes = text.as_bytes();

        let mut separator: Option<usize> = None;
        for (i, b) in bytes.iter().enumerate() {
            if *b == b'_' {
                if separator.is_some() {
                    return None; // more than one separator — not a simple pair
                }
                separator = Some(i);
            }
        }
        let separator = separator?;
        let base = &bytes[..separator];
        let quote = &bytes[separator + 1..];
        if base.is_empty() || quote.is_empty() {
            return None;
        }

        let mut buf = [0u8; MAX_PAIR_SYMBOL_LEN + 1];
        let mut n = 0usize;
        for &b in quote {
            buf[n] = b;
            n += 1;
        }
        buf[n] = b'_';
        n += 1;
        for &b in base {
            buf[n] = b;
            n += 1;
        }
        let inverted = core::str::from_utf8(&buf[..n]).ok()?;
        Some(Symbol::new(env, inverted))
    }

    /// Join two currency symbols into a `FROM_TO` pair symbol. Returns `None`
    /// when either symbol is empty or the joined result would exceed the
    /// 32-character `Symbol` limit.
    fn join_pair(env: &Env, from: &Symbol, to: &Symbol) -> Option<Symbol> {
        let from_str = SymbolStr::try_from_val(env, &from.to_symbol_val()).ok()?;
        let to_str = SymbolStr::try_from_val(env, &to.to_symbol_val()).ok()?;
        let from_bytes = {
            let s: &str = from_str.as_ref();
            s.as_bytes()
        };
        let to_bytes = {
            let s: &str = to_str.as_ref();
            s.as_bytes()
        };
        if from_bytes.is_empty()
            || to_bytes.is_empty()
            || from_bytes.len() + to_bytes.len() + 1 > MAX_PAIR_SYMBOL_LEN
        {
            return None;
        }

        let mut buf = [0u8; MAX_PAIR_SYMBOL_LEN + 1];
        let mut n = 0usize;
        for &b in from_bytes {
            buf[n] = b;
            n += 1;
        }
        buf[n] = b'_';
        n += 1;
        for &b in to_bytes {
            buf[n] = b;
            n += 1;
        }
        let joined = core::str::from_utf8(&buf[..n]).ok()?;
        Some(Symbol::new(env, joined))
    }

    pub fn get_staleness_threshold(env: Env) -> u64 {
        env.storage()
            .instance()
            .get(&OracleDataKey::StalenessThreshold)
            .unwrap_or(MAX_RATE_AGE_SECS)
    }

    /// Upgrade the contract WASM.
    ///
    /// Only the admin can call this. Emits a `CONTRACT/UPGRADED` event with the
    /// old and new version strings on success.
    pub fn upgrade(
        env: Env,
        admin: Address,
        new_wasm_hash: BytesN<32>,
    ) -> Result<(), FXOracleError> {
        admin.require_auth();

        if !AccessControl::has_role(&env, &role_admin(&env), &admin) {
            return Err(FXOracleError::Unauthorized);
        }

        let old_version = String::from_str(&env, "1.0.0");
        env.deployer().update_current_contract_wasm(new_wasm_hash);

        env.events().publish(
            (Symbol::new(&env, "CONTRACT"), Symbol::new(&env, "UPGRADED")),
            (old_version.clone(), String::from_str(&env, "1.0.1")),
        );

        Ok(())
    }

    pub fn set_staleness_threshold(
        env: Env,
        admin: Address,
        threshold: u64,
    ) -> Result<(), FXOracleError> {
        Self::set_max_staleness(env, admin, threshold)
    }

    /// Set the maximum accepted age of a rate in seconds.
    pub fn set_max_staleness(
        env: Env,
        admin: Address,
        threshold: u64,
    ) -> Result<(), FXOracleError> {
        admin.require_auth();

        if !AccessControl::has_role(&env, &role_admin(&env), &admin) {
            return Err(FXOracleError::Unauthorized);
        }

        if !(MIN_RATE_AGE_SECS..=MAX_RATE_AGE_SECS).contains(&threshold) {
            return Err(FXOracleError::InvalidStalenessThreshold);
        }

        env.storage()
            .instance()
            .set(&OracleDataKey::StalenessThreshold, &threshold);
        Ok(())
    }

    /// Issue #478: Set maximum allowed rate deviation per currency pair in basis points.
    /// Example: 1000 = 10% max allowed deviation.
    pub fn set_rate_deviation_limit(
        env: Env,
        admin: Address,
        pair: Symbol,
        max_deviation_bps: u32,
    ) -> Result<(), FXOracleError> {
        admin.require_auth();

        if !AccessControl::has_role(&env, &role_admin(&env), &admin) {
            return Err(FXOracleError::Unauthorized);
        }

        env.storage().persistent().set(
            &OracleDataKey::MaxDeviation(pair.clone()),
            &max_deviation_bps,
        );

        // Emit event: (RATE, DEVIATION_LIMIT_SET), pair
        env.events().publish(
            (
                Symbol::new(&env, "RATE"),
                Symbol::new(&env, "DEVIATION_LIMIT_SET"),
            ),
            (pair, max_deviation_bps),
        );

        Ok(())
    }

    /// Issue #478: Get the maximum allowed rate deviation for a pair (in basis points).
    /// Returns 0 if no limit is configured (unlimited).
    pub fn get_rate_deviation_limit(env: Env, pair: Symbol) -> u32 {
        env.storage()
            .persistent()
            .get::<OracleDataKey, u32>(&OracleDataKey::MaxDeviation(pair))
            .unwrap_or(0) // 0 = no limit
    }
}
