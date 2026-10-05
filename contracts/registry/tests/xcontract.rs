//! Cross-contract call safety tests for `callora-registry`.
//!
//! Verifies that when an external callee reverts or panics during registration
//! or during a lifecycle operation (metadata update, developer transfer,
//! deregistration), the registry does not persist partial state
//! (`RegisteredCount`, offering records, or events implying success).
//!
//! ## Visible API
//!
//! The lifecycle suite at the end of this module covers the entrypoints added
//! for offering updates — `CalloraRegistry::update_offering_metadata`,
//! `transfer_offering` and `deregister_offering` — including their
//! authorization and catalog-failure paths. Together with the registration
//! tests below they invoke:
//!
//! - `OfferingCatalog::put_offering` (catalog contract)
//! - `OfferingCatalog::remove_offering` (catalog contract, deregistration)
//! - `token::Client::balance` (SEP-41 token) in the balance-gated path

extern crate std;

use callora_registry::{admin, CalloraRegistry, CalloraRegistryClient, RegistryError};
use soroban_sdk::testutils::Address as _;
use soroban_sdk::testutils::{Ledger, LedgerInfo};
use soroban_sdk::token;
use soroban_sdk::{contract, contractimpl, Address, Env, String, Symbol};

// ---------------------------------------------------------------------------
// Mock callees - each in separate modules to avoid symbol conflicts
// ---------------------------------------------------------------------------

pub mod ok_catalog {
    use super::*;

    #[contract]
    pub struct OkCatalog;

    #[contractimpl]
    impl OkCatalog {
        pub fn put_offering(
            _env: Env,
            _registry: Address,
            _offering_id: String,
            _metadata: String,
        ) {
        }
    }
}

pub mod update_catalog {
    use super::*;

    #[contract]
    pub struct UpdateCatalog;

    #[contractimpl]
    impl UpdateCatalog {
        pub fn put_offering(
            _env: Env,
            _registry: Address,
            _offering_id: String,
            _metadata: String,
        ) {
        }

        pub fn remove_offering(_env: Env, _registry: Address, _offering_id: String) {}
    }
}

/// Catalog double that records what the registry published (`put_offering`) and
/// removed (`remove_offering`), and can be told to revert on either path so
/// failure safety can be exercised on a registry that already holds a record.
///
/// Both mutating entrypoints enforce the identity contract from `catalog.rs`:
/// `registry.require_auth()` runs before anything is recorded, so only the real
/// registry contract can publish or remove an offering.
pub mod counting_catalog {
    use super::*;

    fn bump(env: &Env, key: &str) -> u32 {
        let k = Symbol::new(env, key);
        let count: u32 = env.storage().instance().get(&k).unwrap_or(0);
        env.storage().instance().set(&k, &(count + 1));
        count + 1
    }

    fn count(env: &Env, key: &str) -> u32 {
        env.storage()
            .instance()
            .get(&Symbol::new(env, key))
            .unwrap_or(0)
    }

    fn is_failing(env: &Env, key: &str) -> bool {
        env.storage()
            .instance()
            .get(&Symbol::new(env, key))
            .unwrap_or(false)
    }

    #[contract]
    pub struct CountingCatalog;

    #[contractimpl]
    impl CountingCatalog {
        pub fn put_offering(env: Env, registry: Address, offering_id: String, metadata: String) {
            registry.require_auth();
            env.storage()
                .instance()
                .set(&Symbol::new(&env, "last_published_id"), &offering_id);
            env.storage()
                .instance()
                .set(&Symbol::new(&env, "last_published_metadata"), &metadata);
            bump(&env, "puts");
            if is_failing(&env, "fail_put") {
                panic!("catalog put_offering revert");
            }
        }

        pub fn remove_offering(env: Env, registry: Address, offering_id: String) {
            registry.require_auth();
            env.storage()
                .instance()
                .set(&Symbol::new(&env, "last_removed_id"), &offering_id);
            bump(&env, "removes");
            if is_failing(&env, "fail_remove") {
                panic!("catalog remove_offering revert");
            }
        }

        /// Test hook: make every subsequent `put_offering` revert.
        pub fn fail_put(env: Env, fail: bool) {
            env.storage()
                .instance()
                .set(&Symbol::new(&env, "fail_put"), &fail);
        }

