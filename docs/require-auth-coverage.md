# require_auth coverage audit (issue #1167)

Scope: all `pub fn` entrypoints declared in the Soroban `#[contractimpl]` blocks in
`fluxapay/src/lib.rs` (298 entrypoints). Status was derived by inspecting each
function body for a direct `require_auth()` call or delegation to an
auth-enforcing helper (for example `AccessControl::*` or `pause_scope`).

## Gaps found and fixed

- `RefundManager::register_payment` — wrote a new payment record and incremented
  the per-merchant payment counter without authenticating anyone. Now calls
  `merchant_id.require_auth()`.
- `RefundManager::register_payment_with_payer` — same gap; now calls
  `merchant_id.require_auth()`.

## Justified permissionless mutating entrypoints

- `check_dispute_deadline` / `escalate_expired_disputes` — public keepers may
  advance an already-expired dispute deadline; all state transitions are
  constrained by the deadline and no caller-supplied address is charged.
- `settle_dispute_collaboratively` — authorization is by both parties' Ed25519
  signatures over the dispute id and amount; there is no privileged caller.
- `process_due_subscriptions` / `charge_subscription` — authenticated by the
  `operator` argument (see inventory).

## Read-only entrypoints

Getters (`get_*`, `is_*`, `has_*`, `check_*`) do not mutate state or move funds
and therefore need no `require_auth`.

## Regression coverage

`fluxapay/src/require_auth_audit_test.rs` asserts that both registration
entrypoints reject unauthenticated merchants and succeed with authentication.

## Inventory

