// events::publish is deprecated in favour of #[contractevent]; migration tracked separately.
#![allow(deprecated)]
#![allow(dead_code)]
use crate::merchant_registry::KycTier;
use soroban_sdk::{contracterror, contracttype, vec, Address, Env, Symbol, Vec};

// Role-based access control implementation
pub fn role_admin(env: &Env) -> Symbol {
    Symbol::new(env, "ADMIN")
}

pub fn role_oracle(env: &Env) -> Symbol {
    Symbol::new(env, "ORACLE")
}

pub fn role_merchant(env: &Env) -> Symbol {
    Symbol::new(env, "MERCHANT")
}

pub fn role_settlement_operator(env: &Env) -> Symbol {
    Symbol::new(env, "SETTLEMENT_OPERATOR")
}

pub fn role_arbitrator(env: &Env) -> Symbol {
    Symbol::new(env, "ARBITRATOR")
}

#[contracterror]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AccessControlError {
    Unauthorized = 1,
    RoleAlreadyGranted = 2,
    RoleNotGranted = 3,
    CannotRenounceAdmin = 4,
    InvalidAdmin = 5,
    RevocationCooldownActive = 6,
    NoPendingRevocation = 7,
    RecoveryKeyNotSet = 8,
    ProposalNotFound = 9,
    ProposalAlreadyVoted = 10,
    ProposalExpired = 11,
    ProposalThresholdNotMet = 12,
    PendingAdminTransfer = 13,
    InvalidRecovery = 14,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PendingRevocation {
    pub role: Symbol,
    pub account: Address,
    pub created_at: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdminProposal {
    pub nonce: u64,
    pub action: AdminAction,
    pub approvals: Vec<Address>,
    pub created_at: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AdminAction {
    SetGlobalPause(bool, Symbol),
    AllowToken(Address),
    GrantRole(Symbol, Address),
    RevokeRole(Symbol, Address),
    TransferAdmin(Address),
    EmergencyRevokeRole(Symbol, Address),
    /// Set the global dispute bond amount (in stablecoin stroops).
    SetDisputeBond(i128),
    /// Set the monthly volume cap for a specific KYC tier (in stablecoin stroops).
    SetVolumeCap(KycTier, i128),
    /// Set the refund fee in basis points (e.g. 100 = 1%).
    SetRefundFeeBps(i128),
    /// Set the global payment creation rate limit: window_secs and max_per_window.
    SetRateLimit(u32, u64),
}

#[contracttype]
pub enum AccessControlDataKey {
    Role(Symbol, Address),
    Admin,
    /// Stores the list of all addresses holding a given role.
    RoleMembers(Symbol),
    /// Pending role revocation: (role, account) → PendingRevocation
    PendingRevocation(Symbol, Address),
    /// Recovery key address
    RecoveryKey,
    /// Pending admin transfer (two-step)
    PendingAdminTransfer,
    /// Admin transfer lock-in period (in seconds)
    AdminTransferLockIn,
    /// Multi-sig config: (threshold, signers)
    MultisigConfig,
    /// Next proposal nonce
    NextProposalNonce,
    /// Proposals: nonce → AdminProposal
    Proposal(u64),
}

pub struct AccessControl;

impl AccessControl {
    pub fn initialize(env: &Env, admin: Address) {
        env.storage()
            .persistent()
            .set(&AccessControlDataKey::Admin, &admin);
        Self::grant_role_internal(env, &role_admin(env), &admin);

        // Set default multi-sig config: threshold 1, only admin as signer
        let signers = vec![env, admin.clone()];
        env.storage()
            .persistent()
            .set(&AccessControlDataKey::MultisigConfig, &(1u32, signers));

        // Initialize next proposal nonce
        env.storage()
            .persistent()
            .set(&AccessControlDataKey::NextProposalNonce, &0u64);

        // Set default admin transfer lock-in to 7 days
        let lock_in = 7 * 24 * 60 * 60; // 7 days in seconds
        env.storage()
            .persistent()
            .set(&AccessControlDataKey::AdminTransferLockIn, &lock_in);
    }

    pub fn initialize_with_recovery(env: &Env, admin: Address, recovery_key: Address) {
        Self::initialize(env, admin.clone());
        env.storage()
            .persistent()
            .set(&AccessControlDataKey::RecoveryKey, &recovery_key);
    }

    pub fn set_multisig_config(
        env: &Env,
        admin: Address,
        threshold: u32,
        signers: Vec<Address>,
    ) -> Result<(), AccessControlError> {
        admin.require_auth();
        if !Self::has_role(env, &role_admin(env), &admin) {
            return Err(AccessControlError::Unauthorized);
        }
        if threshold == 0 || threshold > signers.len() {
            return Err(AccessControlError::InvalidAdmin);
        }
        env.storage()
            .persistent()
            .set(&AccessControlDataKey::MultisigConfig, &(threshold, signers));
        Ok(())
    }

    pub fn get_multisig_config(env: &Env) -> (u32, Vec<Address>) {
        env.storage()
            .persistent()
            .get(&AccessControlDataKey::MultisigConfig)
            .unwrap_or((1u32, vec![env]))
    }

    pub fn create_proposal(
        env: &Env,
        signer: Address,
        action: AdminAction,
    ) -> Result<u64, AccessControlError> {
        signer.require_auth();
        let (_, signers) = Self::get_multisig_config(env);
        if !signers.iter().any(|s| s == signer) {
            return Err(AccessControlError::Unauthorized);
        }

        let nonce: u64 = env
            .storage()
            .persistent()
            .get(&AccessControlDataKey::NextProposalNonce)
            .unwrap_or(0u64);

        let proposal = AdminProposal {
            nonce,
            action,
            approvals: vec![env, signer.clone()],
            created_at: env.ledger().timestamp(),
        };

        env.storage()
            .persistent()
            .set(&AccessControlDataKey::Proposal(nonce), &proposal);
        env.storage()
            .persistent()
            .set(&AccessControlDataKey::NextProposalNonce, &(nonce + 1));

        env.events().publish(
            (
                Symbol::new(env, "ACCESS_CONTROL"),
                Symbol::new(env, "PROPOSAL_CREATED"),
            ),
            (nonce,),
        );

        Ok(nonce)
    }

    pub fn vote_proposal(env: &Env, signer: Address, nonce: u64) -> Result<(), AccessControlError> {
        signer.require_auth();
        let (_, signers) = Self::get_multisig_config(env);
        if !signers.iter().any(|s| s == signer) {
            return Err(AccessControlError::Unauthorized);
        }

        let mut proposal: AdminProposal = env
            .storage()
            .persistent()
            .get(&AccessControlDataKey::Proposal(nonce))
            .ok_or(AccessControlError::ProposalNotFound)?;

        for a in proposal.approvals.iter() {
            if a == signer {
                return Err(AccessControlError::ProposalAlreadyVoted);
            }
        }

        proposal.approvals.push_back(signer.clone());
        env.storage()
            .persistent()
            .set(&AccessControlDataKey::Proposal(nonce), &proposal);

        env.events().publish(
            (
                Symbol::new(env, "ACCESS_CONTROL"),
                Symbol::new(env, "PROPOSAL_VOTED"),
            ),
            (nonce, signer),
        );

        Ok(())
    }

    pub fn execute_proposal(
        env: &Env,
        nonce: u64,
    ) -> Result<Option<AdminAction>, AccessControlError> {
        let (threshold, _) = Self::get_multisig_config(env);
        let proposal: AdminProposal = env
            .storage()
            .persistent()
            .get(&AccessControlDataKey::Proposal(nonce))
            .ok_or(AccessControlError::ProposalNotFound)?;

        let now = env.ledger().timestamp();
        let expiry = proposal.created_at + 48 * 60 * 60; // 48-hour expiry
        if now > expiry {
            return Err(AccessControlError::ProposalExpired);
        }

        if proposal.approvals.len() < threshold {
            return Err(AccessControlError::ProposalThresholdNotMet);
        }

        let remaining_action = match &proposal.action {
            AdminAction::GrantRole(role, account) => {
                if Self::has_role(env, role, account) {
                    return Err(AccessControlError::RoleAlreadyGranted);
                }
                Self::grant_role_internal(env, role, account);
                None
            }
            AdminAction::RevokeRole(role, account) => {
                if !Self::has_role(env, role, account) {
                    return Err(AccessControlError::RoleNotGranted);
                }
                Self::revoke_role_internal(env, role, account);
                None
            }
            AdminAction::EmergencyRevokeRole(role, account) => {
                if !Self::has_role(env, role, account) {
                    return Err(AccessControlError::RoleNotGranted);
                }
                Self::revoke_role_internal(env, role, account);
                None
            }
            AdminAction::TransferAdmin(new_admin) => {
                let old_admin = Self::get_admin(env).unwrap();
                Self::revoke_role_internal(env, &role_admin(env), &old_admin);
                Self::grant_role_internal(env, &role_admin(env), new_admin);
                env.storage()
                    .persistent()
                    .set(&AccessControlDataKey::Admin, new_admin);
                None
            }
            AdminAction::SetGlobalPause(_, _) => Some(proposal.action.clone()),
            AdminAction::AllowToken(_) => Some(proposal.action.clone()),
            // New parameter-change actions: return to caller for dispatch
            AdminAction::SetDisputeBond(_) => Some(proposal.action.clone()),
            AdminAction::SetVolumeCap(_, _) => Some(proposal.action.clone()),
            AdminAction::SetRefundFeeBps(_) => Some(proposal.action.clone()),
            AdminAction::SetRateLimit(_, _) => Some(proposal.action.clone()),
        };

        env.storage()
            .persistent()
            .remove(&AccessControlDataKey::Proposal(nonce));

        Ok(remaining_action)
    }

    pub fn get_proposal(env: &Env, nonce: u64) -> Option<AdminProposal> {
        env.storage()
            .persistent()
            .get(&AccessControlDataKey::Proposal(nonce))
    }

    pub fn grant_role(
        env: &Env,
        admin: Address,
        role: Symbol,
        account: Address,
    ) -> Result<(), AccessControlError> {
        admin.require_auth();
        if !Self::has_role(env, &role_admin(env), &admin) {
            return Err(AccessControlError::Unauthorized);
        }

        if Self::has_role(env, &role, &account) {
            return Err(AccessControlError::RoleAlreadyGranted);
        }

        Self::grant_role_internal(env, &role, &account);

        // Emit event
        let now = env.ledger().timestamp();
        env.events().publish(
            (
                Symbol::new(env, "ACCESS_CONTROL"),
                Symbol::new(env, "ROLE_GRANTED"),
            ),
            (role, account.clone(), admin.clone(), now),
        );

        Ok(())
    }

    const REVOCATION_COOLDOWN_SECS: u64 = 24 * 60 * 60; // 24 hours

    pub fn revoke_role(
        env: &Env,
        admin: Address,
        role: Symbol,
        account: Address,
    ) -> Result<(), AccessControlError> {
        admin.require_auth();
        if !Self::has_role(env, &role_admin(env), &admin) {
            return Err(AccessControlError::Unauthorized);
        }

        if !Self::has_role(env, &role, &account) {
            return Err(AccessControlError::RoleNotGranted);
        }

        // Check if this is a critical role needing cooldown
        let is_critical = role == role_oracle(env) || role == role_settlement_operator(env);

        if is_critical {
            // Start cooldown period
            let pending = PendingRevocation {
                role: role.clone(),
                account: account.clone(),
                created_at: env.ledger().timestamp(),
            };
            env.storage().persistent().set(
                &AccessControlDataKey::PendingRevocation(role.clone(), account.clone()),
                &pending,
            );

            env.events().publish(
                (
                    Symbol::new(env, "ACCESS_CONTROL"),
                    Symbol::new(env, "REVOCATION_PENDING"),
                ),
                (role, account.clone(), pending.created_at),
            );

            Ok(())
        } else {
            // Revoke immediately for non-critical roles
            Self::revoke_role_internal(env, &role, &account);

            let now = env.ledger().timestamp();
            env.events().publish(
                (
                    Symbol::new(env, "ACCESS_CONTROL"),
                    Symbol::new(env, "ROLE_REVOKED"),
                ),
                (role, account.clone(), admin.clone(), now),
            );

            Ok(())
        }
    }

    pub fn finalize_revocation(
        env: &Env,
        admin: Address,
        role: Symbol,
        account: Address,
    ) -> Result<(), AccessControlError> {
        admin.require_auth();
        if !Self::has_role(env, &role_admin(env), &admin) {
            return Err(AccessControlError::Unauthorized);
        }

        let pending: PendingRevocation = env
            .storage()
            .persistent()
            .get(&AccessControlDataKey::PendingRevocation(
                role.clone(),
                account.clone(),
            ))
            .ok_or(AccessControlError::NoPendingRevocation)?;

        let now = env.ledger().timestamp();
        if now < pending.created_at + Self::REVOCATION_COOLDOWN_SECS {
            return Err(AccessControlError::RevocationCooldownActive);
        }

        if !Self::has_role(env, &role, &account) {
            return Err(AccessControlError::RoleNotGranted);
        }

        Self::revoke_role_internal(env, &role, &account);
        env.storage()
            .persistent()
            .remove(&AccessControlDataKey::PendingRevocation(
                role.clone(),
                account.clone(),
            ));

        env.events().publish(
            (
                Symbol::new(env, "ACCESS_CONTROL"),
                Symbol::new(env, "ROLE_REVOKED"),
            ),
            (role, account.clone(), admin.clone(), now),
        );

        Ok(())
    }

    pub fn cancel_revocation(
        env: &Env,
        admin: Address,
        role: Symbol,
        account: Address,
    ) -> Result<(), AccessControlError> {
        admin.require_auth();
        if !Self::has_role(env, &role_admin(env), &admin) {
            return Err(AccessControlError::Unauthorized);
        }

        if !env
            .storage()
            .persistent()
            .has(&AccessControlDataKey::PendingRevocation(
                role.clone(),
                account.clone(),
            ))
        {
            return Err(AccessControlError::NoPendingRevocation);
        }

        env.storage()
            .persistent()
            .remove(&AccessControlDataKey::PendingRevocation(
                role.clone(),
                account.clone(),
            ));

        let now = env.ledger().timestamp();
        env.events().publish(
            (
                Symbol::new(env, "ACCESS_CONTROL"),
                Symbol::new(env, "REVOCATION_CANCELLED"),
            ),
            (role, account.clone(), admin.clone(), now),
        );

        Ok(())
    }

    pub fn emergency_revoke_role(
        env: &Env,
        admin: Address,
        role: Symbol,
        account: Address,
    ) -> Result<(), AccessControlError> {
        admin.require_auth();
        if !Self::has_role(env, &role_admin(env), &admin) {
            return Err(AccessControlError::Unauthorized);
        }

        if !Self::has_role(env, &role, &account) {
            return Err(AccessControlError::RoleNotGranted);
        }

        Self::revoke_role_internal(env, &role, &account);

        // Clean up any pending revocation if exists
        if env
            .storage()
            .persistent()
            .has(&AccessControlDataKey::PendingRevocation(
                role.clone(),
                account.clone(),
            ))
        {
            env.storage()
                .persistent()
                .remove(&AccessControlDataKey::PendingRevocation(
                    role.clone(),
                    account.clone(),
                ));
        }

        let now = env.ledger().timestamp();
        env.events().publish(
            (
                Symbol::new(env, "ACCESS_CONTROL"),
                Symbol::new(env, "ROLE_REVOKED_EMERGENCY"),
            ),
            (role, account.clone(), admin.clone(), now),
        );

        Ok(())
    }

    pub fn get_pending_revocation(
        env: &Env,
        role: Symbol,
        account: Address,
    ) -> Option<PendingRevocation> {
        env.storage()
            .persistent()
            .get(&AccessControlDataKey::PendingRevocation(role, account))
    }

    pub fn has_role(env: &Env, role: &Symbol, account: &Address) -> bool {
        env.storage()
            .persistent()
            .get(&AccessControlDataKey::Role(role.clone(), account.clone()))
            .unwrap_or(false)
    }

    pub fn renounce_role(
        env: &Env,
        account: Address,
        role: Symbol,
    ) -> Result<(), AccessControlError> {
        if role == role_admin(env) {
            return Err(AccessControlError::CannotRenounceAdmin);
        }

        if !Self::has_role(env, &role, &account) {
            return Ok(());
        }

        Self::revoke_role_internal(env, &role, &account);

        let now = env.ledger().timestamp();
        env.events().publish(
            (
                Symbol::new(env, "ACCESS_CONTROL"),
                Symbol::new(env, "ROLE_RENOUNCED"),
            ),
            (role, account.clone(), now),
        );

        Ok(())
    }

    pub fn propose_admin(
        env: &Env,
        current_admin: Address,
        new_admin: Address,
    ) -> Result<(), AccessControlError> {
        current_admin.require_auth();
        if !Self::has_role(env, &role_admin(env), &current_admin) {
            return Err(AccessControlError::Unauthorized);
        }

        if env
            .storage()
            .persistent()
            .has(&AccessControlDataKey::PendingAdminTransfer)
        {
            return Err(AccessControlError::PendingAdminTransfer);
        }

        // Start pending transfer instead of immediate
        let now = env.ledger().timestamp();
        env.storage().persistent().set(
            &AccessControlDataKey::PendingAdminTransfer,
            &(new_admin.clone(), now),
        );

        env.events().publish(
            (
                Symbol::new(env, "ACCESS_CONTROL"),
                Symbol::new(env, "ADMIN_TRANSFER_PROPOSED"),
            ),
            (new_admin.clone(), now),
        );

        Ok(())
    }

    pub fn transfer_admin(
        env: &Env,
        current_admin: Address,
        new_admin: Address,
    ) -> Result<(), AccessControlError> {
        Self::propose_admin(env, current_admin, new_admin)
    }

    pub fn claim_admin(env: &Env, new_admin: Address) -> Result<(), AccessControlError> {
        new_admin.require_auth();
        let (pending_admin, _proposed_at): (Address, u64) = env
            .storage()
            .persistent()
            .get(&AccessControlDataKey::PendingAdminTransfer)
            .ok_or(AccessControlError::PendingAdminTransfer)?;

        if pending_admin != new_admin {
            return Err(AccessControlError::Unauthorized);
        }

        let now = env.ledger().timestamp();
        let old_admin = Self::get_admin(env).unwrap();

        Self::revoke_role_internal(env, &role_admin(env), &old_admin);
        Self::grant_role_internal(env, &role_admin(env), &new_admin);

        env.storage()
            .persistent()
            .set(&AccessControlDataKey::Admin, &new_admin);
        env.storage()
            .persistent()
            .remove(&AccessControlDataKey::PendingAdminTransfer);

        env.events().publish(
            (
                Symbol::new(env, "ACCESS_CONTROL"),
                Symbol::new(env, "ADMIN_TRANSFER_COMPLETED"),
            ),
            (old_admin, new_admin.clone(), now),
        );

        Ok(())
    }

    pub fn accept_admin_transfer(env: &Env, new_admin: Address) -> Result<(), AccessControlError> {
        Self::claim_admin(env, new_admin)
    }

    pub fn cancel_admin_transfer(
        env: &Env,
        current_admin: Address,
    ) -> Result<(), AccessControlError> {
        current_admin.require_auth();
        if !Self::has_role(env, &role_admin(env), &current_admin) {
            return Err(AccessControlError::Unauthorized);
        }

        if !env
            .storage()
            .persistent()
            .has(&AccessControlDataKey::PendingAdminTransfer)
        {
            return Err(AccessControlError::PendingAdminTransfer);
        }

        env.storage()
            .persistent()
            .remove(&AccessControlDataKey::PendingAdminTransfer);

        Ok(())
    }

    pub fn get_pending_admin_transfer(env: &Env) -> Option<(Address, u64)> {
        env.storage()
            .persistent()
            .get(&AccessControlDataKey::PendingAdminTransfer)
    }

    pub fn get_admin(env: &Env) -> Option<Address> {
        env.storage().persistent().get(&AccessControlDataKey::Admin)
    }

    pub fn set_recovery_key(
        env: &Env,
        admin: Address,
        recovery_key: Address,
    ) -> Result<(), AccessControlError> {
        admin.require_auth();
        if !Self::has_role(env, &role_admin(env), &admin) {
            return Err(AccessControlError::Unauthorized);
        }
        env.storage()
            .persistent()
            .set(&AccessControlDataKey::RecoveryKey, &recovery_key);
        Ok(())
    }

    pub fn get_recovery_key(env: &Env) -> Option<Address> {
        env.storage()
            .persistent()
            .get(&AccessControlDataKey::RecoveryKey)
    }

    pub fn recovery_initiate_admin_transfer(
        env: &Env,
        recovery_key: Address,
        new_admin: Address,
    ) -> Result<(), AccessControlError> {
        recovery_key.require_auth();
        let stored_recovery: Address = env
            .storage()
            .persistent()
            .get(&AccessControlDataKey::RecoveryKey)
            .ok_or(AccessControlError::RecoveryKeyNotSet)?;

        if recovery_key != stored_recovery {
            return Err(AccessControlError::Unauthorized);
        }

        let now = env.ledger().timestamp();
        let lock_in = 30 * 24 * 60 * 60; // 30 days for recovery-initiated transfer
        env.storage().persistent().set(
            &AccessControlDataKey::PendingAdminTransfer,
            &(new_admin.clone(), now),
        );
        env.storage()
            .persistent()
            .set(&AccessControlDataKey::AdminTransferLockIn, &lock_in);

        env.events().publish(
            (
                Symbol::new(env, "ACCESS_CONTROL"),
                Symbol::new(env, "RECOVERY_ADMIN_TRANSFER_PROPOSED"),
            ),
            (new_admin.clone(), now),
        );

        Ok(())
    }

    #[allow(dead_code)]
    pub fn require_role(
        env: &Env,
        role: &Symbol,
        account: &Address,
    ) -> Result<(), AccessControlError> {
        if !Self::has_role(env, role, account) {
            return Err(AccessControlError::Unauthorized);
        }
        Ok(())
    }

    /// Returns all addresses currently holding the given role.
    pub fn get_role_members(env: &Env, role: &Symbol) -> Vec<Address> {
        env.storage()
            .persistent()
            .get(&AccessControlDataKey::RoleMembers(role.clone()))
            .unwrap_or_else(|| vec![env])
    }

    fn grant_role_internal(env: &Env, role: &Symbol, account: &Address) {
        env.storage().persistent().set(
            &AccessControlDataKey::Role(role.clone(), account.clone()),
            &true,
        );

        // Maintain the role members index
        let key = AccessControlDataKey::RoleMembers(role.clone());
        let mut members: Vec<Address> = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| vec![env]);

        // Only add if not already present (guard against double-add)
        let mut found = false;
        for m in members.iter() {
            if m == *account {
                found = true;
                break;
            }
        }
        if !found {
            members.push_back(account.clone());
            env.storage().persistent().set(&key, &members);
        }
    }

    fn revoke_role_internal(env: &Env, role: &Symbol, account: &Address) {
        env.storage()
            .persistent()
            .remove(&AccessControlDataKey::Role(role.clone(), account.clone()));

        // Remove from the role members index
        let key = AccessControlDataKey::RoleMembers(role.clone());
        let members: Vec<Address> = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| vec![env]);

        let mut updated = vec![env];
        for m in members.iter() {
            if m != *account {
                updated.push_back(m);
            }
        }
        env.storage().persistent().set(&key, &updated);
    }
}