        /// Test hook: make every subsequent `remove_offering` revert.
        pub fn fail_remove(env: Env, fail: bool) {
            env.storage()
                .instance()
                .set(&Symbol::new(&env, "fail_remove"), &fail);
        }

        pub fn put_count(env: Env) -> u32 {
            count(&env, "puts")
        }

        pub fn remove_count(env: Env) -> u32 {
            count(&env, "removes")
        }

        pub fn last_published_metadata(env: Env) -> String {
            env.storage()
                .instance()
                .get(&Symbol::new(&env, "last_published_metadata"))
                .unwrap()
        }

        pub fn last_removed_id(env: Env) -> String {
            env.storage()
                .instance()
                .get(&Symbol::new(&env, "last_removed_id"))
                .unwrap()
        }
    }
}

pub mod panicking_catalog {
    use super::*;

    #[contract]
    pub struct PanickingCatalog;

    #[contractimpl]
    impl PanickingCatalog {
        pub fn put_offering(
            _env: Env,
            _registry: Address,
            _offering_id: String,
            _metadata: String,
        ) {
            panic!("catalog callee panic");
        }
    }
}

pub mod revert_catalog {
    use super::*;

    #[contract]
    pub struct RevertCatalog;

    #[contractimpl]
    impl RevertCatalog {
        pub fn put_offering(
            _env: Env,
            _registry: Address,
            _offering_id: String,
            _metadata: String,
        ) {
            panic!("catalog callee revert");
        }
    }
}

/// Catalog that enforces the cross-contract caller identity contract from
/// `catalog.rs` (issue #1060): `registry.require_auth()` must succeed before
/// anything is recorded. This is the reference implementation of what a
/// production catalog must do.
pub mod identity_catalog {
    use super::*;

    #[contract]
    pub struct IdentityCatalog;

    #[contractimpl]
    impl IdentityCatalog {
        pub fn put_offering(env: Env, registry: Address, _offering_id: String, _metadata: String) {
            // Identity check FIRST: only the real registry contract may
            // publish. A spoofing caller that passes the registry address as
            // an argument does not satisfy this check and fails closed.
            registry.require_auth();
            let key = Symbol::new(&env, "published");
            let count: u32 = env.storage().instance().get(&key).unwrap_or(0);
            env.storage().instance().set(&key, &(count + 1));
        }

        /// Number of offerings successfully published (identity-verified).
        pub fn published_count(env: Env) -> u32 {
            env.storage()
                .instance()
                .get(&Symbol::new(&env, "published"))
                .unwrap_or(0)
        }
    }
}

pub mod panicking_token {
    use super::*;

    #[contract]
    pub struct PanickingToken;

    #[contractimpl]
    impl PanickingToken {
        pub fn balance(_env: Env, _id: Address) -> i128 {
            panic!("token balance panic");
        }

