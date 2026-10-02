#![cfg(test)]

use soroban_sdk::{contract, contracterror, contractimpl, token, vec, Address, Env, Symbol, Vec};

pub const OUTPUT_KEY: &str = "output";
pub const FAIL_SWAP_KEY: &str = "fail_swap";

#[contracterror]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MockDexError {
    SwapFailed = 1,
    InvalidPath = 2,
    InsufficientOutput = 3,
}

#[contract]
pub struct MockDexRouter;

#[contractimpl]
impl MockDexRouter {
    fn read_output(env: &Env) -> Option<i128> {
        env.storage()
            .persistent()
            .get(&Symbol::new(env, OUTPUT_KEY))
    }

    fn build_amounts(env: &Env, amount_in: i128, output: i128) -> Vec<i128> {
        let mut amounts = vec![env];
        amounts.push_back(amount_in);
        amounts.push_back(output);
        amounts
    }

    pub fn get_amounts_out(env: Env, amount_in: i128, path: Vec<Address>) -> Vec<i128> {
        if path.len() < 2 {
            return vec![&env];
        }

        match Self::read_output(&env) {
            Some(output) => Self::build_amounts(&env, amount_in, output),
            None => Self::build_amounts(&env, amount_in, amount_in),
        }
    }

    pub fn swap_exact_tokens_for_tokens(
        env: Env,
        amount_in: i128,
        amount_out_min: i128,
        path: Vec<Address>,
        to: Address,
        _deadline: u64,
    ) -> Result<Vec<i128>, MockDexError> {
        if path.len() < 2 {
            return Err(MockDexError::InvalidPath);
        }

        let fail_swap: bool = env
            .storage()
            .persistent()
            .get(&Symbol::new(&env, FAIL_SWAP_KEY))
            .unwrap_or(false);
        if fail_swap {
            return Err(MockDexError::SwapFailed);
        }

        let stored_output = Self::read_output(&env);
        match stored_output {
            Some(output) => {
                if output < amount_out_min {
                    return Err(MockDexError::InsufficientOutput);
                }
                let token_in = path.get(0).unwrap();
                let token_out = path.get(path.len() - 1).unwrap();
                token::Client::new(&env, &token_in).transfer(
                    &env.current_contract_address(),
                    &to,
                    &amount_in,
                );
                token::Client::new(&env, &token_out).transfer(
                    &env.current_contract_address(),
                    &to,
                    &output,
                );
                Ok(Self::build_amounts(&env, amount_in, output))
            }
            None => Err(MockDexError::SwapFailed),
        }
    }

    pub fn execute_swap(
        env: Env,
        caller: Address,
        token_in: Address,
        token_out: Address,
        amount_in: i128,
        min_amount_out: i128,
        max_slippage_bps: u32,
    ) -> Result<i128, MockDexError> {
        if max_slippage_bps > 5000 {
            return Err(MockDexError::InsufficientOutput);
        }

        let fail_swap: bool = env
            .storage()
            .persistent()
            .get(&Symbol::new(&env, FAIL_SWAP_KEY))
            .unwrap_or(false);
        if fail_swap {
            return Err(MockDexError::SwapFailed);
        }

        let stored_output = Self::read_output(&env);
        match stored_output {
            Some(output) => {
                let min_from_bps = output
                    .saturating_mul(10_000 - max_slippage_bps as i128)
                    .saturating_div(10_000);
                if output < min_amount_out || output < min_from_bps {
                    return Err(MockDexError::InsufficientOutput);
                }
                token::Client::new(&env, &token_in).transfer(
                    &caller,
                    &env.current_contract_address(),
                    &amount_in,
                );
                token::Client::new(&env, &token_out).transfer(
                    &env.current_contract_address(),
                    &caller,
                    &output,
                );
                Ok(output)
            }
            None => Err(MockDexError::SwapFailed),
        }
    }
}

pub fn configure_mock_dex(env: &Env, mock_dex: &Address, output: i128, fail_swap: bool) {
    env.as_contract(mock_dex, || {
        env.storage()
            .persistent()
            .set(&Symbol::new(env, OUTPUT_KEY), &output);
        env.storage()
            .persistent()
            .set(&Symbol::new(env, FAIL_SWAP_KEY), &fail_swap);
    });
}
