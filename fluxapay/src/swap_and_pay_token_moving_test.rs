//! Issue #1177: end-to-end swap_and_pay with a mock router that really moves
//! tokens, followed by verification and settlement.
#![cfg(test)]

use crate::merchant_registry::MaybeFeeConfig;
use crate::mock_dex_router::MockDexError;
use crate::{
    merchant_registry::{MerchantRegistry, MerchantRegistryClient},
    token, BytesN, Error, PaymentProcessor, PaymentProcessorClient, PaymentStatus, SettlementSplit,
    SwapAndPayArgs,
};
use soroban_sdk::{
    contract, contractimpl, testutils::Address as _, token::StellarAssetClient, vec, Address, Env,
    String, Symbol, Vec,
};

const PAYER_KEY: &str = "TMR_PAYER";
const OUTPUT_KEY: &str = "TMR_OUTPUT";

#[contract]
pub struct TokenMovingDexRouter;

#[contractimpl]
impl TokenMovingDexRouter {
    pub fn get_amounts_out(env: Env, amount_in: i128, path: Vec<Address>) -> Vec<i128> {
        if path.len() < 2 {
            return vec![&env];
        }
        let output: i128 = env.storage().persistent().get(&Symbol::new(&env, OUTPUT_KEY)).unwrap();
        let mut amounts = vec![&env];
        amounts.push_back(amount_in);
        amounts.push_back(output);
        amounts
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
        let output: i128 = env.storage().persistent().get(&Symbol::new(&env, OUTPUT_KEY)).unwrap();
        if output < amount_out_min {
            return Err(MockDexError::InsufficientOutput);
        }
        let payer: Address = env.storage().persistent().get(&Symbol::new(&env, PAYER_KEY)).unwrap();
        let token_in = path.get(0).unwrap();
        let token_out = path.get(path.len() - 1).unwrap();
        let router = env.current_contract_address();
        token::Client::new(&env, &token_in).transfer(&payer, &router, &amount_in);
        token::Client::new(&env, &token_out).transfer(&router, &to, &output);
        let mut amounts = vec![&env];
        amounts.push_back(amount_in);
        amounts.push_back(output);
        Ok(amounts)
    }
}

fn configure_router(env: &Env, router: &Address, payer: &Address, output: i128) {
    env.as_contract(router, || {
        env.storage()
            .persistent()
            .set(&Symbol::new(env, PAYER_KEY), payer);
        env.storage()
            .persistent()
            .set(&Symbol::new(env, OUTPUT_KEY), &output);
    });
}

struct Fixture<'a> {
    env: Env,
    admin: Address,
    client: PaymentProcessorClient<'a>,
    merchant_client: MerchantRegistryClient<'a>,
    merchant: Address,
    router: Address,
    token_in: Address,
    token_out: Address,
    payer: Address,
    deposit: Address,
}

fn setup(env: &Env, output: i128) -> Fixture<'_> {
    let payment_processor = env.register(PaymentProcessor, ());
    let merchant_registry = env.register(MerchantRegistry, ());
    let client = PaymentProcessorClient::new(env, &payment_processor);
    let merchant_client = MerchantRegistryClient::new(env, &merchant_registry);
    let admin = Address::generate(env);
    client.initialize_payment_processor(&admin);
    merchant_client.initialize(&admin);
    client.set_merchant_registry_address(&admin, &merchant_registry);

    let token_admin = Address::generate(env);
    let token_in = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();
    let token_out = env
        .register_stellar_asset_contract_v2(token_admin)
        .address();
    client.allow_token(&admin, &token_out);

    let router = env.register(TokenMovingDexRouter, ());
    let payer = Address::generate(env);
    let deposit = Address::generate(env);
    configure_router(env, &router, &payer, output);

    let merchant = Address::generate(env);
    merchant_client.register_merchant(
        &merchant,
        &String::from_str(env, "TokenMovingShop"),
        &String::from_str(env, "USD"),
        &None,
        &None,
        &MaybeFeeConfig::None,
    );
    merchant_client.verify_merchant(&admin, &merchant);
    client.grant_role(&admin, &Symbol::new(env, "MERCHANT"), &merchant);

    Fixture {
        env: env.clone(),
        admin,
        client,
        merchant_client,
        merchant,
        router,
        token_in,
        token_out,
        payer,
        deposit,
    }
}

