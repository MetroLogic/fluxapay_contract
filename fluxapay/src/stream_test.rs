#![cfg(test)]

use super::stream::{PaymentStreaming, PaymentStreamingClient, StreamError, StreamStatus};
use crate::utils::format_id;
use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    token, vec, Address, Env, String,
};

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn setup(env: &Env) -> (PaymentStreamingClient<'_>, Address, Address, Address) {
    let contract_id = env.register(PaymentStreaming, ());
    let client = PaymentStreamingClient::new(env, &contract_id);

    let token_admin = Address::generate(env);
    let token_id = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();

    let sender = Address::generate(env);
    let receiver = Address::generate(env);

    // Fund sender with 1 000 000 tokens.
    let token_admin_client = token::StellarAssetClient::new(env, &token_id);
    token_admin_client.mint(&sender, &1_000_000i128);

    // Allow contract to pull tokens from sender (pre-approve).
    // mock_all_auths covers both sender.require_auth() and token transfer.
    (client, sender, receiver, token_id)
}

// ─── create_stream ─────────────────────────────────────────────────────────────

#[test]
fn test_create_stream_success() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, sender, receiver, token) = setup(&env);

    let stream_id = String::from_str(&env, "stream_01");
    let rate = 10i128; // 10 tokens/s
    let deposit = 1_000i128;

    let stream = client.create_stream(&sender, &receiver, &token, &rate, &deposit, &stream_id, &None::<i128>);

    assert_eq!(stream.stream_id, stream_id);
    assert_eq!(stream.sender, sender);
    assert_eq!(stream.receiver, receiver);
    assert_eq!(stream.rate_per_second, rate);
    assert_eq!(stream.remaining_deposit, deposit);
    assert_eq!(stream.accrued_at_checkpoint, 0);
    assert_eq!(stream.status, StreamStatus::Active);
}

#[test]
fn test_create_stream_invalid_rate() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, sender, receiver, token) = setup(&env);
    let stream_id = String::from_str(&env, "stream_rate_err");

    let err = client.try_create_stream(&sender, &receiver, &token, &0i128, &500i128, &stream_id, &None::<i128>);
    assert_eq!(err, Err(Ok(StreamError::InvalidRate)));
}

#[test]
fn test_create_stream_invalid_deposit() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, sender, receiver, token) = setup(&env);
    let stream_id = String::from_str(&env, "stream_dep_err");

    let err = client.try_create_stream(&sender, &receiver, &token, &5i128, &0i128, &stream_id, &None::<i128>);
    assert_eq!(err, Err(Ok(StreamError::InvalidDeposit)));
}

#[test]
fn test_create_stream_duplicate_id() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, sender, receiver, token) = setup(&env);
    let stream_id = String::from_str(&env, "stream_dup");

    client.create_stream(&sender, &receiver, &token, &10i128, &500i128, &stream_id, &None::<i128>);

    let err = client.try_create_stream(&sender, &receiver, &token, &10i128, &500i128, &stream_id, &None::<i128>);
    assert_eq!(err, Err(Ok(StreamError::StreamAlreadyExists)));
}

// ─── decrease_rate_per_second ─────────────────────────────────────────────────

/// After 50 seconds at 10 tok/s → accrued = 500.
/// New rate = 5. Old unlocked = 1000 − 500 = 500; old_secs_left = 500/10 = 50.
/// new_needed = 50 * 5 = 250 → surplus = 500 − 250 = 250.
#[test]
fn test_decrease_rate_checkpoints_and_refunds_surplus() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, sender, receiver, token) = setup(&env);

    let stream_id = String::from_str(&env, "stream_dec");
    let deposit = 1_000i128;
    let old_rate = 10i128;

    client.create_stream(&sender, &receiver, &token, &old_rate, &deposit, &stream_id, &None::<i128>);

    // Advance time by 50 seconds.
    env.ledger().set_timestamp(env.ledger().timestamp() + 50);

    client.decrease_rate_per_second(&sender, &stream_id, &5i128);

    let stream = client.get_stream(&stream_id);

    // Checkpoint should reflect 50s * 10 tok/s = 500 accrued.
    assert_eq!(stream.accrued_at_checkpoint, 500);
    // New rate applied.
    assert_eq!(stream.rate_per_second, 5);
    // Deposit reduced by surplus (250).
    assert_eq!(stream.remaining_deposit, deposit - 250); // 750
}

