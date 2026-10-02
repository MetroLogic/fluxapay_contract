//! Sponsored meta-transactions (Issue #893).
//!
//! A merchant signs a domain-separated payload off-chain. A relayer submits
//! that payload and pays the network fee. The contract recovers the merchant
//! from the Ed25519 key embedded in `merchant_id`, checks the domain
//! separator, deadline, and sequence nonce, then runs the requested merchant
//! action without a second on-chain signature from the merchant.
//!
//! The signed message is the SHA-256 digest of a canonical byte string. The
//! TypeScript helper in `sdk/src/metaTx.ts` builds the same bytes.

use soroban_sdk::{
    contracterror, contracttype, xdr::FromXdr, Address, Bytes, BytesN, Env, String, Symbol,
};

use crate::{utils, CreatePaymentArgs, PaymentProcessor};

const DOMAIN_NAME: &[u8] = b"FluxaPay";
/// Persistent nonce entries are bumped for about a year (5s ledgers) so a
/// consumed sequence number cannot fall out of state and be replayed.
const NONCE_TTL: u32 = 6_307_200;

pub const FN_CREATE_CHARGE: &str = "create_charge";
pub const FN_AUTHORIZE_REFUND: &str = "authorize_refund";
pub const FN_ROTATE_WEBHOOK_KEY: &str = "rotate_webhook_key";
pub const FN_SET_FEE_RECIPIENT: &str = "set_fee_recipient";

