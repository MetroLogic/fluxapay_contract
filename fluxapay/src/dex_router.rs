// Updated DexRouter with pre-execution checks, path validation, price impact guard, and fallback logic.
use soroban_sdk::{contract, contracterror, contractimpl, token, Address, Env, Symbol, Vec};

#[contracterror]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DexRouterError {
    SwapFailed = 1,
    InvalidPath = 2,
    InsufficientLiquidity = 3,
    SlippageExceeded = 4,
    PriceImpactExceeded = 5,
    NoOutputAmount = 6,
    Refunded = 7,
}

/// DEX Router interface for Soroswap-style swaps.
/// This provides a generic interface for atomic token swaps.
#[contract]
pub struct DexRouter;

#[cfg_attr(
    any(not(target_arch = "wasm32"), feature = "contract-dex-router"),
    contractimpl
)]
impl DexRouter {
    /// Initialize the router with an admin and Soroswap factory address.
    pub fn initialize(env: Env, admin: Address, factory: Address) {
        let admin_key = Symbol::new(&env, "Admin");
        if env.storage().instance().has(&admin_key) {
            panic!("already initialized");
        }
        env.storage().instance().set(&admin_key, &admin);
        env.storage().instance().set(&Symbol::new(&env, "Factory"), &factory);
    }

    /// Admin-configurable factory address (#1145).
    pub fn set_factory(env: Env, admin: Address, new_factory: Address) {
        admin.require_auth();
        let admin_key = Symbol::new(&env, "Admin");
        if let Some(stored_admin) = env.storage().instance().get::<Symbol, Address>(&admin_key) {
            if admin != stored_admin {
                panic!("unauthorized");
            }
        } else {
            env.storage().instance().set(&admin_key, &admin);
        }
        env.storage().instance().set(&Symbol::new(&env, "Factory"), &new_factory);
    }

    /// Admin-configurable external router address (#1146).
    pub fn set_router(env: Env, admin: Address, router: Address) {
        admin.require_auth();
        let admin_key = Symbol::new(&env, "Admin");
        if let Some(stored_admin) = env.storage().instance().get::<Symbol, Address>(&admin_key) {
            if admin != stored_admin {
                panic!("unauthorized");
            }
        } else {
            env.storage().instance().set(&admin_key, &admin);
        }
        env.storage().instance().set(&Symbol::new(&env, "Router"), &router);
    }

    /// Get configured external router address.
    pub fn router(env: Env) -> Option<Address> {
        env.storage().instance().get(&Symbol::new(&env, "Router"))
    }

    /// Get the router's factory address. Returns configured Soroswap factory or placeholder.
    pub fn factory(env: Env) -> Address {
        env.storage()
            .instance()
            .get(&Symbol::new(&env, "Factory"))
            .unwrap_or_else(|| env.current_contract_address())
    }

    /// Admin configuration for pool reserve between token_in and token_out (#1144, #1147).
    pub fn set_pool_reserve(
        env: Env,
        admin: Address,
        token_in: Address,
        token_out: Address,
        reserve: i128,
    ) {
        admin.require_auth();
        let admin_key = Symbol::new(&env, "Admin");
        if let Some(stored_admin) = env.storage().instance().get::<Symbol, Address>(&admin_key) {
            if admin != stored_admin {
                panic!("unauthorized");
            }
        } else {
            env.storage().instance().set(&admin_key, &admin);
        }
        let reserve_key = (Symbol::new(&env, "Reserve"), token_in, token_out);
        env.storage().instance().set(&reserve_key, &reserve);
    }

    /// Admin configuration for simulation mode toggle (#1144, #1147).
    pub fn set_simulation_mode(env: Env, admin: Address, enabled: bool) {
        admin.require_auth();
        let admin_key = Symbol::new(&env, "Admin");
        if let Some(stored_admin) = env.storage().instance().get::<Symbol, Address>(&admin_key) {
            if admin != stored_admin {
                panic!("unauthorized");
            }
        } else {
            env.storage().instance().set(&admin_key, &admin);
        }
        env.storage().instance().set(&Symbol::new(&env, "SimMode"), &enabled);
    }