/// Verify that the accrued view helper is correct before and after a rate decrease.
#[test]
fn test_get_accrued_amount_reflects_elapsed_time() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, sender, receiver, token) = setup(&env);

    let stream_id = String::from_str(&env, "stream_accrued");
    client.create_stream(&sender, &receiver, &token, &10i128, &500i128, &stream_id, &None::<i128>);

    // 30 seconds in → 300 accrued (lazily).
    env.ledger().set_timestamp(env.ledger().timestamp() + 30);
    assert_eq!(client.get_accrued_amount(&stream_id), 300);
}

/// Decreasing rate requires the new rate < current rate.
#[test]
fn test_decrease_rate_rejects_equal_rate() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, sender, receiver, token) = setup(&env);

    let stream_id = String::from_str(&env, "stream_eq");
    client.create_stream(&sender, &receiver, &token, &10i128, &500i128, &stream_id, &None::<i128>);

    let err = client.try_decrease_rate_per_second(&sender, &stream_id, &10i128);
    assert_eq!(err, Err(Ok(StreamError::RateNotDecreased)));
}

/// Decreasing rate to higher value is also rejected.
#[test]
fn test_decrease_rate_rejects_higher_rate() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, sender, receiver, token) = setup(&env);

    let stream_id = String::from_str(&env, "stream_hi");
    client.create_stream(&sender, &receiver, &token, &10i128, &500i128, &stream_id, &None::<i128>);

    let err = client.try_decrease_rate_per_second(&sender, &stream_id, &20i128);
    assert_eq!(err, Err(Ok(StreamError::RateNotDecreased)));
}

/// Only the original sender may decrease the rate.
#[test]
fn test_decrease_rate_unauthorized_caller() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, sender, receiver, token) = setup(&env);

    let stream_id = String::from_str(&env, "stream_auth");
    client.create_stream(&sender, &receiver, &token, &10i128, &500i128, &stream_id, &None::<i128>);

    let impostor = Address::generate(&env);
    let err = client.try_decrease_rate_per_second(&impostor, &stream_id, &5i128);
    assert_eq!(err, Err(Ok(StreamError::Unauthorized)));
}

/// Decreasing rate on a non-existent stream returns StreamNotFound.
#[test]
fn test_decrease_rate_stream_not_found() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, sender, _receiver, _token) = setup(&env);

    let bad_id = String::from_str(&env, "no_such_stream");
    let err = client.try_decrease_rate_per_second(&sender, &bad_id, &5i128);
    assert_eq!(err, Err(Ok(StreamError::StreamNotFound)));
}

/// Multiple sequential rate decreases each checkpoint correctly.
#[test]
fn test_multiple_sequential_rate_decreases() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, sender, receiver, token) = setup(&env);

    // Deposit 10 000 tok, rate 100 tok/s → would last 100s.
    let stream_id = String::from_str(&env, "stream_multi");
    client.create_stream(
        &sender,
        &receiver,
        &token,
        &100i128,
        &10_000i128,
        &stream_id,
        &None::<i128>,
    );

    // After 20s → accrued 2000; reduce to 50 tok/s.
    // Unlocked after checkpoint: 10000−2000=8000; old_secs_left=8000/100=80
    // new_needed=80*50=4000; surplus=4000; deposit becomes 6000.
    env.ledger().set_timestamp(env.ledger().timestamp() + 20);
    client.decrease_rate_per_second(&sender, &stream_id, &50i128);

    let s = client.get_stream(&stream_id);
    assert_eq!(s.accrued_at_checkpoint, 2_000);
    assert_eq!(s.rate_per_second, 50);
    assert_eq!(s.remaining_deposit, 6_000);

    // After another 10s → accrued_since_checkpoint = 10*50 = 500; reduce to 10.
    // Unlocked: 6000−(2000+500)=3500; old_secs_left=3500/50=70
    // new_needed=70*10=700; surplus=2800; deposit becomes 6000−2800=3200.
    env.ledger().set_timestamp(env.ledger().timestamp() + 10);
    client.decrease_rate_per_second(&sender, &stream_id, &10i128);

    let s2 = client.get_stream(&stream_id);
    assert_eq!(s2.accrued_at_checkpoint, 2_500); // 2000+500
    assert_eq!(s2.rate_per_second, 10);
    assert_eq!(s2.remaining_deposit, 3_200);
}

