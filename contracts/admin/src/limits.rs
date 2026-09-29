//! Per-account limits enforcement for bets, positions, and subscriptions.
//!
//! # Status and consumers (issue #1248)
//!
//! **Experimental / unwired.** No contract in this workspace depends on
//! `callora-admin`, and this module provides free functions only — it exposes
//! no `#[contract]` entrypoints — so nothing can reach it on-chain yet. The
//! *bet* / *position* / *subscription* terms are placeholders pending a
//! concrete Callora product definition; they do not currently map to any
//! shipped Callora flow.
//!
//! The same per-account cap concept is already implemented, tested, and wired
//! up as a standalone contract in `contracts/yield` (`CalloraYieldLimits`; see
//! `contracts/yield/YIELD_LIMITS.md` and `contracts/yield/src/limits.rs`).
//! Key differences: `yield` stores per-account cap overrides in **instance**
//! storage and reports `YieldLimitError`, whereas this module stores both caps
//! and counters in **persistent** storage and reports
//! [`crate::errors::AdminLimitError`]. Consolidate or delete one of the two
//! before either gains a consumer.
//!
//! # Problem
//!
//! Without per-account caps, a single account can open an unbounded number
//! of bets, positions, and subscriptions, bloating persistent storage and
//! degrading performance for every other participant. This module enforces
//! per-account state caps to prevent abuse.
//!
//! # Architecture
//!
//! Two [`contracttype`] structs drive the limits surface:
//!
//! | Type              | Purpose                                                     |
//! |-------------------|-------------------------------------------------------------|
//! | [`AccountLimits`] | The configured `(max_bets, max_positions, max_subscriptions)` caps for an account. |
//! | [`AccountUsage`]  | The current `(bets, positions, subscriptions)` counters for an account.           |
//!
//! # Storage Layout
//!
//! - [`AccountLimits`] are stored in **persistent** storage under
//!   [`StorageKey::Limits`]`(Address)`. They are sparse overrides; an
//!   account with no explicit override falls back to [`DEFAULT_LIMITS`].
//!
//! - [`AccountUsage`] counters are stored in **persistent** storage under
//!   [`StorageKey::Usage`]`(Address)`.
//!
//! # Auth Model
//!
//! | Function                                                        | Authorized by                     |
//! |-----------------------------------------------------------------|-----------------------------------|
//! | `set_default_limits`, `set_account_limits`, `clear_account_limits` | admin (`caller == admin`)       |
//! | `consume_bet`, `release_bet`, `consume_position`,               | account (their own counter)       |
//! | `release_position`, `consume_subscription`, `release_subscription` | account (their own counter)     |
//!
//! Read-only views (`get_account_limits`, `get_account_usage`,
//! `can_place_bet`, `can_open_position`, `can_subscribe`,
//! `get_default_limits`) do **not** call `require_auth`.
//!
//! # Overflow Safety
//!
//! Every count mutation uses `u32::checked_add` / `u32::checked_sub`. The
//! outcomes are folded into typed [`AdminLimitError`] variants so no
//! production code path invokes `unwrap()` or `panic!()`.

use soroban_sdk::{contracttype, Address, Env};

use crate::errors::AdminLimitError;
use crate::events;

// ---------------------------------------------------------------------------
// Constants — TTL and defaults
// ---------------------------------------------------------------------------

/// Per-day ledger count at a 5-second close cadence (matches vault).
pub const LEDGERS_PER_DAY: u32 = 17_280;

/// TTL bump threshold for persistent storage keys (`Usage`).
///
/// When the remaining TTL of the key falls below this value the contract
/// re-extends the TTL on every increment / decrement so account counters do
/// not silently archive.
pub const STATE_BUMP_THRESHOLD: u32 = LEDGERS_PER_DAY * 7;

/// TTL bump amount for persistent storage keys (`Usage`).
pub const STATE_BUMP_AMOUNT: u32 = LEDGERS_PER_DAY * 30;

/// TTL bump threshold for persistent storage keys (`Limits`).
pub const LIMITS_BUMP_THRESHOLD: u32 = LEDGERS_PER_DAY * 7;

/// TTL bump amount for persistent storage keys (`Limits`).
pub const LIMITS_BUMP_AMOUNT: u32 = LEDGERS_PER_DAY * 30;

/// Maximum allowable value for any single cap dimension.
///
/// Counts are stored as `u32`, so this matches the practical ceiling. The
/// cap itself is never compared against `u32::MAX` so this is a conservative
/// sanity ceiling rather than the absolute type ceiling.
pub const MAX_CAP: u32 = 1_000_000;

