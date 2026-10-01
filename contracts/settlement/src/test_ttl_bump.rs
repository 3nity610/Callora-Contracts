#![cfg(test)]

use crate::{
    CalloraSettlement, CalloraSettlementClient, StorageKey, INSTANCE_BUMP_AMOUNT,
    INSTANCE_BUMP_THRESHOLD, PERSISTENT_BUMP_AMOUNT, PERSISTENT_BUMP_THRESHOLD,
};
use soroban_sdk::testutils::storage::{Instance as _, Persistent as _};
use soroban_sdk::testutils::{Address as _, Ledger};
use soroban_sdk::{Address, Env, Symbol, Vec};

fn setup() -> (Env, Address, Address, CalloraSettlementClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let vault = Address::generate(&env);
    let contract_id = env.register(CalloraSettlement, ());
    let client = CalloraSettlementClient::new(&env, &contract_id);
    client.init(&admin, &vault);
    (env, admin, vault, client)
}

#[test]
fn test_get_admin_bumps_instance_ttl() {
    let (env, admin, _vault, client) = setup();

    // Advance sequence number to reduce instance TTL below threshold.
    let seq = env.ledger().sequence();
    env.ledger()
        .set_sequence_number(seq + INSTANCE_BUMP_AMOUNT - INSTANCE_BUMP_THRESHOLD + 10);

    let ttl_before = env.storage().instance().get_ttl();
    assert!(ttl_before < INSTANCE_BUMP_THRESHOLD);

    let res = client.get_admin();
    assert_eq!(res, admin);

    let ttl_after = env.storage().instance().get_ttl();
    assert_eq!(ttl_after, INSTANCE_BUMP_AMOUNT);
}

#[test]
fn test_get_vault_bumps_instance_ttl() {
    let (env, _admin, vault, client) = setup();

    let seq = env.ledger().sequence();
    env.ledger()
        .set_sequence_number(seq + INSTANCE_BUMP_AMOUNT - INSTANCE_BUMP_THRESHOLD + 10);

    let res = client.get_vault();
    assert_eq!(res, vault);

    let ttl_after = env.storage().instance().get_ttl();
    assert_eq!(ttl_after, INSTANCE_BUMP_AMOUNT);
}

#[test]
fn test_get_global_pool_bumps_instance_ttl() {
    let (env, _admin, vault, client) = setup();

    // Credit into the pool so total_balance is non-zero.
    let token = Address::generate(&env);
    client.add_supported_token(&_admin, &token);
    client.receive_payment(&vault, &1000i128, &true, &None, &token, &1u32);

    let seq = env.ledger().sequence();
    env.ledger()
        .set_sequence_number(seq + INSTANCE_BUMP_AMOUNT - INSTANCE_BUMP_THRESHOLD + 10);

    let pool = client.get_global_pool();
    assert_eq!(pool.total_balance, 1000i128);

    let ttl_after = env.storage().instance().get_ttl();
    assert_eq!(ttl_after, INSTANCE_BUMP_AMOUNT);
}

#[test]
fn test_get_total_received_bumps_instance_ttl() {
    let (env, _admin, _vault, client) = setup();

    let seq = env.ledger().sequence();
    env.ledger()
        .set_sequence_number(seq + INSTANCE_BUMP_AMOUNT - INSTANCE_BUMP_THRESHOLD + 10);

    let total = client.get_total_received();
    assert_eq!(total, 0);

    let ttl_after = env.storage().instance().get_ttl();
    assert_eq!(ttl_after, INSTANCE_BUMP_AMOUNT);
}

#[test]
fn test_get_developer_balance_bumps_ttl() {
    let (env, admin, _vault, client) = setup();
    let dev = Address::generate(&env);
    let token = Address::generate(&env);
    let reason = Symbol::new(&env, "test");

    client.force_credit_developer(&admin, &dev, &500i128, &token, &reason);

    let key = StorageKey::DeveloperBalance(dev.clone(), token.clone());

    // Advance sequence number to decrease persistent TTL
    let seq = env.ledger().sequence();
    env.ledger()
        .set_sequence_number(seq + PERSISTENT_BUMP_AMOUNT - PERSISTENT_BUMP_THRESHOLD + 10);

    let ttl_before = env.storage().persistent().get_ttl(&key);
    assert!(ttl_before < PERSISTENT_BUMP_THRESHOLD);

    let bal = client.get_developer_balance(&dev, &token);
    assert_eq!(bal, 500);

    let ttl_after = env.storage().persistent().get_ttl(&key);
    assert_eq!(ttl_after, PERSISTENT_BUMP_AMOUNT);

    let inst_ttl = env.storage().instance().get_ttl();
    assert_eq!(inst_ttl, INSTANCE_BUMP_AMOUNT);
}