#[test]
fn test_set_stream_destination_and_trigger_withdrawal() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, sender, receiver, token) = setup(&env);
    let token_client = token::StellarAssetClient::new(&env, &token);

    let stream_id = String::from_str(&env, "stream_dest");
    let destination = Address::generate(&env);

    client.create_stream(&sender, &receiver, &token, &10i128, &500i128, &stream_id, &None::<i128>);
    client.set_stream_destination(&receiver, &stream_id, &destination);
    client.approve_stream_milestone(&sender, &stream_id);

    env.ledger().set_timestamp(env.ledger().timestamp() + 10);
    let processed = client.trigger_withdrawal(&stream_id);

    assert_eq!(processed, stream_id);
    assert_eq!(token_client.balance(&destination), 100i128);

    let stream = client.get_stream(&stream_id);
    assert_eq!(stream.accrued_at_checkpoint, 0);
    assert_eq!(stream.remaining_deposit, 400);
}

#[test]
fn test_withdraw_all_for_recipient_limits_execution() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, sender, receiver, token) = setup(&env);
    let token_client = token::StellarAssetClient::new(&env, &token);

    let stream_id1 = String::from_str(&env, "stream_all_1");
    let stream_id2 = String::from_str(&env, "stream_all_2");
    let stream_id3 = String::from_str(&env, "stream_all_3");

    client.create_stream(&sender, &receiver, &token, &10i128, &500i128, &stream_id1, &None::<i128>);
    client.create_stream(&sender, &receiver, &token, &20i128, &500i128, &stream_id2, &None::<i128>);
    client.create_stream(&sender, &receiver, &token, &30i128, &500i128, &stream_id3, &None::<i128>);
    client.approve_stream_milestone(&sender, &stream_id1);
    client.approve_stream_milestone(&sender, &stream_id2);
    client.approve_stream_milestone(&sender, &stream_id3);

    env.ledger().set_timestamp(env.ledger().timestamp() + 10);

    let processed = client.withdraw_all_for_recipient(&receiver, &2u32);
    assert_eq!(processed.len(), 2);
    assert_eq!(token_client.balance(&receiver), 100 + 200);

    let next = client.withdraw_all_for_recipient(&receiver, &2u32);
    assert_eq!(next.len(), 1);
    assert_eq!(token_client.balance(&receiver), 100 + 200 + 300);
}

#[test]
fn test_get_sender_streams_pagination() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, sender, receiver, token) = setup(&env);

    for i in 0..5 {
        let stream_id = format_id(&env, "sender_page_", i as u64);
        client.create_stream(&sender, &receiver, &token, &10i128, &500i128, &stream_id, &None::<i128>);
    }

    let page1 = client.get_sender_streams(&sender, &0u32, &2u32);
    assert_eq!(page1.len(), 2);
    let page2 = client.get_sender_streams(&sender, &1u32, &2u32);
    assert_eq!(page2.len(), 2);
    let page3 = client.get_sender_streams(&sender, &2u32, &2u32);
    assert_eq!(page3.len(), 1);
}

// ─── Milestone approval gate ───────────────────────────────────────────────────

#[test]
fn test_withdraw_blocked_before_milestone_approval() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, sender, receiver, token) = setup(&env);

    let stream_id = String::from_str(&env, "stream_milestone_locked");
    let destination = Address::generate(&env);
    client.create_stream(&sender, &receiver, &token, &10i128, &500i128, &stream_id, &None::<i128>);
    client.set_stream_destination(&receiver, &stream_id, &destination);

    env.ledger().set_timestamp(env.ledger().timestamp() + 10);
    let err = client.try_trigger_withdrawal(&stream_id);
    assert_eq!(err, Err(Ok(StreamError::MilestoneNotApproved)));
}

#[test]
fn test_withdrawn_event_includes_remaining_deposit() {
fn test_approve_stream_milestone_unblocks_withdrawal() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, sender, receiver, token) = setup(&env);
    let token_client = token::StellarAssetClient::new(&env, &token);

    let stream_id = String::from_str(&env, "stream_milestone_approved");
    let destination = Address::generate(&env);
    client.create_stream(&sender, &receiver, &token, &10i128, &500i128, &stream_id, &None::<i128>);
    client.set_stream_destination(&receiver, &stream_id, &destination);
    client.approve_stream_milestone(&sender, &stream_id);

    env.ledger().set_timestamp(env.ledger().timestamp() + 10);
    let processed = client.trigger_withdrawal(&stream_id);

    assert_eq!(processed, stream_id);
    assert_eq!(token_client.balance(&destination), 100i128);
}

