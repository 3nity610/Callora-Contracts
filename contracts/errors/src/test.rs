#![cfg(test)]

use super::*;
use soroban_sdk::testutils::storage::Persistent;
use soroban_sdk::testutils::{Address as _, Events as _, Ledger as _};
use soroban_sdk::{Address, Env, IntoVal, String, Symbol};

/// Init a fresh contract and return the client + admin.
fn setup(env: &Env) -> (ErrorsContractClient<'_>, Address) {
    env.mock_all_auths();
    let contract_id = env.register_contract(None, ErrorsContract);
    let client = ErrorsContractClient::new(env, &contract_id);
    let admin = Address::generate(env);
    client.init(&admin);
    (client, admin)
}

#[test]
fn test_init_and_register() {
    let env = Env::default();
    let (client, admin) = setup(&env);

    assert_eq!(
        client.try_init(&admin).unwrap_err().unwrap(),
        Error::AlreadyInitialized
    );

    let desc = String::from_str(&env, "Insufficient Balance");
    assert_eq!(client.register_error(&admin, &101, &desc), ());
    assert_eq!(client.get_error_description(&101), Some(desc.clone()));
}

#[test]
fn test_unauthorized_registration() {
    let env = Env::default();
    let (client, _admin) = setup(&env);
    let fake_admin = Address::generate(&env);

    let desc = String::from_str(&env, "Unauthorized Action");
    assert_eq!(
        client
            .try_register_error(&fake_admin, &102, &desc)
            .unwrap_err()
            .unwrap(),
        Error::Unauthorized
    );

    // A rejected registration must not store anything.
    assert_eq!(client.get_error_description(&102), None);
}

#[test]
fn test_log_error_rejects_unknown_code() {
    let env = Env::default();
    let (client, _admin) = setup(&env);
    let user = Address::generate(&env);

    // 999 was never registered → typed error, nothing recorded.
    assert_eq!(
        client.try_log_error(&user, &999).unwrap_err().unwrap(),
        Error::UnknownErrorCode
    );
    assert_eq!(client.get_recent_error(&user), None);
}

#[test]
fn test_log_error_after_registration() {
    let env = Env::default();
    let (client, admin) = setup(&env);
    let user = Address::generate(&env);

    let desc = String::from_str(&env, "Insufficient Balance");
    client.register_error(&admin, &101, &desc);

    assert_eq!(client.log_error(&user, &101), ());
    assert_eq!(client.get_recent_error(&user), Some(101));
}

#[test]
fn test_get_error_description_returns_stored_data() {
    let env = Env::default();
    let (client, admin) = setup(&env);

    let desc = String::from_str(&env, "Rate Limited");
    client.register_error(&admin, &429, &desc);

    assert_eq!(client.get_error_description(&429), Some(desc.clone()));
    assert_eq!(client.get_error_description(&430), None);
}

#[test]
fn test_log_error_emits_event() {
    let env = Env::default();
    let (client, admin) = setup(&env);
    let user = Address::generate(&env);

    let desc = String::from_str(&env, "Insufficient Balance");
    client.register_error(&admin, &101, &desc);

    client.log_error(&user, &101);

    let events = env.events().all();
    let event = events.last().unwrap();

    let topics = &event.1;
    assert_eq!(topics.len(), 2);
    let topic0: Symbol = topics.get(0).unwrap().into_val(&env);
    let topic1: Address = topics.get(1).unwrap().into_val(&env);
    assert_eq!(topic0, Symbol::new(&env, "error_logged"));
    assert_eq!(topic1, user);

    let data: u32 = event.2.into_val(&env);
    assert_eq!(data, 101);
}

#[test]
fn test_registry_lookup_extends_ttl() {
    let env = Env::default();
    let (client, admin) = setup(&env);

    let desc = String::from_str(&env, "Insufficient Balance");
    client.register_error(&admin, &101, &desc);
    let key = DataKey::ErrorReg(101);

    // Advance the ledger until the registry entry's remaining TTL has dropped
    // below the refresh threshold, but the entry has not yet expired.
    env.ledger()
        .set_sequence_number(REGISTRY_TTL_BUMP - REGISTRY_TTL_THRESHOLD + 1);

    let ttl_before = env.as_contract(&client.address, || env.storage().persistent().get_ttl(&key));
    assert!(
        ttl_before < REGISTRY_TTL_THRESHOLD,
        "sanity: TTL should be below the bump threshold before the read"
    );

    // The read-through view must refresh the entry.
    assert_eq!(client.get_error_description(&101), Some(desc.clone()));

    let ttl_after = env.as_contract(&client.address, || env.storage().persistent().get_ttl(&key));
    assert_eq!(
        ttl_after, REGISTRY_TTL_BUMP,
        "get_error_description must extend the registry entry's TTL"
    );
}

