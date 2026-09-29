//! Focused tests for the `escrow` contract's admin cool-off feature (issue #914).
//!
//! Coverage targets: cooldown configuration bounds, per-action isolation,
//! window enforcement across ledger time, auth/authorization gating, the
//! two-step admin rotation, read-only views, and the `release` critical action.

use crate::admin::{DEFAULT_COOLDOWN_SECS, MAX_COOLDOWN_SECS, MIN_COOLDOWN_SECS};
use crate::{
    CalloraEscrow, CalloraEscrowClient, EscrowError, ACTION_PAUSE, ACTION_RELEASE, ACTION_ROTATE,
};
use soroban_sdk::testutils::{Address as _, Ledger as _};
use soroban_sdk::{Address, Env, Symbol};

/// Helper: register a fresh escrow contract initialized with `cooldown_secs`
/// and return `(env, admin, signer, client)`. Auth is mocked for convenience.
fn setup(cooldown_secs: Option<u64>) -> (Env, Address, Address, CalloraEscrowClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let signer = Address::generate(&env);
    let contract_id = env.register(CalloraEscrow, ());
    let client = CalloraEscrowClient::new(&env, &contract_id);
    client.init(&admin, &signer, &cooldown_secs);
    (env, admin, signer, client)
}

/// Advance the ledger timestamp by `secs` seconds.
fn advance(env: &Env, secs: u64) {
    let now = env.ledger().timestamp();
    env.ledger().set_timestamp(now + secs);
}

// ===========================================================================
// Initialisation
// ===========================================================================

#[test]
fn test_init_defaults_cooldown() {
    let (_env, admin, signer, client) = setup(None);
    assert_eq!(client.get_admin(), admin);
    assert_eq!(client.get_signer(), signer);
    assert_eq!(client.get_cooldown(), DEFAULT_COOLDOWN_SECS);
    assert!(!client.is_paused());
}

#[test]
fn test_init_custom_cooldown() {
    let (_env, _admin, _signer, client) = setup(Some(120));
    assert_eq!(client.get_cooldown(), 120);
}

#[test]
fn test_init_twice_fails() {
    let (env, _admin, _signer, client) = setup(Some(60));
    let other = Address::generate(&env);
    let res = client.try_init(&other, &other, &None);
    assert_eq!(res, Err(Ok(EscrowError::AlreadyInitialized)));
}

#[test]
fn test_init_rejects_out_of_range_cooldown() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let signer = Address::generate(&env);
    let contract_id = env.register(CalloraEscrow, ());
    let client = CalloraEscrowClient::new(&env, &contract_id);

    let res = client.try_init(&admin, &signer, &Some(MAX_COOLDOWN_SECS + 1));
    assert_eq!(res, Err(Ok(EscrowError::InvalidCooldown)));
}

#[test]
fn test_views_before_init_return_not_initialized() {
    let env = Env::default();
    let contract_id = env.register(CalloraEscrow, ());
    let client = CalloraEscrowClient::new(&env, &contract_id);
    assert_eq!(client.try_get_admin(), Err(Ok(EscrowError::NotInitialized)));
    assert_eq!(
        client.try_get_signer(),
        Err(Ok(EscrowError::NotInitialized))
    );
    assert_eq!(
        client.try_get_cooldown(),
        Err(Ok(EscrowError::NotInitialized))
    );
    // is_paused / views without init default gracefully.
    assert!(!client.is_paused());
    assert_eq!(client.get_pending_admin(), None);
}

// ===========================================================================
// Cooldown configuration
// ===========================================================================

#[test]
fn test_set_cooldown_updates_value() {
    let (_env, admin, _signer, client) = setup(Some(60));
    client.set_cooldown(&admin, &900);
    assert_eq!(client.get_cooldown(), 900);
}

#[test]
fn test_set_cooldown_boundaries_accepted() {
    let (_env, admin, _signer, client) = setup(Some(60));
    client.set_cooldown(&admin, &MIN_COOLDOWN_SECS);
    assert_eq!(client.get_cooldown(), MIN_COOLDOWN_SECS);
    client.set_cooldown(&admin, &MAX_COOLDOWN_SECS);
    assert_eq!(client.get_cooldown(), MAX_COOLDOWN_SECS);
}