#[test]
fn test_revoke_stream_milestone_relocks_withdrawal() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, sender, receiver, token) = setup(&env);

    let stream_id = String::from_str(&env, "stream_milestone_revoked");
    let destination = Address::generate(&env);
    client.create_stream(&sender, &receiver, &token, &10i128, &500i128, &stream_id, &None::<i128>);
    client.set_stream_destination(&receiver, &stream_id, &destination);
    client.approve_stream_milestone(&sender, &stream_id);
    client.revoke_stream_milestone(&sender, &stream_id);

    env.ledger().set_timestamp(env.ledger().timestamp() + 10);
    let err = client.try_trigger_withdrawal(&stream_id);
    assert_eq!(err, Err(Ok(StreamError::MilestoneNotApproved)));
}

#[test]
fn test_approve_stream_milestone_unauthorized_caller() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, sender, receiver, token) = setup(&env);

    let stream_id = String::from_str(&env, "stream_milestone_auth");
    client.create_stream(&sender, &receiver, &token, &10i128, &500i128, &stream_id, &None::<i128>);

    let impostor = Address::generate(&env);
    let err = client.try_approve_stream_milestone(&impostor, &stream_id);
    assert_eq!(err, Err(Ok(StreamError::Unauthorized)));
}

#[test]
fn test_withdrawn_event_includes_remaining_deposit() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, sender, receiver, token) = setup(&env);
    let token_client = token::StellarAssetClient::new(&env, &token);

    let stream_id = String::from_str(&env, "stream_event");
    let destination = Address::generate(&env);

    // rate=10, deposit=500 → after 10s: withdrawable=100, remaining=400
    client.create_stream(&sender, &receiver, &token, &10i128, &500i128, &stream_id, &None::<i128>);
    client.set_stream_destination(&receiver, &stream_id, &destination);
    client.approve_stream_milestone(&sender, &stream_id);

    env.ledger().set_timestamp(env.ledger().timestamp() + 10);
    client.trigger_withdrawal(&stream_id);

    let stream = client.get_stream(&stream_id);
    assert_eq!(stream.remaining_deposit, 400);
    assert_eq!(token_client.balance(&destination), 100i128);
}

#[test]
fn test_top_up_stream_success() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, sender, receiver, token) = setup(&env);
    let token_client = token::StellarAssetClient::new(&env, &token);

    let stream_id = String::from_str(&env, "stream_top_up");
    client.create_stream(&sender, &receiver, &token, &10i128, &500i128, &stream_id, &None::<i128>);

    client.top_up_stream(&sender, &stream_id, &250i128);

    let stream = client.get_stream(&stream_id);
    assert_eq!(stream.remaining_deposit, 750);
    assert_eq!(token_client.balance(&client.address), 750i128);
}

#[test]
fn test_top_up_multiple_streams_success() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, sender, receiver, token) = setup(&env);

    let stream_id1 = String::from_str(&env, "stream_top_up_multi_1");
    let stream_id2 = String::from_str(&env, "stream_top_up_multi_2");
    client.create_stream(&sender, &receiver, &token, &10i128, &500i128, &stream_id1, &None::<i128>);
    client.create_stream(&sender, &receiver, &token, &20i128, &500i128, &stream_id2, &None::<i128>);

    let top_ups = vec![&env, (stream_id1.clone(), 100i128), (stream_id2.clone(), 200i128)];
    client.top_up_multiple_streams(&sender, &top_ups);

    let stream1 = client.get_stream(&stream_id1);
    let stream2 = client.get_stream(&stream_id2);
    assert_eq!(stream1.remaining_deposit, 600);
    assert_eq!(stream2.remaining_deposit, 700);
}

#[test]
fn test_top_up_multiple_streams_unauthorized() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, sender, receiver, token) = setup(&env);

    let stream_id1 = String::from_str(&env, "stream_top_up_multi_auth_1");
    client.create_stream(&sender, &receiver, &token, &10i128, &500i128, &stream_id1, &None::<i128>);

    let impostor = Address::generate(&env);
    let top_ups = vec![&env, (stream_id1.clone(), 100i128)];
    let result = client.try_top_up_multiple_streams(&impostor, &top_ups);
    assert_eq!(result, Err(Ok(StreamError::Unauthorized)));
}