    /// Check simulation mode status. Defaults to true.
    pub fn simulation_mode(env: Env) -> bool {
        env.storage()
            .instance()
            .get(&Symbol::new(&env, "SimMode"))
            .unwrap_or(true)
    }

    /// Set external router address (#1144, #1147).
    pub fn set_router(env: Env, admin: Address, router: Address) {
        admin.require_auth();
        let admin_key = Symbol::new(&env, "Admin");
        if let Some(stored_admin) = env.storage().instance().get::<Symbol, Address>(&admin_key) {
            if admin != stored_admin {
                panic!("unauthorized");
            }
        } else {
            env.storage().instance().set(&admin_key, &admin);
        }
        env.storage().instance().set(&Symbol::new(&env, "Router"), &router);
    }

    /// Get amounts out by querying real pools or external router, falling back to simulation formula (#1144).
    pub fn get_amounts_out(env: Env, amount_in: i128, path: Vec<Address>) -> Vec<i128> {
        let mut amounts = Vec::new(&env);
        if path.is_empty() {
            return amounts;
        }

        // Delegate to external router if configured
        if let Some(router) = env
            .storage()
            .instance()
            .get::<Symbol, Address>(&Symbol::new(&env, "Router"))
        {
            if router != env.current_contract_address() {
                return DexRouterClient::new(&env, &router).get_amounts_out(&amount_in, &path);
            }
        }

        amounts.push_back(amount_in);
        let mut current = amount_in;
        for i in 1..path.len() {
            let t_in = path.get(i - 1).unwrap();
            let t_out = path.get(i).unwrap();
            let reserve_key = (Symbol::new(&env, "Reserve"), t_in, t_out);
            if let Some(reserve_out) = env.storage().instance().get::<_, i128>(&reserve_key) {
                // Bound output by known pool reserve
                let quote = current.saturating_mul(99).saturating_div(100);
                current = if quote > reserve_out { reserve_out } else { quote };
            } else {
                let sim_mode: bool = env
                    .storage()
                    .instance()
                    .get(&Symbol::new(&env, "SimMode"))
                    .unwrap_or(true);
                if sim_mode {
                    // Fall back to default 1% per-hop haircut only in simulation mode
                    current = current.saturating_mul(99).saturating_div(100);
                } else {
                    current = 0;
                }
            }
            amounts.push_back(current);
        }
        amounts
    }

    /// Internal: Validate that the provided path is non-empty and has at least two hops.
    fn validate_path(path: &Vec<Address>) -> Result<(), DexRouterError> {
        if path.len() < 2 {
            return Err(DexRouterError::InvalidPath);
        }
        for i in 1..path.len() {
            if path.get(i) == path.get(i - 1) {
                return Err(DexRouterError::InvalidPath);
            }
        }
        Ok(())
    }

    /// Check whether every hop along the path has liquidity (> 0 reserve) (#1147).
    pub fn check_liquidity(env: Env, path: Vec<Address>) -> bool {
        if path.len() < 2 {
            return false;
        }

        // Delegate to external router if configured
        if let Some(router) = env
            .storage()
            .instance()
            .get::<Symbol, Address>(&Symbol::new(&env, "Router"))
        {
            if router != env.current_contract_address() {
                let amounts = DexRouterClient::new(&env, &router).get_amounts_out(&1000, &path);
                return !amounts.is_empty() && amounts.get(amounts.len() - 1).unwrap_or(0) > 0;
            }
        }

        for i in 1..path.len() {
            let t_in = path.get(i - 1).unwrap();
            let t_out = path.get(i).unwrap();
            let reserve_key = (Symbol::new(&env, "Reserve"), t_in, t_out);
            if let Some(reserve) = env.storage().instance().get::<_, i128>(&reserve_key) {
                if reserve <= 0 {
                    return false;
                }
            } else {
                let sim_mode: bool = env
                    .storage()
                    .instance()
                    .get(&Symbol::new(&env, "SimMode"))
                    .unwrap_or(true);
                if !sim_mode {
                    return false;
                }
            }
        }
        true
    }

    fn price_impact_guard(input: i128, output: i128) -> Result<(), DexRouterError> {
        if input <= 0 {
            return Err(DexRouterError::InvalidPath);
        }
        let impact_basis_points = ((input - output) * 10_000) / input;
        if impact_basis_points > 500 {
            return Err(DexRouterError::PriceImpactExceeded);
        }
        Ok(())
    }

