#![cfg(test)]

use crate::access_control::role_settlement_operator;
use crate::merchant_registry::{KycTier, MerchantRegistry, MerchantRegistryClient, MaybeFeeConfig};
use crate::refund_manager::{RefundManager, RefundManagerClient};
use crate::{DataKey, REFUND_FEE_BPS_BASIC, REFUND_FEE_BPS_BUSINESS, REFUND_FEE_BPS_FULL};
use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    token, Address, Env, String, Symbol,
};

fn setup(
    env: &Env,
) -> (
    Address, // admin
    Address, // operator
    Address, // payer
    Address, // refund_manager id
    RefundManagerClient<'_>,
    MerchantRegistryClient<'_>,
    Address, // usdc token
) {
    env.mock_all_auths();
    env.ledger().with_mut(|li| li.timestamp = 1_000);

    let admin = Address::generate(env);
    let operator = Address::generate(env);
    let payer = Address::generate(env);

    let usdc_admin = Address::generate(env);
    let usdc_token = env.register_stellar_asset_contract(usdc_admin);

    let registry_id = env.register(MerchantRegistry, ());
    let registry_client = MerchantRegistryClient::new(env, &registry_id);
    registry_client.initialize(&admin);

    let refund_manager_id = env.register(RefundManager, ());
    let refund_client = RefundManagerClient::new(env, &refund_manager_id);
    refund_client.initialize_refund_manager(&admin, &usdc_token);
    refund_client.grant_role(&admin, &role_settlement_operator(env), &operator);

    // Wire tier lookup from RefundManager -> MerchantRegistry.
    env.as_contract(&refund_manager_id, || {
        env.storage()
            .persistent()
            .set(&DataKey::MerchantRegistryAddress, &registry_id);
    });

    // Fund RefundManager so processed refunds can be paid out.
    let token_admin = token::StellarAssetClient::new(env, &usdc_token);
    token_admin.mint(&refund_manager_id, &1_000_000i128);

    (
        admin,
        operator,
        payer,
        refund_manager_id,
        refund_client,
        registry_client,
        usdc_token,
    )
}

fn register_merchant_with_tier(
    env: &Env,
    admin: &Address,
    registry: &MerchantRegistryClient<'_>,
    merchant_id: &Address,
    tier: KycTier,
) {
    registry.register_merchant(
        merchant_id,
        &String::from_str(env, "Merchant"),
        &String::from_str(env, "USDC"),
        &None,
        &None,
        &MaybeFeeConfig::None,
    );
    match tier {
        KycTier::Unverified => {}
        KycTier::Basic => {
            registry.verify_merchant(admin, merchant_id);
        }
        KycTier::Full | KycTier::Business => {
            registry.set_kyc_tier(admin, merchant_id, &tier, &None);
        }
    }
}

fn process_refund_and_get_fee(
    env: &Env,
    operator: &Address,
    payer: &Address,
    refund_client: &RefundManagerClient<'_>,
    merchant_id: &Address,
    payment_id: &str,
    amount: i128,
) -> i128 {
    let payment_id = String::from_str(env, payment_id);
    refund_client.register_payment_with_payer(
        &payment_id,
        merchant_id,
        payer,
        &amount,
        &Symbol::new(env, "USDC"),
    );

    // Pass refund cooldown (default 300s).
    env.ledger().with_mut(|li| li.timestamp += 301);

    let before = refund_client.get_treasury_balance();
    let refund_id = refund_client.create_refund(
        &payment_id,
        &amount,
        &String::from_str(env, "tiered fee test"),
        payer,
    );
    refund_client.process_refund(operator, &refund_id);
    let after = refund_client.get_treasury_balance();
    after - before
}

#[test]
fn test_each_kyc_tier_applies_configured_refund_fee_bps() {
    let env = Env::default();
    let (admin, operator, payer, _rm_id, refund_client, registry_client, _usdc) = setup(&env);

    let cases = [
        (KycTier::Basic, REFUND_FEE_BPS_BASIC),
        (KycTier::Full, REFUND_FEE_BPS_FULL),
        (KycTier::Business, REFUND_FEE_BPS_BUSINESS),
    ];

    for (idx, (tier, expected_bps)) in cases.iter().enumerate() {
        let merchant = Address::generate(&env);
        register_merchant_with_tier(&env, &admin, &registry_client, &merchant, tier.clone());

        let amount = 10_000i128;
        let payment_id = match idx {
            0 => "pay_basic",
            1 => "pay_full",
            _ => "pay_business",
        };
        let fee = process_refund_and_get_fee(
            &env,
            &operator,
            &payer,
            &refund_client,
            &merchant,
            payment_id,
            amount,
        );
        let expected_fee = amount * expected_bps / 10_000;
        assert_eq!(fee, expected_fee, "tier {:?} fee mismatch", tier);
    }
}

#[test]
fn test_admin_refund_fee_override_precedence_and_treasury_accrual() {
    let env = Env::default();
    let (admin, operator, payer, _rm_id, refund_client, registry_client, _usdc) = setup(&env);

    // Admin override applies as the default fee (used for Unverified tier / fallback).
    refund_client.set_refund_fee_bps(&admin, &200i128);
    assert_eq!(refund_client.get_refund_fee_bps(), 200i128);

    let merchant = Address::generate(&env);
    register_merchant_with_tier(
        &env,
        &admin,
        &registry_client,
        &merchant,
        KycTier::Unverified,
    );

    let amount = 10_000i128;
    let before = refund_client.get_treasury_balance();
    let fee = process_refund_and_get_fee(
        &env,
        &operator,
        &payer,
        &refund_client,
        &merchant,
        "pay_unverified_override",
        amount,
    );

    assert_eq!(fee, 200i128); // 2% admin override takes precedence over default
    assert_eq!(refund_client.get_treasury_balance(), before + 200i128);
}
