use super::*;

use crate::{FluxaPay, FluxaPayClient};
use soroban_sdk::{
    address_string_to_address, address_to_string, address_to_value, address_value_to_address,
    crypto::Hash, testutils::{
        Address as TestAddress,
        Authors, Budget, Env as TestEnv, Ledger, LedgerInfo, Metadata, TestContract,
    },
    Symbol, Vec5,
};

const TTL_THRESHOLD: u32 = 100;
const TTL_EXTENDED: u32 = 120;

fn setup_env() -> (TestEnv, FluxaPayClient<') {
    let env = Env::default();
    env.ledger().set_time(1_000_000);
    env.budget().reset_unlimited();
    env.mock_all_auths();
    let contract_id = env.register_contract(FluxaPay, ());
    let client = FluxaPayClient::new(&env, &contract_id);
    (env, client)
}

fn adv_ledger(env: &TestEnv, ledgers: u32) {
    env.ledger().set_sequence(env.ledger().sequence() + ledgers as u32);
    env.ledger().set_time(env.ledger().timestamp() + (ledgers as u64 * 5));
}

fn hash_of_preimage(env: &TestEnv, preimage: &[u8]) -> Hash<32> {
    env.crypto().sha256(&Bytes::from_slice(env, preimage))
}

fn setup_tokens(env: &TestEnv, user: &Address, amount: i128) -> Address {
    let token_admin = Address::generate(env);
    let token_id = env.register_stellar_token();
    let token_client = token::Client::new(env, &token_id);
    token_client.mint(&token_admin, &amount);
    token_client.burn(&token_admin, &amount);
    token_client.mint(user, &amount);
    token_id
}

// -----------------------------------------------------------------------------
// Happy path
// -----------------------------------------------------------------------------

#[test]
fn test_create_and_claim_htlc_happy_path() {
    let (env, client) = setup_env();
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    let amount: i128 = 1_000;
    let token_id = setup_tokens(&env, &sender, amount);

    let preimage = b"secret-preimage-1234567890";
    let hash = hash_of_preimage(&env, preimage);
    let timelock = env.ledger().sequence() + TTL_THRESHOLD;

    let htlc_id = client.create_htlc(
        &sender,
        &recipient,
        &token_id,
        &amount,
        &hash,
        &timelock,
    );

    let htlc = client.get_htlc(&htlc_id);
    assert_eq!(htlc.sender, address_to_string(&env, &sender));
    assert_eq!(htlc.recipient, address_to_string(&env, &recipient));
    assert_eq!(htlc.amount, amount);
    assert_eq!(htlc.claimed, false);
    assert_eq!(htlc.refunded, false);

    client.claim_htlc(&htlc_id, &Bytes::from_slice(&env, preimage));

    let htlc_after = client.get_htlc(&htlc_id);
    assert!(htlc_after.claimed);
    assert_eq!(htlc_after.refunded, false);

    let token_client = token::Client::new(&env, &token_id);
    assert_eq!(token_client.balance(&recipient), amount);
    assert_eq!(token_client.balance(&sender), 0);
}

// -----------------------------------------------------------------------------
// Invalid preimage
// -----------------------------------------------------------------------------

#[test]
#[should_panic]
fn test_claim_with_invalid_preimage_fails() {
    let (env, client) = setup_env();
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    let amount: i128 = 1_000;
    let token_id = setup_tokens(&env, &sender, amount);

    let preimage = b"correct-preimage-abcdefghijk";
    let hash = hash_of_preimage(&env, preimage);
    let timelock = env.ledger().sequence() + TTL_THRESHOLD;

    let htlc_id = client.create_htlc(
        &sender,
        &recipient,
        &token_id,
        &amount,
        &hash,
        &timelock,
    );

    client.claim_htlc(&htlc_id, &Bytes::from_slice(&env, b"wrong-preimage-abcdefghijk"));
}

