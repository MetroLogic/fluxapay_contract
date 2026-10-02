use soroban_sdk::{contracterror, contracttype, Address, BytesN, Env, Symbol};

/// Errors returned by the dispute contract.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum DisputeError {
    /// Caller is not authorized to perform the action.
    Unauthorized = 1,
    /// The referenced dispute does not exist.
    DisputeNotFound = 2,
    /// The dispute is not in a state that allows the requested action.
    InvalidState = 3,
    /// The supplied bond is below the required minimum.
    BondTooLow = 4,
}

/// Storage keys used by the dispute module.
#[contracttype]
#[derive(Clone)]
pub enum DisputeKey {
    /// Absolute minimum bond in stroops (1 USDC by default).
    AbsoluteMinBond,
    /// Minimum bond in basis points of the disputed amount (200 = 2% by default).
    MinBondBps,
}

/// Default absolute minimum bond: 1 USDC (7 decimal places).
pub const DEFAULT_ABSOLUTE_MIN_BOND: i128 = 10_000_000;
/// Default minimum bond in basis points: 2%.
pub const DEFAULT_MIN_BOND_BPS: i128 = 200;

/// A single dispute record.
#[contracttype]
#[derive(Clone)]
pub struct Dispute {
    pub id: u64,
    pub payment_id: u64,
    pub opener: Address,
    pub disputed_amount: i128,
    pub bond_amount: i128,
    pub resolved: bool,
    pub evidence_hash: BytesN<32>,
}

/// Read the configured absolute minimum bond, falling back to the default.
fn absolute_min_bond(env: &Env) -> i128 {
    env.storage()
        .instance()
        .get(&DisputeKey::AbsoluteMinBond)
        .unwrap_or(DEFAULT_ABSOLUTE_MIN_BOND)
}

/// Read the configured minimum bond basis points, falling back to the default.
fn min_bond_bps(env: &Env) -> i128 {
    env.storage()
        .instance()
        .get(&DisputeKey::MinBondBps)
        .unwrap_or(DEFAULT_MIN_BOND_BPS)
}

/// Compute the minimum bond required for a given disputed amount.
///
/// `min_bond = max(ABSOLUTE_MIN_BOND, disputed_amount * MIN_BOND_BPS / 10000)`
pub fn min_bond_for(env: &Env, disputed_amount: i128) -> i128 {
    let proportional = disputed_amount
        .saturating_mul(min_bond_bps(env))
        .saturating_div(10_000);
    let absolute = absolute_min_bond(env);
    if proportional > absolute {
        proportional
    } else {
        absolute
    }
}

/// Admin-only configuration of the dispute bond parameters.
pub fn set_dispute_bond_params(
    env: &Env,
    admin: Address,
    absolute_min_bond: i128,
    min_bond_bps: i128,
) -> Result<(), DisputeError> {
    admin.require_auth();
    env.storage()
        .instance()
        .set(&DisputeKey::AbsoluteMinBond, &absolute_min_bond);
    env.storage()
        .instance()
        .set(&DisputeKey::MinBondBps, &min_bond_bps);
    Ok(())
}

/// Open a dispute against a payment, enforcing a proportional minimum bond and storing SHA-256 evidence hash.
pub fn open_dispute(
    env: &Env,
    opener: Address,
    payment_id: u64,
    disputed_amount: i128,
    bond_amount: i128,
    evidence_hash: BytesN<32>,
) -> Result<u64, DisputeError> {
    opener.require_auth();

    let required = min_bond_for(env, disputed_amount);
    if bond_amount < required {
        return Err(DisputeError::BondTooLow);
    }

    let id: u64 = env
        .storage()
        .instance()
        .get(&Symbol::new(env, "dispute_count"))
        .unwrap_or(0u64)
        + 1;
    env.storage()
        .instance()
        .set(&Symbol::new(env, "dispute_count"), &id);

    let dispute = Dispute {
        id,
        payment_id,
        opener: opener.clone(),
        disputed_amount,
        bond_amount,
        resolved: false,
        evidence_hash: evidence_hash.clone(),
    };
    env.storage()
        .persistent()
        .set(&(Symbol::new(env, "dispute"), id), &dispute);

    // DISPUTE/OPENED event payload includes the hash (Issue #773).
    env.events().publish(
        (Symbol::new(env, "DISPUTE"), Symbol::new(env, "OPENED")),
        (id, payment_id, opener, evidence_hash),
    );

    Ok(id)
}

/// Read-only entry point returning the stored evidence hash for a given dispute ID (Issue #773).
pub fn verify_evidence(env: &Env, dispute_id: u64) -> Result<BytesN<32>, DisputeError> {
    let dispute: Dispute = env
        .storage()
        .persistent()
        .get(&(Symbol::new(env, "dispute"), dispute_id))
        .ok_or(DisputeError::DisputeNotFound)?;
    Ok(dispute.evidence_hash)
}
