//! Admin module with a cool-off window between upgrade actions.
//!
//! Enforces a cooldown period between critical upgrade actions to prevent
//! rapid abuse. This implements the requirements for the GrantFox FWC26 campaign.
//!
//! ## Events
//!
//! | Function                  | Topic              | Topics                          | Data                              |
//! |---------------------------|--------------------|---------------------------------|-----------------------------------|
//! | `check_and_record_upgrade`| `upgrade_started`  | `(topic, caller)`               | `(current_timestamp, cooldown)`   |
//! | `check_and_record_upgrade`| `upgrade_recorded` | `(topic, caller)`               | `recorded_timestamp`              |
//! | `set_cooldown`            | `cooldown_set`     | `(topic, caller)`               | `new_cooldown_secs`               |

use soroban_sdk::{Address, Env, Symbol};

use crate::errors::UpgradeError;
use crate::events;

const LAST_UPGRADE_TIME_KEY: &str = "last_upg_tm";
const UPGRADE_COOLDOWN_KEY: &str = "upg_cooldown";

/// Default cooldown is 24 hours (86400 seconds).
pub const DEFAULT_COOLDOWN_SECONDS: u64 = 86400;

/// Set the cooldown window (in seconds).
///
/// Requires auth from the caller.
///
/// # Events
/// Emits `cooldown_set` with `caller` as topic and `cooldown` as data.
pub fn set_cooldown(env: &Env, caller: &Address, cooldown: u64) {
    caller.require_auth();
    env.storage()
        .instance()
        .set(&Symbol::new(env, UPGRADE_COOLDOWN_KEY), &cooldown);

    env.events()
        .publish((events::event_cooldown_set(env), caller), cooldown);
}

/// Retrieve the current cooldown window.
///
/// Returns the configured cooldown window, or `DEFAULT_COOLDOWN_SECONDS` if not set.
pub fn get_cooldown(env: &Env) -> u64 {
    env.storage()
        .instance()
        .get::<_, u64>(&Symbol::new(env, UPGRADE_COOLDOWN_KEY))
        .unwrap_or(DEFAULT_COOLDOWN_SECONDS)
}

/// Verify that the cooldown period has elapsed since the last upgrade.
///
/// Requires auth from the caller. Returns `Ok(())` if the cooldown has elapsed
/// or if this is the first upgrade. Otherwise, returns `UpgradeError::CooldownNotElapsed`.
/// Also updates the last upgrade time to the current ledger timestamp upon success.
///
/// # Events
/// On success, emits two events in order:
/// 1. `upgrade_started` — topics `(upgrade_started, caller)`, data `(current_timestamp, cooldown)`.
///    Signals that auth passed and the cooldown constraint was satisfied.
/// 2. `upgrade_recorded` — topics `(upgrade_recorded, caller)`, data `recorded_timestamp`.
///    Signals that the new baseline timestamp has been persisted to storage.
///
/// No events are emitted when the call returns `Err`.
pub fn check_and_record_upgrade(env: &Env, caller: &Address) -> Result<(), UpgradeError> {
    caller.require_auth();

    let current_time = env.ledger().timestamp();
    let last_time = env
        .storage()
        .instance()
        .get::<_, u64>(&Symbol::new(env, LAST_UPGRADE_TIME_KEY))
        .unwrap_or(0);

    let cooldown = get_cooldown(env);

    if last_time != 0 {
        let elapsed = current_time
            .checked_sub(last_time)
            .ok_or(UpgradeError::Overflow)?;
        if elapsed < cooldown {
            return Err(UpgradeError::CooldownNotElapsed);
        }
    }

    // Auth passed and cooldown satisfied — signal the start of the upgrade.
    env.events().publish(
        (events::event_upgrade_started(env), caller),
        (current_time, cooldown),
    );

    env.storage()
        .instance()
        .set(&Symbol::new(env, LAST_UPGRADE_TIME_KEY), &current_time);

    // Timestamp is now persisted — confirm the record.
    env.events()
        .publish((events::event_upgrade_recorded(env), caller), current_time);

    Ok(())
}