#[test]
fn test_top_up_stream_unauthorized_caller() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, sender, receiver, token) = setup(&env);

    let stream_id = String::from_str(&env, "stream_top_up_auth");
    client.create_stream(&sender, &receiver, &token, &10i128, &500i128, &stream_id, &None::<i128>);

    let impostor = Address::generate(&env);
    let result = client.try_top_up_stream(&impostor, &stream_id, &50i128);
    assert_eq!(result, Err(Ok(StreamError::Unauthorized)));
}

#[test]
fn test_top_up_stream_rejects_inactive_stream() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, sender, receiver, token) = setup(&env);

    let stream_id = String::from_str(&env, "stream_top_up_inactive");
    client.create_stream(&sender, &receiver, &token, &10i128, &500i128, &stream_id, &None::<i128>);
    client.cancel_stream(&sender, &stream_id);

    let result = client.try_top_up_stream(&sender, &stream_id, &50i128);
    assert_eq!(result, Err(Ok(StreamError::StreamNotActive)));
}

// ─── Minimum rate enforcement ───────────────────────────────────────────────────

#[test]
fn test_decrease_rate_rejects_below_configured_min() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, sender, receiver, token) = setup(&env);

    let stream_id = String::from_str(&env, "stream_min_rate");
    client.create_stream(&sender, &receiver, &token, &10i128, &500i128, &stream_id, &None::<i128>);
    client.set_stream_min_rate(&sender, &stream_id, &5i128);

    let err = client.try_decrease_rate_per_second(&sender, &stream_id, &4i128);
    assert_eq!(err, Err(Ok(StreamError::InvalidRate)));
}

#[test]
fn test_decrease_rate_to_exactly_min_rate_succeeds() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, sender, receiver, token) = setup(&env);

    let stream_id = String::from_str(&env, "stream_min_exact");
    client.create_stream(&sender, &receiver, &token, &100i128, &5000i128, &stream_id, &Some(50i128));

    let stream = client.get_stream(&stream_id);
    assert_eq!(stream.rate_per_second, 100);
    assert_eq!(stream.min_rate_per_second, 50);

    client.decrease_rate_per_second(&sender, &stream_id, &50i128);

    let stream = client.get_stream(&stream_id);
    assert_eq!(stream.rate_per_second, 50);
}

#[test]
fn test_decrease_rate_below_min_rate_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, sender, receiver, token) = setup(&env);

    let stream_id = String::from_str(&env, "stream_min_below");
    client.create_stream(&sender, &receiver, &token, &100i128, &5000i128, &stream_id, &Some(50i128));

    let result = client.try_decrease_rate_per_second(&sender, &stream_id, &49i128);
    assert_eq!(result, Err(Ok(StreamError::RateBelowMinimum)));
}

#[test]
fn test_decrease_rate_with_zero_min_rate() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, sender, receiver, token) = setup(&env);

    let stream_id = String::from_str(&env, "stream_zero_min");
    client.create_stream(&sender, &receiver, &token, &100i128, &5000i128, &stream_id, &Some(0i128));

    let stream = client.get_stream(&stream_id);
    assert_eq!(stream.min_rate_per_second, 0);

    client.decrease_rate_per_second(&sender, &stream_id, &1i128);

    let stream = client.get_stream(&stream_id);
    assert_eq!(stream.rate_per_second, 1);
}

#[test]
fn test_cancel_multiple_streams_with_partial_invalid_id_errors() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, sender, recipient, token) = setup(&env);

    let stream_id1 = String::from_str(&env, "stream_cancel_partial_1");
    client.create_stream(&sender, &recipient, &token, &100i128, &500i128, &stream_id1, &None::<i128>);

    let missing_stream_id = String::from_str(&env, "stream_cancel_missing");
    let stream_ids = vec![&env, stream_id1.clone(), missing_stream_id];

    let result = client.try_cancel_multiple_streams(&sender, &stream_ids);
    assert_eq!(result, Err(Ok(StreamError::StreamNotFound)));

    let stream1 = client.get_stream(&stream_id1);
    assert_eq!(stream1.status, StreamStatus::Active);
}

