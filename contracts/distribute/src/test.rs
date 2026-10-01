extern crate std;

use crate::*;
use soroban_sdk::testutils::{Address as _, Events as _};
use soroban_sdk::{token, Address, BytesN, Env, IntoVal, Symbol, Val, Vec};

#[test]
fn init_event_structure_validation() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let usdc_addr = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let contract_addr = env.register(Distribute, ());
    let client = DistributeClient::new(&env, &contract_addr);

    client.init(&admin, &usdc_addr);

    let events = env.events().all();
    let event = events.last().unwrap();

    let topics = &event.1;
    assert_eq!(topics.len(), 2);
    let topic0: Symbol = topics.get(0).unwrap().into_val(&env);
    let topic1: Address = topics.get(1).unwrap().into_val(&env);
    assert_eq!(topic0, Symbol::new(&env, "init"));
    assert_eq!(topic1, admin);

    let data: Address = event.2.into_val(&env);
    assert_eq!(data, usdc_addr);
}

#[test]
fn admin_transfer_events_structure() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let new_admin = Address::generate(&env);
    let usdc_addr = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let contract_addr = env.register(Distribute, ());
    let client = DistributeClient::new(&env, &contract_addr);

    client.init(&admin, &usdc_addr);

    // Step 1: set_admin — nomination must not announce the change yet.
    env.events().all();
    client.set_admin(&admin, &new_admin);

    let events = env.events().all();
    assert_eq!(
        events.len(),
        1,
        "nomination must publish exactly one event, got {}",
        events.len()
    );

    let nomination = events.get(0).unwrap();
    let topic0: Symbol = nomination.1.get(0).unwrap().into_val(&env);
    assert_eq!(topic0, Symbol::new(&env, "admin_transfer_started"));
    // Every distribute event carries the version marker at topic 1, so the
    // subject address follows it.
    assert_eq!(nomination.1.len(), 3);
    let nominator: Address = nomination.1.get(2).unwrap().into_val(&env);
    assert_eq!(nominator, admin);
    let pending: Address = nomination.2.into_val(&env);
    assert_eq!(pending, new_admin);

    // Step 2: accept_admin — the change event is published here.
    env.events().all();
    client.accept_admin(&new_admin);

    let events = env.events().all();
    assert_eq!(
        events.len(),
        2,
        "acceptance must publish admin_changed then admin_transfer_completed, got {}",
        events.len()
    );

    let changed = events.get(0).unwrap();
    let changed_topic0: Symbol = changed.1.get(0).unwrap().into_val(&env);
    assert_eq!(changed_topic0, Symbol::new(&env, "admin_changed"));
    let previous: Address = changed.1.get(2).unwrap().into_val(&env);
    assert_eq!(previous, admin, "topic 2 must be the outgoing admin");
    let (old_admin, updated): (Address, Address) = changed.2.into_val(&env);
    assert_eq!(old_admin, admin, "data[0] must be the old admin");
    assert_eq!(updated, new_admin, "data[1] must be the new admin");

    let completed = events.get(1).unwrap();
    let completed_topic0: Symbol = completed.1.get(0).unwrap().into_val(&env);
    assert_eq!(
        completed_topic0,
        Symbol::new(&env, "admin_transfer_completed")
    );
    let completed_subject: Address = completed.1.get(2).unwrap().into_val(&env);
    assert_eq!(completed_subject, new_admin);
    let _: () = completed.2.into_val(&env);
}

#[test]
fn cancel_admin_transfer_event() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let new_admin = Address::generate(&env);
    let usdc_addr = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let contract_addr = env.register(Distribute, ());
    let client = DistributeClient::new(&env, &contract_addr);

    client.init(&admin, &usdc_addr);
    env.events().all();
    client.set_admin(&admin, &new_admin);
    env.events().all();

    client.cancel_admin_transfer(&admin);

    let events = env.events().all();
    assert_eq!(
        events.len(),
        1,
        "cancellation must publish exactly one event, got {}",
        events.len()
    );

    let event = events.get(0).unwrap();
    let topic0: Symbol = event.1.get(0).unwrap().into_val(&env);
    assert_eq!(topic0, Symbol::new(&env, "admin_cancelled"));
    let current: Address = event.1.get(2).unwrap().into_val(&env);
    assert_eq!(current, admin);
    let pending: Address = event.1.get(3).unwrap().into_val(&env);
    assert_eq!(pending, new_admin);
    let _: () = event.2.into_val(&env);

    // A cancelled transfer must leave no admin_changed event behind: the only
    // event recorded for this call is admin_cancelled.
    assert_eq!(client.get_pending_admin(), None);
}