#[test]
fn test_set_cooldown_zero_rejected() {
    let (_env, admin, _signer, client) = setup(Some(60));
    let res = client.try_set_cooldown(&admin, &0);
    assert_eq!(res, Err(Ok(EscrowError::InvalidCooldown)));
}

#[test]
fn test_set_cooldown_too_large_rejected() {
    let (_env, admin, _signer, client) = setup(Some(60));
    let res = client.try_set_cooldown(&admin, &(MAX_COOLDOWN_SECS + 1));
    assert_eq!(res, Err(Ok(EscrowError::InvalidCooldown)));
}

#[test]
fn test_set_cooldown_non_admin_rejected() {
    let (env, _admin, _signer, client) = setup(Some(60));
    let intruder = Address::generate(&env);
    let res = client.try_set_cooldown(&intruder, &120);
    assert_eq!(res, Err(Ok(EscrowError::Unauthorized)));
}

// ===========================================================================
// Cool-off enforcement — release (primary escrow action)
// ===========================================================================

#[test]
fn test_release_available_immediately_after_init() {
    let (env, _admin, _signer, client) = setup(Some(60));
    let release = Symbol::new(&env, ACTION_RELEASE);
    assert!(client.is_ready(&release));
    assert_eq!(client.cooldown_remaining(&release), 0);
}

#[test]
fn test_second_release_within_window_rejected() {
    let (env, admin, _signer, client) = setup(Some(300));
    let recipient = Address::generate(&env);
    client.release(&admin, &recipient);

    // A second release within the window is rejected.
    let res = client.try_release(&admin, &recipient);
    assert_eq!(res, Err(Ok(EscrowError::CooldownActive)));
}

#[test]
fn test_release_allowed_after_window_elapses() {
    let (env, admin, _signer, client) = setup(Some(300));
    let recipient = Address::generate(&env);
    client.release(&admin, &recipient);
    let res = client.try_release(&admin, &recipient);
    assert_eq!(res, Err(Ok(EscrowError::CooldownActive)));

    // Just before the window closes it is still blocked.
    advance(&env, 299);
    let release = Symbol::new(&env, ACTION_RELEASE);
    assert_eq!(client.cooldown_remaining(&release), 1);
    assert_eq!(
        client.try_release(&admin, &recipient),
        Err(Ok(EscrowError::CooldownActive))
    );

    // At the boundary it becomes available again.
    advance(&env, 1);
    assert!(client.is_ready(&release));
    client.release(&admin, &recipient);
}

// ===========================================================================
// Cool-off enforcement — pause / unpause
// ===========================================================================

#[test]
fn test_action_available_immediately_after_init() {
    let (env, _admin, _signer, client) = setup(Some(60));
    let pause = Symbol::new(&env, ACTION_PAUSE);
    assert!(client.is_ready(&pause));
    assert_eq!(client.cooldown_remaining(&pause), 0);
}

#[test]
fn test_second_pause_within_window_rejected() {
    let (_env, admin, _signer, client) = setup(Some(300));
    client.pause(&admin);
    assert!(client.is_paused());

    // Immediately unpausing is a different action tag → allowed.
    client.unpause(&admin);
    assert!(!client.is_paused());

    // A second pause within the window is rejected.
    let res = client.try_pause(&admin);
    assert_eq!(res, Err(Ok(EscrowError::CooldownActive)));
}

#[test]
fn test_pause_allowed_after_window_elapses() {
    let (env, admin, _signer, client) = setup(Some(300));
    client.pause(&admin);
    let res = client.try_pause(&admin);
    assert_eq!(res, Err(Ok(EscrowError::CooldownActive)));

    advance(&env, 299);
    let pause = Symbol::new(&env, ACTION_PAUSE);
    assert_eq!(client.cooldown_remaining(&pause), 1);
    assert_eq!(
        client.try_pause(&admin),
        Err(Ok(EscrowError::CooldownActive))
    );

    advance(&env, 1);
    assert!(client.is_ready(&pause));
    client.pause(&admin);
}

// ===========================================================================
// Per-action isolation
// ===========================================================================

