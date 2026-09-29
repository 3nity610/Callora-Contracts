# Admin Lifecycle Events — Implementation Summary (Issue #832)

> **Status (issue #1248):** this summary covers the `admin` lifecycle surface
> only. The `limits` module that ships in the same crate is **experimental and
> unwired** (no consumer), and its per-account cap concept is duplicated by the
> wired `contracts/yield` limits contract. See `docs/storage.md` and
> `contracts/yield/YIELD_LIMITS.md`.

## What was added

A new Soroban crate `contracts/admin` exposing:

- `init(env, admin)` — one-time bootstrap, emits `admin_init`.
- `set_admin(env, caller, new_admin)` — current admin nominates a
  successor, emits `admin_nominated`.
- `accept_admin(env, caller)` — pending admin accepts the role, emits
  `admin_changed` with `(previous_admin, new_admin)` payload.
- `cancel_admin_transfer(env, caller)` — current admin revokes the
  pending nomination, emits `admin_cancelled`.
- `get_admin(env) -> Option<Address>` — view.
- `get_pending_admin(env) -> Option<Address>` — view.

## Topic shape (canonical 2-topic)

```text
topics: (action: Symbol, caller: Address)
data:   <event-specific payload>
```

| Topic bytes | Topic function | Topics | Data |
|-------------|---------------|--------|------|
| `"admin_init"` | `event_admin_init` | `(topic, initial_admin)` | `()` |
| `"admin_nominated"` | `event_admin_nominated` | `(topic, current_admin)` | `pending_admin: Address` |
| `"admin_changed"` | `event_admin_changed` | `(topic, new_admin)` | `(previous_admin, new_admin)` |
| `"admin_cancelled"` | `event_admin_cancelled` | `(topic, current_admin)` | `pending_admin: Address` |

## Storage keys

| Key | Type | Tier | Purpose |
|-----|------|------|---------|
| `admin` | `Address` | Instance | The active admin |
| `pending_admin` | `Option<Address>` | Instance | Pending nominee during a two-step transfer |

Both keys are stored in instance storage; the contract bumps
`LIFETIME_THRESHOLD (1_000)` → `BUMP_AMOUNT (10_000)` ledgers of TTL on
every state write so the admin slot never falls into Soroban's archive
window.

## Guarantees

1. **Auth-first.** Every state-changing function starts with
   `caller.require_auth()` — auth failures panic before any storage is
   touched.
2. **No silent admin changes.** All four transitions emit exactly one
   canonical event, with deterministic topic ordering
   (`admin_nominated` precedes the matching `admin_changed` /
   `admin_cancelled`).
3. **Two-step default.** `set_admin` alone never transfers power; only
   `accept_admin` flips the active slot, and an explicit
   `cancel_admin_transfer` may revoke a mistyped nominee.
4. **No `unwrap()` in production paths.** All `.expect(...)` calls
   reference a named error constant; read-only views use
   `.unwrap_or(default)` only on intentionally-defaultable fields.
5. **Byte-pinned topics.** Four `#[test]` snapshot tests verify that
   each `event_admin_*` function still produces exactly the documented
   bytes — accidental renames will fail these tests loudly.

## Test inventory

`contracts/admin/src/test.rs` covers:

- 2 view behaviour tests
- 2 init tests (happy path + double-init panic + event shape)
- 4 set_admin tests (auth, unauthorized, event shape, state-delta)
- 4 accept_admin tests (auth, wrong-caller, no-pending, event shape)
- 4 cancel_admin_transfer tests (auth, unauthorized, no-pending,
  event shape)
- 3 lifecycle integration tests (full rotation, cancel-between,
  repeated rotations)
- 3 negative-path "no events emitted" tests
- 3 event-log completeness tests (init emits exactly 1, set_admin adds
  exactly 1, full rotation emits exactly 3 total)

Plus 4 byte-identity snapshot tests in `events.rs` inline `#[cfg(test)]`
mod. Total: **30 tests**, all expected to pass on
`cargo test -p callora-admin`.

## Cross-contract integration

Future work (not in this PR):

- Refactor `revenue_pool`, `settlement`, `checkpoint`, `hot`, `limits`,
  `errors`, `vault` admin modules to re-use the topic symbols from
  `callora_admin::events` so the audit trail becomes homogeneous
  workspace-wide.
- This issue (#832) only adds the canonical source; the refactor is a
  separate change so it can be reviewed independently.