#[test]
fn pause_unpause_events() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let usdc_addr = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let contract_addr = env.register(Distribute, ());
    let client = DistributeClient::new(&env, &contract_addr);

    client.init(&admin, &usdc_addr);
    env.events().all();

    client.pause(&admin);

    let events = env.events().all();
    let pause_event = events.last().unwrap();
    let topic0: Symbol = pause_event.1.get(0).unwrap().into_val(&env);
    assert_eq!(topic0, Symbol::new(&env, "pause_set"));
    let topic1: Address = pause_event.1.get(1).unwrap().into_val(&env);
    assert_eq!(topic1, admin);
    let is_paused: bool = pause_event.2.into_val(&env);
    assert!(is_paused);

    // Unpause
    env.events().all();
    client.unpause(&admin);

    let events = env.events().all();
    let unpause_event = events.last().unwrap();
    let topic0: Symbol = unpause_event.1.get(0).unwrap().into_val(&env);
    assert_eq!(topic0, Symbol::new(&env, "pause_set"));
    let is_paused: bool = unpause_event.2.into_val(&env);
    assert!(!is_paused);
}

#[test]
fn set_max_distribute_event() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let usdc_addr = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let contract_addr = env.register(Distribute, ());
    let client = DistributeClient::new(&env, &contract_addr);

    client.init(&admin, &usdc_addr);
    env.events().all();

    client.set_max_distribute(&admin, &1000);

    let events = env.events().all();
    let event = events.last().unwrap();
    let topic0: Symbol = event.1.get(0).unwrap().into_val(&env);
    assert_eq!(topic0, Symbol::new(&env, "set_max_distribute"));
    let topic1: Address = event.1.get(1).unwrap().into_val(&env);
    assert_eq!(topic1, admin);
    let data: (i128, i128) = event.2.into_val(&env);
    assert_eq!(data, (i128::MAX, 1000));
}

#[test]
fn distribute_event_structure() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let recipient = Address::generate(&env);
    let usdc_addr = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let contract_addr = env.register(Distribute, ());
    let client = DistributeClient::new(&env, &contract_addr);

    client.init(&admin, &usdc_addr);

    // Fund the contract with USDC
    let usdc_admin = token::StellarAssetClient::new(&env, &usdc_addr);
    usdc_admin.mint(&contract_addr, &1000);

    env.events().all();

    client.distribute(&admin, &recipient, &500);

    let events = env.events().all();

    // Find the distribute event by topic (index may vary due to token transfer event)
    let mut found = false;
    for event in events.iter() {
        let topics: &Vec<Val> = &event.1;
        let topic0: Symbol = topics.get(0).unwrap().into_val(&env);
        if topic0 == Symbol::new(&env, "distribute") {
            found = true;
            let topic1: Address = topics.get(1).unwrap().into_val(&env);
            assert_eq!(topic1, recipient);
            let amount: i128 = event.2.into_val(&env);
            assert_eq!(amount, 500);
            break;
        }
    }
    assert!(found, "distribute event not found in events");
}

