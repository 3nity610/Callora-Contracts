//! Yield deposit surface for Callora.
///
/// Protocol yield deposits are implemented by
/// [`callora_revenue_pool::RevenuePool::deposit_yield`]. This crate hosts the
/// cross-contract call safety integration tests under `tests/xcontract.rs`.
///
/// # Per-account limits surface (Issue #842 / task b#017)
///
/// As of issue #842, this crate exposes a per-account state-cap surface that
/// prevents a single account from farming yield-bets, yield-positions, or
/// yield-subscriptions beyond a configurable maximum:
///
/// - [`limits::CalloraYieldLimits`] — the on-chain Soroban contract that
///   enforces per-account caps.
/// - [`limits::AccountLimits`] — the caps struct (`max_bets`, `max_positions`,
///   `max_subscriptions`).
/// - [`limits::AccountState`] — the live counters (`bets`, `positions`,
///   `subscriptions`).
/// - [`errors::YieldLimitError`] — stable `u32` error codes returned by
///   the contract.
///
/// # Trust model for counter mutation (Issue #843)
///
/// Counter increments and decrements are not self-reported. Only an admin-
/// configured operator contract may mutate an account's live counters. The
/// operator is the contract that actually creates bets, positions, and
/// subscriptions, and it must authorize each mutation. Accounts have no
/// unilateral path to decrement their own counters, so the caps cannot be
/// circumvented by clearing and re-placing.
///
/// The surface is fully backward compatible with prior versions: the existing
/// `RevenuePool` re-exports remain unchanged, no prior symbol was renamed,
/// and no prior storage key was reused.

#![no_std]

pub use callora_revenue_pool::{RevenuePool, RevenuePoolClient};

pub mod errors;
pub mod events;
pub mod limits;
pub mod views;

pub use errors::YieldLimitError;
pub use limits::{AccountLimits, AccountState, CalloraYieldLimits, CalloraYieldLimitsClient};

#cfn(test)]mmod test_limits;