#[test]
fn test_batch_withdraw_to_multiple_destinations() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, sender, recipient, token) = setup(&env);
    let token_client = token::StellarAssetClient::new(&env, &token);

    let stream_id1 = String::from_str(&env, "stream_withdraw_multi_1");
    let stream_id2 = String::from_str(&env, "stream_withdraw_multi_2");
    let destination1 = Address::generate(&env);
    let destination2 = Address::generate(&env);

    token_client.mint(&sender, &10_000i128);
    client.create_stream(&sender, &recipient, &token, &100i128, &1_000i128, &stream_id1, &None::<i128>);
    client.create_stream(&sender, &recipient, &token, &200i128, &2_000i128, &stream_id2, &None::<i128>);

    client.approve_stream_milestone(&sender, &stream_id1);
    client.approve_stream_milestone(&sender, &stream_id2);
    env.ledger().set_timestamp(env.ledger().timestamp() + 5);

    let withdrawal1 = crate::WithdrawalRecipient {
        stream_id: stream_id1.clone(),
        destination: destination1.clone(),
        amount: 100,
    };
    let withdrawal2 = crate::WithdrawalRecipient {
        stream_id: stream_id2.clone(),
        destination: destination2.clone(),
        amount: 100,
    };
    let withdrawals = vec![&env, withdrawal1, withdrawal2];

    let processed = client.batch_withdraw_to(&recipient, &withdrawals);
    assert_eq!(processed.len(), 2);
    assert_eq!(token_client.balance(&destination1), 100);
    assert_eq!(token_client.balance(&destination2), 100);
}

#[test]
fn test_batch_withdraw_to_skips_zero_accrued_streams() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, sender, recipient, token) = setup(&env);
    let token_client = token::StellarAssetClient::new(&env, &token);

    let stream_id = String::from_str(&env, "stream_withdraw_zero_accrued");
    let destination = Address::generate(&env);

    token_client.mint(&sender, &10_000i128);
    client.create_stream(&sender, &recipient, &token, &100i128, &1_000i128, &stream_id, &None::<i128>);
    client.approve_stream_milestone(&sender, &stream_id);

    let withdrawal = crate::WithdrawalRecipient {
        stream_id: stream_id.clone(),
        destination: destination.clone(),
        amount: 100,
    };
    let withdrawals = vec![&env, withdrawal];

    let processed = client.batch_withdraw_to(&recipient, &withdrawals);
    assert_eq!(processed.len(), 0);
    assert_eq!(token_client.balance(&destination), 0);
}

#[test]
fn test_default_min_rate_of_one() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, sender, receiver, token) = setup(&env);

    let stream_id = String::from_str(&env, "stream_default_min");
    client.create_stream(&sender, &receiver, &token, &100i128, &5000i128, &stream_id, &None::<i128>);

    let stream = client.get_stream(&stream_id);
    assert_eq!(stream.min_rate_per_second, 1);

    let result = client.try_decrease_rate_per_second(&sender, &stream_id, &0i128);
    assert_eq!(result, Err(Ok(StreamError::RateBelowMinimum)));
}

// ─── Issue #627: bulk_bump_stream_ttls ────────────────────────────────────────

#[test]
fn test_bulk_bump_stream_ttls_counts_only_existing_streams() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, sender, receiver, token) = setup(&env);

    // 3 valid streams + 1 ID that was never created.
    let s1 = String::from_str(&env, "ttl_stream_1");
    let s2 = String::from_str(&env, "ttl_stream_2");
    let s3 = String::from_str(&env, "ttl_stream_3");
    let missing = String::from_str(&env, "ttl_stream_missing");

    client.create_stream(&sender, &receiver, &token, &10i128, &1_000i128, &s1, &None::<i128>);
    client.create_stream(&sender, &receiver, &token, &10i128, &1_000i128, &s2, &None::<i128>);
    client.create_stream(&sender, &receiver, &token, &10i128, &1_000i128, &s3, &None::<i128>);

    let bumped = client.bulk_bump_stream_ttls(&vec![&env, s1, s2, s3, missing]);
    assert_eq!(bumped, 3);
}

#[test]
fn test_bulk_bump_stream_ttls_empty_batch_returns_zero() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _sender, _receiver, _token) = setup(&env);

    let bumped = client.bulk_bump_stream_ttls(&vec![&env]);
    assert_eq!(bumped, 0);
}

#[test]
fn test_bulk_bump_stream_ttls_rejects_oversized_batch() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _sender, _receiver, _token) = setup(&env);

    let mut ids = vec![&env];
    for _ in 0..51u32 {
        ids.push_back(String::from_str(&env, "s"));
    }

    let result = client.try_bulk_bump_stream_ttls(&ids);
    assert_eq!(result, Err(Ok(StreamError::BatchTooLarge)));
}
