//! External catalog interface invoked by the registry during offering lifecycle
//! operations (registration, metadata update, transfer, and deregistration).
//!
//! Production deployments point `init`'s `catalog` address at a contract that
//! persists offering metadata (for example an on-chain catalog or vault hook).
//! Integration tests swap in mock callees that revert or panic to exercise
//! cross-contract failure safety.
//!
//! # Cross-contract trust boundary (issue #1060)
//!
//! `registry` is the address of the contract performing the cross-contract
//! call. A catalog implementation **must** treat this as a caller-supplied
//! identity and enforce it before recording anything: the first statement of
//! `put_offering` must be `registry.require_auth()`. That check succeeds only
//! when the invocation actually originates from the registry contract (the
//! host authorizes the immediate contract caller), and fails closed for any
//! other caller — including a spoofing contract that passes the registry's
//! address as an argument. Without this check any contract could call the
//! catalog directly with an arbitrary `registry` value and inject unverified
//! offering metadata at a trust boundary.
//!
//! The same requirement applies to [`OfferingCatalog::remove_offering`]: only
//! the real registry may drop an offering from the catalog, so a spoofing
//! caller cannot make catalogue state diverge from registry state.
//!
//! See `contracts/registry/tests/xcontract.rs` (`identity_catalog`) for a
//! reference implementation and adversarial coverage of this boundary.

use soroban_sdk::{contractclient, Address, Env, String};

/// Cross-contract callee surface used by the registry entrypoints that mutate
/// an offering: [`crate::CalloraRegistry::register_offering`],
/// [`crate::CalloraRegistry::register_offering_with_gate`],
/// [`crate::CalloraRegistry::update_offering_metadata`],
/// [`crate::CalloraRegistry::transfer_offering`] and
/// [`crate::CalloraRegistry::deregister_offering`].
///
/// The catalog is keyed by `offering_id`, so a metadata update and a developer
/// transfer both re-publish the offering through [`OfferingCatalog::put_offering`]
/// (the transfer cannot change the catalogue entry's contents — the catalogue
/// does not track developers — but re-anchoring keeps the entry present and
/// current under the registry's authority), while deregistration is propagated
/// through [`OfferingCatalog::remove_offering`].
#[contractclient(name = "OfferingCatalogClient")]
pub trait OfferingCatalog {
    /// Publish or anchor `metadata` for `offering_id` on behalf of `registry`.
    ///
    /// # Security requirement
    /// Implementations MUST call `registry.require_auth()` before any state
    /// mutation or event, so only the real registry contract can publish.
    fn put_offering(env: Env, registry: Address, offering_id: String, metadata: String);

    /// Remove `offering_id` from the catalog on behalf of `registry`.
    ///
    /// Called by [`crate::CalloraRegistry::deregister_offering`] before the
    /// registry deletes its own record, so the catalogue never drops an entry
    /// that the registry still holds: if this call reverts or panics, the
    /// registry record and `RegisteredCount` remain untouched.
    ///
    /// # Security requirement
    /// Implementations MUST call `registry.require_auth()` before any state
    /// mutation or event, exactly like [`OfferingCatalog::put_offering`], so
    /// only the real registry contract can remove an offering.
    fn remove_offering(env: Env, registry: Address, offering_id: String);
}