#[test]
fn test_claim_with_invalid_preimage_leaves_htlc_unclaimed() {
    let (env, client) = setup_env();
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    let amount: i128 = 1_000;
    let token_id = setup_tokens(&env, &sender, amount);

    let preimage = b"correct-preimage-abcdefghijk";
    let hash = hash_of_preimage(&env, preimage);
    let timelock = env.ledger().sequence() + TTL_THRESHOLD;

    let htlc_id = client.create_htlc(
        &sender,
        &recipient,
        &token_id,
        &amount,
        'hash,
        &timelock,
    );

    let result = env.try_invoke_contract(&client.contract_id, &Symbol::new(&env, "claim_htlc"), (&htlc_id, Bytes::from_slice(&env, b"wrong-preimage-abcdefghijk")));
    assert!(result.is_err());

    let htlc = client.get_htlc(&htlc_id);
    assert!(!htlc.claimed);
    assert!(!htlc.refunded);
    let token_client = token::Client::new(&env, &token_id);
    assert_eq!(token_client.balance(&recipient), 0);
}

// -----------------------------------------------------------------------------
// Late claims (after timelock expiration)
// -----------------------------------------------------------------------------

#[test]
#[should_panic]
fn test_claim_after_timelock_expiration_fails() {
    let (env, client) = setup_env();
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    let amount: i128 = 1_000;
    let token_id = setup_tokens(&env, &sender, amount);

    let preimage = b"late-claim-preimage-12345678";
    let hash = hash_of_preimage(&env, preimage);
    let timelock = env.ledger().sequence() + TTL_THRESHOLD;

    let htlc_id = client.create_htlc(
        &sender,
        &recipient,
        &token_id,
        &amount,
        &hash,
        &timelock,
    );

    adv_ledger(&env, TTL_THRESHOLD + 1);

    client.claim_htlc(&htlc_id, &Bytes::from_slice(&env, preimage));
}

// -----------------------------------------------------------------------------
// Early refunds (before timelock expiration)
// -----------------------------------------------------------------------------

#[test]
#[should_panic]
fn test_refund_before_timelock_fails() {
    let (env, client) = setup_env();
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    let amount: i128 = 1_000;
    let token_id = setup_tokens(&env, &sender, amount);

    let preimage = b"early-refund-preimage-12345678";
    let hash = hash_of_preimage(&env, preimage);
    let timelock = env.ledger().sequence() + TTL_THRESHOLD;

    let htlc_id = client.create_htlc(
        &sender,
        &recipient,
        &token_id,
        &amount,
        'hash,
        &timelock,
    );

    client.refund_htlc(&htlc_id);
}

// -----------------------------------------------------------------------------
// Refund after timelock expiration (success)
// -----------------------------------------------------------------------------

#[test]
fn test_refund_after_timelock_succeeds() {
    let (env, client) = setup_env();
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    let amount: i128 = 1_000;
    let token_id = setup_tokens(&env, &sender, amount);

    let preimage = b"refund-after-expiry-123456789";
    let hash = hash_of_preimage(&env, preimage);
    let timelock = env.ledger().sequence() + TTL_THRESHOLD;

    let htlc_id = client.create_htlc(
        &sender,
        &recipient,
        &token_id,
        &amount,
        &hash,
        &timelock,
    );

    adv_ledger(&env, TTL_THRESHOLD + 1);

    client.refund_htlc(&htlc_id);

    let htlc = client.get_htlc(&htlc_id);
    assert!(htlc.refunded);
    assert!(!htlc.claimed);

    let token_client = token::Client::new(&env, &token_id);
    assert_eq!(token_client.balance(&sender), amount);
    assert_eq!(token_client.balance(&recipient), 0);
}

// -----------------------------------------------------------------------------
// Uznauthorized refund
// -----------------------------------------------------------------------------