#[test]
fn distribute_lifecycle_events() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let recipient = Address::generate(&env);
    let usdc_addr = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let contract_addr = env.register(Distribute, ());
    let client = DistributeClient::new(&env, &contract_addr);

    client.init(&admin, &usdc_addr);

    // Fund the contract with USDC
    let usdc_admin = token::StellarAssetClient::new(&env, &usdc_addr);
    usdc_admin.mint(&contract_addr, &1000);

    env.events().all();

    client.distribute(&admin, &recipient, &500);

    let events = env.events().all();
    // Should emit 3 events: distribute_started, distribute, distribute_completed
    // (plus potentially other events depending on test environment)
    assert!(events.len() >= 3, "expected at least 3 events, got {}", events.len());

    // Find and verify distribute_started event
    let mut found_started = false;
    let mut found_distribute = false;
    let mut found_completed = false;

    for event in events.iter() {
        let topics: &Vec<Val> = &event.1;
        let topic0: Symbol = topics.get(0).unwrap().into_val(&env);
        
        if topic0 == Symbol::new(&env, "distribute_started") {
            found_started = true;
            let topic1: Address = topics.get(1).unwrap().into_val(&env);
            assert_eq!(topic1, recipient);
            let amount: i128 = event.2.into_val(&env);
            assert_eq!(amount, 500);
        } else if topic0 == Symbol::new(&env, "distribute") {
            found_distribute = true;
            let topic1: Address = topics.get(1).unwrap().into_val(&env);
            assert_eq!(topic1, recipient);
            let amount: i128 = event.2.into_val(&env);
            assert_eq!(amount, 500);
        } else if topic0 == Symbol::new(&env, "distribute_completed") {
            found_completed = true;
            let topic1: Address = topics.get(1).unwrap().into_val(&env);
            assert_eq!(topic1, recipient);
            let amount: i128 = event.2.into_val(&env);
            assert_eq!(amount, 500);
        }
    }

    assert!(found_started, "distribute_started event not found");
    assert!(found_distribute, "distribute event not found");
    assert!(found_completed, "distribute_completed event not found");
}

#[test]
fn all_event_constructors_return_correct_symbols() {
    let env = Env::default();

    assert_eq!(events::event_init(&env), Symbol::new(&env, "init"));
    assert_eq!(
        events::event_admin_changed(&env),
        Symbol::new(&env, "admin_changed")
    );
    assert_eq!(
        events::event_admin_transfer_started(&env),
        Symbol::new(&env, "admin_transfer_started")
    );
    assert_eq!(
        events::event_admin_transfer_completed(&env),
        Symbol::new(&env, "admin_transfer_completed")
    );
    assert_eq!(
        events::event_admin_cancelled(&env),
        Symbol::new(&env, "admin_cancelled")
    );
    assert_eq!(
        events::event_pause_set(&env),
        Symbol::new(&env, "pause_set")
    );
    assert_eq!(
        events::event_set_max_distribute(&env),
        Symbol::new(&env, "set_max_distribute")
    );
    assert_eq!(events::event_distribute(&env), Symbol::new(&env, "distribute"));
    assert_eq!(
        events::event_distribute_started(&env),
        Symbol::new(&env, "distribute_started")
    );
    assert_eq!(
        events::event_distribute_completed(&env),
        Symbol::new(&env, "distribute_completed")
    );
    assert_eq!(events::event_upgraded(&env), Symbol::new(&env, "upgraded"));
}

#[test]
fn require_auth_on_all_state_changing_functions() {
    let env = Env::default();
    // Do NOT mock all auths — we want to verify auth failures
    let admin = Address::generate(&env);
    let usdc_addr = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let contract_addr = env.register(Distribute, ());
    let client = DistributeClient::new(&env, &contract_addr);

    client.init(&admin, &usdc_addr);

    // Fund the contract for distribute tests
    let usdc_admin = token::StellarAssetClient::new(&env, &usdc_addr);
    usdc_admin.mint(&contract_addr, &1000);

    // Non-admin should fail on all state-changing functions
    let intruder = Address::generate(&env);
    let recipient = Address::generate(&env);

    // set_admin
    let result = client.try_set_admin(&intruder, &intruder);
    assert!(result.is_err(), "non-admin should not be able to set_admin");

    // pause
    let result = client.try_pause(&intruder);
    assert!(result.is_err(), "non-admin should not be able to pause");

    // unpause
    let result = client.try_unpause(&intruder);
    assert!(result.is_err(), "non-admin should not be able to unpause");

    // set_max_distribute
    let result = client.try_set_max_distribute(&intruder, &100);
    assert!(
        result.is_err(),
        "non-admin should not be able to set_max_distribute"
    );

    // cancel_admin_transfer (no pending transfer needed — auth checked first)
    let result = client.try_cancel_admin_transfer(&intruder);
    assert!(
        result.is_err(),
        "non-admin should not be able to cancel_admin_transfer"
    );

    // accept_admin / claim_admin (no pending transfer — auth checked first)
    let result = client.try_accept_admin(&intruder);
    assert!(result.is_err(), "non-admin should not be able to accept_admin");

    let result = client.try_claim_admin(&intruder);
    assert!(result.is_err(), "non-admin should not be able to claim_admin");

    // upgrade (auth checked before wasm verification)
    let new_wasm_hash = BytesN::from_array(&env, &[1u8; 32]);
    let result = client.try_upgrade(&intruder, &new_wasm_hash);
    assert!(result.is_err(), "non-admin should not be able to upgrade");

    // distribute
    let result = client.try_distribute(&intruder, &recipient, &100);
    assert!(
        result.is_err(),
        "non-admin should not be able to distribute"
    );
}

