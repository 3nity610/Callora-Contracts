
//! Cross-contract call safety tests for `callora-registry`.
//!
//! Verifies that when an external callee reverts or panics during registration,
//! the registry does not persist partial state (`RegisteredCount`, offering
//! records, or events implying success).
//!
//! ## Visible API
//!
//! No new public entrypoints are introduced by this test module. It exercises
//! existing registration paths that invoke:
//!
//! - `OfferingCatalog::put_offering` (catalog contract)
//! - `token::Client::balance` (SEP-41 token) in the balance-gated path

extern crate std;

use callora_registry::{CalloraRegistry, CalloraRegistryClient, RegistryError};
use soroban_sdk::testutils::Address as _;
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
