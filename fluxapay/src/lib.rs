#![no_std]
#![allow(clippy::too_many_arguments)]

pub mod constants;
pub mod data_keys;
pub mod payment_processor;
pub mod refund_manager;
pub mod types;

pub use constants::*;
pub use data_keys::*;
pub use payment_processor::*;
pub use refund_manager::*;
pub use types::*;

mod access_control;
pub mod account_abstraction;
mod dex_router;
pub mod events;
pub mod fx_oracle;
pub mod merchant_auth;
mod payment_state_machine;

pub use access_control::AccessControlDataKey;
pub use access_control::{AdminAction, AdminProposal};
pub use dex_router::{DexRouter, DexRouterClient};
pub use fx_oracle::{FXOracle, FXOracleClient, FXOracleError};
pub use merchant_auth::{MerchantAuthError, MerchantAuthorization, MerchantPreAuth};

pub mod stream;
pub use stream::{PaymentStream, PaymentStreaming, StreamError, StreamStatus};

pub mod utils;
pub use utils::{format_id, is_valid_cid, validate_id, validate_ipfs_multihash};

pub mod gas_estimator;
pub use gas_estimator::{CostEstimate, GasEstimator, GasEstimatorClient, Operation};

pub mod merchant_registry;
pub use merchant_registry::{
    FeeConfig, KycTier, MaybeFeeConfig, Merchant, MerchantError, MerchantRegistry, MerchantRegistryClient,
};

pub mod payment_link;
pub use payment_link::{
    CreateLinkArgs, FiatConfig, LinkAnalytics, MaybeFiatConfig, PaymentLink, PaymentLinkManager,
    PaymentLinkManagerClient,
};

#[cfg(test)] mod test;
#[cfg(test)] mod stream_test;
#[cfg(test)] mod subscription_test;
#[cfg(test)] mod arbitrage_test;
#[cfg(test)] mod auth_test;
#[cfg(test)] mod batch_payment_test;
#[cfg(test)] mod dex_router_test;
#[cfg(test)] mod dispute_test;
#[cfg(test)] mod escalate_disputes_test;
#[cfg(test)] mod feature_tests;
#[cfg(test)] mod fx_oracle_test;
#[cfg(test)] mod integration_test;
#[cfg(test)] mod memo_test;
#[cfg(test)] mod merchant_ranking_test;
#[cfg(test)] mod merchant_registry_test;
#[cfg(test)] mod mock_dex_router;
#[cfg(test)] mod muxed_payer_test;
#[cfg(test)] mod oracle_sanitization_test;
#[cfg(test)] mod partial_overpaid_test;
#[cfg(test)] mod pause_test;
#[cfg(test)] mod payment_link_test;
#[cfg(test)] mod payment_metadata_test;
#[cfg(test)] mod proptests;
#[cfg(test)] mod router_allowlist_test;
#[cfg(test)] mod settlement_test;
#[cfg(test)] mod swap_test;
