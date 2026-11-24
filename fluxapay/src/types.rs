use soroban_sdk::{contracttype, Address, String, Symbol, Vec};

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SwapSplit {
    pub router: Address,
    pub amount: i128,
    pub path: Vec<Address>,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SwapAndPayArgsV2 {
    pub payer: Address,
    pub payment_id: String,
    pub merchant_id: Address,
    pub currency: Symbol,
    pub deposit_address: Address,
    pub token_in: Address,
    pub expires_at: Option<u64>,
    pub fx_oracle: Option<Address>,
    pub oracle_pair: Option<Symbol>,
    pub max_deviation_bps: u32,
    pub splits: Vec<SwapSplit>,
    pub min_output: i128,
}

pub const MAX_SWAP_SPLITS: u32 = 3;