#[test]
fn test_per_action_isolation() {
    let (env, admin, _signer, client) = setup(Some(1000));
    let new_signer = Address::generate(&env);

    client.pause(&admin);
    // rotate is a distinct action; not blocked by pause's window.
    client.rotate_signer(&admin, &new_signer);
    assert_eq!(client.get_signer(), new_signer);

    // But a second rotate is now blocked.
    let another = Address::generate(&env);
    let res = client.try_rotate_signer(&admin, &another);
    assert_eq!(res, Err(Ok(EscrowError::CooldownActive)));

    let rotate = Symbol::new(&env, ACTION_ROTATE);
    assert_eq!(client.cooldown_remaining(&rotate), 1000);
}

#[test]
fn test_release_and_pause_are_independently_cooled() {
    let (env, admin, _signer, client) = setup(Some(500));
    let recipient = Address::generate(&env);
    let new_signer = Address::generate(&env);

    // Release first.
    client.release(&admin, &recipient);

    // pause and rotate are distinct tags — not blocked by release's window.
    client.pause(&admin);
    client.rotate_signer(&admin, &new_signer);
    client.unpause(&admin);

    // But release is still blocked by its own cooldown.
    let res = client.try_release(&admin, &recipient);
    assert_eq!(res, Err(Ok(EscrowError::CooldownActive)));
}

#[test]
fn test_shorter_cooldown_takes_effect_for_next_check() {
    let (env, admin, _signer, client) = setup(Some(1000));
    client.pause(&admin);

    // Shorten the window; the pending pause becomes available sooner.
    client.set_cooldown(&admin, &10);
    advance(&env, 10);
    let pause = Symbol::new(&env, ACTION_PAUSE);
    assert!(client.is_ready(&pause));
    client.pause(&admin);
}

// ===========================================================================
// Auth gating
// ===========================================================================

#[test]
fn test_guarded_actions_require_admin() {
    let (env, _admin, _signer, client) = setup(Some(60));
    let intruder = Address::generate(&env);
    let target = Address::generate(&env);
    assert_eq!(
        client.try_pause(&intruder),
        Err(Ok(EscrowError::Unauthorized))
    );
    assert_eq!(
        client.try_unpause(&intruder),
        Err(Ok(EscrowError::Unauthorized))
    );
    assert_eq!(
        client.try_rotate_signer(&intruder, &target),
        Err(Ok(EscrowError::Unauthorized))
    );
    assert_eq!(
        client.try_release(&intruder, &target),
        Err(Ok(EscrowError::Unauthorized))
    );
}

// ===========================================================================
// Two-step admin rotation
// ===========================================================================

#[test]
fn test_admin_rotation_happy_path() {
    let (env, admin, _signer, client) = setup(Some(60));
    let new_admin = Address::generate(&env);

    client.set_admin(&admin, &new_admin);
    assert_eq!(client.get_pending_admin(), Some(new_admin.clone()));
    // Current admin unchanged until accepted.
    assert_eq!(client.get_admin(), admin);

    client.accept_admin(&new_admin);
    assert_eq!(client.get_admin(), new_admin);
    assert_eq!(client.get_pending_admin(), None);
}

#[test]
fn test_set_admin_non_admin_rejected() {
    let (env, _admin, _signer, client) = setup(Some(60));
    let intruder = Address::generate(&env);
    let res = client.try_set_admin(&intruder, &intruder);
    assert_eq!(res, Err(Ok(EscrowError::Unauthorized)));
}

#[test]
fn test_accept_admin_without_pending_rejected() {
    let (env, _admin, _signer, client) = setup(Some(60));
    let stranger = Address::generate(&env);
    let res = client.try_accept_admin(&stranger);
    assert_eq!(res, Err(Ok(EscrowError::NoPendingAdmin)));
}

#[test]
fn test_accept_admin_wrong_caller_rejected() {
    let (env, admin, _signer, client) = setup(Some(60));
    let new_admin = Address::generate(&env);
    let wrong = Address::generate(&env);
    client.set_admin(&admin, &new_admin);
    let res = client.try_accept_admin(&wrong);
    assert_eq!(res, Err(Ok(EscrowError::Unauthorized)));
}

#[test]
fn test_new_admin_controls_cooldown_after_rotation() {
    let (env, admin, _signer, client) = setup(Some(60));
    let new_admin = Address::generate(&env);
    client.set_admin(&admin, &new_admin);
    client.accept_admin(&new_admin);

    // Old admin can no longer configure cooldown.
    assert_eq!(
        client.try_set_cooldown(&admin, &120),
        Err(Ok(EscrowError::Unauthorized))
    );
    // New admin can.
    client.set_cooldown(&new_admin, &120);
    assert_eq!(client.get_cooldown(), 120);
}

