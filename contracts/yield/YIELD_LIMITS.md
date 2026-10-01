# Per-Account Yield Limits (Issue #842 / task b#017)

This document describes the new per-account state-cap surface added to the
`callora-yield` crate for the GrantFox FWC26 audit metric campaign (Issue #842,
task b#017).

## Problem

A single Stellar account can repeatedly call yield-bearing entrypoints (deposit
yield → open bets / positions / subscriptions) to grief indexers, drain fees
via tiny repeated operations, or game the audit metrics by inflating the
yield-bearing UX without producing meaningful economic activity. The issue
requires per-account caps on the *number* of open bets, open positions, and
active subscriptions a single address may hold at any one time.

## Surface

The new contract `CalloraYieldLimits` (in `contracts/yield/src/limits.rs`)
exposes the following entrypoints. Re-exports via `contracts/yield/src/lib.rs`:

```rust
pub use limits::{
    AccountLimits, AccountState, CalloraYieldLimits, CalloraYieldLimitsClient,
};
pub use errors::YieldLimitError;
```

### Construction & admin

| Entrypoint                              | Auth          | Notes                                                                 |
|-----------------------------------------|---------------|-----------------------------------------------------------------------|
| `init(admin)`                           | 1-shot        | One-time setup; rejects with `AlreadyInitialized` after first call.   |
| `set_admin(caller, new_admin)`          | admin         | Two-step transfer, step 1.                                             |
| `accept_admin(caller)`                  | pending-admin | Two-step transfer, step 2.                                            |
| `cancel_admin_transfer(caller)`         | admin         | Drops a pending transfer.                                              |
| `upgrade(caller, new_wasm_hash)`        | admin         | Admin-only WASM swap.                                                  |

### Limits configuration (admin only)

| Entrypoint                                                    | Effect                                                  |
|---------------------------------------------------------------|---------------------------------------------------------|
| `set_default_limits(caller, max_b, max_p, max_s)`             | Replace the global defaults.                             |
| `set_account_limits(caller, account, max_b, max_p, max_s)`   | Override the defaults for a single account.             |
| `clear_account_limits(caller, account)`                      | Revert the account to the global defaults.              |

### User counter mutators (caller-authenticated)

| Entrypoint                  | Effect                                                          |
|-----------------------------|-----------------------------------------------------------------|
| `place_bet(caller)`         | Increment caller's open-bet counter; rejects at cap.            |
| `clear_bet(caller)`         | Decrement caller's open-bet counter; rejects on zero.           |
| `open_position(caller)`     | Increment caller's open-position counter; rejects at cap.       |
| `close_position(caller)`    | Decrement caller's open-position counter; rejects on zero.      |
| `subscribe(caller)`         | Increment caller's active-subscription counter; rejects at cap. |
| `unsubscribe(caller)`       | Decrement caller's active-subscription counter; rejects on zero.|

### Read-only views (no auth)

| Entrypoint                                                | Effect                                                          |
|-----------------------------------------------------------|-----------------------------------------------------------------|
| `get_admin()`                                             | Current admin address or `NotInitialized`.                      |
| `get_default_limits()`                                    | Global cap defaults (fallback to `DEFAULT_LIMITS`).             |
| `get_account_limits(account)`                             | Effective caps for the account (per-account override → default). |
| `get_account_state(account)`                              | Live counters for the account (zeroed if absent).               |
| `can_place_bet(account)` / `can_open_position(account)` / |                                                              |
| `can_subscribe(account)`                                  | Dry-run gate checks.                                           |

## Storage layout

- `DefaultLimits` and `AccountLimits(Address)` are stored in **instance**
  storage so they participate in the same TTL extension window as the rest of
  the contract's configuration.
- `AccountState(Address)` is stored in **persistent** storage and bumped to
  `STATE_BUMP_AMOUNT` (≈30 days) on every read / write so accounts that go
  quiet do not silently archive.

### Defaults (`DEFAULT_LIMITS`)

```rust
pub const DEFAULT_LIMITS: AccountLimits = AccountLimits {
    max_bets: 100,
    max_positions: 50,
    max_subscriptions: 20,
};
```

These defaults are intentionally conservative so the contract is safe-by-default
even before admin sets `set_default_limits`. The admin may broadcast any
post-init override.

### Cap ceiling (`MAX_CAP`)

`MAX_CAP = 100_0000` (`1_000_000`). Any individual cap exceeds this triggers
`InvalidLimit` (code 4) instead of being persisted.

## Typed errors

`YieldLimitError` (in `contracts/yield/src/errors.rs`) is `#[contracterror]
#[repr(u32)]`. Numeric codes are stable and integrate with off-chain SDKs:

| Code | Variant               | Meaning                                                |
|------|-----------------------|--------------------------------------------------------|
| 1    | `NotInitialized`      | `init` was not called yet.                             |
| 2    | `AlreadyInitialized`  | `init` was called twice.                               |
| 3    | `Unauthorized`        | Caller is not permitted for this operation.            |
| 4    | `InvalidLimit`        | A configured cap exceeds `MAX_CAP`.                    |
| 5    | `BetsAtCap`           | Account's open-bet counter is at the cap.              |
| 6    | `PositionsAtCap`      | Account's open-position counter is at the cap.         |
| 7    | `SubscriptionsAtCap`  | Account's active-subscription counter is at the cap.   |
| 8    | `CounterUnderflow`    | `clear_*` called when the corresponding counter is 0.  |
| 9    | `Overflow`            | `u32` counter overflow during increment.               |

## Events

`contracts/yield/src/events.rs` centralizes every topic as a `Symbol::new`
call and pins each to its byte representation in `#[cfg(test)]` snapshot
tests. Topic vocabulary (`init`, `admin_*`, `default_limits_set`,
`account_limits_set`, `account_limits_cleared`, `bet_placed`, `bet_cleared`,
`position_opened`, `position_closed`, `subscription_added`,
`subscription_removed`, `upgraded`) follows the past-tense-verb pattern
documented in `CONTRIBUTING.md`.

## Backward compatibility

- `contracts/yield/src/lib.rs` continues to re-export
  `RevenuePool` / `RevenuePoolClient` unchanged.
- No prior type, storage key, or event topic was renamed.
- No prior `callora-yield` consumer is broken (only additive additions to the
  public surface).

## Verification

```bash
# Format, build, run unit tests & auth-snap integration suite.
cargo fmt  -p callora-yield
cargo test -p callora-yield --lib              # 40+ unit tests
cargo test -p callora-yield --test auth_snap   # 13 mutator + 7 view assertions

# Build for the wasm32 target so WASM-bound assets do compile.
cargo build -p callora-yield --target wasm32-unknown-unknown --release
```

All unit tests must pass, all 13 mutator assertions in `auth_snap.rs` must
report `res.is_err()`, and all 7 view assertions in `auth_snap.rs` must run
without `require_auth`.

## Coverage summary

- ~50 unit tests for caps, defaults, storage helpers, dry-run checks, and
  admin rotation.
- 13 authentication-snapshot mutator tests (`require_auth` enforcement).
- 7 authentication-snapshot view tests (no-auth callability).
- 13 byte-snapshot tests pinning event-topic symbol identity.