#[test]
fn test_log_error_extends_registry_ttl() {
    let env = Env::default();
    let (client, admin) = setup(&env);
    let user = Address::generate(&env);

    let desc = String::from_str(&env, "Insufficient Balance");
    client.register_error(&admin, &101, &desc);
    let key = DataKey::ErrorReg(101);

    env.ledger()
        .set_sequence_number(REGISTRY_TTL_BUMP - REGISTRY_TTL_THRESHOLD + 1);
    let ttl_before = env.as_contract(&client.address, || env.storage().persistent().get_ttl(&key));
    assert!(ttl_before < REGISTRY_TTL_THRESHOLD);

    client.log_error(&user, &101);

    let ttl_after = env.as_contract(&client.address, || env.storage().persistent().get_ttl(&key));
    assert_eq!(
        ttl_after, REGISTRY_TTL_BUMP,
        "log_error must extend the registry entry's TTL for a live code"
    );
}

#[test]
fn test_ttl_constants() {
    assert_eq!(LEDGERS_PER_DAY, 17_280);
    assert_eq!(REGISTRY_TTL_THRESHOLD, LEDGERS_PER_DAY * 30);
    assert_eq!(REGISTRY_TTL_BUMP, LEDGERS_PER_DAY * 60);
}

#[test]
fn test_overflow_protection() {
    let env = Env::default();
    let (client, admin) = setup(&env);
    let user = Address::generate(&env);

    // Registering u32::MAX makes the code valid, so the arithmetic guard is
    // what rejects it.
    let desc = String::from_str(&env, "Max Code");
    client.register_error(&admin, &u32::MAX, &desc);

    assert_eq!(
        client.try_log_error(&user, &u32::MAX).unwrap_err().unwrap(),
        Error::Overflow
    );
}

#[test]
fn test_overflow_boundary_safe() {
    let env = Env::default();
    let (client, admin) = setup(&env);
    let user = Address::generate(&env);

    // u32::MAX - 1 is the largest code that does NOT overflow when adding 1.
    let desc = String::from_str(&env, "Boundary");
    client.register_error(&admin, &(u32::MAX - 1), &desc);

    assert_eq!(client.log_error(&user, &(u32::MAX - 1)), ());
}

#[test]
fn test_overflow_zero_code() {
    let env = Env::default();
    let (client, admin) = setup(&env);
    let user = Address::generate(&env);

    // code = 0: checked_add(1) -> Some(1), no overflow.
    let desc = String::from_str(&env, "Zero");
    client.register_error(&admin, &0u32, &desc);

    assert_eq!(client.log_error(&user, &0u32), ());
}

#[test]
fn test_overflow_edge_round_trip() {
    let env = Env::default();
    let (client, admin) = setup(&env);
    let user = Address::generate(&env);

    let desc = String::from_str(&env, "Boundary");
    client.register_error(&admin, &(u32::MAX - 1), &desc);
    client.register_error(&admin, &u32::MAX, &desc);

    // Boundary: code at max - 1 succeeds, max fails, then max - 1 still succeeds.
    assert_eq!(client.log_error(&user, &(u32::MAX - 1)), ());
    assert_eq!(
        client.try_log_error(&user, &u32::MAX).unwrap_err().unwrap(),
        Error::Overflow
    );
    assert_eq!(client.log_error(&user, &(u32::MAX - 1)), ());
}

#[test]
fn test_overflow_multiple_users_safe() {
    let env = Env::default();
    let (client, admin) = setup(&env);

    // Multiple users logging at different (registered) code values — all safe.
    for i in 0..10u32 {
        let code = u32::MAX - 1 - i;
        let desc = String::from_str(&env, "Range");
        client.register_error(&admin, &code, &desc);

        let user = Address::generate(&env);
        assert_eq!(client.log_error(&user, &code), ());
    }
}