        pub fn transfer(_env: Env, _from: Address, _to: Address, _amount: i128) {
            // unused stub for interface completeness in tests
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn offering_id(env: &Env, suffix: &str) -> String {
    String::from_str(env, &format!("offering-{suffix}"))
}

fn metadata(env: &Env) -> String {
    String::from_str(
        env,
        "ipfs://bafybeigdyrzt5sfp7udm7hu76uh7y26nf3efuylqabf3oclgtqy55fbzdi",
    )
}

/// Advance the ledger clock exactly `seconds` into the future. Used to clear
/// the per-developer registration cooldown between lifecycle steps.
fn advance(env: &Env, seconds: u64) {
    let current = env.ledger().get().timestamp;
    env.ledger().set(LedgerInfo {
        timestamp: current + seconds,
        ..env.ledger().get()
    });
}

fn setup_registry(env: &Env, catalog: Address) -> (Address, CalloraRegistryClient<'_>, Address) {
    env.mock_all_auths();
    let admin = Address::generate(env);
    let developer = Address::generate(env);
    let registry_id = env.register(CalloraRegistry, ());
    let client = CalloraRegistryClient::new(env, &registry_id);
    client.init(&admin, &catalog);
    (admin, client, developer)
}

// ---------------------------------------------------------------------------
// Catalog callee failure
// ---------------------------------------------------------------------------

#[test]
fn register_offering_catalog_panic_leaves_registry_clean() {
    let env = Env::default();
    let catalog = env.register(panicking_catalog::PanickingCatalog, ());
    let (admin, client, developer) = setup_registry(&env, catalog);

    let oid = offering_id(&env, "panic");
    let meta = metadata(&env);

    let result = client.try_register_offering(&admin, &developer, &oid, &meta);
    assert!(result.is_err(), "catalog panic must fail registration");

    assert_eq!(client.registered_count(), 0);
    assert!(!client.is_offering_registered(&oid));
}

#[test]
fn register_offering_catalog_revert_leaves_registry_clean() {
    let env = Env::default();
    let catalog = env.register(revert_catalog::RevertCatalog, ());
    let (admin, client, developer) = setup_registry(&env, catalog);

    let oid = offering_id(&env, "revert");
    let meta = metadata(&env);

    let result = client.try_register_offering(&admin, &developer, &oid, &meta);
    assert!(result.is_err(), "catalog revert must fail registration");

    assert_eq!(client.registered_count(), 0);
    assert!(!client.is_offering_registered(&oid));
}

#[test]
fn register_offering_success_after_healthy_catalog() {
    let env = Env::default();
    let catalog = env.register(ok_catalog::OkCatalog, ());
    let (admin, client, developer) = setup_registry(&env, catalog);

    let oid = offering_id(&env, "ok");
    let meta = metadata(&env);

    client.register_offering(&admin, &developer, &oid, &meta);

    assert_eq!(client.registered_count(), 1);
    assert!(client.is_offering_registered(&oid));
    let record = client.get_offering(&oid);
    assert_eq!(record.developer, developer);
    assert_eq!(record.metadata, meta);
}

// ---------------------------------------------------------------------------
// Token balance callee failure (balance-gated registration)
// ---------------------------------------------------------------------------

#[test]
fn balance_gate_token_panic_leaves_registry_clean() {
    let env = Env::default();
    let catalog = env.register(ok_catalog::OkCatalog, ());
    let (admin, client, developer) = setup_registry(&env, catalog);

    let token_addr = env.register(panicking_token::PanickingToken, ());

    let oid = offering_id(&env, "token-panic");
    let meta = metadata(&env);

    let result = client.try_register_offering_with_gate(
        &admin,
        &developer,
        &token_addr,
        &100i128,
        &oid,
        &meta,
    );
    assert!(
        result.is_err(),
        "token balance panic must abort registration"
    );

    assert_eq!(client.registered_count(), 0);
    assert!(!client.is_offering_registered(&oid));
}

#[test]
fn balance_gate_catalog_panic_after_balance_read_leaves_registry_clean() {
    let env = Env::default();
    let catalog = env.register(panicking_catalog::PanickingCatalog, ());
    let (admin, client, developer) = setup_registry(&env, catalog);

    let owner = Address::generate(&env);
    let sac = env.register_stellar_asset_contract_v2(owner.clone());
    let token_addr = sac.address();
    let token_admin = token::StellarAssetClient::new(&env, &token_addr);
    token_admin.mint(&developer, &1_000);

    let oid = offering_id(&env, "gate-panic");
    let meta = metadata(&env);

    let result = client.try_register_offering_with_gate(
        &admin,
        &developer,
        &token_addr,
        &100i128,
        &oid,
        &meta,
    );
    assert!(
        result.is_err(),
        "catalog panic must fail gated registration"
    );

    assert_eq!(client.registered_count(), 0);
    assert!(!client.is_offering_registered(&oid));
}

#[test]
fn balance_gate_success_commits_registry_state() {
    let env = Env::default();
    let catalog = env.register(ok_catalog::OkCatalog, ());
    let (admin, client, developer) = setup_registry(&env, catalog);

    let owner = Address::generate(&env);
    let sac = env.register_stellar_asset_contract_v2(owner);
    let token_addr = sac.address();
    let token_admin = token::StellarAssetClient::new(&env, &token_addr);
    token_admin.mint(&developer, &500);

    let oid = offering_id(&env, "gate-ok");
    let meta = metadata(&env);

    client.register_offering_with_gate(&admin, &developer, &token_addr, &100i128, &oid, &meta);

    assert_eq!(client.registered_count(), 1);
    assert!(client.is_offering_registered(&oid));
}

#[test]
fn balance_gate_insufficient_balance_does_not_call_catalog() {
    let env = Env::default();
    let catalog = env.register(panicking_catalog::PanickingCatalog, ());
    let (admin, client, developer) = setup_registry(&env, catalog);

    let owner = Address::generate(&env);
    let sac = env.register_stellar_asset_contract_v2(owner);
    let token_addr = sac.address();

    let oid = offering_id(&env, "low-balance");
    let meta = metadata(&env);

    let result = client.try_register_offering_with_gate(
        &admin,
        &developer,
        &token_addr,
        &100i128,
        &oid,
        &meta,
    );
    assert!(
        matches!(result, Err(Ok(RegistryError::InsufficientDeveloperBalance))),
        "expected InsufficientDeveloperBalance, got {:?}",
        result
    );

    assert_eq!(client.registered_count(), 0);
    assert!(!client.is_offering_registered(&oid));
}

// ---------------------------------------------------------------------------
// Cross-contract caller identity (issue #1060)
// ---------------------------------------------------------------------------
//
// The catalog is a trust boundary: it must only accept `put_offering` calls
// that genuinely originate from the registry contract. `registry.require_auth()`
// is the identity check — it succeeds only when the immediate caller is the
// registry (the host authorizes the contract caller) and fails closed for any
// other caller, even one that passes the registry's address as an argument.

/// A spoofing caller (here, the test acting as an external account) that
/// passes the real registry's address as `registry` must be rejected by an
/// identity-enforcing catalog, and nothing may be recorded.
#[test]
fn identity_enforcing_catalog_rejects_spoofed_registry() {
    let env = Env::default();
    let catalog = env.register(identity_catalog::IdentityCatalog, ());
    let registry_addr = env.register(CalloraRegistry, ());
    let catalog_client = identity_catalog::IdentityCatalogClient::new(&env, &catalog);

    let oid = offering_id(&env, "spoof");
    let meta = metadata(&env);

    // Deliberately no `mock_all_auths()`: an external caller invoking the
    // catalog directly with the registry's address is NOT the registry.
    let result = catalog_client.try_put_offering(&registry_addr, &oid, &meta);
    assert!(
        result.is_err(),
        "catalog must reject a caller that is not the registry contract"
    );
    // Fail closed: nothing was recorded at the boundary.
    assert_eq!(catalog_client.published_count(), 0);
}

/// The real registry can still publish through an identity-enforcing catalog:
/// its own cross-contract call satisfies `registry.require_auth()`.
#[test]
fn registry_publishes_through_identity_enforcing_catalog() {
    let env = Env::default();
    let catalog_id = env.register(identity_catalog::IdentityCatalog, ());
    let (admin, client, developer) = setup_registry(&env, catalog_id.clone());

    let oid = offering_id(&env, "real");
    let meta = metadata(&env);

    client.register_offering(&admin, &developer, &oid, &meta);

    assert_eq!(client.registered_count(), 1);
    assert!(client.is_offering_registered(&oid));
    // The identity-enforcing catalog recorded exactly one verified offering.
    let catalog_client = identity_catalog::IdentityCatalogClient::new(&env, &catalog_id);
    assert_eq!(catalog_client.published_count(), 1);
}

// ---------------------------------------------------------------------------
// Update / transfer / deregister (issue: support offering updates)
// ---------------------------------------------------------------------------

#[test]
fn update_offering_metadata_revalidates_and_emits() {
    let env = Env::default();
    let catalog = env.register(update_catalog::UpdateCatalog, ());
    let (admin, client, developer) = setup_registry(&env, catalog);

    let oid = offering_id(&env, "upd");
    let meta = metadata(&env);
    client.register_offering(&admin, &developer, &oid, &meta);

    let new_meta = String::from_str(&env, "ipfs://newcid");
    client.update_offering_metadata(&admin, &oid, &new_meta);

    let record = client.get_offering(&oid);
    assert_eq!(record.metadata, new_meta);
    assert_eq!(client.registered_count(), 1);
}

#[test]
fn update_offering_metadata_unknown_id_returns_not_found() {
    let env = Env::default();
    let catalog = env.register(update_catalog::UpdateCatalog, ());
    let (admin, client, _developer) = setup_registry(&env, catalog);

    let oid = offering_id(&env, "missing");
    let new_meta = metadata(&env);
    let result = client.try_update_offering_metadata(&admin, &oid, &new_meta);
    assert!(
        matches!(result, Err(Ok(RegistryError::OfferingNotFound))),
        "expected OfferingNotFound, got {:?}",
        result
    );
}

#[test]
fn transfer_offering_changes_developer() {
    let env = Env::default();
    let catalog = env.register(update_catalog::UpdateCatalog, ());
    let (admin, client, developer) = setup_registry(&env, catalog);

    let oid = offering_id(&env, "xfer");
    let meta = metadata(&env);
    client.register_offering(&admin, &developer, &oid, &meta);

    let new_dev = Address::generate(&env);
    client.transfer_offering(&admin, &oid, &new_dev);

    let record = client.get_offering(&oid);
    assert_eq!(record.developer, new_dev);
}

#[test]
fn transfer_offering_unknown_id_returns_not_found() {
    let env = Env::default();
    let catalog = env.register(update_catalog::UpdateCatalog, ());
    let (admin, client, _developer) = setup_registry(&env, catalog);

    let oid = offering_id(&env, "xfer-missing");
    let new_dev = Address::generate(&env);
    let result = client.try_transfer_offering(&admin, &oid, &new_dev);
    assert!(
        matches!(result, Err(Ok(RegistryError::OfferingNotFound))),
        "expected OfferingNotFound, got {:?}",
        result
    );
}

#[test]
fn deregister_offering_removes_record_and_decrements_count() {
    let env = Env::default();
    let catalog = env.register(update_catalog::UpdateCatalog, ());
    let (admin, client, developer) = setup_registry(&env, catalog);

    let oid = offering_id(&env, "del");
    let meta = metadata(&env);
    client.register_offering(&admin, &developer, &oid, &meta);
    assert_eq!(client.registered_count(), 1);

    client.deregister_offering(&admin, &oid);

    assert_eq!(client.registered_count(), 0);
    assert!(!client.is_offering_registered(&oid));
}

#[test]
fn deregister_offering_unknown_id_returns_not_found() {
    let env = Env::default();
    let catalog = env.register(update_catalog::UpdateCatalog, ());
    let (admin, client, _developer) = setup_registry(&env, catalog);

    let oid = offering_id(&env, "del-missing");
    let result = client.try_deregister_offering(&admin, &oid);
    assert!(
        matches!(result, Err(Ok(RegistryError::OfferingNotFound))),
        "expected OfferingNotFound, got {:?}",
        result
    );
}

// ---------------------------------------------------------------------------
// Update / transfer / deregister: authorization, re-validation, catalog
// propagation, and failure safety
// ---------------------------------------------------------------------------

/// A caller that is neither the admin nor the offering's developer cannot
/// rewrite metadata, and the attempt is rejected before the catalog is
/// consulted.
#[test]
fn update_offering_metadata_rejects_stranger() {
    let env = Env::default();
    let catalog = env.register(counting_catalog::CountingCatalog, ());
    let (admin, client, developer) = setup_registry(&env, catalog.clone());
    let catalog_client = counting_catalog::CountingCatalogClient::new(&env, &catalog);

    let oid = offering_id(&env, "upd-stranger");
    let meta = metadata(&env);
    client.register_offering(&admin, &developer, &oid, &meta);

    let stranger = Address::generate(&env);
    let new_meta = String::from_str(&env, "ipfs://stranger");
    let result = client.try_update_offering_metadata(&stranger, &oid, &new_meta);
    assert!(
        matches!(result, Err(Ok(RegistryError::Unauthorized))),
        "expected Unauthorized, got {:?}",
        result
    );

    assert_eq!(client.get_offering(&oid).metadata, meta);
    assert_eq!(catalog_client.put_count(), 1);
}

/// The offering's own developer may amend its metadata without an admin, and
/// the new metadata is re-published to the catalog. `RegisteredCount` is
/// unchanged: the offering was already counted.
#[test]
fn update_offering_metadata_developer_can_update_own_offering() {
    let env = Env::default();
    let catalog = env.register(counting_catalog::CountingCatalog, ());
    let (admin, client, developer) = setup_registry(&env, catalog.clone());
    let catalog_client = counting_catalog::CountingCatalogClient::new(&env, &catalog);

    let oid = offering_id(&env, "upd-owner");
    let meta = metadata(&env);
    client.register_offering(&admin, &developer, &oid, &meta);

    let new_meta = String::from_str(&env, "ipfs://owner-update");
    client.update_offering_metadata(&developer, &oid, &new_meta);

    assert_eq!(client.get_offering(&oid).metadata, new_meta);
    assert_eq!(client.registered_count(), 1);
    // Re-published: the second `put_offering` carries the new metadata.
    assert_eq!(catalog_client.put_count(), 2);
    assert_eq!(catalog_client.last_published_metadata(), new_meta);
}

/// Invalid replacement metadata is rejected by the same validator registration
/// uses, before any catalog call or storage write.
#[test]
fn update_offering_metadata_rejects_invalid_metadata_before_catalog() {
    let env = Env::default();
    let catalog = env.register(counting_catalog::CountingCatalog, ());
    let (admin, client, developer) = setup_registry(&env, catalog.clone());
    let catalog_client = counting_catalog::CountingCatalogClient::new(&env, &catalog);

    let oid = offering_id(&env, "upd-invalid");
    let meta = metadata(&env);
    client.register_offering(&admin, &developer, &oid, &meta);

    let empty = String::from_str(&env, "");
    let result = client.try_update_offering_metadata(&admin, &oid, &empty);
    assert!(
        matches!(result, Err(Ok(RegistryError::InvalidMetadata))),
        "expected InvalidMetadata, got {:?}",
        result
    );

    assert_eq!(client.get_offering(&oid).metadata, meta);
    assert_eq!(catalog_client.put_count(), 1);
}

/// If the catalog reverts while re-publishing, the stored record keeps the
/// previous metadata: the write happens only after the callee succeeds.
#[test]
fn update_offering_metadata_catalog_failure_keeps_previous_metadata() {
    let env = Env::default();
    let catalog = env.register(counting_catalog::CountingCatalog, ());
    let (admin, client, developer) = setup_registry(&env, catalog.clone());
    let catalog_client = counting_catalog::CountingCatalogClient::new(&env, &catalog);

    let oid = offering_id(&env, "upd-panic");
    let meta = metadata(&env);
    client.register_offering(&admin, &developer, &oid, &meta);

    catalog_client.fail_put(&true);

    let new_meta = String::from_str(&env, "ipfs://never-stored");
    let result = client.try_update_offering_metadata(&admin, &oid, &new_meta);
    assert!(result.is_err(), "a reverting catalog must fail the update");

    assert_eq!(client.get_offering(&oid).metadata, meta);
    assert_eq!(client.registered_count(), 1);
    // The failed publish was rolled back with the call.
    assert_eq!(catalog_client.put_count(), 1);
}

/// Only the admin may move an offering to another developer.
#[test]
fn transfer_offering_rejects_non_admin() {
    let env = Env::default();
    let catalog = env.register(counting_catalog::CountingCatalog, ());
    let (admin, client, developer) = setup_registry(&env, catalog);

    let oid = offering_id(&env, "xfer-nonadmin");
    let meta = metadata(&env);
    client.register_offering(&admin, &developer, &oid, &meta);

    // The current developer cannot hand the offering on, and neither can a
    // third party.
    let new_dev = Address::generate(&env);
    for caller in [developer.clone(), Address::generate(&env)] {
        let result = client.try_transfer_offering(&caller, &oid, &new_dev);
        assert!(
            matches!(result, Err(Ok(RegistryError::Unauthorized))),
            "expected Unauthorized, got {:?}",
            result
        );
        assert_eq!(client.get_offering(&oid).developer, developer);
    }
}

/// A transfer to the address that already owns the offering is a no-op and is
/// rejected as an invalid developer.
#[test]
fn transfer_offering_rejects_current_developer() {
    let env = Env::default();
    let catalog = env.register(counting_catalog::CountingCatalog, ());
    let (admin, client, developer) = setup_registry(&env, catalog);

    let oid = offering_id(&env, "xfer-same");
    let meta = metadata(&env);
    client.register_offering(&admin, &developer, &oid, &meta);

    let result = client.try_transfer_offering(&admin, &oid, &developer);
    assert!(
        matches!(result, Err(Ok(RegistryError::InvalidDeveloper))),
        "expected InvalidDeveloper, got {:?}",
        result
    );
    assert_eq!(client.get_offering(&oid).developer, developer);
}

/// The registry contract itself cannot become an offering's developer.
#[test]
fn transfer_offering_rejects_registry_as_developer() {
    let env = Env::default();
    let catalog = env.register(counting_catalog::CountingCatalog, ());
    let (admin, client, developer) = setup_registry(&env, catalog);

    let oid = offering_id(&env, "xfer-registry");
    let meta = metadata(&env);
    client.register_offering(&admin, &developer, &oid, &meta);

    let registry_addr = client.address.clone();
    let result = client.try_transfer_offering(&admin, &oid, &registry_addr);
    assert!(
        matches!(result, Err(Ok(RegistryError::InvalidDeveloper))),
        "expected InvalidDeveloper, got {:?}",
        result
    );
    assert_eq!(client.get_offering(&oid).developer, developer);
}

/// A successful transfer re-anchors the (unchanged) catalog entry and leaves
/// `RegisteredCount` alone.
#[test]
fn transfer_offering_republishes_to_catalog() {
    let env = Env::default();
    let catalog = env.register(counting_catalog::CountingCatalog, ());
    let (admin, client, developer) = setup_registry(&env, catalog.clone());
    let catalog_client = counting_catalog::CountingCatalogClient::new(&env, &catalog);

    let oid = offering_id(&env, "xfer-ok");
    let meta = metadata(&env);
    client.register_offering(&admin, &developer, &oid, &meta);

    let new_dev = Address::generate(&env);
    client.transfer_offering(&admin, &oid, &new_dev);

    let record = client.get_offering(&oid);
    assert_eq!(record.developer, new_dev);
    assert_eq!(record.metadata, meta);
    assert_eq!(client.registered_count(), 1);
    assert_eq!(catalog_client.put_count(), 2);
    assert_eq!(catalog_client.last_published_metadata(), meta);
}

/// Deregistration notifies the catalog with the offering id, decrements
/// `RegisteredCount`, and frees the id for a future registration.
#[test]
fn deregister_offering_notifies_catalog_and_frees_id() {
    let env = Env::default();
    let catalog = env.register(counting_catalog::CountingCatalog, ());
    let (admin, client, developer) = setup_registry(&env, catalog.clone());
    let catalog_client = counting_catalog::CountingCatalogClient::new(&env, &catalog);

    let oid = offering_id(&env, "del-ok");
    let meta = metadata(&env);
    client.register_offering(&admin, &developer, &oid, &meta);

    client.deregister_offering(&admin, &oid);

    assert_eq!(catalog_client.remove_count(), 1);
    assert_eq!(catalog_client.last_removed_id(), oid);
    assert_eq!(client.registered_count(), 0);
    assert!(!client.is_offering_registered(&oid));

    // The id is usable again once deregistered; the next registration by this
    // developer must still clear the per-developer registration cooldown.
    advance(&env, admin::COOLDOWN_SECONDS);
    client.register_offering(&admin, &developer, &oid, &meta);
    assert_eq!(client.registered_count(), 1);
    assert!(client.is_offering_registered(&oid));
}

/// Only the admin may deregister an offering.
#[test]
fn deregister_offering_rejects_non_admin() {
    let env = Env::default();
    let catalog = env.register(counting_catalog::CountingCatalog, ());
    let (admin, client, developer) = setup_registry(&env, catalog.clone());
    let catalog_client = counting_catalog::CountingCatalogClient::new(&env, &catalog);

    let oid = offering_id(&env, "del-nonadmin");
    let meta = metadata(&env);
    client.register_offering(&admin, &developer, &oid, &meta);

    let stranger = Address::generate(&env);
    let result = client.try_deregister_offering(&stranger, &oid);
    assert!(
        matches!(result, Err(Ok(RegistryError::Unauthorized))),
        "expected Unauthorized, got {:?}",
        result
    );

    assert_eq!(client.registered_count(), 1);
    assert!(client.is_offering_registered(&oid));
    assert_eq!(catalog_client.remove_count(), 0);
}

/// If the catalog reverts while being notified, the record survives and the
/// count is not decremented.
#[test]
fn deregister_offering_catalog_failure_keeps_record_and_count() {
    let env = Env::default();
    let catalog = env.register(counting_catalog::CountingCatalog, ());
    let (admin, client, developer) = setup_registry(&env, catalog.clone());
    let catalog_client = counting_catalog::CountingCatalogClient::new(&env, &catalog);

    let oid = offering_id(&env, "del-panic");
    let meta = metadata(&env);
    client.register_offering(&admin, &developer, &oid, &meta);

    catalog_client.fail_remove(&true);

    let result = client.try_deregister_offering(&admin, &oid);
    assert!(
        result.is_err(),
        "a reverting catalog must fail the deregistration"
    );

    assert_eq!(client.registered_count(), 1);
    assert!(client.is_offering_registered(&oid));
    assert_eq!(client.get_offering(&oid).metadata, meta);
    assert_eq!(catalog_client.remove_count(), 0);
}