fn args(f: &Fixture<'_>, amount: i128, amount_in: i128, amount_out_min: i128) -> SwapAndPayArgs {
    SwapAndPayArgs {
        payer: f.payer.clone(),
        payment_id: String::from_str(&f.env, "SWAP_TOKEN_MOVING_01"),
        merchant_id: f.merchant.clone(),
        amount,
        currency: Symbol::new(&f.env, "USDC"),
        deposit_address: f.deposit.clone(),
        token_in: f.token_in.clone(),
        amount_in,
        amount_out_min,
        path: vec![&f.env, f.token_in.clone(), f.token_out.clone()],
        expires_at: Some(f.env.ledger().timestamp() + 3600),
        dex_router: f.router.clone(),
        fx_oracle: None,
        oracle_pair: None,
        max_deviation_bps: 0,
    }
}

#[test]
fn test_swap_and_pay_moves_tokens_through_settle() {
    let env = Env::default();
    env.mock_all_auths();
    let f = setup(&env, 9_900);

    StellarAssetClient::new(&env, &f.token_in).mint(&f.payer, &10_000);
    StellarAssetClient::new(&env, &f.token_out).mint(&f.router, &100_000);
    let token_in = token::Client::new(&env, &f.token_in);
    let token_out = token::Client::new(&env, &f.token_out);

    let payment = f.client.swap_and_pay(&args(&f, 9_900, 10_000, 9_900));
    assert_eq!(payment.status, PaymentStatus::Pending);

    // The router pulled input tokens from the payer and pushed output to the
    // deposit address — real balance changes, not just mocked return values.
    assert_eq!(token_in.balance(&f.payer), 0);
    assert_eq!(token_in.balance(&f.router), 10_000);
    assert_eq!(token_out.balance(&f.deposit), 9_900);
    assert_eq!(token_out.balance(&f.merchant), 0);

    let oracle = Address::generate(&env);
    f.client
        .grant_role(&f.admin, &Symbol::new(&env, "ORACLE"), &oracle);
    f.client.verify_payment(
        &oracle,
        &payment.payment_id,
        &BytesN::<32>::from_array(&env, &[7u8; 32]),
        &f.payer,
        &9_900,
        &None::<u64>,
        &None::<i128>,
    );
    assert_eq!(f.client.get_payment(&payment.payment_id).status, PaymentStatus::Confirmed);

    let operator = Address::generate(&env);
    f.client.grant_role(
        &f.admin,
        &Symbol::new(&env, "SETTLEMENT_OPERATOR"),
        &operator,
    );
    let splits = vec![
        &env,
        SettlementSplit {
            recipient: f.merchant.clone(),
            amount: 9_900,
        },
    ];
    f.client.settle_payment(&operator, &payment.payment_id, &splits);

    assert_eq!(token_out.balance(&f.merchant), 9_900);
    assert_eq!(token_out.balance(&f.deposit), 0);
    assert_eq!(f.client.get_payment(&payment.payment_id).status, PaymentStatus::Settled);
}

#[test]
fn test_swap_and_pay_slippage_revert_leaves_balances_untouched() {
    let env = Env::default();
    env.mock_all_auths();
    let f = setup(&env, 8_000);

    StellarAssetClient::new(&env, &f.token_in).mint(&f.payer, &10_000);
    StellarAssetClient::new(&env, &f.token_out).mint(&f.router, &100_000);

    let result = f.client.try_swap_and_pay(&args(&f, 9_900, 10_000, 9_900));
    assert!(result.is_err(), "quote below minimum must be rejected");
    assert_eq!(
        result,
        Err(Ok(Error::InvalidAmount)),
        "slippage path rejects the insufficient quote directly"
    );

    assert_eq!(token::Client::new(&env, &f.token_in).balance(&f.payer), 10_000);
    assert_eq!(token::Client::new(&env, &f.token_out).balance(&f.router), 100_000);
    assert!(f.client.try_get_payment(&String::from_str(&env, "SWAP_TOKEN_MOVING_01")).is_err());
}
