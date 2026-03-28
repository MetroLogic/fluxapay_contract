use soroban_sdk::{contract, contractimpl, contracttype, Address, Env, String, Symbol};

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaymentLink {
    pub link_id: String,
    pub merchant_id: Address,
    pub amount: Option<i128>,
    pub currency: Symbol,
    pub description: String,
    pub expires_at: Option<u64>,
    pub max_uses: Option<u32>,
    pub use_count: u32,
    pub active: bool,
}

#[contracttype]
pub enum LinkDataKey {
    Link(String),
}

#[contract]
pub struct PaymentLinkManager;

#[contractimpl]
impl PaymentLinkManager {
    pub fn create_link(
        env: Env,
        merchant: Address,
        link_id: String,
        amount: Option<i128>,
        currency: Symbol,
        description: String,
        expires_at: Option<u64>,
        max_uses: Option<u32>,
    ) -> String {
        merchant.require_auth();

        let link = PaymentLink {
            link_id: link_id.clone(),
            merchant_id: merchant.clone(),
            amount,
            currency,
            description,
            expires_at,
            max_uses,
            use_count: 0,
            active: true,
        };

        env.storage()
            .persistent()
            .set(&LinkDataKey::Link(link_id.clone()), &link);

        // Emit LINK/CREATED event
        env.events().publish(
            (Symbol::new(&env, "LINK"), Symbol::new(&env, "CREATED")),
            (link_id.clone(), merchant),
        );

        link_id
    }

    pub fn use_link(
        env: Env,
        payer: Address,
        link_id: String,
        amount: i128,
    ) -> Result<String, crate::Error> {
        payer.require_auth();

        let mut link = Self::get_link_internal(&env, &link_id)?;

        if !link.active {
            return Err(crate::Error::Unauthorized);
        }

        if let Some(expires_at) = link.expires_at {
            if env.ledger().timestamp() > expires_at {
                return Err(crate::Error::PaymentExpired);
            }
        }

        if let Some(max_uses) = link.max_uses {
            if link.use_count >= max_uses {
                return Err(crate::Error::PaymentAlreadyProcessed);
            }
        }

        if let Some(fixed_amount) = link.amount {
            if amount != fixed_amount {
                return Err(crate::Error::InvalidAmount);
            }
        } else if amount <= 0 {
            return Err(crate::Error::InvalidAmount);
        }

        link.use_count += 1;
        env.storage()
            .persistent()
            .set(&LinkDataKey::Link(link_id.clone()), &link);

        // Generate a virtual payment ID for tracking
        let payment_id = crate::format_id(&env, "lnk_pay_", env.ledger().timestamp());

        // Emit LINK/USED event
        env.events().publish(
            (Symbol::new(&env, "LINK"), Symbol::new(&env, "USED")),
            (link_id, payer, amount, payment_id.clone()),
        );

        Ok(payment_id)
    }

    pub fn deactivate_link(env: Env, merchant: Address, link_id: String) -> Result<(), crate::Error> {
        merchant.require_auth();

        let mut link = Self::get_link_internal(&env, &link_id)?;

        if link.merchant_id != merchant {
            return Err(crate::Error::Unauthorized);
        }

        link.active = false;
        env.storage()
            .persistent()
            .set(&LinkDataKey::Link(link_id.clone()), &link);

        // Emit LINK/DEACTIVATED event
        env.events().publish(
            (Symbol::new(&env, "LINK"), Symbol::new(&env, "DEACTIVATED")),
            link_id,
        );

        Ok(())
    }

    pub fn get_link(env: Env, link_id: String) -> Result<PaymentLink, crate::Error> {
        Self::get_link_internal(&env, &link_id)
    }

    fn get_link_internal(env: &Env, link_id: &String) -> Result<PaymentLink, crate::Error> {
        env.storage()
            .persistent()
            .get(&LinkDataKey::Link(link_id.clone()))
            .ok_or(crate::Error::PaymentNotFound)
    }
}
