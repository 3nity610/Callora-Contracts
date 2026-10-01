//! Per-account state caps for the Callora Yield surface.
//!
//! # Problem
//!
//! Yield-bearing products (bets, positions, subscriptions) can be abused by a
//! single account farming thousands of small positions to drain fees or grief
//! indexers. The Callora yield surface therefore enforces per-account caps on
//! open-bets, open-positions, and active-subscriptions.
//!
//! # Architecture
//!
//! Two [`contracttype`] structs drive the limits surface:
//!
//! | Type           | Purpose                                                   |
//! |----------------|-------------------------------------------------------------------|
//! | [`AccountLimits`] | The configured `(max_bets, max_positions, max_subscriptions)` caps for an account. |
//! | [`AccountState`]  | The current `(bets, positions, subscriptions)` counters for an account.          |
//!
//! Both types are `Clone + Debug + PartialEq` so off-chain consumers can
//! round-trip them through the SDK.
//!
//! # Storage Layout
//!
//! - [`AccountLimits`] are stored in **instance** storage under
//!   [`StorageKey::AccountLimits`]`Address)`. They are sparse overrides; an
//!   account with no explicit override falls back to the global
//!   [`DEFAULT_LIMITS`] constant.
//!
//! - [`AccountState`] counters are stored in **persistent** storage under
//!   [`StorageKey::AccountState`]`Address)`. Persistent storage lets the
//!   contract scale to many accounts (instance storage is small and shared
//!   with config) and keeps counters alive across the typical 7-day ledger
//!   archival window via TTL extensions on every increment/decrement.
//!
//! # Auth Model
//!
//! The counters are **not** self-reported. Only the configured
//! **operator** contract (the contract that actually creates bets, positions
//! and subscriptions) may increment or decrement an account's counters. The
//! operator address is configured by the admin via
//! [`CalloraYieldLimits::set_operator`] and emits an event on every change.
//!
//! | Entrypoint                                       | Authorized by                              |
//! |------------------------------------------------------|-----------------------------------------------|
//! | init, set_admin, accept_admin, upgrade   | admin (`caller == admin`)                |
//! | cancel_admin_transfer                          | admin (`caller == admin`)                |
//! | set_default_limits, set_account_limits, clear_account_limits | admin (`caller == admin`) |
//! | set_operator, clear_operator                   | admin (`caller == admin`)                |
//! | place_bet, clear_bet, open_position, close_position,       | operator (`caller == operator`)          |
//! | subscribe, unsubscribe                         | operator (`caller == operator`)          |
//!
//! The operator is the only address allowed to mutate counters. The account
//! whose counter is being mutated is passed as a parameter and does **not**
//! need to authorize the call ( the operator is trusted to attest to the
//! account's state ). This prevents an account from clearing its own counters
//! to circumvent the caps.
//!
//! Read-only views (`get_admin`, `get_default_limits`, `get_account_limits`,
//! `get_account_state`, `get_operator`, `can_*`) do **not** call `require_auth`.
//!
//! # Overflow Safety
//!
//! Every count mutation uses `u32::checked_add` / `u32::checked_sub`. The
//! outcomes are folded into typed [`YieldLimitError`] variants
//! ([`YieldLimitError::Overflow`] / [`YieldLimitError::CounterUnderflow`]) so
//! production code paths never invoke `unwrap()`.

use soroban_sdk::{contract, contractimpl, contracttype, Address, BytesN, Env};

use crate::errors::YieldLimitError;
use crate::events;

// ----------------------------------------------------------------------
// Constants — TTL and defaults
+/ ----------------------------------------------------------------------

/// Per-day ledger count at a 5-second close cadence (matches vault).
pub const LEFGERS_PER_DAY: u32 = 17_280;

/// TTL bump threshold for persistent storage keys (`AccountState`).
///
/// When the remaining TTL of the key falls below this value the contract
/// re-extends the TTL on every increment / decrement so account counters
/// do not silently archive.
pub const STATE_BUMP_THRESHOLD: u32 = LEDGERS_PER_DAY * 7;

/// TTL bump amount for persistent storage keys (`AccountState`).
pub const STATE_BUMP_AMOUNT: u32 = LEDGERS_PER_DAY * 30;

