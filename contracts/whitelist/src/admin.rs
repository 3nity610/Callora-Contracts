//! Global cool-off guard for critical whitelist admin actions.
//!
//! Whitelist state-changing actions — `add_address`, `remove_address`, `clear_all` —
//! are gated by a configurable cool-off window that prevents rapid successive
//! admin operations. Once an action succeeds, no other critical admin action may
//! execute until the window elapses.
//!
//! The window defaults to **one hour** and is configurable between **1 second**
//! and **30 days** by the contract admin.
//!
//! ## Storage Layout
//! - `WhitelistAdminCooldown` — instance storage, `u64` seconds.
//! - `WhitelistLastCriticalAction` — instance storage, [`CriticalAdminAction`].

use crate::{StorageKey, WhitelistError};
use soroban_sdk::{contracttype, Env, Symbol};

/// Minimum configurable cool-off window: one second.
pub const MIN_COOLDOWN_SECONDS: u64 = 1;

/// Maximum configurable cool-off window: thirty days.
pub const MAX_COOLDOWN_SECONDS: u64 = 30 * 24 * 60 * 60;

/// Default cool-off window: one hour.
pub const DEFAULT_COOLDOWN_SECONDS: u64 = 60 * 60;

/// Audit record for the most recently executed critical whitelist admin action.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CriticalAdminAction {
    /// Stable action tag such as `"add_address"`, `"remove_address"`, or `"clear_all"`.
    pub action: Symbol,
    /// Ledger timestamp at which the action was executed.
    pub executed_at: u64,
}

/// Return the configured cool-off window, falling back to the secure default.
///
/// Reads the `WhitelistAdminCooldown` storage key. Returns
/// [`DEFAULT_COOLDOWN_SECONDS`] (1 hour) when no window has been explicitly set.
pub fn get_cooldown(env: &Env) -> u64 {
    match env
        .storage()
        .instance()
        .get(&StorageKey::WhitelistAdminCooldown)
    {
        Some(seconds) => seconds,
        None => DEFAULT_COOLDOWN_SECONDS,
    }
}

/// Validate and persist a new cool-off window.
///
/// # Errors
/// Returns [`WhitelistError::InvalidAdminCooldown`] when `seconds` is outside
/// [`MIN_COOLDOWN_SECONDS`]..=[`MAX_COOLDOWN_SECONDS`].
pub fn set_cooldown(env: &Env, seconds: u64) -> Result<(), WhitelistError> {
    if !(MIN_COOLDOWN_SECONDS..=MAX_COOLDOWN_SECONDS).contains(&seconds) {
        return Err(WhitelistError::InvalidAdminCooldown);
    }

    env.storage()
        .instance()
        .set(&StorageKey::WhitelistAdminCooldown, &seconds);
    Ok(())
}

/// Return the last successfully executed critical admin action, if any.
///
/// Returns `None` when no critical action has been recorded yet.
pub fn last_action(env: &Env) -> Option<CriticalAdminAction> {
    env.storage()
        .instance()
        .get(&StorageKey::WhitelistLastCriticalAction)
}

/// Return the timestamp at which the next critical action becomes available.
///
/// Saturating arithmetic prevents timestamp wraparound. A contract with no prior
/// critical action returns `0`, meaning an action may execute immediately.
pub fn ready_at(env: &Env) -> u64 {
    match last_action(env) {
        Some(record) => record.executed_at.saturating_add(get_cooldown(env)),
        None => 0,
    }
}

/// Return the seconds remaining in the admin cool-off window.
///
/// Returns `0` when the window has elapsed or no action has been recorded.
pub fn remaining(env: &Env) -> u64 {
    ready_at(env).saturating_sub(env.ledger().timestamp())
}

/// Return whether a critical admin action may execute at the current timestamp.
pub fn is_ready(env: &Env) -> bool {
    remaining(env) == 0
}

/// Enforce and arm the admin cool-off window for `action`.
///
/// Call this only after authorization and action-specific validations have
/// succeeded. Soroban transaction rollback ensures the record is not retained
/// if the subsequent critical operation fails.
///
/// # Errors
/// Returns [`WhitelistError::AdminCooldownActive`] while another critical action's
/// cool-off window is still active.
pub fn guard(env: &Env, action: Symbol) -> Result<(), WhitelistError> {
    if !is_ready(env) {
        return Err(WhitelistError::AdminCooldownActive);
    }

    let record = CriticalAdminAction {
        action,
        executed_at: env.ledger().timestamp(),
    };
    env.storage()
        .instance()
        .set(&StorageKey::WhitelistLastCriticalAction, &record);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::testutils::Ledger as _;
    use soroban_sdk::{contract, Env};

    #[contract]
    struct CooldownHarness;

    fn in_contract<T>(env: &Env, f: impl FnOnce() -> T) -> T {
        let contract_id = env.register(CooldownHarness, ());
        env.as_contract(&contract_id, f)
    }

    #[test]
    fn defaults_and_configuration_bounds_are_stable() {
        let env = Env::default();
        in_contract(&env, || {
            assert_eq!(WhitelistError::AdminCooldownActive as u32, 49);
            assert_eq!(WhitelistError::InvalidAdminCooldown as u32, 50);
            assert_eq!(get_cooldown(&env), DEFAULT_COOLDOWN_SECONDS);
            assert_eq!(
                set_cooldown(&env, 0),
                Err(WhitelistError::InvalidAdminCooldown)
            );
            assert_eq!(
                set_cooldown(&env, MAX_COOLDOWN_SECONDS.saturating_add(1)),
                Err(WhitelistError::InvalidAdminCooldown)
            );
            assert_eq!(set_cooldown(&env, MIN_COOLDOWN_SECONDS), Ok(()));
            assert_eq!(get_cooldown(&env), MIN_COOLDOWN_SECONDS);
            assert_eq!(set_cooldown(&env, MAX_COOLDOWN_SECONDS), Ok(()));
            assert_eq!(get_cooldown(&env), MAX_COOLDOWN_SECONDS);
        });
    }

    #[test]
    fn one_action_blocks_a_different_action_until_boundary() {
        let env = Env::default();
        env.ledger().set_timestamp(1_000);
        in_contract(&env, || {
            set_cooldown(&env, 300).unwrap();
            assert_eq!(guard(&env, Symbol::new(&env, "add_address")), Ok(()));
            assert_eq!(remaining(&env), 300);
            assert_eq!(
                guard(&env, Symbol::new(&env, "remove_address")),
                Err(WhitelistError::AdminCooldownActive)
            );

            env.ledger().set_timestamp(1_299);
            assert_eq!(remaining(&env), 1);
            assert!(!is_ready(&env));

            env.ledger().set_timestamp(1_300);
            assert!(is_ready(&env));
            assert_eq!(guard(&env, Symbol::new(&env, "clear_all")), Ok(()));

            let record = last_action(&env).expect("critical action record");
            assert_eq!(record.action, Symbol::new(&env, "clear_all"));
            assert_eq!(record.executed_at, 1_300);
        });
    }

    #[test]
    fn readiness_math_saturates_at_timestamp_limit() {
        let env = Env::default();
        env.ledger().set_timestamp(u64::MAX - 10);
        in_contract(&env, || {
            set_cooldown(&env, 60).unwrap();
            guard(&env, Symbol::new(&env, "add_address")).unwrap();
            assert_eq!(ready_at(&env), u64::MAX);
            assert_eq!(remaining(&env), 10);

            env.ledger().set_timestamp(u64::MAX);
            assert!(is_ready(&env));
        });
    }
}
