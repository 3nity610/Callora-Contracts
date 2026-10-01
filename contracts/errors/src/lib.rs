#![no_std]
use soroban_sdk::{Address, Env, String, Symbol, contract, contracterror, contractimpl, contracttype};

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    Unauthorized = 3,
    Overflow = 4,
    /// `log_error` was called with a code that `register_error` never defined.
    UnknownErrorCode = 5,
}

#[contracttype]
pub enum DataKey {
    Admin,
    ErrorReg(u32),
    RecentErr(Address),
}

/// Ledger cadence used across the workspace for TTL policy.
const LEDGERS_PER_DAY: u32 = 17_280;

/// TTL policy for the persistent error registry.
///
/// A registry entry is refreshed when its remaining TTL drops below ~30 days,
/// extending it back out to ~60 days. This mirrors the repo-wide convention
/// (`contracts/fee`: `INSTANCE_BUMP_THRESHOLD` = 30 days,
/// `INSTANCE_BUMP_AMOUNT` = 60 days) so a long-lived registry cannot archive
/// out from under integrators that resolve descriptions on the read path.
const REGISTRY_TTL_THRESHOLD: u32 = LEDGERS_PER_DAY * 30;
const REGISTRY_TTL_BUMP: u32 = LEDGERS_PER_DAY * 60;

#[contract]
pub struct ErrorsContract;

#[contractimpl]
impl ErrorsContract {
    pub fn init(env: Env, admin: Address) -> Result<(), Error> {
        admin.require_auth();

        if env.storage().instance().has(&DataKey::Admin) {
            return Err(Error::AlreadyInitialized);
        }

        env.storage().instance().set(&DataKey::Admin, &admin);
        Ok(())
    }

    /// Register (or replace) the description for `code`. Admin only.
    pub fn register_error(env: Env, admin: Address, code: u32, desc: String) -> Result<(), Error> {
        admin.require_auth();

        let stored_admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(Error::NotInitialized)?;
        if admin != stored_admin {
            return Err(Error::Unauthorized);
        }

        let key = DataKey::ErrorReg(code);
        env.storage().persistent().set(&key, &desc);
        env.storage()
            .persistent()
            .extend_ttl(&key, REGISTRY_TTL_THRESHOLD, REGISTRY_TTL_BUMP);
        Ok(())
    }

    /// Return the description registered for `code`, or `None` when the code is
    /// unknown.
    ///
    /// A successful lookup refreshes the registry entry's TTL, so codes that
    /// integrators actively resolve stay available.
    pub fn get_error_description(env: Env, code: u32) -> Option<String> {
        let key = DataKey::ErrorReg(code);
        let desc: Option<String> = env.storage().persistent().get(&key);
        if desc.is_some() {
            env.storage()
                .persistent()
                .extend_ttl(&key, REGISTRY_TTL_THRESHOLD, REGISTRY_TTL_BUMP);
        }
        desc
    }

    /// Log an error for `user`.
    ///
    /// Only codes previously defined by [`Self::register_error`] are accepted;
    /// anything else returns [`Error::UnknownErrorCode`] so consumers can trust
    /// that every logged code resolves to a known description. On success the
    /// code is stored as the user's most recent error and an `error_logged`
    /// event is published.
    pub fn log_error(env: Env, user: Address, code: u32) -> Result<(), Error> {
        user.require_auth();

        let reg_key = DataKey::ErrorReg(code);

        // Only registered codes may be logged.
        if !env.storage().persistent().has(&reg_key) {
            return Err(Error::UnknownErrorCode);
        }

        // Overflow-safe: checked_add prevents silent wrap at u32::MAX.
        // This is the only arithmetic path in the contract; all other
        // operations are storage reads/writes and comparisons.
        let _safe_calc = code.checked_add(1).ok_or(Error::Overflow)?;

        // A code that is actively logged is actively used — keep it warm.
        env.storage()
            .persistent()
            .extend_ttl(&reg_key, REGISTRY_TTL_THRESHOLD, REGISTRY_TTL_BUMP);

        env.storage()
            .temporary()
            .set(&DataKey::RecentErr(user.clone()), &code);

        env.storage()
            .temporary()
            .extend_ttl(&DataKey::RecentErr(user.clone()), 100, 100);

        env.events()
            .publish((Symbol::new(&env, "error_logged"), user), code);

        Ok(())
    }

    /// Return the most recent code logged by `user`, or `None` if they never
    /// logged an error.
    pub fn get_recent_error(env: Env, user: Address) -> Option<u32> {
        env.storage().temporary().get(&DataKey::RecentErr(user))
    }
}

#[cfg(test)]
mod test;