#[test]
fn test_new_admin_can_perform_guarded_actions_after_rotation() {
    let (env, admin, _signer, client) = setup(Some(60));
    let new_admin = Address::generate(&env);
    let recipient = Address::generate(&env);

    client.set_admin(&admin, &new_admin);
    client.accept_admin(&new_admin);

    // Old admin can no longer pause.
    assert_eq!(client.try_pause(&admin), Err(Ok(EscrowError::Unauthorized)));
    // New admin can pause and release.
    client.pause(&new_admin);
    assert!(client.is_paused());
    client.unpause(&new_admin);
    client.release(&new_admin, &recipient);
}

/// An approved-asset flag is never set by default (deny-by-default).
#[test]
fn test_is_asset_approved_deny_by_default() {
    let (env, _admin, _signer, client) = setup(Some(60));
    let asset = Address::generate(&env);
    assert!(!client.is_asset_approved(&asset));
}

/// Approving an asset flips the deny-by-default flag to `true`.
#[test]
fn test_add_approved_asset_flips_flag() {
    let (env, admin, _signer, client) = setup(Some(60));
    let asset = Address::generate(&env);
    assert!(!client.is_asset_approved(&asset));
    client.add_approved_asset(&admin, &asset);
    assert!(client.is_asset_approved(&asset));
}

/// Only the admin may approve an asset.
#[test]
fn test_add_approved_asset_requires_admin() {
    let (env, _admin, _signer, client) = setup(Some(60));
    let intruder = Address::generate(&env);
    let asset = Address::generate(&env);
    assert_eq!(
        client.try_add_approved_asset(&intruder, &asset),
        Err(Ok(EscrowError::Unauthorized))
    );
}

/// Approving the contract's own address is rejected as malformed.
#[test]
fn test_add_approved_asset_rejects_self() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let signer = Address::generate(&env);
    let contract_id = env.register(CalloraEscrow, ());
    let client = CalloraEscrowClient::new(&env, &contract_id);
    client.init(&admin, &signer, &Some(60));

    assert_eq!(
        client.try_add_approved_asset(&admin, &contract_id),
        Err(Ok(EscrowError::InvalidInput))
    );
}

/// Revoking an approved asset flips the flag back to `false`.
#[test]
fn test_remove_approved_asset_revokes() {
    let (env, admin, _signer, client) = setup(Some(60));
    let asset = Address::generate(&env);
    client.add_approved_asset(&admin, &asset);
    assert!(client.is_asset_approved(&asset));
    client.remove_approved_asset(&admin, &asset);
    assert!(!client.is_asset_approved(&asset));
}

/// Only the admin may revoke an asset approval.
#[test]
fn test_remove_approved_asset_requires_admin() {
    let (env, _admin, _signer, client) = setup(Some(60));
    let intruder = Address::generate(&env);
    let asset = Address::generate(&env);
    assert_eq!(
        client.try_remove_approved_asset(&intruder, &asset),
        Err(Ok(EscrowError::Unauthorized))
    );
}

/// Revoking the contract's own address is rejected as malformed.
#[test]
fn test_remove_approved_asset_rejects_self() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let signer = Address::generate(&env);
    let contract_id = env.register(CalloraEscrow, ());
    let client = CalloraEscrowClient::new(&env, &contract_id);
    client.init(&admin, &signer, &Some(60));

    assert_eq!(
        client.try_remove_approved_asset(&admin, &contract_id),
        Err(Ok(EscrowError::InvalidInput))
    );
}

/// Creating an escrow against an approved asset records it and emits an event.
#[test]
fn test_create_escrow_success_and_record() {
    let (env, admin, _signer, client) = setup(Some(60));
    let asset = Address::generate(&env);
    let recipient = Address::generate(&env);
    client.add_approved_asset(&admin, &asset);

    let now = env.ledger().timestamp();
    client.create_escrow(&admin, &asset, &recipient, &1000);

    let record = client.get_escrow(&asset, &recipient).unwrap();
    assert_eq!(record.payment_asset, asset);
    assert_eq!(record.recipient, recipient);
    assert_eq!(record.amount, 1000);
    assert_eq!(record.created_at, now);
}

