use soroban_sdk::{contract, contracterror, contractimpl, contracttype, Address, Env, Symbol};

#[contract]
pub struct InvoiceContract;

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InvoiceStatus {
    Draft,
    Issued,
    Paid,
    Cancelled,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Invoice {
    pub invoice_id: Symbol,
    pub merchant_id: Address,
    pub amount: i128,
    pub currency: Symbol,
    pub due_date: u64,
    pub status: InvoiceStatus,
    pub payment_id: Option<Symbol>,
}

#[contracttype]
pub enum DataKey {
    Invoice(Symbol),
    Admin,
    PaymentProcessor,
}

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum InvoiceError {
    NotFound = 1,
    AlreadyExists = 2,
    Unauthorized = 3,
    InvalidStatus = 4,
    AlreadyPaid = 5,
}

#[contractimpl]
impl InvoiceContract {
    pub fn invoice_initialize(env: Env, admin: Address, payment_processor: Address) {
        env.storage()
            .persistent()
            .set(&DataKey::Admin, &admin);
        env.storage()
            .persistent()
            .set(&DataKey::PaymentProcessor, &payment_processor);
    }

    pub fn invoice_get_admin(env: Env) -> Option<Address> {
        env.storage().persistent().get(&DataKey::Admin)
    }

    pub fn invoice_get_payment_processor(env: Env) -> Option<Address> {
        env.storage().persistent().get(&DataKey::PaymentProcessor)
    }

    pub fn create_invoice(
        env: Env,
        merchant_id: Address,
        invoice_id: Symbol,
        amount: i128,
        currency: Symbol,
        due_date: u64,
    ) -> Result<Invoice, InvoiceError> {
        merchant_id.require_auth();

        let key = DataKey::Invoice(invoice_id.clone());
        if env.storage().persistent().has(&key) {
            return Err(InvoiceError::AlreadyExists);
        }

        let invoice = Invoice {
            invoice_id: invoice_id.clone(),
            merchant_id: merchant_id.clone(),
            amount,
            currency,
            due_date,
            status: InvoiceStatus::Draft,
            payment_id: None,
        };

        env.storage().persistent().set(&key, &invoice);

        Ok(invoice)
    }

    pub fn issue_invoice(env: Env, invoice_id: Symbol) -> Result<(), InvoiceError> {
        let key = DataKey::Invoice(invoice_id.clone());
        let mut invoice: Invoice = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(InvoiceError::NotFound)?;

        invoice.merchant_id.require_auth();

        if invoice.status != InvoiceStatus::Draft {
            return Err(InvoiceError::InvalidStatus);
        }

        invoice.status = InvoiceStatus::Issued;
        env.storage().persistent().set(&key, &invoice);

        env.events().publish(
            (
                Symbol::new(&env, "INVOICE"),
                Symbol::new(&env, "ISSUED"),
            ),
            invoice_id.clone(),
        );

        Ok(())
    }

    pub fn mark_paid(
        env: Env,
        invoice_id: Symbol,
        payment_id: Symbol,
    ) -> Result<(), InvoiceError> {
        let processor: Address = env
            .storage()
            .persistent()
            .get(&DataKey::PaymentProcessor)
            .ok_or(InvoiceError::Unauthorized)?;
        processor.require_auth();

        let key = DataKey::Invoice(invoice_id.clone());
        let mut invoice: Invoice = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(InvoiceError::NotFound)?;

        if invoice.payment_id.is_some() {
            return Err(InvoiceError::AlreadyPaid);
        }

        if invoice.status != InvoiceStatus::Issued {
            return Err(InvoiceError::InvalidStatus);
        }

        invoice.status = InvoiceStatus::Paid;
        invoice.payment_id = Some(payment_id.clone());
        env.storage().persistent().set(&key, &invoice);

        env.events().publish(
            (
                Symbol::new(&env, "INVOICE"),
                Symbol::new(&env, "PAID"),
            ),
            (invoice_id.clone(), payment_id),
        );

        Ok(())
    }

    pub fn cancel_invoice(env: Env, invoice_id: Symbol) -> Result<(), InvoiceError> {
        let key = DataKey::Invoice(invoice_id.clone());
        let mut invoice: Invoice = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(InvoiceError::NotFound)?;

        invoice.merchant_id.require_auth();

        if invoice.status == InvoiceStatus::Paid {
            return Err(InvoiceError::InvalidStatus);
        }

        if invoice.status != InvoiceStatus::Draft && invoice.status != InvoiceStatus::Issued {
            return Err(InvoiceError::InvalidStatus);
        }

        invoice.status = InvoiceStatus::Cancelled;
        env.storage().persistent().set(&key, &invoice);

        env.events().publish(
            (
                Symbol::new(&env, "INVOICE"),
                Symbol::new(&env, "CANCELLED"),
            ),
            invoice_id.clone(),
        );

        Ok(())
    }

    pub fn get_invoice(env: Env, invoice_id: Symbol) -> Result<Invoice, InvoiceError> {
        let key = DataKey::Invoice(invoice_id);
        env.storage()
            .persistent()
            .get(&key)
            .ok_or(InvoiceError::NotFound)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use soroban_sdk::testutils::Address as _;

    fn setup(env: &Env) -> (Address, Address, InvoiceContractClient<'_>) {
        let contract_id = env.register(InvoiceContract, ());
        let client = InvoiceContractClient::new(env, &contract_id);
        let admin = Address::generate(env);
        let payment_processor = Address::generate(env);
        client.invoice_initialize(&admin, &payment_processor);
        (admin, payment_processor, client)
    }

    #[test]
    fn invoice_initialize_sets_admin_and_payment_processor() {
        let env = Env::default();
        env.mock_all_auths();
        let (admin, payment_processor, client) = setup(&env);
        assert_eq!(client.invoice_get_admin(), Some(admin));
        assert_eq!(client.invoice_get_payment_processor(), Some(payment_processor));
    }

    #[test]
    fn create_invoice_success_and_duplicate_returns_already_exists() {
        let env = Env::default();
        env.mock_all_auths();
        let (_, _, client) = setup(&env);
        let merchant = Address::generate(&env);
        let inv_id = Symbol::new(&env, "INV001");
        let currency = Symbol::new(&env, "USDC");
        let due = env.ledger().timestamp() + 86400;

        let inv = client.create_invoice(&merchant, &inv_id, &100i128, &currency, &due);
        assert_eq!(inv.status, InvoiceStatus::Draft);
        assert_eq!(inv.payment_id, None);

        let err = client.try_create_invoice(&merchant, &inv_id, &100i128, &currency, &due);
        assert!(matches!(
            err,
            Err(Ok(InvoiceError::AlreadyExists))
        ));
    }

    #[test]
    fn issue_invoice_draft_to_issued() {
        let env = Env::default();
        env.mock_all_auths();
        let (_, _, client) = setup(&env);
        let merchant = Address::generate(&env);
        let inv_id = Symbol::new(&env, "INV002");
        let currency = Symbol::new(&env, "USDC");
        let due = env.ledger().timestamp() + 86400;

        client.create_invoice(&merchant, &inv_id, &50i128, &currency, &due);
        client.issue_invoice(&inv_id);

        let got = client.get_invoice(&inv_id);
        assert_eq!(got.status, InvoiceStatus::Issued);
    }

    #[test]
    fn issue_invoice_wrong_status_returns_invalid_status() {
        let env = Env::default();
        env.mock_all_auths();
        let (_, _, client) = setup(&env);
        let merchant = Address::generate(&env);
        let inv_id = Symbol::new(&env, "INV003");
        let currency = Symbol::new(&env, "USDC");
        let due = env.ledger().timestamp() + 86400;

        client.create_invoice(&merchant, &inv_id, &10i128, &currency, &due);
        client.issue_invoice(&inv_id);
        let err = client.try_issue_invoice(&inv_id);
        assert!(matches!(err, Err(Ok(InvoiceError::InvalidStatus))));
    }

    #[test]
    fn mark_paid_success_sets_paid_and_payment_id() {
        let env = Env::default();
        env.mock_all_auths();
        let (_, _payment_processor, client) = setup(&env);
        let merchant = Address::generate(&env);
        let inv_id = Symbol::new(&env, "INV004");
        let pay_id = Symbol::new(&env, "PAY99");
        let currency = Symbol::new(&env, "USDC");
        let due = env.ledger().timestamp() + 86400;

        client.create_invoice(&merchant, &inv_id, &200i128, &currency, &due);
        client.issue_invoice(&inv_id);
        client.mark_paid(&inv_id, &pay_id);

        let got = client.get_invoice(&inv_id);
        assert_eq!(got.status, InvoiceStatus::Paid);
        assert_eq!(got.payment_id, Some(pay_id));
    }

    #[test]
    fn mark_paid_twice_returns_already_paid() {
        let env = Env::default();
        env.mock_all_auths();
        let (_, _payment_processor, client) = setup(&env);
        let merchant = Address::generate(&env);
        let inv_id = Symbol::new(&env, "INV005");
        let currency = Symbol::new(&env, "USDC");
        let due = env.ledger().timestamp() + 86400;

        client.create_invoice(&merchant, &inv_id, &1i128, &currency, &due);
        client.issue_invoice(&inv_id);
        client.mark_paid(&inv_id, &Symbol::new(&env, "P1"));
        let err = client.try_mark_paid(&inv_id, &Symbol::new(&env, "P2"));
        assert!(matches!(err, Err(Ok(InvoiceError::AlreadyPaid))));
    }

    #[test]
    fn mark_paid_draft_returns_invalid_status() {
        let env = Env::default();
        env.mock_all_auths();
        let (_, _payment_processor, client) = setup(&env);
        let merchant = Address::generate(&env);
        let inv_id = Symbol::new(&env, "INV007");
        let currency = Symbol::new(&env, "USDC");
        let due = env.ledger().timestamp() + 86400;

        client.create_invoice(&merchant, &inv_id, &1i128, &currency, &due);
        let err = client.try_mark_paid(&inv_id, &Symbol::new(&env, "PX"));
        assert!(matches!(err, Err(Ok(InvoiceError::InvalidStatus))));
    }

    #[test]
    fn cancel_invoice_draft_and_issued() {
        let env = Env::default();
        env.mock_all_auths();
        let (_, _payment_processor, client) = setup(&env);
        let merchant = Address::generate(&env);
        let currency = Symbol::new(&env, "USDC");
        let due = env.ledger().timestamp() + 86400;

        let id_draft = Symbol::new(&env, "INVD");
        client.create_invoice(&merchant, &id_draft, &1i128, &currency, &due);
        client.cancel_invoice(&id_draft);
        assert_eq!(
            client.get_invoice(&id_draft).status,
            InvoiceStatus::Cancelled
        );

        let id_issued = Symbol::new(&env, "INVI");
        client.create_invoice(&merchant, &id_issued, &1i128, &currency, &due);
        client.issue_invoice(&id_issued);
        client.cancel_invoice(&id_issued);
        assert_eq!(
            client.get_invoice(&id_issued).status,
            InvoiceStatus::Cancelled
        );
    }

    #[test]
    fn cancel_paid_returns_invalid_status() {
        let env = Env::default();
        env.mock_all_auths();
        let (_, _payment_processor, client) = setup(&env);
        let merchant = Address::generate(&env);
        let inv_id = Symbol::new(&env, "INVP");
        let currency = Symbol::new(&env, "USDC");
        let due = env.ledger().timestamp() + 86400;

        client.create_invoice(&merchant, &inv_id, &1i128, &currency, &due);
        client.issue_invoice(&inv_id);
        client.mark_paid(&inv_id, &Symbol::new(&env, "PY"));
        let err = client.try_cancel_invoice(&inv_id);
        assert!(matches!(err, Err(Ok(InvoiceError::InvalidStatus))));
    }

    #[test]
    fn get_invoice_not_found() {
        let env = Env::default();
        env.mock_all_auths();
        let (_, _, client) = setup(&env);
        let err = client.try_get_invoice(&Symbol::new(&env, "NOPE"));
        assert!(matches!(err, Err(Ok(InvoiceError::NotFound))));
    }
}
