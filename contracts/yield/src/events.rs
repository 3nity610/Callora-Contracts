//! Event topic Symbol constructors for the Callora Yield surface.
//!
//! This module centralises every event topic string emitted by
//! [`crate::limits`] and the [`crate::CalloraYieldLimits`] contract, plus
//! **structured yield lifecycle events** for off-chain indexers.
//!
//! # Lifecycle events
//!
//! Yield lifecycle events carry a versioned
//! [`DistributionLifecycleEvent`] payload so indexers can correlate
//! `distribute_started` ↔ `distribute_completed` pairs across the entire
//! distribution pipeline.
//!
//! # Snapshot tests
//!
//! Snapshot tests at the bottom of the file pin every topic to its raw byte
//! representation so accidental renames fail loudly in `cargo test` *before*
//! a pull request is opened.

#![allow(dead_code)]

use soroban_sdk::{contracttype, Address, Env, Symbol};

// ---------------------------------------------------------------------------
// Lifecycle types — shared with the revenue pool but defined here so the
// yield crate does not depend on callora-revenue-pool internals.
// ---------------------------------------------------------------------------

/// Schema version for structured distribution lifecycle event payloads.
pub const DISTRIBUTION_EVENT_VERSION: u32 = 1;

/// Identifies which distribution entry point emitted a lifecycle event.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DistributionMode {
    /// Single-call `distribute`.
    Single,
    /// Multi-leg `batch_distribute`.
    Batch,
}

/// Stable, versioned payload shared by distribution lifecycle events.
///
/// Off-chain indexers can match `distribute_started` / `distribute_completed`
/// pairs by `(ledger_sequence, amount, recipient)` to verify atomicity.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DistributionLifecycleEvent {
    /// Payload schema version (bump when adding fields).
    pub version: u32,
    /// USDC amount in base units.
    pub amount: i128,
    /// Whether this is a single or batch distribution.
    pub mode: DistributionMode,
    /// Zero-based index inside a batch (`0` for single distributions).
    pub batch_index: u32,
    /// Total legs in the batch (`1` for single distributions).
    pub batch_size: u32,
    /// Ledger sequence at the time of emission.
    pub ledger_sequence: u32,
    /// Ledger timestamp at the time of emission.
    pub timestamp: u64,
}

impl DistributionLifecycleEvent {
    /// Construct a new lifecycle event payload from call-site data.
    pub fn new(
        env: &Env,
        amount: i128,
        mode: DistributionMode,
        batch_index: u32,
        batch_size: u32,
    ) -> Self {
        Self {
            version: DISTRIBUTION_EVENT_VERSION,
            amount,
            mode,
            batch_index,
            batch_size,
            ledger_sequence: env.ledger().sequence(),
            timestamp: env.ledger().timestamp(),
        }
    }
}

// ---------------------------------------------------------------------------
// Limits-surface event topics (existing — see also crate docs)
// ---------------------------------------------------------------------------

/// Returns the Symbol for the `"init"` event topic.
///
/// Emitted when the Callora yield-limits contract is first initialized with
/// the admin address.
pub fn event_init(env: &Env) -> Symbol {
    Symbol::new(env, "init")
}

/// Returns the Symbol for the `"admin_nominated"` event topic.
///
/// Emitted when the current admin nominates a new admin via
/// [`crate::CalloraYieldLimits::set_admin`]. The nominated admin must call
/// [`crate::CalloraYieldLimits::accept_admin`] to complete the transfer.
pub fn event_admin_nominated(env: &Env) -> Symbol {
    Symbol::new(env, "admin_nominated")
}

/// Returns the Symbol for the `"admin_accepted"` event topic.
///
/// Emitted when the pending admin accepts the role via
/// [`crate::CalloraYieldLimits::accept_admin`], completing the two-step
/// handover.
pub fn event_admin_accepted(env: &Env) -> Symbol {
    Symbol::new(env, "admin_accepted")
}

/// Returns the Symbol for the `"admin_cancelled"` event topic.
///
/// Emitted when the current admin cancels a pending admin transfer via
/// [`crate::CalloraYieldLimits::cancel_admin_transfer`].
pub fn event_admin_cancelled(env: &Env) -> Symbol {
    Symbol::new(env, "admin_cancelled")
}

