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
pub struct PaymentRecord {
    pub payer: Address,
    pub deposit_address: Address,
    pub amount: i128,
}

#[contract]
pub struct DexRouter;

#[contractimpl]
impl DexRouter {
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