#[test]
fn test_get_developer_min_balance_bumps_ttl() {
    let (env, admin, _vault, client) = setup();
    let dev = Address::generate(&env);

    client.set_developer_min_balance(&admin, &dev, &100i128);

    let key = StorageKey::DeveloperMinBalance(dev.clone());

    let seq = env.ledger().sequence();
    env.ledger()
        .set_sequence_number(seq + PERSISTENT_BUMP_AMOUNT - PERSISTENT_BUMP_THRESHOLD + 10);

    let ttl_before = env.storage().persistent().get_ttl(&key);
    assert!(ttl_before < PERSISTENT_BUMP_THRESHOLD);

    let min_bal = client.get_developer_min_balance(&dev);
    assert_eq!(min_bal, 100);

    let ttl_after = env.storage().persistent().get_ttl(&key);
    assert_eq!(ttl_after, PERSISTENT_BUMP_AMOUNT);
}

#[test]
fn test_get_developer_claim_window_bumps_ttl() {
    let (env, admin, _vault, client) = setup();
    let dev = Address::generate(&env);

    client.set_developer_claim_window(&admin, &dev, &1000u64, &2000u64);

    let key = StorageKey::DeveloperClaimWindow(dev.clone());

    let seq = env.ledger().sequence();
    env.ledger()
        .set_sequence_number(seq + PERSISTENT_BUMP_AMOUNT - PERSISTENT_BUMP_THRESHOLD + 10);

    let ttl_before = env.storage().persistent().get_ttl(&key);
    assert!(ttl_before < PERSISTENT_BUMP_THRESHOLD);

    let win = client.get_developer_claim_window(&dev).unwrap();
    assert_eq!(win.start_ts, 1000);
    assert_eq!(win.end_ts, 2000);

    let ttl_after = env.storage().persistent().get_ttl(&key);
    assert_eq!(ttl_after, PERSISTENT_BUMP_AMOUNT);
}

#[test]
fn test_get_daily_withdraw_cap_bumps_ttl() {
    let (env, admin, _vault, client) = setup();
    let dev = Address::generate(&env);

    client.set_daily_withdraw_cap(&admin, &dev, &5000i128);

    let key = StorageKey::DailyWithdrawCap(dev.clone());

    let seq = env.ledger().sequence();
    env.ledger()
        .set_sequence_number(seq + PERSISTENT_BUMP_AMOUNT - PERSISTENT_BUMP_THRESHOLD + 10);

    let cap = client.get_daily_withdraw_cap(&dev);
    assert_eq!(cap, 5000);

    let ttl_after = env.storage().persistent().get_ttl(&key);
    assert_eq!(ttl_after, PERSISTENT_BUMP_AMOUNT);
}

#[test]
fn test_get_withdrawal_today_bumps_ttl() {
    let (env, admin, _vault, client) = setup();
    let dev = Address::generate(&env);
    let usdc = Address::generate(&env);

    client.set_usdc_token(&admin, &usdc);
    client.force_credit_developer(&admin, &dev, &1000i128, &usdc, &Symbol::new(&env, "credit"));

    // We check get_withdrawal_today after setting up key
    let key = StorageKey::WithdrawalToday(dev.clone());

    let seq = env.ledger().sequence();
    env.ledger()
        .set_sequence_number(seq + PERSISTENT_BUMP_AMOUNT - PERSISTENT_BUMP_THRESHOLD + 10);

    let today_withdrawn = client.get_withdrawal_today(&dev);
    assert_eq!(today_withdrawn, 0);

    // If key existed, it would bump TTL; calling get_withdrawal_today on empty key doesn't crash
    let inst_ttl = env.storage().instance().get_ttl();
    assert_eq!(inst_ttl, INSTANCE_BUMP_AMOUNT);
}

#[test]
fn test_get_all_developer_balances_bumps_ttl() {
    let (env, admin, _vault, client) = setup();
    let dev1 = Address::generate(&env);
    let dev2 = Address::generate(&env);
    let token = Address::generate(&env);

    client.force_credit_developer(&admin, &dev1, &100i128, &token, &Symbol::new(&env, "c1"));
    client.force_credit_developer(&admin, &dev2, &200i128, &token, &Symbol::new(&env, "c2"));

    let key1 = StorageKey::DeveloperBalance(dev1.clone(), token.clone());
    let key2 = StorageKey::DeveloperBalance(dev2.clone(), token.clone());

    let seq = env.ledger().sequence();
    env.ledger()
        .set_sequence_number(seq + PERSISTENT_BUMP_AMOUNT - PERSISTENT_BUMP_THRESHOLD + 10);

    let balances = client.get_all_developer_balances(&admin, &token);
    assert_eq!(balances.len(), 2);

    let ttl1 = env.storage().persistent().get_ttl(&key1);
    let ttl2 = env.storage().persistent().get_ttl(&key2);
    assert_eq!(ttl1, PERSISTENT_BUMP_AMOUNT);
    assert_eq!(ttl2, PERSISTENT_BUMP_AMOUNT);
}

