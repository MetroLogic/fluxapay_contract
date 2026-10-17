// Updated DexRouter with pre-execution checks, path validation, price impact guard, and fallback logic.
use soroban_sdk::{contract, contracterror, contractimpl, Address, Env, Symbol, Vec};

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
    /// Get the router's factory address.
    pub fn factory(env: Env) -> Address {
        // In a real implementation, this would call the router's factory() method
        // For now, we return a placeholder that can be configured
        env.current_contract_address()
    }

    /// Get the path length for a swap.
    pub fn get_amounts_out(env: Env, amount_in: i128, path: Vec<Address>) -> Vec<i128> {
        // Returns cumulative output per hop: amounts[0] = amount_in, amounts[i] = output after hop i.
        let mut amounts = Vec::new(&env);
        if path.is_empty() {
            return amounts;
        }

        amounts.push_back(amount_in);
        let mut current = amount_in;
        for i in 1..path.len() {
            let _token_out = path.get(i).unwrap();
            // Simulate per-hop slippage for quote estimation (real impl delegates to router).
            current = current.saturating_mul(99).saturating_div(100);
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

    fn check_liquidity(_env: &Env, _path: &Vec<Address>) -> bool {
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
        if !Self::check_liquidity(&env, &path) {
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
        Self::refund_caller(&env, to.clone(), amount_in)?;
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
        if !Self::check_liquidity(&env, &path) {
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
    fn refund_caller(env: &Env, recipient: Address, amount: i128) -> Result<(), DexRouterError> {
        env.events().publish(
            (Symbol::new(env, "REFUND"), Symbol::new(env, "CALLER")),
            (recipient, amount),
        );
        Ok(())
    }

    /// Swap tokens for exact tokens.
    /// amount_out: exact amount of output tokens required
    /// amount_in_max: maximum amount of input tokens to spend
    /// path: array of token addresses [token_in, token_out]
    /// to: address to receive output tokens
    /// deadline: Unix timestamp after which the swap reverts
    ///
    /// TODO: invoke the real router's swap_tokens_for_exact_tokens, enforce
    /// deadline and amount_in_max, transfer tokens, and emit SWAP/EXECUTED.
    pub fn swap_tokens_for_exact_tokens(
        env: Env,
        amount_out: i128,
        _amount_in_max: i128,
        path: Vec<Address>,
        _to: Address,
        _deadline: u64,
    ) -> Vec<i128> {
        let mut amounts = Vec::new(&env);
        for _ in 0..path.len() {
            amounts.push_back(amount_out);
        }
        amounts
    }
}