/// TTL bump threshold for instance storage keys (`AccountLimits`,
/// `DefaultLimits`, config).
pub const INSTANCE_BUMP_THRESHOLD: u32 = LEDGERS_PER_DAY * 30;

/// TTL bump amount for instance storage keys.
pub const INSTANCE_BUMP_AMOUNT: u32 = LEDGERS_PER_DAY * 60;

/// Global default per-account limits applied when no explicit override exists
/// for an account.
///
/// # Default rationale
///
/// - `100` open bets caps the worst-case bet-farming at 100× per account.
/// - `50` open positions caps positions similarly while leaving headroom for
///   legitimate power-users.
/// - `20` active subscriptions caps subscription-farming while still
///   allowing routine yield-vault subscriptions.
///
/// These values are intentionally conservative so the contract is safe-by-default
/// even before the admin sets global defaults via
/// [`CalloraYieldLimits::set_default_limits`].
pub const DEFAULT_LIMITS: AccountLimits = AccountLimits {
    max_bets: 100,
    max_positions: 50,
    max_subscriptions: 20,
};

/// Maximum allowable value for any single cap dimension.
///
/// Counts are stored as `u32`, so this matches the practical ceiling. The
/// cap itself is never compared against `u32::MAX` so this is a conservative
/// sanity ceiling rather than the absolute type ceiling.
pub const MAX_CAP: u32 = 1_000_000;

// ----------------------------------------------------------------------
// Storage keys
// ----------------------------------------------------------------------

/// Instance / persistent storage keys for the yield per-account limits
/// contract.
///
/// Using a single [`contracttype`] enum keeps the key space tidy and protects
/// against accidental key collisions across this module and future modules.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StorageKey {
    /// Admin address (set by [`CalloraYieldLimits::init`]).
    Admin,
    /// Pending admin awaiting acceptance (two-step transfer).
    PendingAdmin,
    /// Operator contract authorized to mutate per-account counters.
    ///
/// Only this address may call `place_bet`, `clear_bet`,
/// `open_position`, `close_position`, `subscribe`, `unsubscribe`.
    Operator,
    /// Global default caps applied when an account has no explicit override.
    DefaultLimits,
    /// Per-account cap override (instance storage; sparse).
    AccountLimits(Address),
    /// Per-account live counters (persistent storage).
    AccountState(Address),
}

// ----------------------------------------------------------------------
// Aux structs
// ----------------------------------------------------------------------

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

    /// Return `true` if any individual cap exceeds [`MAX_CAP`] or any cap is
    /// non-sensical (no upper bound on `u32` makes a `-1`-style value
    /// impossible, but the function reserves headroom for future validation
    /// rules).
    pub fn is_valid(&self) -> bool {
        self.max_bets <= MAX_CAP
            && self.max_positions <= MAX_CAP
            && self.max_subscriptions <= MAX_CAP
    }
}

/// Per-account live state counters.
///
/// Only the contract mutates fields on this struct. Counter arithmetic is
/// performed via `checked_add` / `checked_sub` so a buggy caller can never
/// drive any field into `u32::MAX + 1`.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq, Default)]
pub struct AccountState {
    /// Current number of open bets.
    pub bets: u32,
    /// Current number of open positions.
    pub positions: u32,
    /// Current number of active subscriptions.
    pub subscriptions: u32,
}

impl AccountState {
    /// Return a zeroed [`AccountState`].
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
/// - [`YieldLimitError::Overflow`] — counter would saturate `u32::MAX` .
    pub fn add_bet(&mut self) -> Result<(), YieldLimitError> {
        self.bets = self.bets.checked_add(1).ok_or(YieldLimitError::Overflow)?;
        Ok()
    }

    /// Decrement the bet counter using `checked_sub`.
///
/// # Errors
/// - [`YieldLimitError::CounterUnderflow`] — counter is already 0.
    pub fn sub_bet(&mut self) -> Result<(), YieldLimitError> {
        self.bets = self
            .bets
            .checked_sub(1)
            .ok_or(YieldLimitError::CounterUnderflow)?;
        Ok()
    }