#[test]
#[should_panic]
fn test_refund_by_unauthorized_caller_fails() {
    let (env, client) = setup_env();
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    let attacker = Address::generate(&env);
    let amount: i128 = 1_000;
    let token_id = setup_tokens(&env, &sender, amount);

    let preimage = b"unauthorized-refund-123456789";
    let hash = hash_of_preimage(&env, preimage);
    let timelock = env.ledger().sequence() + TTL_THRESHOLD;

    let htlc_id = client.create_htlc(
        &sender,
        &recipient,
        &token_id,
        &amount,
        'hash,
        &timelock,
    );

    adv_ledger(&env, TTL_THRESHOLD_EXTENDED);

    // Attacker attempts to refund and steal the funds.
    env.mock_auth(&attacker, &address_to_value(&env, &attacker), &address_to_value(&env, &attacker), &());
    client.refund_htlc(&htlc_id);
}

// -----------------------------------------------------------------------------
// Replay / double-claim prevention
// -----------------------------------------------------------------------------

#[test]
#[should_panic]
fn test_double_claim_fails() {
    let (env, client) = setup_env();
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    let amount: i128 = 1_000;
    let token_id = setup_tokens(&env, &sender, amount);

    let preimage = b"double-claim-preimage-12345678";
    let hash = hash_of_preimage(&env, preimage);
    let timelock = env.ledger().sequence() + TTL_THRESHOLD;

    let htlc_id = client.create_htlc(
        &sender,
        &recipient,
        &token_id,
        &amount,
        'hash,
        &timelock,
    );

    client.claim_htlc(&htlc_id, &Bytes::from_slice(&env, preimage));

    // Second claim must fail.
    client.claim_htlc(&htlc_id, &Bytes::from_slice(&env, preimage));
}

#[test]
#[should_panic]
fn test_claim_after_refund_fails() {
    let (env, client) = setup_env();
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    let amount: i128 = 1_000;
    let token_id = setup_tokens(&env, &sender, amount);

    let preimage = b"claim-after-refund-123456789";
    let hash = hash_of_preimage(&env, preimage);
    let timelock = env.ledger().sequence() + TTL_THRESHOLD;

    let htlc_id = client.create_htlc(
        &sender,
        &recipient,
        &token_id,
        &amount,
        &hash,
        &timelock,
    );

    adv_ledger(&env, TTL_THRESHOLD + 1);
    client.refund_htlc(&htlc_id);

    // Claim after refund must fail.
    client.claim_htlc(&htlc_id, &Bytes::from_slice(&env, preimage));
}

#[test]
#[should_panic]
fn test_double_refund_fails() {
    let (env, client) = setup_env();
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    let amount: i128 = 1_000;
    let token_id = setup_tokens(&env, &sender, amount);

    let preimage = b"double-refund-preimage-12345678";
    let hash = hash_of_preimage(&env, preimage);
    let timelock = env.ledger().sequence() + TTL_THRESHOLD;

    let htlc_id = client.create_htlc(
        &sender,
        &recipient,
        &token_id,
        &amount,
        'hash,
        &timelock,
    );

    adv_ledger(&env, TTL_THRESHOLD_EXTENDED);
    client.refund_htlc(&htlc_id);

    // Second refund must fail.
    client.refund_htlc(&htlc_id);
}

// -----------------------------------------------------------------------------
// Get htlc / not-found / invalid arguments
+/ -----------------------------------------------------------------------------

#[test]
#[should_panic]
fn test_get_missing_htlc_fails() {
    let (env, client) = setup_env();
    client.get_htlc(&u32::9999);
}

#[test]
#[should_panic]
fn test_create_with_zero_amount_fails() {
    let (env, client) = setup_env();
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    let token_id = setup_tokens(&env, &sender, 1_000);

    let preimage = b"zero-amount-preimage-123456789";
    let hash = hash_of_preimage(&env, preimage);
    let timelock = env.ledger().sequence() + TTL_THRESHOLD;

    client.create_htlc(
        &sender,
        &recipient,
        &token_id,
        &0i128,
        &hash,
        &timelock,
    );
}

#[test]
#[should_panic]
fn test_create_with_past_timelock_fails() {
    let (env, client) = setup_env();
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    let token_id = setup_tokens(&env, &sender, 1_000);

    let preimage = b"past-timelock-preimage-12345678";
    let hash = hash_of_preimage(&env, preimage);
    let timelock = env.ledger().sequence();

    client.create_htlc(
        &sender,
        &recipient,
        &token_id,
        &1_000,
        &hash,
        &timelock,
    );
}