    /// TODO: delegate token transfers to the real Soroswap router contract and
    /// pull liquidity from on-chain pools instead of simulated quotes.
    pub fn swap_exact_tokens_for_tokens(
        env: Env,
        amount_in: i128,
        amount_out_min: i128,
        path: Vec<Address>,
        to: Address,
        deadline: u64,
    ) -> Result<Vec<i128>, DexRouterError> {
        Self::validate_path(&path)?;
        if !Self::check_liquidity(env.clone(), path.clone()) {
            return Err(DexRouterError::InsufficientLiquidity);
        }

        let primary_result =
            Self::execute_swap_internal(&env, amount_in, amount_out_min, &path, to.clone(), deadline);
        if primary_result.is_ok() {
            return primary_result;
        }

        // Return the primary swap error directly. Reversing the path does not produce a valid
        // fallback swap — it would attempt to trade the output token back for the input token
        // (opposite direction). If a fallback strategy is desired, use swap_with_fallback_router
        // with a different router from the allowlist using the same path.
        let token_in = path.get(0).unwrap();
        Self::refund_caller(env.clone(), token_in, to.clone(), amount_in)?;
        Err(DexRouterError::SwapFailed)
    }

    /// Execute a direct token swap with slippage tolerance and max_slippage_bps guard.
    ///
    /// # Parameters
    /// * `caller` - The account requesting the swap (authorized)
    /// * `token_in` - The input token contract address
    /// * `token_out` - The output token contract address
    /// * `amount_in` - The exact amount of token_in to swap
    /// * `min_amount_out` - Minimum output amount acceptable to caller (reverts with SlippageExceeded if below)
    /// * `max_slippage_bps` - Maximum allowable slippage in basis points against quoted price (≤ 5000 = 50% max)
    #[allow(deprecated)] // events::publish — migrate to #[contractevent] in a follow-up
    pub fn execute_swap(
        env: Env,
        caller: Address,
        token_in: Address,
        token_out: Address,
        amount_in: i128,
        min_amount_out: i128,
        max_slippage_bps: u32,
    ) -> Result<i128, DexRouterError> {
        caller.require_auth();

        if max_slippage_bps > 5000 {
            return Err(DexRouterError::SlippageExceeded);
        }

        let mut path = Vec::new(&env);
        path.push_back(token_in.clone());
        path.push_back(token_out.clone());

        Self::validate_path(&path)?;
        if !Self::check_liquidity(env.clone(), path.clone()) {
            return Err(DexRouterError::InsufficientLiquidity);
        }

        let amounts = Self::get_amounts_out(env.clone(), amount_in, path.clone());
        if amounts.len() < 2 {
            return Err(DexRouterError::NoOutputAmount);
        }

        let quoted_out = amounts.get(1).unwrap_or(0i128);
        if quoted_out <= 0 {
            return Err(DexRouterError::NoOutputAmount);
        }

        // Secondary check against quoted price
        let min_from_bps = quoted_out
            .saturating_mul(10_000 - max_slippage_bps as i128)
            .saturating_div(10_000);

        let actual_out = quoted_out;
        if actual_out < min_amount_out || actual_out < min_from_bps {
            return Err(DexRouterError::SlippageExceeded);
        }

        Self::price_impact_guard(amount_in, actual_out)?;

        env.events().publish(
            (Symbol::new(&env, "SWAP"), Symbol::new(&env, "EXECUTED")),
            (amount_in, actual_out, caller, env.ledger().timestamp()),
        );

        Ok(actual_out)
    }

    #[allow(deprecated)] // events::publish — migrate to #[contractevent] in a follow-up
    fn execute_swap_internal(
        env: &Env,
        amount_in: i128,
        amount_out_min: i128,
        path: &Vec<Address>,
        to: Address,
        deadline: u64,
    ) -> Result<Vec<i128>, DexRouterError> {
        if env.ledger().timestamp() > deadline {
            return Err(DexRouterError::SwapFailed);
        }

        let amounts = Self::get_amounts_out(env.clone(), amount_in, path.clone());
        if amounts.is_empty() {
            return Err(DexRouterError::NoOutputAmount);
        }
        let final_output = amounts.get(amounts.len() - 1).unwrap_or(0i128);
        if final_output < amount_out_min {
            return Err(DexRouterError::SlippageExceeded);
        }
        Self::price_impact_guard(amount_in, final_output)?;
        env.events().publish(
            (Symbol::new(env, "SWAP"), Symbol::new(env, "EXECUTED")),
            (amount_in, final_output, to, deadline),
        );
        Ok(amounts)
    }