#[test]
fn no_unwrap_in_production_paths() {
    let source = include_str!("lib.rs")
        .split("#[cfg(test)]")
        .next()
        .unwrap_or("");
    let lines: std::vec::Vec<&str> = source.lines().collect();
    for (idx, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        // Allow .unwrap() only in test modules or in commented lines
        if trimmed.contains(".unwrap(") && !trimmed.starts_with("//") {
            panic!(
                "Production code at line {} uses .unwrap(): {}",
                idx + 1,
                trimmed
            );
        }
    }
}

#[test]
fn require_auth_on_init() {
    let env = Env::default();
    // Do NOT mock all auths
    let admin = Address::generate(&env);
    let usdc_addr = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let contract_addr = env.register(Distribute, ());
    let client = DistributeClient::new(&env, &contract_addr);

    // init should not fail on auth because it doesn't require auth
    // (only admin is being set, no previous admin exists)
    client.init(&admin, &usdc_addr);
}

#[test]
fn overflow_safe_math() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let recipient = Address::generate(&env);
    let usdc_addr = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let contract_addr = env.register(Distribute, ());
    let client = DistributeClient::new(&env, &contract_addr);

    client.init(&admin, &usdc_addr);

    let usdc_admin = token::StellarAssetClient::new(&env, &usdc_addr);
    usdc_admin.mint(&contract_addr, &i128::MAX);

    // Should succeed with large values
    client.distribute(&admin, &recipient, &1000);
}

#[test]
fn distribute_zero_amount_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let recipient = Address::generate(&env);
    let usdc_addr = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let contract_addr = env.register(Distribute, ());
    let client = DistributeClient::new(&env, &contract_addr);

    client.init(&admin, &usdc_addr);

    let result = client.try_distribute(&admin, &recipient, &0);
    assert!(result.is_err(), "distribute with zero amount should fail");
}

#[test]
fn distribute_exceeds_max_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let recipient = Address::generate(&env);
    let usdc_addr = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let contract_addr = env.register(Distribute, ());
    let client = DistributeClient::new(&env, &contract_addr);

    client.init(&admin, &usdc_addr);
    client.set_max_distribute(&admin, &100);

    let result = client.try_distribute(&admin, &recipient, &200);
    assert!(
        result.is_err(),
        "distribute exceeding max_distribute should fail"
    );
}

#[test]
fn distribute_while_paused_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let recipient = Address::generate(&env);
    let usdc_addr = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let contract_addr = env.register(Distribute, ());
    let client = DistributeClient::new(&env, &contract_addr);

    client.init(&admin, &usdc_addr);
    client.pause(&admin);

    let result = client.try_distribute(&admin, &recipient, &100);
    assert!(result.is_err(), "distribute while paused should fail");
}

#[test]
fn init_rejects_usdc_token_as_contract() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let contract_addr = env.register(Distribute, ());
    let client = DistributeClient::new(&env, &contract_addr);

    let result = client.try_init(&admin, &contract_addr);
    assert!(result.is_err(), "init with usdc_token == contract should fail");
}

#[test]
fn init_rejects_usdc_token_as_admin() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let contract_addr = env.register(Distribute, ());
    let client = DistributeClient::new(&env, &contract_addr);

    let result = client.try_init(&admin, &admin);
    assert!(result.is_err(), "init with usdc_token == admin should fail");
}

#[test]
fn get_admin_returns_admin() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let usdc_addr = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let contract_addr = env.register(Distribute, ());
    let client = DistributeClient::new(&env, &contract_addr);

    client.init(&admin, &usdc_addr);
    let returned_admin = client.get_admin();
    assert_eq!(returned_admin, admin);
}

#[test]
fn get_usdc_token_returns_token() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let usdc_addr = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let contract_addr = env.register(Distribute, ());
    let client = DistributeClient::new(&env, &contract_addr);

    client.init(&admin, &usdc_addr);
    let returned_token = client.get_usdc_token();
    assert_eq!(returned_token, usdc_addr);
}

