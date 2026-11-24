/**
 * Client-side port of the contract's lazy stream accrual formula
 * (`fluxapay/src/stream.rs::compute_total_accrued`).
 *
 * The contract only checkpoints accrual on-chain when a stream is mutated
 * (withdrawal, top-up, rate change, etc). Between those checkpoints, the
 * true accrued amount is a pure function of elapsed time, so a client can
 * reproduce it exactly without an extra RPC round-trip — this is what lets
 * `useStreamAccrual` animate smoothly between polls instead of jumping only
 * when a new poll resolves.
 *
 * Mirrors the Rust implementation's saturating/clamping semantics using
 * `bigint` so results match exactly for on-chain `i128` values.
 */
export function computeTotalAccrued(
  accruedAtCheckpoint: bigint,
  lastCheckpointAt: bigint,
  now: bigint,
  ratePerSecond: bigint,
  remainingDeposit: bigint,
): bigint {
  const rate = ratePerSecond > 0n ? ratePerSecond : 0n;
  const deposit = remainingDeposit > 0n ? remainingDeposit : 0n;
  const elapsed = now > lastCheckpointAt ? now - lastCheckpointAt : 0n;
  const newlyAccrued = elapsed * rate;

  let total = accruedAtCheckpoint + newlyAccrued;
  if (total > deposit) total = deposit;
  if (total < 0n) total = 0n;
  return total;
}

/**
 * Remaining deposit after a withdrawal; never negative.
 * Mirrors `fluxapay/src/stream.rs::compute_remaining_after_withdraw`.
 */
export function computeRemainingAfterWithdraw(
  remainingDeposit: bigint,
  withdrawAmount: bigint,
): bigint {
  const withdraw = withdrawAmount > 0n ? withdrawAmount : 0n;
  const remaining = remainingDeposit - withdraw;
  return remaining > 0n ? remaining : 0n;
}