/// Global default per-account limits applied when no explicit override exists
/// for an account.
///
/// # Default rationale
///
/// - `100` open bets caps the worst-case bet-farming at 100× per account.
/// - `50` open positions caps positions similarly while leaving headroom for
///   legitimate power-users.
/// - `20` active subscriptions caps subscription-farming while still
///   allowing routine usage.
///
/// These values are intentionally conservative so the contract is safe-by-default
/// even before the admin sets explicit caps via
/// [`set_account_limits`] or [`set_default_limits`].
pub const DEFAULT_LIMITS: AccountLimits = AccountLimits {
    max_bets: 100,
    max_positions: 50,
    max_subscriptions: 20,
};

// ---------------------------------------------------------------------------
// Data structures
// ---------------------------------------------------------------------------

/// Per-account state caps configured by the admin.
///
/// Each field sets the maximum allowed concurrent state of that kind for a
/// single account. A value of `0` disables that kind entirely (no new bets,
/// positions, or subscriptions can be opened while the cap is `0`).
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccountLimits {
    /// Maximum concurrent open bets.
    pub max_bets: u32,
    /// Maximum concurrent open positions.
    pub max_positions: u32,
    /// Maximum concurrent active subscriptions.
    pub max_subscriptions: u32,
}

impl AccountLimits {
    /// Construct a new [`AccountLimits`] with all three caps set to `count`.
    pub const fn uniform(count: u32) -> Self {
        Self {
            max_bets: count,
            max_positions: count,
            max_subscriptions: count,
        }
    }

    /// Return `true` if every individual cap is within the valid range
    /// (`<= MAX_CAP`).
    pub fn is_valid(&self) -> bool {
        self.max_bets <= MAX_CAP
            && self.max_positions <= MAX_CAP
            && self.max_subscriptions <= MAX_CAP
    }

    /// Return `true` if all caps are `0` (fully disabled account).
    pub fn is_fully_disabled(&self) -> bool {
        self.max_bets == 0 && self.max_positions == 0 && self.max_subscriptions == 0
    }
}

/// Per-account live usage counters.
///
/// Only the contract mutates fields on this struct. Counter arithmetic is
/// performed via `checked_add` / `checked_sub` so a buggy caller can never
/// drive any field into `u32::MAX + 1`.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccountUsage {
    /// Current number of open bets.
    pub bets: u32,
    /// Current number of open positions.
    pub positions: u32,
    /// Current number of active subscriptions.
    pub subscriptions: u32,
}

impl AccountUsage {
    /// Return a zeroed [`AccountUsage`].
    pub const fn zero() -> Self {
        Self {
            bets: 0,
            positions: 0,
            subscriptions: 0,
        }
    }

    /// Increment the bet counter using `checked_add`.
    ///
    /// # Errors
    /// - [`AdminLimitError::Overflow`] — counter would saturate `u32::MAX`.
    pub fn add_bet(&mut self) -> Result<(), AdminLimitError> {
        self.bets = self.bets.checked_add(1).ok_or(AdminLimitError::Overflow)?;
        Ok(())
    }

    /// Decrement the bet counter using `checked_sub`.
    ///
    /// # Errors
    /// - [`AdminLimitError::CounterUnderflow`] — counter is already 0.
    pub fn sub_bet(&mut self) -> Result<(), AdminLimitError> {
        self.bets = self
            .bets
            .checked_sub(1)
            .ok_or(AdminLimitError::CounterUnderflow)?;
        Ok(())
    }

    /// Increment the position counter using `checked_add`.
    pub fn add_position(&mut self) -> Result<(), AdminLimitError> {
        self.positions = self
            .positions
            .checked_add(1)
            .ok_or(AdminLimitError::Overflow)?;
        Ok(())
    }

    /// Decrement the position counter using `checked_sub`.
    pub fn sub_position(&mut self) -> Result<(), AdminLimitError> {
        self.positions = self
            .positions
            .checked_sub(1)
            .ok_or(AdminLimitError::CounterUnderflow)?;
        Ok(())
    }

    /// Increment the subscription counter using `checked_add`.
    pub fn add_subscription(&mut self) -> Result<(), AdminLimitError> {
        self.subscriptions = self
            .subscriptions
            .checked_add(1)
            .ok_or(AdminLimitError::Overflow)?;
        Ok(())
    }