#[test]
fn test_get_developer_balances_page_bumps_ttl() {
    let (env, admin, _vault, client) = setup();
    let dev1 = Address::generate(&env);
    let token = Address::generate(&env);

    client.force_credit_developer(&admin, &dev1, &100i128, &token, &Symbol::new(&env, "c1"));

    let key1 = StorageKey::DeveloperBalance(dev1.clone(), token.clone());

    let seq = env.ledger().sequence();
    env.ledger()
        .set_sequence_number(seq + PERSISTENT_BUMP_AMOUNT - PERSISTENT_BUMP_THRESHOLD + 10);

    let page = client.get_developer_balances_page(&admin, &0u32, &10u32, &token);
    assert_eq!(page.len(), 1);

    let ttl1 = env.storage().persistent().get_ttl(&key1);
    assert_eq!(ttl1, PERSISTENT_BUMP_AMOUNT);
}

#[test]
fn test_get_developer_balances_cursor_bumps_ttl() {
    let (env, admin, _vault, client) = setup();
    let dev1 = Address::generate(&env);
    let token = Address::generate(&env);

    client.force_credit_developer(&admin, &dev1, &100i128, &token, &Symbol::new(&env, "c1"));

    let key1 = StorageKey::DeveloperBalance(dev1.clone(), token.clone());

    let seq = env.ledger().sequence();
    env.ledger()
        .set_sequence_number(seq + PERSISTENT_BUMP_AMOUNT - PERSISTENT_BUMP_THRESHOLD + 10);

    let (items, _) = client.get_developer_balances_cursor(&admin, &None, &10u32, &token);
    assert_eq!(items.len(), 1);

    let ttl1 = env.storage().persistent().get_ttl(&key1);
    assert_eq!(ttl1, PERSISTENT_BUMP_AMOUNT);
}

#[test]
fn test_get_pending_admin_bumps_instance_ttl() {
    let (env, admin, _vault, client) = setup();
    let new_admin = Address::generate(&env);

    client.set_admin(&admin, &new_admin);

    let seq = env.ledger().sequence();
    env.ledger()
        .set_sequence_number(seq + INSTANCE_BUMP_AMOUNT - INSTANCE_BUMP_THRESHOLD + 10);

    let pending = client.get_pending_admin();
    assert_eq!(pending, Some(new_admin));

    let inst_ttl = env.storage().instance().get_ttl();
    assert_eq!(inst_ttl, INSTANCE_BUMP_AMOUNT);
}

#[test]
fn test_get_version_bumps_instance_ttl() {
    let (env, _admin, _vault, client) = setup();

    let seq = env.ledger().sequence();
    env.ledger()
        .set_sequence_number(seq + INSTANCE_BUMP_AMOUNT - INSTANCE_BUMP_THRESHOLD + 10);

    let ver = client.get_version();
    assert_eq!(ver, None);

    let inst_ttl = env.storage().instance().get_ttl();
    assert_eq!(inst_ttl, INSTANCE_BUMP_AMOUNT);
}

#[test]
fn test_get_balance_migration_bumps_ttl() {
    let (env, admin, _vault, client) = setup();
    let dev1 = Address::generate(&env);
    let dev2 = Address::generate(&env);

    client.propose_balance_migration(&admin, &dev1, &dev2);

    let key = StorageKey::PendingDeveloperMigration(dev1.clone());

    let seq = env.ledger().sequence();
    env.ledger()
        .set_sequence_number(seq + PERSISTENT_BUMP_AMOUNT - PERSISTENT_BUMP_THRESHOLD + 10);

    let mig = client.get_balance_migration(&dev1);
    assert!(mig.is_some());

    let ttl = env.storage().persistent().get_ttl(&key);
    assert_eq!(ttl, PERSISTENT_BUMP_AMOUNT);
}

#[test]
fn test_migration_storage_version_bumps_instance_ttl() {
    let (env, _admin, _vault, client) = setup();

    let seq = env.ledger().sequence();
    env.ledger()
        .set_sequence_number(seq + INSTANCE_BUMP_AMOUNT - INSTANCE_BUMP_THRESHOLD + 10);

    let ver = client.migration_storage_version();
    assert_eq!(ver, 1);

    let inst_ttl = env.storage().instance().get_ttl();
    assert_eq!(inst_ttl, INSTANCE_BUMP_AMOUNT);
}
