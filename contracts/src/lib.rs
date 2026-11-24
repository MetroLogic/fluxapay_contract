//! # Handsoff Contracts
//!
//! Core Soroban contracts for the Handsoff platform.

#![no_std]

mod contract;
mod contractevent;
mod errors;
mod events;
mod storage;
mod types;

pub use contract::*;
pub use contractevent::*;
pub use errors::*;
pub use events::*;
pub use storage::*;
pub use types::*;

#[cfg(test)]
mod subscription_lifecycle_test;