    /// Decrement the subscription counter using `checked_sub`.
    pub fn sub_subscription(&mut self) -> Result<(), AdminLimitError> {
        self.subscriptions = self
            .subscriptions
            .checked_sub(1)
            .ok_or(AdminLimitError::CounterUnderflow)?;
        Ok(())
    }
}

/// Canonical storage keys for the admin limits module.
#[contracttype]
#[derive(Clone)]
enum StorageKey {
    /// Per-account caps override (persistent storage; sparse).
    Limits(Address),
    /// Per-account live usage counters (persistent storage).
    Usage(Address),
    /// Global default caps applied when an account has no explicit override.
    DefaultLimits,
}

// ---------------------------------------------------------------------------
// Internal helpers — auth
// ---------------------------------------------------------------------------

/// Assert `caller` equals the stored admin.
///
/// Runs `caller.require_auth()` first so misconfigured callers are rejected
/// deterministically without consuming the underlying signature.
///
/// # Errors
/// - [`AdminLimitError::Unauthorized`] — caller is not the stored admin.
/// - [`AdminLimitError::NotInitialized`] — admin has never been set.
fn require_admin(env: &Env, caller: &Address) -> Result<(), AdminLimitError> {
    caller.require_auth();
    let admin = crate::admin::get_admin(env).ok_or(AdminLimitError::NotInitialized)?;
    if *caller != admin {
        return Err(AdminLimitError::Unauthorized);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Internal helpers — storage reads/writes
// ---------------------------------------------------------------------------

/// Read the per-account limits caps or fall back to [`get_default_limits`].
fn limits_for(env: &Env, account: &Address) -> AccountLimits {
    let key = StorageKey::Limits(account.clone());
    let limits: Option<AccountLimits> = env.storage().persistent().get(&key);
    if let Some(caps) = limits {
        env.storage()
            .persistent()
            .extend_ttl(&key, LIMITS_BUMP_THRESHOLD, LIMITS_BUMP_AMOUNT);
        return caps;
    }
    get_default_limits(env)
}

/// Read the live per-account usage counters, returning a zero struct if the
/// account has no recorded state yet. Bumps persistent TTL on read.
fn usage_for(env: &Env, account: &Address) -> AccountUsage {
    let key = StorageKey::Usage(account.clone());
    let usage: Option<AccountUsage> = env.storage().persistent().get(&key);
    if let Some(u) = usage {
        env.storage()
            .persistent()
            .extend_ttl(&key, STATE_BUMP_THRESHOLD, STATE_BUMP_AMOUNT);
        return u;
    }
    AccountUsage::zero()
}

/// Persist the live per-account usage counters and bump persistent TTL.
fn save_usage(env: &Env, account: &Address, usage: &AccountUsage) {
    let key = StorageKey::Usage(account.clone());
    env.storage().persistent().set(&key, usage);
    env.storage()
        .persistent()
        .extend_ttl(&key, STATE_BUMP_THRESHOLD, STATE_BUMP_AMOUNT);
}

// ---------------------------------------------------------------------------
// Public API — limits configuration
// ---------------------------------------------------------------------------

/// Read the global default caps or fall back to [`DEFAULT_LIMITS`].
pub fn get_default_limits(env: &Env) -> AccountLimits {
    env.storage()
        .persistent()
        .get(&StorageKey::DefaultLimits)
        .unwrap_or(DEFAULT_LIMITS)
}

/// Set the global default caps (admin only).
///
/// # Arguments
/// * `env` — Soroban environment.
/// * `caller` — Must be the current admin; must authorize.
/// * `max_bets` — Default maximum concurrent open bets per account.
/// * `max_positions` — Default maximum concurrent open positions per account.
/// * `max_subscriptions` — Default maximum concurrent subscriptions per account.
///
/// # Errors
/// - [`AdminLimitError::NotInitialized`] — admin contract not initialized.
/// - [`AdminLimitError::Unauthorized`] — `caller` is not the current admin.
/// - [`AdminLimitError::InvalidLimit`] — any cap exceeds [`MAX_CAP`].
///
/// # Events
/// Emits `default_limits_set` with `(default_limits_set, caller)` topics and
/// the new `AccountLimits` as data.
pub fn set_default_limits(
    env: &Env,
    caller: &Address,
    max_bets: u32,
    max_positions: u32,
    max_subscriptions: u32,
) -> Result<(), AdminLimitError> {
    require_admin(env, caller)?;
    let caps = AccountLimits {
        max_bets,
        max_positions,
        max_subscriptions,
    };
    if !caps.is_valid() {
        return Err(AdminLimitError::InvalidLimit);
    }
    env.storage()
        .persistent()
        .set(&StorageKey::DefaultLimits, &caps);
    env.events().publish(
        (
            events::event_default_limits_set(env),
            events::event_version_v1(env),
            caller.clone(),
            caps.clone(),
        ),
        caps,
    );
    Ok(())
}

/// Set all per-account state caps (admin only).
///
/// Only the current admin may update limits. A zero value disables the
/// corresponding category for the account; the default for an account with no
/// configured limits falls back to [`get_default_limits`].
///
/// # Arguments
/// * `env` — Soroban environment.
/// * `caller` — Must be the current admin; must authorize.
/// * `account` — Target account address.
/// * `max_bets` — Maximum concurrent open bets.
/// * `max_positions` — Maximum concurrent open positions.
/// * `max_subscriptions` — Maximum concurrent subscriptions.
///
/// # Errors
/// - [`AdminLimitError::NotInitialized`] — admin contract not initialized.
/// - [`AdminLimitError::Unauthorized`] — `caller` is not the current admin.
/// - [`AdminLimitError::InvalidLimit`] — any cap exceeds [`MAX_CAP`].
///
/// # Events
/// Emits `account_limits_set` with `(account_limits_set, caller)` topics and
/// `(account, AccountLimits)` as data.
pub fn set_account_limits(
    env: &Env,
    caller: &Address,
    account: &Address,
    max_bets: u32,
    max_positions: u32,
    max_subscriptions: u32,
) -> Result<(), AdminLimitError> {
    require_admin(env, caller)?;
    let caps = AccountLimits {
        max_bets,
        max_positions,
        max_subscriptions,
    };
    if !caps.is_valid() {
        return Err(AdminLimitError::InvalidLimit);
    }
    let key = StorageKey::Limits(account.clone());
    env.storage().persistent().set(&key, &caps);
    env.storage()
        .persistent()
        .extend_ttl(&key, LIMITS_BUMP_THRESHOLD, LIMITS_BUMP_AMOUNT);
    env.events().publish(
        (
            events::event_account_limits_set(env),
            events::event_version_v1(env),
            caller.clone(),
            account.clone(),
        ),
        (account.clone(), caps),
    );
    Ok(())
}

/// Remove any per-account caps override so the account falls back to the
/// global default (admin only).
///
/// # Arguments
/// * `env` — Soroban environment.
/// * `caller` — Must be the current admin; must authorize.
/// * `account` — Target account address.
///
/// # Errors
/// - [`AdminLimitError::NotInitialized`] — admin contract not initialized.
/// - [`AdminLimitError::Unauthorized`] — `caller` is not the current admin.
///
/// # Events
/// Emits `account_limits_cleared` with `(account_limits_cleared, caller)`
/// topics and the `account` address as data.
pub fn clear_account_limits(
    env: &Env,
    caller: &Address,
    account: &Address,
) -> Result<(), AdminLimitError> {
    require_admin(env, caller)?;
    env.storage()
        .persistent()
        .remove(&StorageKey::Limits(account.clone()));
    env.events().publish(
        (
            events::event_account_limits_cleared(env),
            events::event_version_v1(env),
            caller.clone(),
            account.clone(),
        ),
        account.clone(),
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Public API — read-only views
// ---------------------------------------------------------------------------

/// Return the configured caps for an account.
///
/// Falls back to [`get_default_limits`] if no per-account override exists.
pub fn get_account_limits(env: &Env, account: &Address) -> AccountLimits {
    limits_for(env, account)
}

/// Return the current tracked usage for an account.
///
/// Returns a zeroed [`AccountUsage`] if the account has no recorded usage.
pub fn get_account_usage(env: &Env, account: &Address) -> AccountUsage {
    usage_for(env, account)
}

/// Return `true` if `account` can place another bet without exceeding caps.
pub fn can_place_bet(env: &Env, account: &Address) -> bool {
    let caps = limits_for(env, account);
    let usage = usage_for(env, account);
    usage.bets < caps.max_bets
}

/// Return `true` if `account` can open another position without exceeding caps.
pub fn can_open_position(env: &Env, account: &Address) -> bool {
    let caps = limits_for(env, account);
    let usage = usage_for(env, account);
    usage.positions < caps.max_positions
}

/// Return `true` if `account` can subscribe again without exceeding caps.
pub fn can_subscribe(env: &Env, account: &Address) -> bool {
    let caps = limits_for(env, account);
    let usage = usage_for(env, account);
    usage.subscriptions < caps.max_subscriptions
}

// ---------------------------------------------------------------------------
// Public API — increment (consume) operations
// ---------------------------------------------------------------------------

/// Consume one bet slot for an account.
///
/// The account must authorize the state change. The usage counter is updated
/// only when the configured cap has not been reached.
///
/// # Arguments
/// * `env` — Soroban environment.
/// * `account` — Account whose bet counter will be incremented; must authorize.
///
/// # Errors
/// - [`AdminLimitError::BetsAtCap`] — account's open bet count is already at
///   the configured cap.
/// - [`AdminLimitError::Overflow`] — counter would saturate `u32::MAX`.
///
/// # Events
/// Emits `bet_consumed` with `(bet_consumed, account)` topics and
/// `(new_count, cap)` as data.
pub fn consume_bet(env: &Env, account: &Address) -> Result<(), AdminLimitError> {
    account.require_auth();
    let caps = limits_for(env, account);
    let mut usage = usage_for(env, account);
    if usage.bets >= caps.max_bets {
        return Err(AdminLimitError::BetsAtCap);
    }
    usage.add_bet()?;
    save_usage(env, account, &usage);
    env.events().publish(
        (
            events::event_bet_consumed(env),
            events::event_version_v1(env),
            account.clone(),
        ),
        (usage.bets, caps.max_bets),
    );
    Ok(())
}

/// Consume one position slot for an account.
///
/// The account must authorize the state change.
///
/// # Errors
/// - [`AdminLimitError::PositionsAtCap`] — account's open position count is
///   already at the configured cap.
/// - [`AdminLimitError::Overflow`] — counter would saturate `u32::MAX`.
///
/// # Events
/// Emits `position_consumed` with `(position_consumed, account)` topics and
/// `(new_count, cap)` as data.
pub fn consume_position(env: &Env, account: &Address) -> Result<(), AdminLimitError> {
    account.require_auth();
    let caps = limits_for(env, account);
    let mut usage = usage_for(env, account);
    if usage.positions >= caps.max_positions {
        return Err(AdminLimitError::PositionsAtCap);
    }
    usage.add_position()?;
    save_usage(env, account, &usage);
    env.events().publish(
        (
            events::event_position_consumed(env),
            events::event_version_v1(env),
            account.clone(),
        ),
        (usage.positions, caps.max_positions),
    );
    Ok(())
}

/// Consume one subscription slot for an account.
///
/// The account must authorize the state change.
///
/// # Errors
/// - [`AdminLimitError::SubscriptionsAtCap`] — account's subscription count
///   is already at the configured cap.
/// - [`AdminLimitError::Overflow`] — counter would saturate `u32::MAX`.
///
/// # Events
/// Emits `subscription_consumed` with `(subscription_consumed, account)`
/// topics and `(new_count, cap)` as data.
pub fn consume_subscription(env: &Env, account: &Address) -> Result<(), AdminLimitError> {
    account.require_auth();
    let caps = limits_for(env, account);
    let mut usage = usage_for(env, account);
    if usage.subscriptions >= caps.max_subscriptions {
        return Err(AdminLimitError::SubscriptionsAtCap);
    }
    usage.add_subscription()?;
    save_usage(env, account, &usage);
    env.events().publish(
        (
            events::event_subscription_consumed(env),
            events::event_version_v1(env),
            account.clone(),
        ),
        (usage.subscriptions, caps.max_subscriptions),
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Public API — decrement (release) operations
// ---------------------------------------------------------------------------

/// Release one bet slot for an account (decrement counter).
///
/// The account must authorize the state change. Rejects if the counter is
/// already zero.
///
/// # Arguments
/// * `env` — Soroban environment.
/// * `account` — Account whose bet counter will be decremented; must authorize.
///
/// # Errors
/// - [`AdminLimitError::CounterUnderflow`] — bet counter is already 0.
///
/// # Events
/// Emits `bet_released` with `(bet_released, account)` topics and
/// the new count as data.
pub fn release_bet(env: &Env, account: &Address) -> Result<(), AdminLimitError> {
    account.require_auth();
    let mut usage = usage_for(env, account);
    usage.sub_bet()?;
    save_usage(env, account, &usage);
    env.events().publish(
        (
            events::event_bet_released(env),
            events::event_version_v1(env),
            account.clone(),
        ),
        usage.bets,
    );
    Ok(())
}

/// Release one position slot for an account (decrement counter).
///
/// The account must authorize the state change. Rejects if the counter is
/// already zero.
///
/// # Errors
/// - [`AdminLimitError::CounterUnderflow`] — position counter is already 0.
///
/// # Events
/// Emits `position_released` with `(position_released, account)` topics and
/// the new count as data.
pub fn release_position(env: &Env, account: &Address) -> Result<(), AdminLimitError> {
    account.require_auth();
    let mut usage = usage_for(env, account);
    usage.sub_position()?;
    save_usage(env, account, &usage);
    env.events().publish(
        (
            events::event_position_released(env),
            events::event_version_v1(env),
            account.clone(),
        ),
        usage.positions,
    );
    Ok(())
}

/// Release one subscription slot for an account (decrement counter).
///
/// The account must authorize the state change. Rejects if the counter is
/// already zero.
///
/// # Errors
/// - [`AdminLimitError::CounterUnderflow`] — subscription counter is already 0.
///
/// # Events
/// Emits `subscription_released` with `(subscription_released, account)`
/// topics and the new count as data.
pub fn release_subscription(env: &Env, account: &Address) -> Result<(), AdminLimitError> {
    account.require_auth();
    let mut usage = usage_for(env, account);
    usage.sub_subscription()?;
    save_usage(env, account, &usage);
    env.events().publish(
        (
            events::event_subscription_released(env),
            events::event_version_v1(env),
            account.clone(),
        ),
        usage.subscriptions,
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::testutils::Address as _;

    /// `AccountLimits::uniform` sets all three caps to the same value.
    #[test]
    fn account_limits_uniform_constructor() {
        let caps = AccountLimits::uniform(7);
        assert_eq!(caps.max_bets, 7);
        assert_eq!(caps.max_positions, 7);
        assert_eq!(caps.max_subscriptions, 7);
    }

    /// `AccountLimits::is_valid` rejects caps exceeding `MAX_CAP`.
    #[test]
    fn account_limits_is_valid_rejects_oversized_caps() {
        let bad = AccountLimits {
            max_bets: MAX_CAP + 1,
            max_positions: 0,
            max_subscriptions: 0,
        };
        assert!(!bad.is_valid());
    }

    /// `AccountLimits::is_valid` accepts caps at and below `MAX_CAP`.
    #[test]
    fn account_limits_is_valid_accepts_max_cap() {
        let ok = AccountLimits {
            max_bets: MAX_CAP,
            max_positions: MAX_CAP,
            max_subscriptions: MAX_CAP,
        };
        assert!(ok.is_valid());
    }

    /// `AccountLimits::is_fully_disabled` returns `true` only when all caps
    /// are 0.
    #[test]
    fn account_limits_is_fully_disabled() {
        assert!(AccountLimits::uniform(0).is_fully_disabled());
        assert!(!AccountLimits::uniform(1).is_fully_disabled());
        assert!(!AccountLimits {
            max_bets: 1,
            max_positions: 0,
            max_subscriptions: 0,
        }
        .is_fully_disabled());
    }

    /// `AccountUsage::zero` produces a struct with all counters at 0.
    #[test]
    fn account_usage_zero_has_all_counts_at_zero() {
        let z = AccountUsage::zero();
        assert_eq!(z.bets, 0);
        assert_eq!(z.positions, 0);
        assert_eq!(z.subscriptions, 0);
    }

    /// `DEFAULT_LIMITS` are conservative and non-zero.
    #[test]
    fn default_limits_are_conservative() {
        assert!(DEFAULT_LIMITS.max_bets > 0);
        assert!(DEFAULT_LIMITS.max_positions > 0);
        assert!(DEFAULT_LIMITS.max_subscriptions > 0);
        assert!(DEFAULT_LIMITS.max_bets <= 1000);
        assert!(DEFAULT_LIMITS.max_positions <= 1000);
        assert!(DEFAULT_LIMITS.max_subscriptions <= 1000);
        assert!(DEFAULT_LIMITS.is_valid());
    }

    /// `MAX_CAP` is non-trivial.
    #[test]
    fn max_cap_is_reasonable() {
        assert!(MAX_CAP > 0);
        assert!(MAX_CAP <= 10_000_000);
    }

    /// TTL bump constants are internally consistent.
    #[test]
    fn bump_constants_are_sensible() {
        const {
            assert!(STATE_BUMP_AMOUNT > STATE_BUMP_THRESHOLD);
            assert!(LIMITS_BUMP_AMOUNT > LIMITS_BUMP_THRESHOLD);
        }
    }
}