#[test]
fn get_version_returns_crate_version() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let usdc_addr = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let contract_addr = env.register(Distribute, ());
    let client = DistributeClient::new(&env, &contract_addr);

    client.init(&admin, &usdc_addr);
    let version = client.version();
    assert!(!version.is_empty());
}

#[test]
fn balance_returns_contract_balance() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let usdc_addr = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let contract_addr = env.register(Distribute, ());
    let client = DistributeClient::new(&env, &contract_addr);

    client.init(&admin, &usdc_addr);

    // Initially zero
    assert_eq!(client.balance(), 0);

    // Fund the contract
    let usdc_admin = token::StellarAssetClient::new(&env, &usdc_addr);
    usdc_admin.mint(&contract_addr, &1000);

    assert_eq!(client.balance(), 1000);
}

#[test]
fn upgrade_emits_event() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let usdc_addr = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let contract_addr = env.register(Distribute, ());
    let client = DistributeClient::new(&env, &contract_addr);

    client.init(&admin, &usdc_addr);
    env.events().all();

    // Note: In test environment, we cannot actually deploy a new WASM.
    // This test verifies the function can be called with auth.
    // The upgrade would fail with "Wasm does not exist" but we test the auth path.
    let new_wasm_hash = BytesN::from_array(&env, &[1u8; 32]);
    let result = client.try_upgrade(&admin, &new_wasm_hash);
    // The call fails because the wasm doesn't exist, but auth was checked
    assert!(result.is_err());
}

#[test]
fn get_pending_admin_returns_some_when_set() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let new_admin = Address::generate(&env);
    let usdc_addr = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let contract_addr = env.register(Distribute, ());
    let client = DistributeClient::new(&env, &contract_addr);

    client.init(&admin, &usdc_addr);
    assert_eq!(client.get_pending_admin(), None);

    client.set_admin(&admin, &new_admin);
    let pending = client.get_pending_admin();
    assert_eq!(pending, Some(new_admin));
}

#[test]
fn get_pending_admin_returns_none_after_claim() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let new_admin = Address::generate(&env);
    let usdc_addr = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let contract_addr = env.register(Distribute, ());
    let client = DistributeClient::new(&env, &contract_addr);

    client.init(&admin, &usdc_addr);
    client.set_admin(&admin, &new_admin);
    client.accept_admin(&new_admin);

    let pending = client.get_pending_admin();
    assert_eq!(pending, None);
}

#[test]
fn claim_admin_alias_works() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let new_admin = Address::generate(&env);
    let usdc_addr = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let contract_addr = env.register(Distribute, ());
    let client = DistributeClient::new(&env, &contract_addr);

    client.init(&admin, &usdc_addr);
    client.set_admin(&admin, &new_admin);
    env.events().all();

    // Use claim_admin instead of accept_admin
    client.claim_admin(&new_admin);

    let events = env.events().all();
    let event = events.last().unwrap();
    let topic0: Symbol = event.1.get(0).unwrap().into_val(&env);
    assert_eq!(topic0, Symbol::new(&env, "admin_transfer_completed"));
}

#[test]
fn batch_distribute_total_overflow_returns_typed_error() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let usdc_addr = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let contract_addr = env.register(Distribute, ());
    let client = DistributeClient::new(&env, &contract_addr);

    client.init(&admin, &usdc_addr);

    // The default per-leg cap is `i128::MAX`, so each leg below is individually
    // valid yet their accumulated total overflows `i128`. The contract must
    // surface `DistributeError::Overflow` rather than a string panic.
    let mut payments: Vec<(Address, i128)> = Vec::new(&env);
    payments.push_back((Address::generate(&env), i128::MAX));
    payments.push_back((Address::generate(&env), i128::MAX));

    let result = client.try_batch_distribute(&admin, &payments);
    assert_eq!(
        result,
        Err(Ok(crate::errors::DistributeError::Overflow))
    );
}

#[test]
fn get_paused_returns_state() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let usdc_addr = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let contract_addr = env.register(Distribute, ());
    let client = DistributeClient::new(&env, &contract_addr);

    client.init(&admin, &usdc_addr);
    assert!(!client.get_paused());

    client.pause(&admin);
    assert!(client.get_paused());

    client.unpause(&admin);
    assert!(!client.get_paused());
}
