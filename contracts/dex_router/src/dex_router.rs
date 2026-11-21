use soroban_sdk::{contract, contractimpl, contracttype, token, Address, Env, Symbol};

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SwapResult {
    pub amount_in: i128,
    pub amount_out: i128,
    pub recipient: Address,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SwapExecuted {
    pub caller: Address,
    pub token_in: Address,
    pub token_out: Address,
    pub amount_in: i128,
    pub amount_out: i128,
    pub recipient: Address,
}

#[contract]
pub struct DexRouter;

#[contractimpl]
impl DexRouter {
    /// Execute a swap: pull `amount_in` of `token_in` from `caller` into the
    /// router/pool, then push `amount_out` of `token_out` to `recipient`.
    ///
    /// Token movement happens before any SWAP/EXECUTED event is emitted so
    /// that a failed transfer aborts the whole invocation without emitting a
    /// misleading success event.
    pub fn execute_swap(
        env: Env,
        caller: Address,
        token_in: Address,
        token_out: Address,
        amount_in: i128,
        quoted_out: i128,
        recipient: Address,
    ) -> SwapResult {
        caller.require_auth();

        if amount_in <= 0 {
            panic!("amount_in must be positive");
        }
        if quoted_out <= 0 {
            panic!("quoted_out must be positive");
        }

        let actual_out = quoted_out;

        // 1. Pull input tokens from the caller into the router/pool.
        let token_in_client = token::Client::new(&env, &token_in);
        token_in_client.transfer(&caller, &env.current_contract_address(), &amount_in);

        // 2. Push output tokens from the router/pool to the recipient.
        let token_out_client = token::Client::new(&env, &token_out);
        token_out_client.transfer(&env.current_contract_address(), &recipient, &actual_out);

        // 3. Only after both transfers succeed do we emit the event.
        env.events().publish(
            (Symbol::new(&env, "SWAP"), Symbol::new(&env, "EXECUTED")),
            SwapExecuted {
                caller: caller.clone(),
                token_in,
                token_out,
                amount_in,
                amount_out: actual_out,
                recipient: recipient.clone(),
            },
        );

        SwapResult {
            amount_in,
            amount_out: actual_out,
            recipient,
        }
    }
}