/// Returns the Symbol for the `"default_limits_set"` event topic.
///
/// Emitted when the admin updates the global default caps via
/// [`crate::CalloraYieldLimits::set_default_limits`].
pub fn event_default_limits_set(env: &Env) -> Symbol {
    Symbol::new(env, "default_limits_set")
}

/// Returns the Symbol for the canonical event version marker used by Callora.
pub fn event_version_v1(env: &Env) -> Symbol {
    Symbol::new(env, "callora.v1")
}

/// Returns the Symbol for the `"account_limits_set"` event topic.
///
/// Emitted when the admin sets (or overwrites) per-account caps via
/// [`crate::CalloraYieldLimits::set_account_limits`].
pub fn event_account_limits_set(env: &Env) -> Symbol {
    Symbol::new(env, "account_limits_set")
}

/// Returns the Symbol for the `"account_limits_cleared"` event topic.
///
/// Emitted when the admin removes a per-account cap override via
/// [`crate::CalloraYieldLimits::clear_account_limits`].
pub fn event_account_limits_cleared(env: &Env) -> Symbol {
    Symbol::new(env, "account_limits_cleared")
}

/// Returns the Symbol for the `"bet_placed"` event topic.
///
/// Emitted when the caller successfully increments their open-bet counter
/// via [`crate::CalloraYieldLimits::place_bet`].
pub fn event_bet_placed(env: &Env) -> Symbol {
    Symbol::new(env, "bet_placed")
}

/// Returns the Symbol for the `"bet_cleared"` event topic.
///
/// Emitted when the caller successfully decrements their open-bet counter
/// via [`crate::CalloraYieldLimits::clear_bet`].
pub fn event_bet_cleared(env: &Env) -> Symbol {
    Symbol::new(env, "bet_cleared")
}

/// Returns the Symbol for the `"position_opened"` event topic.
///
/// Emitted when the caller successfully increments their active-position
/// counter via [`crate::CalloraYieldLimits::open_position`].
pub fn event_position_opened(env: &Env) -> Symbol {
    Symbol::new(env, "position_opened")
}

/// Returns the Symbol for the `"position_closed"` event topic.
///
/// Emitted when the caller successfully decrements their active-position
/// counter via [`crate::CalloraYieldLimits::close_position`].
pub fn event_position_closed(env: &Env) -> Symbol {
    Symbol::new(env, "position_closed")
}

/// Returns the Symbol for the `"subscription_added"` event topic.
///
/// Emitted when the caller successfully increments their active-subscription
/// counter via [`crate::CalloraYieldLimits::subscribe`].
pub fn event_subscription_added(env: &Env) -> Symbol {
    Symbol::new(env, "subscription_added")
}

/// Returns the Symbol for the `"subscription_removed"` event topic.
///
/// Emitted when the caller successfully decrements their active-subscription
/// counter via [`crate::CalloraYieldLimits::unsubscribe`].
pub fn event_subscription_removed(env: &Env) -> Symbol {
    Symbol::new(env, "subscription_removed")
}

/// Returns the Symbol for the `"upgraded"` event topic.
///
/// Emitted when the admin upgrades the contract to a new WASM hash via
/// [`crate::CalloraYieldLimits::upgrade`].
pub fn event_upgraded(env: &Env) -> Symbol {
    Symbol::new(env, "upgraded")
}

// ---------------------------------------------------------------------------
// Yield lifecycle event topics
// ---------------------------------------------------------------------------

/// Returns the Symbol for the `"yield_deposited"` event topic.
///
/// Emitted when protocol yield is deposited into the revenue pool. Off-chain
/// indexers can track cumulative yield inflows by listening for this topic.
pub fn event_yield_deposited(env: &Env) -> Symbol {
    Symbol::new(env, "yield_deposited")
}

/// Returns the Symbol for the `"distribute"` event topic.
///
/// Emitted when the admin distributes USDC to a single developer wallet.
pub fn event_distribute(env: &Env) -> Symbol {
    Symbol::new(env, "distribute")
}

