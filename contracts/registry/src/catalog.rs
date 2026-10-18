//! External catalog interface invoked by the registry during offering registration.
//!
/// Production deployments point `init 's `catalog` address at a contract that
/// persists offering metadata (for example an on-chain catalog or vault hook).
/// Integration tests swap in mock callees that revert or panic to exercise
/// cross-contract failure safety.
///
/// # Cross-contract trust boundary (issue #1060)
///
/// `registry` is the address of the contract performing the cross-contract
/// call. A catalog implementation **must** treat this as a caller-supplied
/// identity and enforce it before recording anything: the first statement of
/// `put_offering` must be `registry.require_auth()`. That check succeeds only
/// when the invocation actually originates from the registry contract (the
/// host authorizes the immediate contract caller), and fails closed for any
/// other caller — including a spoofing contract that passes the registry's
/// address as an argument. Without this check any contract could call the
/// catalog directly with an arbitrary `registry` value and inject unverified
/// offering metadata at a trust boundary.
///
/// See `contracts/registry/tests/xcontract.rs` (`identity_catalog`) for a
/// reference implementation and adversarial coverage of this boundary.

use soroban_sdk::{contractclient, Address, Env, String};

/// Cross-contract callee surface used by [`crate::CalloraRegistry::register_offering`].
#[contractclient(name = "OfferingCatalogClient")]
pub trait OfferingCatalog {
    /// Publish or anchor `metadata` for `offering_id` on behalf of `registry`.
    ///
    /// # Security requirement
    /// Implementations MUST call `registry.require_auth()` before any state
    /// mutation or event, so only the real registry contract can publish.
    fn put_offering(env: Env, registry: Address, offering_id: String, metadata: String);

    /// Update the metadata of an existing offering on behalf of `registry`.
    ///
    /// # Security requirement
    /// Implementations MUST call `registry.require_auth()` before any state
    /// mutation or event, so only the real registry contract can update.
    fn update_offering(env: Env, registry: Address, offering_id: String, metadata: String);

    /// Reassign an offering to `new_developer` on behalf of `registry`.
    ///
    /// # Security requirement
    /// Implementations MUST call `registry.require_auth()` before any state
    /// mutation or event, so only the real registry contract can transfer.
    fn transfer_offering(env: Env, registry: Address, offering_id: String, new_developer: Address);

    /// Remove an offering from the catalog on behalf of `registry`.
    ///
    /// # Security requirement
    /// Implementations MUST call `registry.require_auth()` before any state
    /// mutation or event, so only the real registry contract can remove.
    fn remove_offering(env: Env, registry: Address, offering_id: String);
}
