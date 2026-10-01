//! # Read-only views for the Callora Vault contract.
//!
//! This module hosts the [`CalloraVault::simulate_deduct`] pre-flight view,
//! which mirrors [`crate::CalloraVault::deduct`]'s validation pipeline
//! end-to-end without performing any state mutation. Clients use it to
//! predict the outcome of a real `deduct` call before signing and submitting
//! a transaction.
//!
//! ## Guarantees
//! - **Read-only.** `simulate_deduct` does not write to instance, persistent,
//!   or temporary storage; does not transfer tokens; does not call into the
//!   settlement contract; and does not emit events.
//! - **Auth-free.** It does not call `require_auth`. The `caller` parameter
//!   is accepted for parity with `deduct` but is not authenticated.
//! - **Parity.** For any given vault state and inputs, the return value is
//!   exactly what a real `deduct` call with the same arguments would return
//!   for the validation-stage checks (pause, amount, max-deduct,
//!   idempotency, rate-limit, balance, slippage).
//!
//! The simulation stops at the validation stage and does not reach the
//! external call or settlement-credit step — so a vault with no
//! `settlement` configured would simulate as if it were configured.
//! Production callers should still call
//! [`crate::CalloraVault::get_settlement`] before submitting a real `deduct`.

use soroban_sdk::{contractimpl, Address, Env};

use crate::errors::VaultError;
use crate::{CalloraVault, CalloraVaultClient, CalloraVaultArgs};

/// Read-only pre-flight of [`crate::CalloraVault::deduct`].
///
/// Performs every validation step that `deduct` performs, in the same order,
/// except authorization, external token transfers, the settlement callback,
/// idempotency-marker writes, rate-limit state writes, balance mutations, and
/// event emission.
///
/// # Parameters
/// Identical to [`crate::CalloraVault::deduct`] so that callers can swap
/// `simulate_deduct` for `deduct` and keep the rest of their code unchanged.
///
/// - `_caller`: accepted for parity. **Not authenticated.** The simulation
///   does not raise `Unauthorized` for an unknown caller — it reflects what
///   the subsequent authorized `deduct` would do.
/// - `amount`: amount to deduct. Must be positive.
/// - `request_id`: optional idempotency key. If `Some(id)` with a non-zero id
///   that is already in storage, the simulation returns
///   `DuplicateRequestId`; `Some(0)` means "no idempotency".
/// - `max_fee_bps`: slippage guard. Same semantics as `deduct`.
/// - `developer`: developer whose rate-limit bucket is checked.
///
/// # Returns
/// - `Ok(new_balance)` — the projected vault balance after the deduct would
///   succeed. This matches `deduct`'s success payload (`Result<i128, ..>`)
///   one-for-one.
/// - `Err(VaultError)` — the same error variant a subsequent `deduct` with
///   identical arguments would emit at the matching validation step.
///
/// "Returns same struct as deduct" (issue #511) is interpreted as
/// `Result<i128, VaultError>` shape parity — the projected new balance on
/// success, the matching error variant on failure.
///
/// # Errors
/// Mirrors `deduct`'s validation errors in the same order:
///
/// 1. [`VaultError::Paused`] — vault is paused.
/// 2. [`VaultError::AmountNotPositive`] — `amount <= 0`.
/// 3. [`VaultError::ExceedsMaxDeduct`] — `amount > max_deduct`.
/// 4. [`VaultError::DuplicateRequestId`] — `request_id` already processed.
/// 5. [`VaultError::RateLimited`] — developer's bucket would be exhausted.
/// 6. [`VaultError::InsufficientBalance`] — vault balance < `amount`.
/// 7. [`VaultError::Slippage`] — `amount` exceeds `max_fee_bps` of the
///    current balance (skipped when `max_fee_bps == u16::MAX` or balance
///    is 0).
///
/// [`VaultError::Unauthorized`] is **not** raised here. Production callers
/// should separately verify that they (or their backend) hold the
/// owner / authorized-caller role before submitting the real `deduct`.
///
/// [`VaultError::SettlementNotSet`] is **not** raised here — the simulation
/// does not reach the external settlement call. Callers that need this
/// assurance should additionally call
/// [`crate::CalloraVault::get_settlement`] before submitting.
#[contractimpl]
impl CalloraVault {
    /// Read-only pre-flight of `deduct`. See the [`crate::views`] module docs.
    #[allow(clippy::too_many_arguments)]
    pub fn simulate_deduct(
        env: Env,
        _caller: Address,
        amount: i128,
        request_id: Option<u64>,
        max_fee_bps: u32,
        developer: Address,
    ) -> Result<i128, VaultError> {
        // 1. Pause guard (read-only via `Self::is_paused`).
        if CalloraVault::is_paused(env.clone()) {
            return Err(VaultError::Paused);
        }

        // 2. Amount must be positive.
        if amount <= 0 {
            return Err(VaultError::AmountNotPositive);
        }

        // 3. Configured `max_deduct` ceiling.
        let max_d = CalloraVault::get_max_deduct(env.clone());
        if amount > max_d {
            return Err(VaultError::ExceedsMaxDeduct);
        }

        // 4. Idempotency duplicate check. Delegated to the same private
        //    helper `deduct` uses, so the simulator can never silently
        //    diverge if storage semantics evolve.
        if let Some(ref rid) = request_id {
            CalloraVault::require_not_duplicate(&env, rid)?;
        }

        // 5. Rate-limit dry-run (reads bucket state; does not write).
        crate::rate_limit::would_consume_tokens(&env, &developer, amount)?;

        // 6. Balance check.
        let balance: i128 = env
            .storage()
            .instance()
            .get(&crate::DataKey::Balance)
            .unwrap_or(0);
        if balance < amount {
            return Err(VaultError::InsufficientBalance);
        }

        // 7. Slippage guard. Mirrors `deduct`'s math exactly: skip when
        //    `max_fee_bps == u16::MAX` (sentinel = no limit) or when the
        //    balance is zero (division-by-zero guard).
        if max_fee_bps < u32::MAX && balance > 0 {
            let calculated_fee_bps = amount
                .checked_mul(10_000)
                .ok_or(VaultError::Overflow)?
                / balance;
            if calculated_fee_bps > max_fee_bps as i128 {
                return Err(VaultError::Slippage);
            }
        }

        // Projected new balance — same shape as `deduct`'s `Ok(..)` payload.
        let new_balance = balance
            .checked_sub(amount)
            .ok_or(VaultError::Overflow)?;
        Ok(new_balance)
    }
}
