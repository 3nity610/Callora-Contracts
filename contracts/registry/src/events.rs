//! Event topic Symbol constructors for the Callora Registry contract.
//!
//! This module centralizes all event topic strings into dedicated functions,
//! ensuring byte-identity is preserved and preventing accidental topic name drift
//! across call sites.

use soroban_sdk::{Env, Symbol};

/// Returns the Symbol for the `"init"` event topic.
///
/// Emitted when the registry contract is first initialized with an admin
/// address and an initial catalog configuration.
pub fn event_init(env: &Env) -> Symbol {
    Symbol::new(env, "init")
}

/// Returns the Symbol for the `"offering_registered"` event topic.
///
/// Emitted when a new offering is registered in the catalog via
/// [`crate::CalloraRegistry::register_offering`]. The offering ID is
/// included as a topic so indexers can track additions without polling.
pub fn event_offering_registered(env: &Env) -> Symbol {
    Symbol::new(env, "offering_registered")
}

/// Returns the Symbol for the `"offering_metadata_updated"` event topic.
///
/// Emitted when an existing offering's metadata is replaced via
/// [`crate::CalloraRegistry::update_offering_metadata`]. The offering ID is
/// included as a topic so indexers can re-read that single record instead of
/// re-scanning every offering, and the updated [`crate::OfferingRecord`] is
/// emitted as the event data.
pub fn event_offering_metadata_updated(env: &Env) -> Symbol {
    Symbol::new(env, "offering_metadata_updated")
}

/// Returns the Symbol for the `"offering_transferred"` event topic.
///
/// Emitted when an offering changes developer via
/// [`crate::CalloraRegistry::transfer_offering`]. The previous and the new
/// developer addresses are emitted as the event data, so ownership history can
/// be reconstructed from the event stream alone.
pub fn event_offering_transferred(env: &Env) -> Symbol {
    Symbol::new(env, "offering_transferred")
}

/// Returns the Symbol for the `"offering_deregistered"` event topic.
///
/// Emitted when an offering is removed from the registry via
/// [`crate::CalloraRegistry::deregister_offering`]. The removed
/// [`crate::OfferingRecord`] is emitted as the event data so indexers can drop
/// the entry without polling, and the offering ID is a topic so the removal can
/// be correlated with the earlier registration event.
pub fn event_offering_deregistered(env: &Env) -> Symbol {
    Symbol::new(env, "offering_deregistered")
}

/// Returns the Symbol for the canonical event version marker used by Callora.
pub fn event_version_v1(env: &Env) -> Symbol {
    Symbol::new(env, "callora.v1")
}

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::Env;

    /// Snapshot: proves event_init still maps to exactly the bytes for "init".
    #[test]
    fn test_event_init_bytes() {
        let env = Env::default();
        assert_eq!(event_init(&env), Symbol::new(&env, "init"));
    }

    /// Snapshot: proves event_offering_registered still maps to exactly the bytes for "offering_registered".
    #[test]
    fn test_event_offering_registered_bytes() {
        let env = Env::default();
        assert_eq!(
            event_offering_registered(&env),
            Symbol::new(&env, "offering_registered")
        );
    }

    /// Snapshot: proves event_offering_metadata_updated still maps to exactly the bytes for "offering_metadata_updated".
    #[test]
    fn test_event_offering_metadata_updated_bytes() {
        let env = Env::default();
        assert_eq!(
            event_offering_metadata_updated(&env),
            Symbol::new(&env, "offering_metadata_updated")
        );
    }

    /// Snapshot: proves event_offering_transferred still maps to exactly the bytes for "offering_transferred".
    #[test]
    fn test_event_offering_transferred_bytes() {
        let env = Env::default();
        assert_eq!(
            event_offering_transferred(&env),
            Symbol::new(&env, "offering_transferred")
        );
    }

    /// Snapshot: proves event_offering_deregistered still maps to exactly the bytes for "offering_deregistered".
    #[test]
    fn test_event_offering_deregistered_bytes() {
        let env = Env::default();
        assert_eq!(
            event_offering_deregistered(&env),
            Symbol::new(&env, "offering_deregistered")
        );
    }
}