/// Returns the Symbol for the `"batch_distribute"` event topic.
///
/// Emitted once per payment leg during a batch distribution.
pub fn event_batch_distribute(env: &Env) -> Symbol {
    Symbol::new(env, "batch_distribute")
}

/// Returns the Symbol for the `"distribute_started"` lifecycle event topic.
///
/// Emitted before a distribution transfer begins. Paired with
/// [`event_distribute_completed`] for lifecycle tracking.
pub fn event_distribute_started(env: &Env) -> Symbol {
    Symbol::new(env, "distribute_started")
}

/// Returns the Symbol for the `"distribute_completed"` lifecycle event topic.
///
/// Emitted after a distribution transfer succeeds. Paired with
/// [`event_distribute_started`] for lifecycle tracking.
pub fn event_distribute_completed(env: &Env) -> Symbol {
    Symbol::new(env, "distribute_completed")
}

/// Emit a structured distribution-started event with a versioned payload.
///
/// Off-chain indexers use the [`DistributionLifecycleEvent`] payload to
/// correlate start/completed pairs and detect stuck or reverted
/// distributions.
pub fn emit_distribute_started(
    env: &Env,
    caller: &Address,
    recipient: &Address,
    payload: &DistributionLifecycleEvent,
) {
    env.events().publish(
        (event_distribute_started(env), caller, recipient),
        payload.clone(),
    );
}