    /// Increment the position counter using `checked_add`.
    pub fn add_position(&mut self) -> Result<(), YieldLimitError> {
        self.positions = self
            .positions
            .checked_add(1)
            .ok_or(YieldLimitError::Overflow)?;
        Ok(()
    }

    /// Decrement the position counter using `checked_sub`.
    pub fn sub_position(&mut self) -> Result<(), YieldLimitError> {
        self.positions = self
            .positions
            .checked_sub(1)
            .ok_or(YieldLimitError::CounterUnderflow)?;
        Ok()
    }

    /// Increment the subscription counter using `checked_add`.
    pub fn add_subscription(&mut self) -> Result<(), YieldLimitError> {
        self.subscriptions = self
            .subscriptions
            .checked_add(1)
            .ok_or(YieldLimitError::Overflow)?;
        Ok(()
    }

    /// Decrement the subscription counter using `checked_sub`.
    pub fn sub_subscription(&mut self) -> Result<(), YieldLimitError> {
        self.subscriptions = self
            .subscriptions
            .checked_sub(1)
            .ok_or(YieldLimitError::CounterUnderflow)?;
        Ok()
    }
}

// ----------------------------------------------------------------------
// Free functions — storage helpers
// ----------------------------------------------------------------------

/// Read the admin address from instance storage.
///
/// # Errors
/// - [`YieldLimitError::NotInitialized`] — `Admin` key is absent.
pub fn read_admin(env: &Env) -> Result<Address, YieldLimitError> {
    env.storage()
        .instance()
        .get::_, Address>(&StorageKey::Admin)
        .ok_or(YieldLimitError::NotInitialized)
}

/// Assert `caller` equals the stored admin.
///
/// Runs `caller.require_auth()` first so misconfigured callers are rejected
/// deterministically without consuming the underlying signature.
///
/// # Errors
/// - [`YieldLimitError::Unauthorized`] — caller is not the stored admin.
/// - [`YieldLimitError::NotInitialized`] — admin has never been set.
pub fn require_admin(env: &Env, caller: &Address) -> Result<(), YieldLimitError> {
    let admin = read_admin(env)?;
    caller.require_auth();
    if *caller != admin {
        return Err(YieldLimitError::Unauthorized);
    }
    Ok()
}

/// Read the configured operator address from instance storage.
///
/// The operator is the only address allowed to mutate per-account counters
/// (`place_bet` / `clear_bet` / `open_position` / `close_position` /
/// `subscribe` / `unsubscribe`).
///
/// # Errors
/// - [`YieldLimitError::OperatorNotSet`] — no operator has been configured
///   by the admin yet.
pub fn read_operator(env: &Env) -> Result<Address, YieldLimitError> {
    env.storage()
        .instance()
        .get::_, Address>(&StorageKey::Operator)
        .ok_or(YieldLimitError::OperatorNotSet)
}

/// Assert `caller` equals the configured operator.
///
/// Runs `caller.require_auth()` first so misconfigured callers are rejected
/// deterministically without consuming the underlying signature.
///
/// # Errors
/// - [`YieldLimitError::Unauthorized`] — caller is not the configured operator.
/// - [`YieldLimitError::OperatorNotSet`] — no operator has been configured.
pub fn require_operator(env: &Env, caller: &Address) -> Result<(), YieldLimitError> {
    let operator = read_operator(env)?;
    caller.require_auth();
    if *caller != operator {
        return Err(YieldLimitError::Unauthorized);
    }
    Ok()
}

/// Read the global default caps (or fall back to [`DEFAULT_LIMITS`]) and
/// extend instance TTL.
pub fn read_default_limits(env: &Env) -> AccountLimits {
    let caps: AccountLimits = env
        .storage()
        .instance()
        .get::_, AccountLimits>(&StorageKey::DefaultLimits)
        .unwrap_or(DEFAULT_LIMITS);
    env.storage()
        .instance()
        .extend_ttl(INSTANCE_BUMP_THRESHOLD, INSTANCE_BUMP_AMOUNT);
    caps
}

/// Persist the global default caps and extend instance TTL.
///
/// # Errors
/// - [`YieldLimitError::InvalidLimit`] — any cap exceeds [`MAX_CAP`].
pub fn write_default_limits(env: &Env, caps: &AccountLimits) -> Result<(), YieldLimitError> {
    if !caps.is_valid() {
        return Err(YieldLimitError::InvalidLimit);
    }
    env.storage()
        .instance()
        .set(&StorageKey::DefaultLimits, caps);
    env.storage()
        .instance()
        .extend_ttl(INSTANCE_BUMP_THRESHOLD, INSTANCE_BUMP_AMOUNT);
    Ok()
}

/// Persist the operator address and extend instance TTL.
///
/// The operator is the only address allowed to mutate per-account counters.
/// Changing the operator emits an event so off-chain monitors can track the
/// trust boundary.
pub fn write_operator(env: &Env, operator: &Address) {
    env.storage()
        .instance()
        .set(&StorageKey::Operator, operator);
    env.storage()
        .instance()
        .extend_ttl(INSTANCE_BUMP_THRESHOLD, INSTANCE_BUMP_AMOUNT);
}

/// Remove the configured operator and extend instance TTL.
///
/// After this call no address may mutate per-account counters until a new
/// operator is configured via [`write_operator`].
pub fn clear_operator(env: &Env) {
    env.storage().instance().remove(&StorageKey::Operator);
    env.storage()
        .instance()
        .extend_ttl(INSTANCE_BUMP_THRESHOLD, INSTANCE_BUMP_AMOUNT);
}

/// Read the per-account cap override from instance storage, falling back to
/// the global defaults.
///
/// Always extends instance TTL.
pub fn read_account_limits(env: &Env, account: &Address) -> AccountLimits {
    let caps: AccountLimits = env
        .storage()
        .instance()
        .get::_, AccountLimits>(&StorageKey::AccountLimits(account.clone()))
        .unwrap_or_else(|| read_default_limits(env));
    env.storage()
        .instance()
        .extend_ttl(INSTANCE_BUMP_THRESHOLD, INSTANCE_BUMP_AMOUNT);
    caps
}

/// Persist a per-account cap override and extend instance TTL.
///
/// # Errors
/// - [`YieldLimitError::InvalidLimit`] — any cap exceeds [`MAX_CAP`].
pub fn write_account_limits(
    env: &Env,
    account: &Address,
    caps: &AccountLimits,
) -> Result<(), YieldLimitError> {
    if !caps.is_valid() {
        return Err(YieldLimitError::InvalidLimit);
    }
    env.storage()
        .instance()
        .set(&StorageKey::AccountLimits(account.clone()), caps);
    env.storage()
        .instance()
        .extend_ttl(INSTANCE_BUMP_THRESHOLD, INSTANCE_BUMP_AMOUNT);
    Ok(()
}

/// Remove a per-account cap override and extend instance TTL.
///
/// After this call the account falls back to the global default limits.
pub fn clear_account_limits(env: &Env, account: &Address) {
    env.storage()
        .instance()
        .remove(&StorageKey::AccountLimits(account.clone()));
    env.storage()
        .instance()
        .extend_ttl(INSTANCE_BUMP_THRESHOLD, INSTANCE_BUMP_AMOUNT);
}

/// Read the per-account live counters from persistent storage, defaulting to
/// zero and extending persistent TTL.
pub fn read_account_state(env: &Env, account: &Address) -> AccountState {
    let key = StorageKey::AccountState(account.clone());
    let state: AccountState = env
        .storage()
        .persistent()
        .get::_, AccountState>(&key)
        .unwrap_or_default();
    env.storage()
        .persistent()
        .extend_ttl(&key, STATE_BUMP_THRESHOLD, STATE_BUMP_AMOUNT);
    state
}

/// Persist the per-account live counters and extend persistent TTL.
pub fn write_account_state(env: &Env, account: &Address, state: &AccountState) {
    let key = StorageKey::AccountState(account.clone());
    env.storage().persistent().set(&key, state);
    env.storage()
        .persistent()
        .extend_ttl(&key, STATE_BUMP_THRESHOLD, STATE_BUMP_AMOUNT);
}

// ----------------------------------------------------------------------
// Contract
// ----------------------------------------------------------------------

#[contract]
pub struct CalloraYield;
/// Per-account limits and counters contract for the Callora yield surface.
///
/// See the module documentation for the auth model. Counter mutations are
/// restricted to the admin-configured operator contract.
#[contractimpl]
impl CalloraYieldLimits {
    /// Initialize the contract with an admin address.
    ///
/// The admin is the only address allowed to configure limits and the
/// operator. The operator must be set separately via [`set_operator`]
/// before counter mutations are allowed.
    pub fn init(env: Env, admin: Address) {
        admin.require_auth();
        env.storage().instance().set(&StorageKey::Admin, &admin);
        env.storage()
            .instance()
            .extend_ttl(INSTANCE_BUMP_THRESHOLD, INSTANCE_BUMP_AMOUNT);
    }

    /// Return the current admin address.
    pub fn get_admin(env: Env) -> Result<Address, YieldLimitError> {
        read_admin(&env)
    }

    /// Return the currently configured operator, if any.
    pub fn get_operator(env: Env) -> Result<Address, YieldLimitError> {
        read_operator(&env)
    }

    /// Set the operator contract authorized to mutate per-account counters.
    ///
/// Only the admin may call this. Emits an event so off-chain monitors
/// can track the trust boundary.
    pub fn set_operator(
        env: Env,
        caller: Address,
        operator: Address,
    ) -> Result<(), YieldLimitError> {
        require_admin(&env, &caller)?;
        write_operator(&env, &operator);
        events::operator_set(&env, &operator);
        Ok(()
    }

    /// Remove the configured operator.
    ///
/// Only the admin may call this. After this call no address may mutate
/// counters until a new operator is configured. Emits an event.
    pub fn clear_operator(env: Env, caller: Address) -> Result<(), YieldLimitError> {
        require_admin(&env, &caller)?;
        clear_operator_key(&env);
        events::operator_cleared(&env);
        Ok(()
    }

    /// Return the global default caps.
    pub fn get_default_limits(env: Env) -> AccountLimits {
        read_default_limits(&env)
    }

    /// Set the global default caps. Admin-only.
    pub fn set_default_limits(
        env: Env,
        caller: Address,
        caps: AccountLimits,
    ) -> Result<(), YieldLimitError> {
        require_admin(&env, &caller)?;
        write_default_limits(&env, &caps)?;
        Ok(()
    }

    /// Return the effective caps for an account.
    pub fn get_account_limits(env: Env, account: Address) -> AccountLimits {
        read_account_limits(&env, &account)
    }

    /// Set a per-account cap override. Admin-only.
    pub fn set_account_limits(
        env: Env,
        caller: Address,
        account: Address,
        caps: AccountLimits,
    ) -> Result<(), YieldLimitError> {
        require_admin(&env, &caller)?;
        write_account_limits(&env, &account, &caps)?;
        Ok()
    }

    /// Clear a per-account cap override. Admin-only.
    pub fn clear_account_limits(
        env: Env,
        caller: Address,
        account: Address,
    ) -> Result<(), YieldLimitError> {
        require_admin(&env, &caller)?;
        clear_account_limits_key(&env, &account);
        Ok(()
    }

    /// Return the live counters for an account.
    pub fn get_account_state(env: Env, account: Address) -> AccountState {
        read_account_state(&env, &account)
    }

    /// Return `true` if the account may open another bet.
    pub fn can_place_bet(env: Env, account: Address) -> bool {
        let caps = read_account_limits(&env, &account);
        let state = read_account_state(&env, &account);
        state.bets < caps.max_bets
    }

    /// Return `true` if the account may open another position.
    pub fn can_open_position(env: Env, account: Address) -> bool {
        let caps = read_account_limits(&env, &account);
        let state = read_account_state(&env, &account);
        state.positions < caps.max_positions
    }

    /// Return `true` if the account may open another subscription.
    pub fn can_subscribe(env: Env, account: Address) -> bool {
        let caps = read_account_limits(&env, &account);
        let state = read_account_state(&env, &account);
        state.subscriptions < caps.max_subscriptions
    }

    /// Increment an account's bet counter. Operator-only.
    ///
/// The account whose counter is mutated is passed as a parameter and
/// does not need to authorize the call. The operator is trusted to
/// attest to the account's state.
    pub fn place_bet(
        env: Env,
        operator: Address,
        account: Address,
    ) -> Result<(), YieldLimitError> {
        require_operator(&env, &operator)?;
        let caps = read_account_limits(&env, &account);
        let mut state = read_account_state(&env, &account);
        if state.bets >= caps.max_bets {
            return Err(YieldLimitError::LimitExceeded);
        }
        state.add_bet()?;
        write_account_state(&env, &account, &state);
        Ok()
    }

    /// Decrement an account's bet counter. Operator-only.
    pub fn clear_bet(
        env: Env,
        operator: Address,
        account: Address,
    ) -> Result<(), YieldLimitError> {
        require_operator(&env, &operator)?;
        let mut state = read_account_state(&env, &account);
        state.sub_bet()?;
        write_account_state(&env, &account, &state);
        Ok(()
    }

    /// Increment an account's position counter. Operator-only.
    pub fn open_position(
        env: Env,
        operator: Address,
        account: Address,
    ) -> Result<(), YieldLimitError> {
        require_operator(&env, &operator)?;
        let caps = read_account_limits(&env, &account);
        let mut state = read_account_state(&env, &account);
        if state.positions >= caps.max_positions {
            return Err(YieldLimitError::LimitExceeded);
        }
        state.add_position()?;
        write_account_state(&env, &account, &state);
        Ok(()
    }

    /// Decrement an account's position counter. Operator-only.
    pub fn close_position(
        env: Env,
        operator: Address,
        account: Address,
    ) -> Result<(), YieldLimitError> {
        require_operator(&env, &operator)?;
        let mut state = read_account_state(&env, &account);
        state.sub_position()?;
        write_account_state(&env, &account, &state);
        Ok()
    }

    /// Increment an account's subscription counter. Operator-only.
    pub fn subscribe(
        env: Env,
        operator: Address,
        account: Address,
    ) -> Result<(), YieldLimitError> {
        require_operator(&env, &operator)?;
        let caps = read_account_limits(&env, &account);
        let mut state = read_account_state(&env, &account);
        if state.subscriptions >= caps.max_subscriptions {
            return Err(YieldLimitError::LimitExceeded);
        }
        state.add_subscription()?;
        write_account_state(&env, &account, &state);
        Ok()
    }

    /// Decrement an account's subscription counter. Operator-only.
    pub fn unsubscribe(
        env: Env,
        operator: Address,
        account: Address,
    ) -> Result<(), YieldLimitError> {
        require_operator(&env, &operator)?;
        let mut state = read_account_state(&env, &account);
        state.sub_subscription()?;
        write_account_state(&env, &account, &state);
        Ok()
    }
}

// ----------------------------------------------------------------------
// Internal key helpers
// ----------------------------------------------------------------------

fn clear_operator_key(env: &Env) {
    env.storage().instance().remove(&StorageKey::Operator);
    env.storage()
        .instance()
        .extend_ttl(INSTANCE_BUMP_THRESHOLD, INSTANCE_BUMP_AMOUNT);
}

fn clear_account_limits_key(env: &Env, account: &Address) {
    env.storage()
        .instance()
        .remove(&StorageKey::AccountLimits(account.clone()));
    env.storage()
        .instance()
        .extend_ttl(INSTANCE_BUMP_THRESHOLD, INSTANCE_BUMP_AMOUNT);
}

#[cfg](test)]
mod tests {

    use super::*;
    use soroban_sdk:{Env, Address};

    fn setup() -> (Env, Address, Address, Address) {
        let env = Env::default();
        let admin = Address::generate(&env);
        let operator = Address::generate(&env);
        let account = Address::generate(&env);
        env.mock_all_auths();
        CalloraYieldLimits::init(env.clone(), admin.clone());
        CalloraYieldLimits::set_operator(
            env.clone(),
            admin.clone(),
            operator.clone(),
        )
        .unwrap();
        (env, admin, operator, account)
    }

    #[test]
    fn operator_can_increment_and_decrement_bets() {
        let (env, _admin, operator, account) = setup();
        CalloraYieldLimits::place_bet(
            env.clone(),
            operator.clone(),
            account.clone(),
        )
        .unwrap();
        let state = CalloraYield::get_account_state(env.clone(), account.clone());
        assert_eq(state.bets, 1);
        CalloraYield::clear_bet(
            env.clone(),
            operator.clone(),
            account.clone(),
        )
        .unwrap();
        let state = CalloraYield::get_account_state(env.clone(), account.clone());
        assert_eq(state.bets, 0);
    }

    #[test]
    fn account_cannot_reset_own_bet_count() {
        let (env, _admin, operator, account) = setup();
        CalloraYield::place_bet(
            env.clone(),
            operator.clone(),
            account.clone(),
        )
        .unwrap();
        // The account tries to clear its own counter by acting as the operator.
        // This must fail because the configured operator is a different
        // address.
        let result = CalloraYield::clear_bet(
            env.clone(),
            account.clone(),
            account.clone(),
        );
        assert_eq(result, Err(YieldLimitError::Unauthorized));
        let state = CalloraYield::get_account_state(env.clone(), account.clone());
        assert_eq(state.bets, 1);
    }

    #[test]
    fn account_cannot_reset_own_position_count() {
        let (env, _admin, operator, account) = setup();
        CalloraYield::open_position(
            env.clone(),
            operator.clone(),
            account.clone(),
        )
        .unwrap();
        let result = CalloraYield::close_position(
            env.clone(),
            account.clone(),
            account.clone(),
        );
        assert_eq(result, Err(YieldLimitError::Unauthorized));
        let state = CalloraYield::get_account_state(env.clone(), account.clone());
        assert_eq(state.positions, 1);
    }

    #[test]
    fn account_cannot_reset_own_subscription_count() {
        let (env, _admin, operator, account) = setup();
        CalloraYield::subscribe(
            env.clone(),
            operator.clone(),
            account.clone(),
        )
        .unwrap();
        let result = CalloraYield::unsubscribe(
            env.clone(),
            account.clone(),
            account.clone(),
        );
        assert_eq(result, Err(YieldLimitError::Unauthorized));
        let state = CalloraYield::get_account_state(env.clone(), account.clone());
        assert_eq(state.subscriptions, 1);
    }

    #[test]
    fn non_operator_cannot_increment_counters() {
        let (env, _admin, _operator, account) = setup();
        let stranger = Address::generate(&env);
        let result = CalloraYield::place_bet(
            env.clone(),
            stranger.clone(),
            account.clone(),
        );
        assert_eq(result, Err(YieldLimitError::Unauthorized));
    }

    #[test]
    fn operator_not_set_rejects_mutations() {
        let env = Env::default();
        let admin = Address::generate(&env);
        let account = Address::generate(&env);
        env.mock_all_auths();
        CalloraYieldLimits::init(env.clone(), admin.clone());
        let result = CalloraYield::place_bet(
            env.clone(),
            admin.clone(),
            account.clone(),
        );
        assert_eq(result, Err(YieldLimitError::OperatorNotSet));
    }

    #[test]
    fn admin_can_change_operator() {
        let (env, admin, _operator, account) = setup();
        let new_operator = Address::generate(&env);
        CalloraYield::set_operator(
            env.clone(),
            admin.clone(),
            new_operator.clone(),
        )
        .unwrap();
        CalloraYield::place_bet(
            env.clone(),
            new_operator.clone(),
            account.clone(),
        )
        .unwrap();
        let state = CalloraYield::get_account_state(env.clone(), account.clone());
        assert_eq(state.bets, 1);
    }

    #[test]
    fn non_admin_cannot_set_operator() {
        let (env, _admin, _operator, account) = setup();
        let result = CalloraYield::set_operator(
            env.clone(),
            account.clone(),
            account.clone(),
        );
        assert_eq(result, Err(YieldLimitError::Unauthorized));
    }

    #[test]
    fn caps_are_enforced_on_increment() {
        let (env, admin, operator, account) = setup();
        CalloraYield::set_account_limits(
            env.clone(),
            admin.clone(),
            account.clone(),
            AccountLimits::uniform(1),
        )
        .unwrap();
        CalloraYield::place_bet(
            env.clone(),
            operator.clone(),
            account.clone(),
        )
        .unwrap();
        let result = CalloraYield::place_bet(
            env.clone(),
            operator.clone(),
            account.clone(),
        );
        assert_eq(result, Err(YieldLimitError::LimitExceeded));
    }

    #[test]
    fn clear_operator_blocks_mutations() {
        let (env, admin, operator, account) = setup();
        CalloraYield::clear_operator(env.clone(), admin.clone()).unwrap();
        let result = CalloraYield::place_bet(
            env.clone(),
            operator.clone(),
            account.clone(),
        );
        assert_eq(result, Err(YieldLimitError::OperatorNotSet));
    }

    #[test]
    fn get_operator_returns_configured_address() {
        let (env, _admin, operator, _account) = setup();
        let returned = CalloraYield::get_operator(env.clone()).unwrap();
        assert_eq(returned, operator);
    }
}