    #[allow(deprecated)] // events::publish — migrate to #[contractevent] in a follow-up
    pub fn refund_caller(
        env: Env,
        token: Address,
        recipient: Address,
        amount: i128,
    ) -> Result<(), DexRouterError> {
        if amount > 0 {
            token::Client::new(&env, &token).transfer(
                &env.current_contract_address(),
                &recipient,
                &amount,
            );
        }
        env.events().publish(
            (Symbol::new(&env, "REFUND"), Symbol::new(&env, "CALLER")),
            (recipient, amount),
        );
        Ok(())
    }

    /// Reverse quote (#888): minimum input across `path` to deliver exactly `amount_out`
    /// of the final token. Walks backwards inverting the per-hop 1% slip from get_amounts_out,
    /// so amounts[0] yields amounts[len-1] = amount_out.
    pub fn get_amounts_in(env: Env, amount_out: i128, path: Vec<Address>) -> Vec<i128> {
        let mut amounts = Vec::new(&env);
        if path.is_empty() {
            return amounts;
        }
        let mut reverse_order = Vec::new(&env);
        reverse_order.push_back(amount_out);
        let mut current = amount_out;
        for _ in 1..path.len() {
            current = current.saturating_mul(100).saturating_div(99); // in = out * 100 / 99
            reverse_order.push_back(current);
        }
        for i in (0..reverse_order.len()).rev() {
            amounts.push_back(reverse_order.get(i).unwrap_or(0i128));
        }
        amounts
    }

    /// Swap for an exact output (#888, #1146): merchant gets exactly `amount_out`;
    /// customer pays only the input computed at execution time, capped by `amount_in_max`.
    #[allow(deprecated)]
    pub fn swap_tokens_for_exact_tokens(
        env: Env,
        amount_out: i128,
        amount_in_max: i128,
        path: Vec<Address>,
        to: Address,
        deadline: u64,
    ) -> Result<Vec<i128>, DexRouterError> {
        Self::validate_path(&path)?;
        if env.ledger().timestamp() > deadline {
            return Err(DexRouterError::SwapFailed); // expired deadline
        }
        if amount_out <= 0 {
            return Err(DexRouterError::NoOutputAmount);
        }

        // Invoke real router if configured
        if let Some(real_router) = env
            .storage()
            .instance()
            .get::<Symbol, Address>(&Symbol::new(&env, "Router"))
        {
            if real_router != env.current_contract_address() {
                return DexRouterClient::new(&env, &real_router).swap_tokens_for_exact_tokens(
                    &amount_out,
                    &amount_in_max,
                    &path,
                    &to,
                    &deadline,
                );
            }
        }

        let amounts = Self::get_amounts_in(env.clone(), amount_out, path.clone());
        if amounts.is_empty() {
            return Err(DexRouterError::NoOutputAmount);
        }
        let required_in = amounts.get(0).unwrap_or(0i128);
        if required_in <= 0 {
            return Err(DexRouterError::InsufficientLiquidity);
        }
        if required_in > amount_in_max {
            return Err(DexRouterError::SlippageExceeded); // never spend more than authorized
        }

        let token_in = path.get(0).unwrap();
        let token_out = path.get(path.len() - 1).unwrap();
        token::Client::new(&env, &token_out).transfer(
            &env.current_contract_address(),
            &to,
            &amount_out,
        );

        let surplus = amount_in_max.saturating_sub(required_in);
        if surplus > 0 {
            Self::refund_caller(env.clone(), token_in, to.clone(), surplus)?; // refund above the necessary input
        }
        env.events().publish(
            (Symbol::new(&env, "SWAP_EXACT_SETTLED"),),
            (path, required_in, amount_out, to, env.ledger().timestamp()),
        );
        Ok(amounts)
    }
}
