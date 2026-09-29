# Admin Contract Storage Documentation

This document outlines the storage keys used by the Admin contract, their storage tiers (Instance, Persistent, or Temporary), and the rationale behind each choice in accordance with Soroban best practices.

## Storage Tiers Overview

- **Instance Storage**: Stored in the contract instance ledger entry. Automatically extended when the contract instance is invoked. Ideal for small, frequently accessed configuration data and global contract state.
- **Persistent Storage**: Stored as independent ledger entries with their own Time-To-Live (TTL). Ideal for user-specific or data-heavy entries that require explicit TTL management.
- **Temporary Storage**: Short-lived entries intended for transient data (e.g., reentrancy guards, short-lived scratchpads).

---

## Storage Keys

### 1. `Admin`
- **Tier**: **Instance**
- **Data Type**: `Address`
- **Rationale**: The administrator address is a core global configuration parameter required for nearly all admin-restricted actions. It must be accessible efficiently across contract invocations and shares the lifecycle of the contract instance.

### 2. `PendingAdmin`
- **Tier**: **Instance**
- **Data Type**: `Address` (Optional / Nullable)
- **Rationale**: Used during a secure two-step admin transfer process to hold the nominated successor before they accept the role. Because this is a singleton configuration state tied directly to the contract's administration lifecycle, instance storage is appropriate.

---

## `limits.rs` Storage Keys

The per-account caps module (`src/limits.rs`) uses its own `StorageKey` enum. All three keys are **persistent**; per-account caps/counters are keyed by `Address` so they scale to many accounts and can be TTL-managed independently of the contract instance.

### 3. `Limits(Address)`
- **Source**: `src/limits.rs` (`StorageKey::Limits`)
- **Tier**: **Persistent**
- **Data Type**: `AccountLimits { max_bets: u32, max_positions: u32, max_subscriptions: u32 }`
- **Rationale**: Sparse per-account cap override. When absent, reads fall back to `DefaultLimits` (or the compile-time `DEFAULT_LIMITS`). The TTL is re-extended on every read/write using `LIMITS_BUMP_THRESHOLD` / `LIMITS_BUMP_AMOUNT`.

### 4. `Usage(Address)`
- **Source**: `src/limits.rs` (`StorageKey::Usage`)
- **Tier**: **Persistent**
- **Data Type**: `AccountUsage { bets: u32, positions: u32, subscriptions: u32 }`
- **Rationale**: Live per-account counters, mutated only by the module using `checked_add` / `checked_sub`. Persistent with the TTL re-extended on every read/write (`STATE_BUMP_THRESHOLD` / `STATE_BUMP_AMOUNT`) so active counters do not silently archive.

### 5. `DefaultLimits`
- **Source**: `src/limits.rs` (`StorageKey::DefaultLimits`)
- **Tier**: **Persistent**
- **Data Type**: `AccountLimits`
- **Rationale**: Global default caps used when an account has no `Limits` override; falls back to the compile-time `DEFAULT_LIMITS` when unset.

> **Note (issue #1248):** these keys belong to the **experimental** `limits`
> module, and their **persistent** tier differs from the standalone
> `contracts/yield` limits contract, which stores per-account cap overrides in
> **instance** storage. See `contracts/yield/YIELD_LIMITS.md`.