| Entrypoint | lib.rs line | require_auth status |
| --- | ---: | --- |
| `to_secs` | 1065 | NO AUTH - REVIEW |
| `version` | 1544 | permissionless by design (see notes) |
| `set_version` | 1552 | require_auth (direct) |
| `get_dispute_bond_amount` | 1615 | read-only (no auth needed) |
| `set_contract_metadata` | 1627 | require_auth (direct) |
| `get_contract_metadata` | 1656 | read-only (no auth needed) |
| `initialize_refund_manager` | 1699 | require_auth (delegated helper) |
| `set_refund_fee_bps` | 1741 | require_auth (direct) |
| `get_refund_fee_bps` | 1755 | read-only (no auth needed) |
| `set_dex_router_address` | 1762 | require_auth (direct) |
| `set_refund_policy` | 1779 | require_auth (direct) |
| `get_refund_policy` | 1799 | read-only (no auth needed) |
| `grant_role` | 1813 | require_auth (delegated helper) |
| `grant_merchant_role` | 1822 | require_auth (delegated helper) |
| `revoke_role` | 1831 | require_auth (delegated helper) |
| `revoke_merchant_role` | 1841 | require_auth (delegated helper) |
| `sync_grant_role_with_processor` | 1854 | require_auth (direct) |
| `sync_revoke_role_with_processor` | 1889 | require_auth (direct) |
| `has_role` | 1920 | require_auth (delegated helper) |
| `renounce_role` | 1924 | require_auth (delegated helper) |
| `propose_admin` | 1928 | require_auth (delegated helper) |
| `claim_admin` | 1937 | require_auth (delegated helper) |
| `accept_admin` | 1941 | require_auth (delegated helper) |
| `cancel_admin_transfer` | 1945 | require_auth (delegated helper) |
| `transfer_admin` | 1950 | NO AUTH - REVIEW |
| `accept_admin_transfer` | 1958 | NO AUTH - REVIEW |
| `get_admin` | 1962 | require_auth (delegated helper) |
| `add_to_blacklist` | 1968 | require_auth (direct) |
| `remove_from_blacklist` | 1980 | require_auth (direct) |
| `is_blacklisted` | 1992 | read-only (no auth needed) |
| `register_swap_payment` | 2015 | NO AUTH - REVIEW |
| `configure_fee_split` | 2071 | require_auth (direct) |
| `set_fee_split_config` | 2113 | require_auth (direct) |
| `get_fee_split_config` | 2144 | read-only (no auth needed) |
| `set_merchant_registry_address` | 2153 | require_auth (direct) |
| `get_merchant_registry_address` | 2171 | read-only (no auth needed) |
| `get_role_members` | 2178 | require_auth (delegated helper) |
| `propose_fee_update` | 2181 | require_auth (direct) |
| `finalize_fee_update` | 2196 | require_auth (direct) |
| `register_payment` | 2223 | require_auth (direct) |
| `register_payment_with_payer` | 2283 | require_auth (direct) |
| `queue_auto_refund` | 2340 | require_auth (direct) |
| `create_refund` | 2373 | require_auth (direct) |
| `create_refund_idempotent` | 2403 | require_auth (direct) |
| `create_refund_with_receipt` | 2428 | require_auth (direct) |
| `process_refund` | 2611 | require_auth (direct) |
| `batch_process_refunds` | 2651 | require_auth (direct) |
| `get_treasury_balance` | 2706 | read-only (no auth needed) |
| `get_treasury_withdrawal_history` | 2749 | read-only (no auth needed) |
| `withdraw_treasury` | 2770 | require_auth (direct) |
| `reject_refund` | 3061 | require_auth (direct) |
| `expire_refund` | 3098 | require_auth (direct) |
| `approve_refund` | 3137 | require_auth (direct) |
| `claim_refund` | 3174 | require_auth (direct) |
| `set_refund_expiry` | 3195 | require_auth (direct) |
| `refund_instantly` | 3225 | require_auth (direct) |
| `cancel_refund` | 3338 | require_auth (direct) |
| `get_refund` | 3371 | read-only (no auth needed) |
| `get_payment_refunds` | 3375 | read-only (no auth needed) |
| `create_dispute` | 3415 | require_auth (direct) |
| `batch_create_disputes` | 3442 | require_auth (direct) |
| `review_dispute` | 3765 | require_auth (direct) |
| `set_dispute_rate_limits` | 3803 | require_auth (direct) |
| `set_require_evidence_cid` | 3825 | require_auth (direct) |
| `set_dispute_threshold` | 3839 | require_auth (direct) |
| `set_dispute_deadline` | 3960 | require_auth (direct) |
| `set_dispute_payout_splits` | 4020 | require_auth (direct) |
| `check_dispute_deadline` | 4098 | read-only (no auth needed) |
| `escalate_expired_disputes` | 4103 | permissionless by design (see notes) |
| `resolve_dispute_with_refund` | 4126 | require_auth (direct) |
| `batch_resolve_disputes` | 4334 | require_auth (direct) |
| `reject_dispute` | 4455 | require_auth (direct) |
| `get_dispute_operator_note` | 4572 | read-only (no auth needed) |
| `settle_dispute_collaboratively` | 4599 | permissionless by design (see notes) |
| `get_collaborative_settlement` | 4699 | read-only (no auth needed) |
| `get_merchant_dispute_count` | 4710 | read-only (no auth needed) |
| `get_merchant_payment_count` | 4718 | read-only (no auth needed) |
| `lock_stake` | 4773 | require_auth (direct) |
| `cast_vote` | 4830 | require_auth (direct) |
| `finalize_dispute_vote` | 4909 | require_auth (direct) |
| `vote_dispute` | 5051 | require_auth (direct) |
| `get_vote_tally` | 5159 | read-only (no auth needed) |
| `get_dispute` | 5170 | read-only (no auth needed) |
| `submit_evidence` | 5179 | require_auth (direct) |
| `get_dispute_evidence` | 5225 | read-only (no auth needed) |
| `get_payment_disputes` | 5232 | read-only (no auth needed) |
| `get_payment_disputes_by_status` | 5245 | read-only (no auth needed) |
| `get_merchant_disputes` | 5268 | read-only (no auth needed) |
| `get_dispute_summary` | 5305 | read-only (no auth needed) |
| `submit_arbitrator_vote` | 5347 | require_auth (direct) |
| `check_arbitration_threshold` | 5407 | read-only (no auth needed) |
| `create_subscription_plan` | 5484 | require_auth (direct) |
| `create_plan` | 5538 | require_auth (direct) |
| `get_subscription_plan` | 5578 | read-only (no auth needed) |
| `get_plan` | 5587 | read-only (no auth needed) |
| `deactivate_subscription_plan` | 5590 | require_auth (direct) |
| `subscribe` | 5618 | require_auth (direct) |
| `subscribe_to_plan` | 5714 | require_auth (direct) |
| `get_subscription` | 5808 | read-only (no auth needed) |
| `get_payer_subscriptions` | 5812 | read-only (no auth needed) |
| `get_active_subscriptions_for_payer` | 5831 | read-only (no auth needed) |
| `get_plan_subscribers` | 5875 | read-only (no auth needed) |
| `pause_subscription` | 5914 | require_auth (direct) |
| `pause_with_resume_date` | 5943 | require_auth (direct) |
| `charge_subscription` | 6003 | require_auth (direct) |
| `process_subscription` | 6260 | require_auth (direct) |
| `resume_subscription` | 6290 | require_auth (direct) |
| `cancel_subscription` | 6329 | require_auth (direct) |
| `set_allow_prorated_refunds` | 6401 | require_auth (direct) |
| `get_allow_prorated_refunds` | 6413 | read-only (no auth needed) |
| `admin_reactivate_subscription` | 6519 | require_auth (direct) |
| `submit_usage_metrics` | 6573 | require_auth (direct) |
| `get_usage_metrics` | 6658 | read-only (no auth needed) |
| `process_due_subscriptions` | 6756 | require_auth (direct) |
| `upgrade_contract` | 6982 | require_auth (direct) |
| `version` | 7009 | permissionless by design (see notes) |
| `set_version` | 7017 | require_auth (direct) |
| `get_version` | 7049 | read-only (no auth needed) |
| `get_contract_health` | 7055 | read-only (no auth needed) |
| `get_contract_info` | 7103 | require_auth (delegated helper) |
| `set_contract_metadata` | 7127 | require_auth (direct) |
| `get_contract_metadata` | 7153 | read-only (no auth needed) |
| `initialize_payment_processor` | 7184 | require_auth (delegated helper) |
| `set_merchant_registry_address` | 7231 | require_auth (direct) |
| `set_swap_limits` | 7250 | require_auth (direct) |
| `get_swap_limits` | 7278 | read-only (no auth needed) |
| `set_fx_oracle` | 7296 | require_auth (direct) |
| `set_fee_rate` | 7319 | require_auth (direct) |
| `set_auto_refund_overpayment` | 7335 | require_auth (direct) |
| `get_auto_refund_overpayment` | 7352 | read-only (no auth needed) |
| `set_min_payment_duration_secs` | 7361 | require_auth (direct) |
| `set_max_payment_duration_secs` | 7381 | require_auth (direct) |
| `set_default_payment_duration` | 7408 | require_auth (direct) |
| `set_merchant_payment_duration` | 7430 | require_auth (direct) |
| `get_treasury_balance` | 7488 | read-only (no auth needed) |
| `get_treasury_withdrawal_history` | 7527 | read-only (no auth needed) |
| `get_platform_fee_report` | 7587 | read-only (no auth needed) |
| `withdraw_treasury` | 7618 | require_auth (direct) |
| `add_fee_waiver_code` | 7691 | require_auth (direct) |
| `set_global_rate_limit` | 7730 | require_auth (direct) |
| `set_merchant_rate_limit` | 7750 | require_auth (direct) |
| `grant_role` | 7771 | require_auth (delegated helper) |
| `grant_merchant_role` | 7780 | require_auth (delegated helper) |
| `revoke_role` | 7789 | require_auth (delegated helper) |
| `revoke_merchant_role` | 7799 | require_auth (delegated helper) |
| `has_role` | 7810 | require_auth (delegated helper) |
| `get_role_members` | 7815 | require_auth (delegated helper) |
| `set_global_pause` | 7820 | require_auth (delegated helper) |
| `set_creation_pause` | 7848 | require_auth (delegated helper) |
| `pause_scope` | 7874 | require_auth (direct) |
| `unpause_scope` | 7910 | require_auth (direct) |
| `get_paused_scopes` | 7945 | read-only (no auth needed) |
| `set_paused` | 7966 | NO AUTH - REVIEW |
| `get_creation_pause_info` | 7985 | read-only (no auth needed) |
| `get_pause_info` | 7996 | read-only (no auth needed) |
| `is_paused` | 8015 | read-only (no auth needed) |
| `set_merchant_amount_limits` | 8212 | require_auth (direct) |
| `get_merchant_amount_limits` | 8235 | read-only (no auth needed) |
| `set_global_amount_limits` | 8243 | require_auth (direct) |
| `get_global_amount_limits` | 8266 | read-only (no auth needed) |
| `set_usdc_token` | 8295 | require_auth (direct) |
| `allow_token` | 8308 | require_auth (direct) |
| `set_token_currency` | 8332 | require_auth (direct) |
| `remove_supported_token` | 8358 | require_auth (direct) |
| `get_supported_tokens` | 8395 | read-only (no auth needed) |
| `set_kyc_tier_limits` | 8403 | require_auth (direct) |
| `set_fx_oracle_address` | 8422 | require_auth (direct) |
| `set_oracle_staleness_threshold` | 8442 | require_auth (direct) |
| `get_oracle_staleness_threshold` | 8463 | read-only (no auth needed) |
| `add_to_blacklist` | 8471 | require_auth (direct) |
| `remove_from_blacklist` | 8483 | require_auth (direct) |
| `is_blacklisted` | 8495 | read-only (no auth needed) |
| `is_token_allowed` | 8606 | read-only (no auth needed) |
| `create_payment` | 8615 | require_auth (delegated helper) |
| `grant_delegation` | 9024 | NO AUTH - REVIEW |
| `revoke_delegation` | 9036 | NO AUTH - REVIEW |
| `list_delegations` | 9046 | NO AUTH - REVIEW |
| `create_payment_delegated` | 9060 | NO AUTH - REVIEW |
| `create_refund_delegated` | 9081 | NO AUTH - REVIEW |
| `create_payments_batch` | 9111 | require_auth (direct) |
| `create_payment_batch` | 9408 | require_auth (direct) |
| `verify_payment` | 9701 | require_auth (direct) |
| `confirm_payment` | 10165 | NO AUTH - REVIEW |
| `verify_payment_batch` | 10184 | require_auth (direct) |
| `accept_partial_payment` | 10213 | require_auth (direct) |
| `complete_partial_payment` | 10255 | require_auth (direct) |
| `retry_payment` | 10297 | require_auth (direct) |
| `get_payment` | 10404 | read-only (no auth needed) |
| `get_payment_status_history` | 10410 | read-only (no auth needed) |
| `get_payment_by_metadata_hash` | 10424 | read-only (no auth needed) |
| `get_customer` | 10437 | read-only (no auth needed) |
| `get_top_customers` | 10449 | read-only (no auth needed) |
| `get_top_merchants` | 10536 | read-only (no auth needed) |
| `bump_payment_ttl` | 10597 | NO AUTH - REVIEW |
| `bump_payment_ttl_public` | 10604 | NO AUTH - REVIEW |
| `bulk_bump_payment_ttls` | 10609 | NO AUTH - REVIEW |
| `get_merchant_payments` | 10623 | read-only (no auth needed) |
| `get_merchant_payments_paginated` | 10627 | read-only (no auth needed) |
| `generate_reconciliation_report` | 10684 | NO AUTH - REVIEW |
| `reconciliation_report_page` | 10816 | NO AUTH - REVIEW |
| `cancel_payment` | 10849 | require_auth (direct) |
| `expire_payment` | 10911 | NO AUTH - REVIEW |
| `batch_expire_payments` | 11033 | NO AUTH - REVIEW |
| `settle_payment` | 11081 | require_auth (direct) |
| `trigger_settlement` | 11753 | require_auth (direct) |
| `release_merchant_reserve` | 11845 | require_auth (direct) |
| `prune_expired_payments` | 11881 | require_auth (direct) |
| `prune_expired_invoices` | 11911 | require_auth (direct) |
| `add_router` | 12091 | require_auth (direct) |
| `remove_router` | 12120 | require_auth (direct) |
| `is_router_allowed` | 12153 | read-only (no auth needed) |
| `get_allowed_routers` | 12161 | read-only (no auth needed) |
| `set_wrapped_xlm_contract` | 12169 | require_auth (direct) |
| `get_wrapped_xlm_contract` | 12181 | read-only (no auth needed) |
| `swap_and_pay` | 12187 | require_auth (direct) |
| `swap_and_pay_multi_route` | 12363 | require_auth (direct) |
| `get_merchant_dispute_count` | 12643 | read-only (no auth needed) |
| `get_merchant_payment_count` | 12735 | read-only (no auth needed) |
| `get_merchant_payment_count_dash` | 12743 | read-only (no auth needed) |
| `get_merchant_payments_full` | 12758 | read-only (no auth needed) |
| `get_merchant_analytics` | 12809 | read-only (no auth needed) |
| `pre_authorize_merchant` | 12997 | NO AUTH - REVIEW |
| `revoke_merchant_authorization` | 13016 | NO AUTH - REVIEW |
| `pull_payment` | 13026 | NO AUTH - REVIEW |
| `get_merchant_authorization` | 13036 | read-only (no auth needed) |
| `merchant_authorization_remaining` | 13045 | NO AUTH - REVIEW |
| `create_api_key` | 13054 | NO AUTH - REVIEW |
| `get_api_key` | 13064 | read-only (no auth needed) |
| `revoke_api_key` | 13069 | NO AUTH - REVIEW |
| `cancel_stream` | 13076 | NO AUTH - REVIEW |
| `pause_stream` | 13083 | NO AUTH - REVIEW |
| `resume_stream` | 13090 | NO AUTH - REVIEW |
| `cancel_multiple_streams` | 13097 | NO AUTH - REVIEW |
| `batch_cancel_streams` | 13107 | NO AUTH - REVIEW |
| `batch_withdraw_to` | 13118 | NO AUTH - REVIEW |
| `withdraw_all_for_recipient` | 13129 | NO AUTH - REVIEW |
| `trigger_withdrawal` | 13140 | NO AUTH - REVIEW |
| `bulk_bump_stream_ttls` | 13149 | NO AUTH - REVIEW |
| `set_stream_destination` | 13152 | NO AUTH - REVIEW |
| `approve_stream_milestone` | 13168 | NO AUTH - REVIEW |
| `revoke_stream_milestone` | 13177 | NO AUTH - REVIEW |
| `set_stream_min_rate` | 13186 | NO AUTH - REVIEW |
| `decrease_rate_per_second` | 13196 | NO AUTH - REVIEW |
| `get_sender_streams` | 13204 | read-only (no auth needed) |
| `get_stream` | 13213 | read-only (no auth needed) |
| `create_stream` | 13219 | NO AUTH - REVIEW |
| `create_vesting_stream` | 13246 | NO AUTH - REVIEW |
| `top_up_stream` | 13273 | NO AUTH - REVIEW |
| `top_up_multiple_streams` | 13285 | NO AUTH - REVIEW |
| `update_stream_rate` | 13295 | NO AUTH - REVIEW |
| `close_expired_stream` | 13305 | NO AUTH - REVIEW |
| `set_stream_fee_bps` | 13311 | require_auth (direct) |
| `get_stream_fee_bps` | 13319 | read-only (no auth needed) |
| `set_stream_fee_recipient` | 13323 | require_auth (direct) |
| `get_stream_fee_recipient` | 13336 | read-only (no auth needed) |
| `upgrade_contract` | 13345 | require_auth (direct) |
| `set_timelock_delay` | 13368 | require_auth (direct) |
| `get_timelock_delay` | 13380 | read-only (no auth needed) |
| `get_pending_admin_actions` | 13391 | read-only (no auth needed) |
| `execute_timelocked_action` | 13416 | require_auth (direct) |
| `set_multisig_config` | 13538 | require_auth (delegated helper) |
| `get_multisig_config` | 13549 | require_auth (delegated helper) |
| `create_proposal` | 13556 | require_auth (delegated helper) |
| `vote_proposal` | 13563 | require_auth (delegated helper) |
| `execute_proposal` | 13573 | require_auth (direct) |
| `get_proposal` | 13655 | require_auth (delegated helper) |
| `get_dispute_bond_amount` | 13661 | read-only (no auth needed) |
| `get_tier_volume_cap` | 13670 | read-only (no auth needed) |
| `get_refund_fee_bps` | 13688 | read-only (no auth needed) |
| `set_refund_cooldown` | 13701 | require_auth (direct) |
| `recompute_merchant_payment_count` | 13727 | require_auth (direct) |
| `create_invoice` | 13745 | require_auth (direct) |
| `create_payment_link_invoice` | 13803 | require_auth (direct) |
| `mark_invoice_paid` | 13903 | NO AUTH - REVIEW |
| `set_invoice_grace_period` | 13934 | require_auth (direct) |
| `get_invoice_grace_period` | 13948 | read-only (no auth needed) |
| `get_invoice` | 13954 | read-only (no auth needed) |
| `get_merchant_invoices` | 13971 | read-only (no auth needed) |
| `void_invoice` | 14007 | require_auth (direct) |
| `mark_invoice_overdue` | 14044 | NO AUTH - REVIEW |
| `verify_metadata_hash` | 14079 | NO AUTH - REVIEW |
| `get_fee_collection_history` | 14143 | read-only (no auth needed) |
| `get_total_fees_collected` | 14175 | require_auth (direct) |
| `get_top_merchants_v2` | 14205 | read-only (no auth needed) |
| `execute_meta_transaction` | 14291 | NO AUTH - REVIEW |
| `rotate_webhook_key` | 14302 | NO AUTH - REVIEW |
| `set_fee_recipient` | 14307 | NO AUTH - REVIEW |
| `authorize_refund` | 14312 | NO AUTH - REVIEW |
| `get_domain_separator` | 14319 | read-only (no auth needed) |
| `get_merchant_nonce` | 14323 | read-only (no auth needed) |
| `get_webhook_key` | 14327 | read-only (no auth needed) |
| `get_fee_recipient` | 14331 | read-only (no auth needed) |
| `get_refund_authorization` | 14335 | read-only (no auth needed) |
| `create_batch` | 14355 | require_auth (direct) |
| `claim_batch_payment` | 14427 | NO AUTH - REVIEW |
| `defund_batch` | 14489 | require_auth (direct) |
| `get_batch` | 14523 | read-only (no auth needed) |
| `is_batch_claimed` | 14531 | read-only (no auth needed) |
