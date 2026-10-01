//! Per-token reserve caps for the Callora Vault.
//!
//! A reserve cap sets the maximum total balance the vault may hold for a given
//! token.  Deposit attempts that would push the balance past the cap are
//! rejected with [`VaultError::ExceedsReserveCap`].
//!
//! # Storage
//! Caps are stored under [`StorageKey::ReserveCap`]`(token)` in **instance**
//! storage so they share the same TTL extension as other vault configuration.
//!
//! # Default
//! When no cap has been set for a token, [`get`] returns `i128::MAX`, which
//! is effectively unlimited and keeps [`check`] on the fast path.

use soroban_sdk::{Address, Env};

use crate::{StorageKey, VaultError, INSTANCE_BUMP_AMOUNT, INSTANCE_BUMP_THRESHOLD};

/// Default maximum deduction cap — effectively unlimited.
pub const DEFAULT_MAX_DEDUCT: i128 = i128::MAX;

/// Check whether `amount` exceeds `max_deduct`. Returns `Err(ExceedsMaxDeduct)` if so.
pub fn check_max_deduct(amount: i128, max_deduct: i128) -> Result<(), VaultError> {
    if amount > max_deduct {
        return Err(VaultError::ExceedsMaxDeduct);
    }
    Ok(())
}

/// Store a per-token reserve cap and return the previous value.
///
/// # Arguments
/// * `env` - Execution environment.
/// * `token` - Token contract address the cap applies to.
/// * `cap` - New maximum balance in token stroops.
///
/// # Returns
/// The previous cap (`Some`) or `None` if no cap was previously configured.
pub fn set(env: &Env, token: &Address, cap: i128) -> Option<i128> {
    let key = StorageKey::ReserveCap(token.clone());
    let prev: Option<i128> = env.storage().instance().get(&key);
    env.storage().instance().set(&key, &cap);
    env.storage()
        .instance()
        .extend_ttl(INSTANCE_BUMP_THRESHOLD, INSTANCE_BUMP_AMOUNT);
    prev
}

/// Return the reserve cap for `token`.
///
/// Returns `i128::MAX` when no cap has been configured (effectively unlimited).
pub fn get(env: &Env, token: &Address) -> i128 {
    env.storage()
        .instance()
        .get(&StorageKey::ReserveCap(token.clone()))
        .unwrap_or(i128::MAX)
}

/// Guard called inside `deposit()`.
///
/// Checks whether `current_balance + deposit_amount` would exceed the
/// configured cap for `token` and returns [`VaultError::ExceedsReserveCap`]
/// if so.  When no cap is set the check is skipped entirely (fast path).
///
/// # Errors
/// - [`VaultError::Overflow`] — if the addition overflows `i128`.
/// - [`VaultError::ExceedsReserveCap`] — if the post-deposit balance would
///   exceed the cap.
pub fn check(
    env: &Env,
    token: &Address,
    current_balance: i128,
    deposit_amount: i128,
) -> Result<(), VaultError> {
    let cap = get(env, token);
    if cap == i128::MAX {
        return Ok(());
    }
    let post_balance = current_balance
        .checked_add(deposit_amount)
        .ok_or(VaultError::Overflow)?;
    if post_balance > cap {
        return Err(VaultError::ExceedsReserveCap);
    }
    Ok(())
}
