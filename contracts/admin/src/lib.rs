#![no_std]

//! Callora admin library.
//!
//! Shared administrative building blocks for Callora Soroban contracts.
//!
//! | Module | Purpose | Status |
//! |--------|---------|--------|
//! | [`admin`] | Two-step admin transfer with a timelock/grace window | Library-only; not yet consumed |
//! | [`limits`] | Per-account caps for *bets* / *positions* / *subscriptions* | **Experimental** — see below |
//! | [`errors`] / [`events`] | Error and event vocabulary shared by the above | — |
//!
//! # Consumers
//!
//! No contract in this workspace currently depends on `callora-admin`: it is a
//! library of free functions with no `#[contract]` entrypoints, so it can be
//! linked but is inert until a contract imports it. Treat the whole crate as
//! scaffolding until a consumer is added.
//!
//! # Relationship to `contracts/yield`
//!
//! [`limits`] duplicates the per-account cap concept that is already
//! implemented, tested, and wired up as a standalone contract in
//! `contracts/yield` (`CalloraYieldLimits`; see `contracts/yield/YIELD_LIMITS.md`).
//! The two differ:
//!
//! | | `callora-admin::limits` | `contracts/yield` |
//! |---|---|---|
//! | Shape | free functions, no entrypoints | `#[contract]` + `#[contractimpl]` |
//! | Per-account caps | persistent | instance |
//! | Live counters | persistent | persistent |
//! | Error type | [`errors::AdminLimitError`] | `YieldLimitError` |
//! | Consumers | none | the yield limits contract |
//!
//! Because [`limits`] has no consumer and its *bet* / *position* /
//! *subscription* terms do not map to a shipped Callora flow, it is
//! **experimental** and should be consolidated into `contracts/yield` or
//! removed before any contract depends on it. See issue #1248.

pub mod admin;
pub mod errors;
pub mod events;
pub mod limits;

#[cfg(test)]
mod test;
