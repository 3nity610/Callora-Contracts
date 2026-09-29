# CEI Audit Report

## Findings and Violations

### 1. `callora-vault` — `deposit`
Token transfer interaction occurred before updating local storage balance.

**Fix:** storage update performed before the token transfer call.

---

### 2. `callora-vault` — `deduct` / `batch_deduct`
External contract settlement/token routing was initiated before mutating the user balance state.

**Fix:** checked internal state subtraction executes before cross-contract interactions.

---

### 3. `callora-settlement` — `withdraw_developer_balance` (fixed in this PR)

#### Violation
`withdraw_developer_balance` called `usdc.transfer(...)` **before** writing the
reduced `DeveloperBalance` and the updated `DailyWithdrawState` to persistent
storage.  In addition, the developer balance was read from storage **twice**
under two different local variable names (`dev_balance_key`/`dev_balance` and
`balance_key`/`current_balance`) using an identical `StorageKey`, making one
read redundant.

**Risk:** A custom SAC wrapper or a future multi-asset token that re-enters
`withdraw_developer_balance` during its `transfer` hook would observe the
*original* (not yet reduced) balance and could drain the same funds a second
time.  Soroban's automatic transaction revert does not protect against
within-transaction reentrancy when the re-entrant call reads and acts on
stale state before the outer call has a chance to commit its effects.

#### Fix
Restructured `withdraw_developer_balance` into strict
Checks → Effects → Interactions order:

1. **Checks** — all validation (auth, freeze, amount, claim window, balance
   sufficiency, daily cap, contract liquidity) runs first.  The duplicate
   storage read is eliminated: `current_balance` is fetched exactly once.

2. **Effects** — `env.storage().persistent().set(&balance_key, &new_balance)`
   and `env.storage().persistent().set(&today_key, &daily)` (plus TTL
   extensions) are committed **before** the transfer is initiated.

3. **Interaction** — `usdc.transfer(&contract_address, &recipient, &amount)`
   executes last, after all state mutations are durable.

#### Test coverage
`contracts/settlement/src/test_reentrancy.rs` contains three new tests using a
`MaliciousToken` mock that intercepts `transfer` and immediately calls back into
`withdraw_developer_balance`:

| Test | What it verifies |
|------|-----------------|
| `reentrancy_cannot_double_withdraw` | Re-entrant call finds zero balance; total drain == credited amount (not 2×). |
| `reentrancy_reentrant_call_observes_zero_balance` | Re-entrant call is blocked; balance is exactly 0 after one successful outer call. |
| `reentrancy_daily_counter_persisted_before_transfer` | Daily counter is persisted first; a re-entrant call that would exceed the cap is rejected even though the outer call is still in progress. |

All three tests pass (`cargo test -p callora-settlement --lib reentrancy`).

---

## Verification

- Applied strict Checks-Effects-Interactions ordering across all state-mutating
  entrypoints.
- Added explicit state-mutation unit tests and reentrancy mock tests to ensure
  adherence to execution order.
- Pre-existing test failures in `test_ttl_bump`, `test_events`, and
  `test_overflow_safe_math` are unrelated to CEI ordering and are present on
  the `main` branch before this change.
