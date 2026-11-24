#![cfg(test)]

use soroban_sdk::{contract, contracterror, contractimpl, vec, Address, Env, Symbol, Vec};

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
    pub fn last_swap(env: Env) -> Option<(i128, Vec<Address>)> {
        env.storage()
            .persistent()
            .get(&Symbol::new(&env, "last_swap"))
    }

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
        _to: Address,
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

        env.storage()
            .persistent()
            .set(&Symbol::new(&env, "last_swap"), &(amount_in, path.clone()));
        let stored_output = Self::read_output(&env);
        match stored_output {
            Some(output) => {
                if output < amount_out_min {
                    return Err(MockDexError::InsufficientOutput);
                }
                Ok(Self::build_amounts(&env, amount_in, output))
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
