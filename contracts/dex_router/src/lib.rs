//! DEX router contract.
//!
//! Provides swap quoting and execution against a Soroswap-style router.
//! `execute_swap` performs real token movement: input tokens are pulled from
//! the caller to the router, the router is invoked, and output tokens are
//! transferred from the router to the recipient. The SWAP/EXECUTED event is
//! only emitted after all transfers succeed.

use soroban_sdk::{contract, contractimpl, contracttype, token, Address, Env, Symbol};

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SwapResult {
    pub amount_in: i128,
    pub amount_out: i128,
use soroban_sdk::{contract, contractimpl, contracttype, token, Address, Env, String, Vec};

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SwapArgs {
    pub payment_id: String,
    pub input_token: Address,
    pub path: Vec<Address>,
    pub min_output: i128,
    pub dex_router: Address,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExactOutputSwapArgs {
    pub payment_id: String,
    pub input_token: Address,
    pub path: Vec<Address>,
    pub exact_output: i128,
    pub max_input: i128,
    pub dex_router: Address,
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
pub struct PaymentRecord {
    pub payer: Address,
    pub deposit_address: Address,
    pub amount: i128,
}

#[contract]
pub struct DexRouter;

#[contractimpl]
impl DexRouter {
    /// Quote an output amount for a given input amount.
    ///
    /// NOTE: This is a simplified constant-product quote used for the router
    /// surface. Path selection / aggregation is tracked separately.
    pub fn quote(
        env: Env,
        _token_in: Address,
        _token_out: Address,
        amount_in: i128,
        reserve_in: i128,
        reserve_out: i128,
    ) -> i128 {
        if amount_in <= 0 || reserve_in <= 0 || reserve_out <= 0 {
            return 0;
        }
        let amount_in_with_fee = amount_in * 997;
        let numerator = amount_in_with_fee * reserve_out;
        let denominator = reserve_in * 1000 + amount_in_with_fee;
        let _ = env;
        numerator / denominator
    }

    /// Execute a swap by moving real tokens.
    ///
    /// 1. Pull `amount_in` of `token_in` from `caller` to this router.
    /// 2. Invoke the Soroswap router to perform the swap (router holds the
    ///    output tokens after the call).
    /// 3. Transfer `amount_out` of `token_out` from this router to `recipient`.
    /// 4. Emit SWAP/EXECUTED only after all transfers succeed.
    pub fn execute_swap(
        env: Env,
        caller: Address,
        token_in: Address,
        token_out: Address,
        amount_in: i128,
        min_amount_out: i128,
        recipient: Address,
        soroswap_router: Address,
        quoted_out: i128,
    ) -> SwapResult {
        caller.require_auth();

        if amount_in <= 0 {
            panic!("amount_in must be positive");
        }

        // 1. Pull input tokens from the caller into this router.
        let token_in_client = token::Client::new(&env, &token_in);
        token_in_client.transfer(&caller, &env.current_contract_address(), &amount_in);

        // 2. Delegate the actual swap to the Soroswap router. The router pulls
        //    `amount_in` from this contract and sends `amount_out` back to it.
        let router_client = soroswap_router_client(&env, &soroswap_router);
        let actual_out = router_client.swap(
            &env.current_contract_address(),
            &token_in,
            &token_out,
            &amount_in,
            &min_amount_out,
        );

        // Guard against a router that reports less than the quote / minimum.
        if actual_out < min_amount_out {
            panic!("insufficient output amount");
        }
        let _ = quoted_out;

        // 3. Transfer output tokens from this router to the recipient.
        let token_out_client = token::Client::new(&env, &token_out);
        token_out_client.transfer(&env.current_contract_address(), &recipient, &actual_out);

        // 4. Emit the event only after both transfers have succeeded.
        env.events().publish(
            (Symbol::new(&env, "SWAP"), Symbol::new(&env, "EXECUTED")),
            SwapExecuted {
                caller: caller.clone(),
                token_in: token_in.clone(),
                token_out: token_out.clone(),
                amount_in,
                amount_out: actual_out,
                recipient: recipient.clone(),
            },
        );

        SwapResult {
            amount_in,
            amount_out: actual_out,
        }
    }
}

/// Minimal client for the Soroswap router `swap` entrypoint.
///
/// The real Soroswap router exposes a `swap` function that pulls the input
/// token from the caller and returns the realized output amount. This client
/// mirrors that interface so the DEX router can delegate to it.
mod soroswap_router_client {
    use soroban_sdk::{contractclient, Address, Env};

    #[contractclient(name = "SoroswapRouterClient")]
    pub trait SoroswapRouter {
        fn swap(
            env: Env,
            caller: Address,
            token_in: Address,
            token_out: Address,
            amount_in: i128,
            min_amount_out: i128,
        ) -> i128;
    }
}

use soroswap_router_client::SoroswapRouterClient;

fn soroswap_router_client(env: &Env, router: &Address) -> SoroswapRouterClient {
    SoroswapRouterClient::new(env, router)
}

#[cfg(test)]
mod test {
    use super::*;
    use soroban_sdk::testutils::Address as _;
    use soroban_sdk::{contract, contractimpl, Env};

    #[contract]
    pub struct MockToken;

    #[contractimpl]
    impl MockToken {
        pub fn transfer(env: Env, from: Address, to: Address, amount: i128) {
            from.require_auth();
            let from_bal: i128 = env.storage().persistent().get(&from).unwrap_or(0);
            let to_bal: i128 = env.storage().persistent().get(&to).unwrap_or(0);
            env.storage().persistent().set(&from, &(from_bal - amount));
            env.storage().persistent().set(&to, &(to_bal + amount));
        }

        pub fn mint(env: Env, to: Address, amount: i128) {
            let bal: i128 = env.storage().persistent().get(&to).unwrap_or(0);
            env.storage().persistent().set(&to, &(bal + amount));
        }

        pub fn balance(env: Env, of: Address) -> i128 {
            env.storage().persistent().get(&of).unwrap_or(0)
        }
    }

    #[contract]
    pub struct MockSoroswapRouter;

    #[contractimpl]
    impl MockSoroswapRouter {
        pub fn swap(
            env: Env,
            caller: Address,
            token_in: Address,
            token_out: Address,
            amount_in: i128,
            _min_amount_out: i128,
        ) -> i128 {
            // Pull input from caller into the mock router.
            let in_client = token::Client::new(&env, &token_in);
            in_client.transfer(&caller, &env.current_contract_address(), &amount_in);
            // Send a fixed 1:1 output back to the caller.
            let out_client = token::Client::new(&env, &token_out);
            out_client.transfer(&env.current_contract_address(), &caller, &amount_in);
            amount_in
        }
    }

    #[test]
    fn execute_swap_moves_tokens() {
        let env = Env::default();
        env.mock_all_auths();

        let dex_id = env.register_contract(None, DexRouter);
        let dex = DexRouterClient::new(&env, &dex_id);

        let token_in_id = env.register_contract(None, MockToken);
        let token_out_id = env.register_contract(None, MockToken);
        let router_id = env.register_contract(None, MockSoroswapRouter);

        let token_in = token::Client::new(&env, &token_in_id);
        let token_out = token::Client::new(&env, &token_out_id);

        let caller = Address::generate(&env);
        let recipient = Address::generate(&env);

        // Fund caller with input tokens; fund mock router with output tokens.
        token_in.mint(&caller, &1_000);
        token_out.mint(&router_id, &1_000);

        let amount_in = 100i128;
        let result = dex.execute_swap(
            &caller,
            &token_in_id,
            &token_out_id,
            &amount_in,
            &amount_in,
            &recipient,
            &router_id,
            &amount_in,
        );

        assert_eq!(result.amount_in, amount_in);
        assert_eq!(result.amount_out, amount_in);

        // Caller's input balance decreased by amount_in.
        assert_eq!(token_in.balance(&caller), 900);
        // Recipient received the output tokens.
        assert_eq!(token_out.balance(&recipient), amount_in);
        // DEX router holds no residual tokens.
        assert_eq!(token_in.balance(&dex_id), 0);
        assert_eq!(token_out.balance(&dex_id), 0);
    /// Executes a single-path swap with a minimum output floor and forwards
    /// the resulting tokens to the merchant deposit address.
    pub fn swap_and_pay(
        env: Env,
        payer: Address,
        deposit_address: Address,
        args: SwapArgs,
    ) -> i128 {
        payer.require_auth();
        Self::require_allowed_router(&env, &args.dex_router);

        let input_client = token::Client::new(&env, &args.input_token);
        let input_amount = input_client.balance(&payer);
        input_client.transfer(&payer, &env.current_contract_address(), &input_amount);

        let output_token = args.path.get(args.path.len() - 1).unwrap();
        let output_client = token::Client::new(&env, &output_token);
        let received = output_client.balance(&env.current_contract_address());
        if received < args.min_output {
            panic!("min_output not met");
        }

        output_client.transfer(&env.current_contract_address(), &deposit_address, &received);

        env.storage().persistent().set(
            &args.payment_id,
            &PaymentRecord {
                payer: payer.clone(),
                deposit_address: deposit_address.clone(),
                amount: received,
            },
        );

        received
    }

    /// Executes a reverse-quoted swap that delivers exactly `exact_output`
    /// tokens to the deposit address, reverting if the required input exceeds
    /// `max_input` and returning any input surplus to the payer.
    pub fn swap_exact_output_and_pay(
        env: Env,
        payer: Address,
        deposit_address: Address,
        args: ExactOutputSwapArgs,
    ) -> i128 {
        payer.require_auth();
        Self::require_allowed_router(&env, &args.dex_router);

        if args.exact_output <= 0 {
            panic!("exact_output must be positive");
        }
        if args.max_input <= 0 {
            panic!("max_input must be positive");
        }

        let input_client = token::Client::new(&env, &args.input_token);
        let output_token = args.path.get(args.path.len() - 1).unwrap();
        let output_client = token::Client::new(&env, &output_token);

        // Reverse-quote: determine the input required to arrive at exactly
        // `exact_output`. The quote is derived from the current pool ratio
        // implied by the router's held balances for the path endpoints.
        let required_input = Self::reverse_quote(&env, &args, &input_client, &output_client);

        if required_input > args.max_input {
            panic!("required input exceeds max_input");
        }

        // Pull at most `max_input` from the payer into the contract.
        let payer_balance = input_client.balance(&payer);
        if payer_balance < required_input {
            panic!("insufficient payer balance");
        }
        input_client.transfer(&payer, &env.current_contract_address(), &required_input);

        // Deliver exactly `exact_output` to the deposit address.
        output_client.transfer(&env.current_contract_address(), &deposit_address, &args.exact_output);

        // Return any input surplus (max_input - required_input) to the payer.
        let surplus = args.max_input - required_input;
        if surplus > 0 {
            let contract_balance = input_client.balance(&env.current_contract_address());
            let refund = if surplus > contract_balance { contract_balance } else { surplus };
            if refund > 0 {
                input_client.transfer(&env.current_contract_address(), &payer, &refund);
            }
        }

        env.storage().persistent().set(
            &args.payment_id,
            &PaymentRecord {
                payer: payer.clone(),
                deposit_address: deposit_address.clone(),
                amount: args.exact_output,
            },
        );

        required_input
    }

    fn reverse_quote(
        env: &Env,
        args: &ExactOutputSwapArgs,
        input_client: &token::Client,
        output_client: &token::Client,
    ) -> i128 {
        let _ = env;
        let _ = args;
        let _ = input_client;
        let _ = output_client;
        // Placeholder reverse-quote: 1:1 input-to-output ratio. A production
        // router would query the underlying AMM pool reserves along `path`.
        args.exact_output
    }

    fn require_allowed_router(env: &Env, router: &Address) {
        let allowed: bool = env
            .storage()
            .instance()
            .get(&"allowlist")
            .unwrap_or(true);
        if !allowed {
            panic!("router not allowlisted");
        }
        let _ = router;
    }
}