/// Same fee schedule as `GasEstimator` (Protocol 21 mainnet defaults).
const FEE_PER_10K_INSTRUCTIONS: i64 = 100;
const FEE_PER_LEDGER_READ: i64 = 6_250;
const FEE_PER_LEDGER_WRITE: i64 = 10_000;
const FEE_PER_EVENT: i64 = 250;
/// Verification, nonce read/write, and the meta-transaction event itself.
const META_TX_OVERHEAD_INSTR: i64 = 15;
const META_TX_OVERHEAD_READS: u32 = 2;
const META_TX_OVERHEAD_WRITES: u32 = 1;
const META_TX_OVERHEAD_EVENTS: u32 = 1;

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MetaTransactionPayload {
    pub merchant_id: Address,
    pub target_function: Symbol,
    pub call_data: Bytes,
    pub nonce: u64,
    pub deadline: u64,
    /// Separator the merchant signed. Must equal [`domain_separator`].
    pub domain_separator: BytesN<32>,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MetaTxGas {
    /// Instruction units, in groups of 10_000, matching `GasEstimator`.
    pub instructions: i64,
    pub ledger_reads: u32,
    pub ledger_writes: u32,
    pub events: u32,
    pub resource_fee_stroops: i64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MetaTxReceipt {
    pub relayer: Address,
    pub merchant_id: Address,
    pub target_function: Symbol,
    pub nonce: u64,
    pub success: bool,
    pub gas: MetaTxGas,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RefundAuthRequest {
    pub payment_id: String,
    pub refund_amount: i128,
    pub reason: String,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RefundAuthorization {
    pub merchant_id: Address,
    pub payment_id: String,
    pub refund_amount: i128,
    pub reason: String,
    pub authorized_at: u64,
}

#[contracterror]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum MetaTxError {
    Expired = 1,
    InvalidNonce = 2,
    InvalidSignature = 3,
    InvalidDomain = 4,
    UnsupportedFunction = 5,
    UnauthorizedMerchant = 6,
    ExecutionFailed = 7,
    InvalidSigner = 8,
    InvalidPayload = 9,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
enum MetaTxStorageKey {
    Nonce(Address),
    WebhookKey(Address),
    FeeRecipient(Address),
    RefundAuth(Address, String),
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
enum MetaAuthKey {
    Merchant,
}

/// Domain separator for this contract on the current network.
///
/// `SHA-256(b"FluxaPay" || contract_id_hash || network_id)`.
pub fn domain_separator(env: &Env) -> BytesN<32> {
    let contract = env.current_contract_address();
    let network_id = env.ledger().network_id();
    domain_separator_for(env, &contract, &network_id)
}

/// Domain separator bound to an explicit contract address and network id.
/// Used to reject signatures that were produced for a different contract.
pub fn domain_separator_for(env: &Env, contract: &Address, network_id: &BytesN<32>) -> BytesN<32> {
    let contract_hash = contract_id_hash(env, contract);
    let mut preimage = Bytes::from_slice(env, DOMAIN_NAME);
    preimage.append(&Bytes::from_array(env, &contract_hash.to_array()));
    preimage.append(&Bytes::from_array(env, &network_id.to_array()));
    env.crypto().sha256(&preimage).to_bytes()
}

pub fn expected_nonce(env: &Env, merchant: &Address) -> u64 {
    env.storage()
        .persistent()
        .get(&MetaTxStorageKey::Nonce(merchant.clone()))
        .unwrap_or(0)
}

pub fn get_webhook_key(env: &Env, merchant: &Address) -> Option<BytesN<32>> {
    env.storage()
        .persistent()
        .get(&MetaTxStorageKey::WebhookKey(merchant.clone()))
}

pub fn get_fee_recipient(env: &Env, merchant: &Address) -> Option<Address> {
    env.storage()
        .persistent()
        .get(&MetaTxStorageKey::FeeRecipient(merchant.clone()))
}

pub fn get_refund_authorization(
    env: &Env,
    merchant: &Address,
    payment_id: &String,
) -> Option<RefundAuthorization> {
    env.storage()
        .persistent()
        .get(&MetaTxStorageKey::RefundAuth(
            merchant.clone(),
            payment_id.clone(),
        ))
}

/// Accept the call when this invocation already authenticated `merchant`
/// via a meta-transaction. Otherwise require the merchant's Soroban auth.
pub fn require_merchant_auth(env: &Env, merchant: &Address) {
    if let Some(active) = env
        .storage()
        .instance()
        .get::<MetaAuthKey, Address>(&MetaAuthKey::Merchant)
    {
        if active == *merchant {
            return;
        }
    }
    merchant.require_auth();
}

pub fn rotate_webhook_key(env: &Env, merchant: &Address, key_hash: BytesN<32>) {
    require_merchant_auth(env, merchant);
    env.storage()
        .persistent()
        .set(&MetaTxStorageKey::WebhookKey(merchant.clone()), &key_hash);
    bump_nonce_ttl(env, &MetaTxStorageKey::WebhookKey(merchant.clone()));
}

pub fn set_fee_recipient(env: &Env, merchant: &Address, recipient: Address) {
    require_merchant_auth(env, merchant);
    env.storage().persistent().set(
        &MetaTxStorageKey::FeeRecipient(merchant.clone()),
        &recipient,
    );
    bump_nonce_ttl(env, &MetaTxStorageKey::FeeRecipient(merchant.clone()));
}

pub fn authorize_refund(
    env: &Env,
    merchant: &Address,
    request: RefundAuthRequest,
) -> Result<RefundAuthorization, MetaTxError> {
    require_merchant_auth(env, merchant);
    if request.refund_amount <= 0 || !utils::validate_id(&request.payment_id) {
        return Err(MetaTxError::InvalidPayload);
    }
    if request.reason.len() == 0 || request.reason.len() > 256 {
        return Err(MetaTxError::InvalidPayload);
    }

    let record = RefundAuthorization {
        merchant_id: merchant.clone(),
        payment_id: request.payment_id.clone(),
        refund_amount: request.refund_amount,
        reason: request.reason,
        authorized_at: env.ledger().timestamp(),
    };
    let key = MetaTxStorageKey::RefundAuth(merchant.clone(), request.payment_id);
    env.storage().persistent().set(&key, &record);
    bump_nonce_ttl(env, &key);
    Ok(record)
}

/// Canonical preimage whose SHA-256 digest is the Ed25519 message.
pub fn signing_preimage(env: &Env, payload: &MetaTransactionPayload) -> Result<Bytes, MetaTxError> {
    let pubkey = ed25519_public_key(env, &payload.merchant_id).ok_or(MetaTxError::InvalidSigner)?;
    let name =
        function_name(&payload.target_function, env).ok_or(MetaTxError::UnsupportedFunction)?;
    let mut msg = Bytes::new(env);
    msg.append(&Bytes::from_array(
        env,
        &payload.domain_separator.to_array(),
    ));
    msg.append(&Bytes::from_array(env, &pubkey.to_array()));
    append_u32(env, &mut msg, name.len() as u32);
    msg.append(&Bytes::from_slice(env, name));
    append_u32(env, &mut msg, payload.call_data.len());
    msg.append(&payload.call_data);
    append_u64(env, &mut msg, payload.nonce);
    append_u64(env, &mut msg, payload.deadline);
    Ok(msg)
}

pub fn signing_digest(
    env: &Env,
    payload: &MetaTransactionPayload,
) -> Result<BytesN<32>, MetaTxError> {
    let preimage = signing_preimage(env, payload)?;
    Ok(env.crypto().sha256(&preimage).to_bytes())
}

#[allow(deprecated)] // events::publish — same pattern as the rest of the contract
pub fn execute(
    env: &Env,
    relayer: Address,
    payload: MetaTransactionPayload,
    signature: BytesN<64>,
) -> Result<MetaTxReceipt, MetaTxError> {
    relayer.require_auth();

    if env.ledger().timestamp() > payload.deadline {
        return Err(MetaTxError::Expired);
    }

    let expected_domain = domain_separator(env);
    if payload.domain_separator != expected_domain {
        return Err(MetaTxError::InvalidDomain);
    }

    let name =
        function_name(&payload.target_function, env).ok_or(MetaTxError::UnsupportedFunction)?;
    let pubkey = ed25519_public_key(env, &payload.merchant_id).ok_or(MetaTxError::InvalidSigner)?;

    let digest = signing_digest(env, &payload)?;
    let message = Bytes::from_array(env, &digest.to_array());
    // `ed25519_verify` traps when the signature does not match `merchant_id`.
    env.crypto().ed25519_verify(&pubkey, &message, &signature);

    let stored = expected_nonce(env, &payload.merchant_id);
    if payload.nonce != stored {
        return Err(MetaTxError::InvalidNonce);
    }

    let next = stored.saturating_add(1);
    let nonce_key = MetaTxStorageKey::Nonce(payload.merchant_id.clone());
    env.storage().persistent().set(&nonce_key, &next);
    bump_nonce_ttl(env, &nonce_key);

    let gas = gas_for(name);
    let outcome = dispatch(env, &payload.merchant_id, name, &payload.call_data);
    let success = outcome.is_ok();
    if !success {
        // The envelope was authentic. Keep the nonce so the same failing
        // payload cannot be replayed, and surface the failure to indexers.
        emit(
            env,
            "FAILED",
            &relayer,
            &payload.merchant_id,
            &payload.target_function,
            payload.nonce,
            &gas,
        );
        return Ok(MetaTxReceipt {
            relayer,
            merchant_id: payload.merchant_id,
            target_function: payload.target_function,
            nonce: payload.nonce,
            success: false,
            gas,
        });
    }

    emit(
        env,
        "EXECUTED",
        &relayer,
        &payload.merchant_id,
        &payload.target_function,
        payload.nonce,
        &gas,
    );
    Ok(MetaTxReceipt {
        relayer,
        merchant_id: payload.merchant_id,
        target_function: payload.target_function,
        nonce: payload.nonce,
        success: true,
        gas,
    })
}

fn dispatch(
    env: &Env,
    merchant: &Address,
    name: &[u8],
    call_data: &Bytes,
) -> Result<(), MetaTxError> {
    set_auth(env, merchant);
    let result = match name {
        b"create_charge" => dispatch_create_charge(env, merchant, call_data),
        b"authorize_refund" => dispatch_authorize_refund(env, merchant, call_data),
        b"rotate_webhook_key" => dispatch_rotate_webhook_key(env, merchant, call_data),
        b"set_fee_recipient" => dispatch_set_fee_recipient(env, merchant, call_data),
        _ => Err(MetaTxError::UnsupportedFunction),
    };
    clear_auth(env);
    result
}

fn dispatch_create_charge(
    env: &Env,
    merchant: &Address,
    call_data: &Bytes,
) -> Result<(), MetaTxError> {
    let args =
        CreatePaymentArgs::from_xdr(env, call_data).map_err(|_| MetaTxError::InvalidPayload)?;
    if args.merchant_id != *merchant {
        return Err(MetaTxError::UnauthorizedMerchant);
    }
    PaymentProcessor::create_payment(env.clone(), args)
        .map_err(|_| MetaTxError::ExecutionFailed)?;
    Ok(())
}

fn dispatch_authorize_refund(
    env: &Env,
    merchant: &Address,
    call_data: &Bytes,
) -> Result<(), MetaTxError> {
    let request =
        RefundAuthRequest::from_xdr(env, call_data).map_err(|_| MetaTxError::InvalidPayload)?;
    authorize_refund(env, merchant, request)?;
    Ok(())
}

fn dispatch_rotate_webhook_key(
    env: &Env,
    merchant: &Address,
    call_data: &Bytes,
) -> Result<(), MetaTxError> {
    let key_hash =
        BytesN::<32>::from_xdr(env, call_data).map_err(|_| MetaTxError::InvalidPayload)?;
    rotate_webhook_key(env, merchant, key_hash);
    Ok(())
}

fn dispatch_set_fee_recipient(
    env: &Env,
    merchant: &Address,
    call_data: &Bytes,
) -> Result<(), MetaTxError> {
    let recipient = Address::from_xdr(env, call_data).map_err(|_| MetaTxError::InvalidPayload)?;
    set_fee_recipient(env, merchant, recipient);
    Ok(())
}

fn set_auth(env: &Env, merchant: &Address) {
    env.storage()
        .instance()
        .set(&MetaAuthKey::Merchant, merchant);
}

fn clear_auth(env: &Env) {
    env.storage().instance().remove(&MetaAuthKey::Merchant);
}

fn emit(
    env: &Env,
    action: &str,
    relayer: &Address,
    merchant: &Address,
    target: &Symbol,
    nonce: u64,
    gas: &MetaTxGas,
) {
    env.events().publish(
        (Symbol::new(env, "META_TX"), Symbol::new(env, action)),
        (
            relayer.clone(),
            merchant.clone(),
            target.clone(),
            nonce,
            gas.clone(),
        ),
    );
}

fn gas_for(name: &[u8]) -> MetaTxGas {
    let (instr, reads, writes, events) = match name {
        b"create_charge" => (50_i64, 5_u32, 4_u32, 1_u32),
        b"authorize_refund" => (20, 1, 1, 0),
        b"rotate_webhook_key" => (8, 0, 1, 0),
        b"set_fee_recipient" => (8, 0, 1, 0),
        _ => (0, 0, 0, 0),
    };
    let instructions = instr + META_TX_OVERHEAD_INSTR;
    let ledger_reads = reads + META_TX_OVERHEAD_READS;
    let ledger_writes = writes + META_TX_OVERHEAD_WRITES;
    let events = events + META_TX_OVERHEAD_EVENTS;
    let resource_fee_stroops = instructions * FEE_PER_10K_INSTRUCTIONS
        + (ledger_reads as i64) * FEE_PER_LEDGER_READ
        + (ledger_writes as i64) * FEE_PER_LEDGER_WRITE
        + (events as i64) * FEE_PER_EVENT;
    MetaTxGas {
        instructions,
        ledger_reads,
        ledger_writes,
        events,
        resource_fee_stroops,
    }
}

fn function_name(symbol: &Symbol, env: &Env) -> Option<&'static [u8]> {
    const NAMES: [&str; 4] = [
        FN_CREATE_CHARGE,
        FN_AUTHORIZE_REFUND,
        FN_ROTATE_WEBHOOK_KEY,
        FN_SET_FEE_RECIPIENT,
    ];
    for name in NAMES {
        if symbol == &Symbol::new(env, name) {
            return Some(name.as_bytes());
        }
    }
    None
}

fn append_u32(env: &Env, buf: &mut Bytes, value: u32) {
    buf.append(&Bytes::from_array(env, &value.to_be_bytes()));
}

fn append_u64(env: &Env, buf: &mut Bytes, value: u64) {
    buf.append(&Bytes::from_array(env, &value.to_be_bytes()));
}

fn bump_nonce_ttl<K>(env: &Env, key: &K)
where
    K: soroban_sdk::IntoVal<Env, soroban_sdk::Val>,
{
    let threshold = core::cmp::max(1, NONCE_TTL / 5);
    env.storage()
        .persistent()
        .extend_ttl(key, threshold, NONCE_TTL);
}

/// Ed25519 public key encoded in an account address (G...), or `None` for
/// contract addresses. This is the key the merchant signs with off-chain.
fn ed25519_public_key(env: &Env, address: &Address) -> Option<BytesN<32>> {
    use soroban_sdk::xdr::ToXdr;
    let xdr = address.clone().to_xdr(env);
    // ScVal::Address header is 4 bytes, then ScAddress discriminant.
    if xdr.len() < 44 {
        return None;
    }
    let addr_type: BytesN<4> = xdr.slice(4..8).try_into().ok()?;
    if addr_type.to_array() != [0, 0, 0, 0] {
        return None;
    }
    let key_type: BytesN<4> = xdr.slice(8..12).try_into().ok()?;
    if key_type.to_array() != [0, 0, 0, 0] {
        return None;
    }
    xdr.slice(12..44).try_into().ok()
}

fn contract_id_hash(env: &Env, address: &Address) -> BytesN<32> {
    use soroban_sdk::xdr::ToXdr;
    let xdr = address.clone().to_xdr(env);
    let addr_type: BytesN<4> = xdr.slice(4..8).try_into().unwrap();
    if addr_type.to_array() != [0, 0, 0, 1] {
        soroban_sdk::panic_with_error!(env, MetaTxError::InvalidDomain);
    }
    xdr.slice(8..40).try_into().unwrap()
}