/// An unapproved asset is rejected and nothing is recorded (fail closed).
#[test]
fn test_create_escrow_unapproved_asset_fails_closed() {
    let (env, admin, _signer, client) = setup(Some(60));
    let asset = Address::generate(&env);
    let recipient = Address::generate(&env);
    assert_eq!(
        client.try_create_escrow(&admin, &asset, &recipient, &1000),
        Err(Ok(EscrowError::AssetNotApproved))
    );
    assert!(client.get_escrow(&asset, &recipient).is_none());
}

/// Only the admin may create an escrow.
#[test]
fn test_create_escrow_requires_admin() {
    let (env, _admin, _signer, client) = setup(Some(60));
    let intruder = Address::generate(&env);
    let asset = Address::generate(&env);
    let recipient = Address::generate(&env);
    assert_eq!(
        client.try_create_escrow(&intruder, &asset, &recipient, &1000),
        Err(Ok(EscrowError::Unauthorized))
    );
}

/// Using the contract itself as the payment asset is rejected as malformed.
#[test]
fn test_create_escrow_rejects_self_payment_asset() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let signer = Address::generate(&env);
    let contract_id = env.register(CalloraEscrow, ());
    let client = CalloraEscrowClient::new(&env, &contract_id);
    client.init(&admin, &signer, &Some(60));

    let recipient = Address::generate(&env);
    assert_eq!(
        client.try_create_escrow(&admin, &contract_id, &recipient, &1000),
        Err(Ok(EscrowError::InvalidInput))
    );
}

/// Using the contract itself as the recipient is rejected as malformed.
#[test]
fn test_create_escrow_rejects_self_recipient() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let signer = Address::generate(&env);
    let contract_id = env.register(CalloraEscrow, ());
    let client = CalloraEscrowClient::new(&env, &contract_id);
    client.init(&admin, &signer, &Some(60));

    let asset = Address::generate(&env);
    client.add_approved_asset(&admin, &asset);
    assert_eq!(
        client.try_create_escrow(&admin, &asset, &contract_id, &1000),
        Err(Ok(EscrowError::InvalidInput))
    );
}

/// A non-positive amount is rejected before any state is written.
#[test]
fn test_create_escrow_rejects_non_positive_amount() {
    let (env, admin, _signer, client) = setup(Some(60));
    let asset = Address::generate(&env);
    let recipient = Address::generate(&env);
    client.add_approved_asset(&admin, &asset);
    assert_eq!(
        client.try_create_escrow(&admin, &asset, &recipient, &0),
        Err(Ok(EscrowError::InvalidInput))
    );
    assert_eq!(
        client.try_create_escrow(&admin, &asset, &recipient, &-1),
        Err(Ok(EscrowError::InvalidInput))
    );
    assert!(client.get_escrow(&asset, &recipient).is_none());
}

/// Replaying the same creation inputs fails closed with `EscrowExists`.
#[test]
fn test_create_escrow_rejects_replay() {
    let (env, admin, _signer, client) = setup(Some(60));
    let asset = Address::generate(&env);
    let recipient = Address::generate(&env);
    client.add_approved_asset(&admin, &asset);
    client.create_escrow(&admin, &asset, &recipient, &1000);
    assert_eq!(
        client.try_create_escrow(&admin, &asset, &recipient, &2000),
        Err(Ok(EscrowError::EscrowExists))
    );
}

/// A stale (revoked) approval is no longer honored for new escrows.
#[test]
fn test_create_escrow_after_revoke_fails_closed() {
    let (env, admin, _signer, client) = setup(Some(60));
    let asset = Address::generate(&env);
    let recipient = Address::generate(&env);
    client.add_approved_asset(&admin, &asset);
    client.remove_approved_asset(&admin, &asset);
    assert_eq!(
        client.try_create_escrow(&admin, &asset, &recipient, &1000),
        Err(Ok(EscrowError::AssetNotApproved))
    );
}

/// Approval is scoped to each escrow instance (cross-tenant isolation).
#[test]
fn test_approval_registry_is_scoped_per_instance() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let signer = Address::generate(&env);
    let asset = Address::generate(&env);

    let c1 = env.register(CalloraEscrow, ());
    let client1 = CalloraEscrowClient::new(&env, &c1);
    client1.init(&admin, &signer, &Some(60));

    let c2 = env.register(CalloraEscrow, ());
    let client2 = CalloraEscrowClient::new(&env, &c2);
    client2.init(&admin, &signer, &Some(60));

    client1.add_approved_asset(&admin, &asset);
    assert!(client1.is_asset_approved(&asset));
    assert!(!client2.is_asset_approved(&asset));
}