/// Emit a structured distribution-completed event with a versioned payload.
///
/// Indexers matching `distribute_started` ↔ `distribute_completed` by
/// `(ledger_sequence, amount, recipient)` can verify atomic completion.
pub fn emit_distribute_completed(
    env: &Env,
    caller: &Address,
    recipient: &Address,
    payload: &DistributionLifecycleEvent,
) {
    env.events().publish(
        (event_distribute_completed(env), caller, recipient),
        payload.clone(),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Limits-surface snapshot tests
    // -----------------------------------------------------------------------

    /// Snapshot: proves `event_init` still maps to exactly the bytes for `"init"`.
    #[test]
    fn test_event_init_bytes() {
        let env = Env::default();
        assert_eq!(event_init(&env), Symbol::new(&env, "init"));
    }

    /// Snapshot: proves `event_admin_nominated` still maps to exactly the bytes for `"admin_nominated"`.
    #[test]
    fn test_event_admin_nominated_bytes() {
        let env = Env::default();
        assert_eq!(
            event_admin_nominated(&env),
            Symbol::new(&env, "admin_nominated")
        );
    }

    /// Snapshot: proves `event_admin_accepted` still maps to exactly the bytes for `"admin_accepted"`.
    #[test]
    fn test_event_admin_accepted_bytes() {
        let env = Env::default();
        assert_eq!(
            event_admin_accepted(&env),
            Symbol::new(&env, "admin_accepted")
        );
    }

    /// Snapshot: proves `event_admin_cancelled` still maps to exactly the bytes for `"admin_cancelled"`.
    #[test]
    fn test_event_admin_cancelled_bytes() {
        let env = Env::default();
        assert_eq!(
            event_admin_cancelled(&env),
            Symbol::new(&env, "admin_cancelled")
        );
    }

    /// Snapshot: proves `event_default_limits_set` still maps to exactly the bytes for `"default_limits_set"`.
    #[test]
    fn test_event_default_limits_set_bytes() {
        let env = Env::default();
        assert_eq!(
            event_default_limits_set(&env),
            Symbol::new(&env, "default_limits_set")
        );
    }

    /// Snapshot: proves `event_account_limits_set` still maps to exactly the bytes for `"account_limits_set"`.
    #[test]
    fn test_event_account_limits_set_bytes() {
        let env = Env::default();
        assert_eq!(
            event_account_limits_set(&env),
            Symbol::new(&env, "account_limits_set")
        );
    }

    /// Snapshot: proves `event_account_limits_cleared` still maps to exactly the bytes for `"account_limits_cleared"`.
    #[test]
    fn test_event_account_limits_cleared_bytes() {
        let env = Env::default();
        assert_eq!(
            event_account_limits_cleared(&env),
            Symbol::new(&env, "account_limits_cleared")
        );
    }

    /// Snapshot: proves `event_bet_placed` still maps to exactly the bytes for `"bet_placed"`.
    #[test]
    fn test_event_bet_placed_bytes() {
        let env = Env::default();
        assert_eq!(event_bet_placed(&env), Symbol::new(&env, "bet_placed"));
    }

    /// Snapshot: proves `event_bet_cleared` still maps to exactly the bytes for `"bet_cleared"`.
    #[test]
    fn test_event_bet_cleared_bytes() {
        let env = Env::default();
        assert_eq!(event_bet_cleared(&env), Symbol::new(&env, "bet_cleared"));
    }

    /// Snapshot: proves `event_position_opened` still maps to exactly the bytes for `"position_opened"`.
    #[test]
    fn test_event_position_opened_bytes() {
        let env = Env::default();
        assert_eq!(
            event_position_opened(&env),
            Symbol::new(&env, "position_opened")
        );
    }

    /// Snapshot: proves `event_position_closed` still maps to exactly the bytes for `"position_closed"`.
    #[test]
    fn test_event_position_closed_bytes() {
        let env = Env::default();
        assert_eq!(
            event_position_closed(&env),
            Symbol::new(&env, "position_closed")
        );
    }

    /// Snapshot: proves `event_subscription_added` still maps to exactly the bytes for `"subscription_added"`.
    #[test]
    fn test_event_subscription_added_bytes() {
        let env = Env::default();
        assert_eq!(
            event_subscription_added(&env),
            Symbol::new(&env, "subscription_added")
        );
    }

    /// Snapshot: proves `event_subscription_removed` still maps to exactly the bytes for `"subscription_removed"`.
    #[test]
    fn test_event_subscription_removed_bytes() {
        let env = Env::default();
        assert_eq!(
            event_subscription_removed(&env),
            Symbol::new(&env, "subscription_removed")
        );
    }

    /// Snapshot: proves `event_upgraded` still maps to exactly the bytes for `"upgraded"`.
    #[test]
    fn test_event_upgraded_bytes() {
        let env = Env::default();
        assert_eq!(event_upgraded(&env), Symbol::new(&env, "upgraded"));
    }

    // -----------------------------------------------------------------------
    // Lifecycle event snapshot tests
    // -----------------------------------------------------------------------

    /// Snapshot: proves `event_yield_deposited` still maps to exactly the bytes for `"yield_deposited"`.
    #[test]
    fn test_event_yield_deposited_bytes() {
        let env = Env::default();
        assert_eq!(
            event_yield_deposited(&env),
            Symbol::new(&env, "yield_deposited")
        );
    }

    /// Snapshot: proves `event_distribute` still maps to exactly the bytes for `"distribute"`.
    #[test]
    fn test_event_distribute_bytes() {
        let env = Env::default();
        assert_eq!(event_distribute(&env), Symbol::new(&env, "distribute"));
    }

    /// Snapshot: proves `event_batch_distribute` still maps to exactly the bytes for `"batch_distribute"`.
    #[test]
    fn test_event_batch_distribute_bytes() {
        let env = Env::default();
        assert_eq!(
            event_batch_distribute(&env),
            Symbol::new(&env, "batch_distribute")
        );
    }

    /// Snapshot: proves lifecycle event topic strings.
    #[test]
    fn test_distribution_lifecycle_event_topics() {
        let env = Env::default();
        assert_eq!(
            event_distribute_started(&env),
            Symbol::new(&env, "distribute_started")
        );
        assert_eq!(
            event_distribute_completed(&env),
            Symbol::new(&env, "distribute_completed")
        );
    }

    /// Proves the lifecycle payload is versioned and carries call-site fields.
    #[test]
    fn test_distribution_lifecycle_payload_is_versioned() {
        let env = Env::default();
        let payload = DistributionLifecycleEvent::new(&env, 42, DistributionMode::Batch, 1, 3);

        assert_eq!(payload.version, DISTRIBUTION_EVENT_VERSION);
        assert_eq!(payload.amount, 42);
        assert_eq!(payload.mode, DistributionMode::Batch);
        assert_eq!(payload.batch_index, 1);
        assert_eq!(payload.batch_size, 3);
        assert_eq!(payload.ledger_sequence, env.ledger().sequence());
        assert_eq!(payload.timestamp, env.ledger().timestamp());
    }
}