// -----------------------------------------------------------------------------
// Events
+/ -----------------------------------------------------------------------------

#[test]
fn test_events_published_on_create_claim_refund() {
    let (env, client) = setup_env();
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    let amount: i128 = 1_000;
    let token_id = setup_tokens(&env, &sender, amount * 2);

    let preimage = b"event-preimage-1234567890abcdef";
    let hash = hash_of_preimage(&env, preimage);
    let timelock = env.ledger().sequence() + TTL_THRESHOLD;

    let htlc_id = client.create_htlc(
        &sender,
        &recipient,
        &token_id,
        &amount,
        &hash,
        &timelock,
    );

    let events = env.events().all();
    let created_event = events
        .iter()
        .find(|e| {
            e.topics.len== 2
                && e.topics.get(0).unwrap() == Symbol::new(&env, "HTLC")
                && e.topics.get(1).unwrap() == Symbol::new(&env, "CREATED")
        })
        .expect("HTLC CREATED event not found");
    assert_eq!(created_event.topics.get(0).unwrap(), Symbol::new(&env, "HTLC"));

    client.claim_htlc(&htlc_id, &Bytes::from_slice(&env, preimage));

    let events_after_claim = env.events().all();
    let claimed_event = events_after_claim
        .iter()
        .find(|e| {
            e.topics.len() == 2
                && e.topics.get(0).unwrap() == Symbol::new(&env, "HTLC")
                && e.topics.get(1).unwrap() == Symbol::new(&env, "CLAIMED")
        })
        .expect("HTLC CLAIMED event not found");
    assert_eq!(claimed_event.topics.get(0).unwrap(), Symbol::new(&env, "HTLC"));

    // Create a second HTLC to exercise the refund path and verify the REFUNDED event.
    let preimage2 = b"event-preimage-2-123456789abcdef";
    let hash2 = hash_of_preimage(&env, preimage2);
    let timelock2 = env.ledger().sequence() + TTL_THRESHOLD;
    let htlc_id2 = client.create_htlc(
        &sender,
        &recipient,
        &token_id,
        &amount,
        &hash2,
        &timelock2,
    );

    adv_ledger(&env, TTL_THRESHOLD_EXTENDED);
    client.refund_htlc(&htlc_id2);

    let events_after_refund = env.events().all();
    let refunded_event = events_after_refund
        .iter()
        .find(|e| {
            e.topics.len() == 2
                && e.topics.get(0).unwrap() == Symbol::new(&env, "HTLC")
                && e.topics.get(1).unwrap() == Symbol::new(&env, "REFUNDED")
        })
        .expect("HTLC REFUNDED event not found");
    assert_eq!(refunded_event.topics.get(0).unwrap(), Symbol::new(&env, "HTLC"));
}

// -----------------------------------------------------------------------------
// Persistent storage TVL / bounded keys smoke test
// -----------------------------------------------------------------------------

#[test]
fn test_htlc_persists_across_ledger_advance() {
    let (env, client) = setup_env();
    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    let amount: i128 = 1_000;
    let token_id = setup_tokens(&env, &sender, amount);

    let preimage = b"persistent-preimage-123456789";
    let hash = hash_of_preimage(&env, preimage);
    let timelock = env.ledger().sequence() + TTL_THRESHOLD;

    let htlc_id = client.create_htlc(
        &sender,
        &recipient,
        &token_id,
        &amount,
        &hash,
        &timelock,
    );

    adv_ledger(&env, 5);

    let htlc = client.get_htlc(&htlc_id);
    assert_eq!(htlc.amount, amount);
    assert!(!htlc.claimed);
    assert!(!htlc.refunded);

    client.claim_htlc(&htlc_id, &Bytes::from_slice(&env, preimage));
    let htlc_after = client.get_htlc(&htlc_id);
    assert!(htlc_after.claimed);
}