// ===========================================================================
// Pause circuit breaker enforcement (issue #1181)
// ===========================================================================

/// Verify `EscrowError::Paused` has discriminant 11.
#[test]
fn test_escrow_error_paused_discriminant() {
    assert_eq!(EscrowError::Paused as u32, 11);
}

/// Escrow creation fails with `EscrowError::Paused` when the contract is paused.
#[test]
fn test_create_escrow_fails_while_paused() {
    let (env, admin, _signer, client) = setup(Some(60));
    let asset = Address::generate(&env);
    let recipient = Address::generate(&env);
    client.add_approved_asset(&admin, &asset);

    client.pause(&admin);
    assert!(client.is_paused());

    let res = client.try_create_escrow(&admin, &asset, &recipient, &1000);
    assert_eq!(res, Err(Ok(EscrowError::Paused)));
    assert!(client.get_escrow(&asset, &recipient).is_none());
}

/// Release fails with `EscrowError::Paused` when the contract is paused.
#[test]
fn test_release_fails_while_paused() {
    let (env, admin, _signer, client) = setup(Some(60));
    let recipient = Address::generate(&env);

    client.pause(&admin);
    assert!(client.is_paused());

    let res = client.try_release(&admin, &recipient);
    assert_eq!(res, Err(Ok(EscrowError::Paused)));
}

/// Unpausing restores both `create_escrow` and `release` functionality.
#[test]
fn test_unpause_restores_create_escrow_and_release() {
    let (env, admin, _signer, client) = setup(Some(60));
    let asset = Address::generate(&env);
    let recipient = Address::generate(&env);
    let release_recipient = Address::generate(&env);
    client.add_approved_asset(&admin, &asset);

    // Pause contract
    client.pause(&admin);
    assert!(client.is_paused());

    // Both fund-affecting actions are blocked while paused
    assert_eq!(
        client.try_create_escrow(&admin, &asset, &recipient, &1000),
        Err(Ok(EscrowError::Paused))
    );
    assert_eq!(
        client.try_release(&admin, &release_recipient),
        Err(Ok(EscrowError::Paused))
    );

    // Unpause restores both
    client.unpause(&admin);
    assert!(!client.is_paused());

    client.create_escrow(&admin, &asset, &recipient, &1000);
    let record = client.get_escrow(&asset, &recipient).unwrap();
    assert_eq!(record.amount, 1000);

    client.release(&admin, &release_recipient);
    assert_eq!(client.get_signer(), release_recipient);
}

/// Administrative functions and views remain allowed while paused.
#[test]
fn test_admin_functions_and_views_allowed_while_paused() {
    let (env, admin, signer, client) = setup(Some(60));
    let asset1 = Address::generate(&env);
    let asset2 = Address::generate(&env);
    let new_signer = Address::generate(&env);
    let new_admin = Address::generate(&env);

    client.pause(&admin);
    assert!(client.is_paused());

    // Views remain functional
    assert_eq!(client.get_admin(), admin);
    assert_eq!(client.get_signer(), signer);
    assert_eq!(client.get_cooldown(), 60);
    assert_eq!(client.get_pending_admin(), None);
    assert!(!client.is_asset_approved(&asset1));

    // Admin config remains functional
    client.set_cooldown(&admin, &120);
    assert_eq!(client.get_cooldown(), 120);

    client.add_approved_asset(&admin, &asset1);
    assert!(client.is_asset_approved(&asset1));
    client.add_approved_asset(&admin, &asset2);
    client.remove_approved_asset(&admin, &asset1);
    assert!(!client.is_asset_approved(&asset1));
    assert!(client.is_asset_approved(&asset2));

    client.rotate_signer(&admin, &new_signer);
    assert_eq!(client.get_signer(), new_signer);

    // Two-step admin rotation works while paused
    client.set_admin(&admin, &new_admin);
    assert_eq!(client.get_pending_admin(), Some(new_admin.clone()));
    client.accept_admin(&new_admin);
    assert_eq!(client.get_admin(), new_admin);

    // New admin can unpause
    client.unpause(&new_admin);
    assert!(!client.is_paused());
}
