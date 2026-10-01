//! Adversarial authorization regression tests for the upgrade entry points
//! (`set_cooldown`, `check_and_record_upgrade`) — issue #1058.
//!
//! Every upgrade entry point must fail closed when the caller has not
//! authorized the invocation — even when every other precondition is
//! satisfied. These tests would fail if a future change removed
//! `caller.require_auth()` from either entry point, or allowed a failed call
//! to mutate state.

extern crate std;

use callora_upgrade::admin::DEFAULT_COOLDOWN_SECONDS;
use callora_upgrade::errors::UpgradeError;
use soroban_sdk::testutils::Address as _;
use soroban_sdk::{contract, contractimpl, Address, Env, Symbol};

// ---------------------------------------------------------------------------
// Thin harness contract that delegates to the `admin` upgrade module so the
// module's entry points are exercised through the real Soroban XCF layer.
// ---------------------------------------------------------------------------

#[contract]
pub struct UpgradeHarness;

#[contractimpl]
impl UpgradeHarness {
    pub fn set_cooldown(env: Env, caller: Address, cooldown: u64) {
        callora_upgrade::admin::set_cooldown(&env, &caller, cooldown);
    }

    pub fn get_cooldown(env: Env) -> u64 {
        callora_upgrade::admin::get_cooldown(&env)
    }

    pub fn check_and_record_upgrade(env: Env, caller: Address) -> Result<(), UpgradeError> {
        callora_upgrade::admin::check_and_record_upgrade(&env, &caller)
    }

    pub fn get_last_upgrade_time(env: Env) -> Option<u64> {
        env.storage()
            .instance()
            .get(&Symbol::new(&env, "last_upg_tm"))
    }
}

fn harness(env: &Env) -> (UpgradeHarnessClient<'_>, Address) {
    let id = env.register(UpgradeHarness, ());
    let client = UpgradeHarnessClient::new(env, &id);
    let caller = Address::generate(env);
    (client, caller)
}

/// `set_cooldown` without caller authorization must fail and leave the cooldown
/// window unchanged (fail closed, no partial state).
#[test]
fn set_cooldown_requires_auth() {
    let env = Env::default();
    // Deliberately no `mock_all_auths()` — the caller does not sign.
    let (client, caller) = harness(&env);

    let result = client.try_set_cooldown(&caller, &1_234);
    assert!(
        result.is_err(),
        "set_cooldown must reject an unauthenticated caller"
    );
    // No partial state: the default cooldown is still in effect.
    assert_eq!(client.get_cooldown(), DEFAULT_COOLDOWN_SECONDS);
}

/// `check_and_record_upgrade` without caller authorization must fail and must
/// not record a last-upgrade timestamp (no partial state).
#[test]
fn check_and_record_upgrade_requires_auth() {
    let env = Env::default();
    // Deliberately no `mock_all_auths()`.
    let (client, caller) = harness(&env);

    let result = client.try_check_and_record_upgrade(&caller);
    assert!(
        result.is_err(),
        "check_and_record_upgrade must reject an unauthenticated caller"
    );
    // No partial state: no upgrade record was persisted.
    assert!(client.get_last_upgrade_time().is_none());
}

/// A non-admin caller (with auth but not the configured admin) is rejected by
/// the cooldown guard only after authentication succeeds; the auth check still
/// runs first. `set_cooldown`/`check_and_record_upgrade` are generic helpers
/// (no admin concept), so the binding authorization requirement is the caller
/// signature itself — verified above and here for a second caller identity.
#[test]
fn second_caller_still_requires_its_own_auth() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, caller) = harness(&env);
    let other = Address::generate(&env);

    // Authorized caller works.
    client.set_cooldown(&caller, &3_600);
    assert_eq!(client.get_cooldown(), 3_600);
    client.check_and_record_upgrade(&caller); // panics (fails closed) if unauthorized

    // Without auth, even a different identity is rejected (fail closed).
    env.set_auths(&[]);
    assert!(client.try_check_and_record_upgrade(&other).is_err());
}
