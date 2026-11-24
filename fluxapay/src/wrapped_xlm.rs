use soroban_sdk::{contractclient, Address, Env};

#[contractclient(name = "WrappedXlmClient")]
pub trait WrappedXlm {
    fn wrap(env: Env, from: Address, amount: i128, recipient: Address);
}
