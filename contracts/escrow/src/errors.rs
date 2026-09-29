use soroban_sdk::contracterror;

/// Stable, machine-readable error codes for the Escrow contract.
///
/// The numeric discriminants in this enum are part of the contract interface and
/// must remain stable over time. Callers and indexers may branch on these `u32`
/// codes instead of parsing panic strings.
///
/// | Code | Variant             | Meaning                                                      |
/// |------|---------------------|--------------------------------------------------------------|
/// | 1    | NotInitialized      | Contract has not been initialized yet                        |
/// | 2    | AlreadyInitialized  | `init` was called more than once                             |
/// | 3    | Unauthorized        | Caller is not authorized for this operation                  |
/// | 4    | CooldownActive      | A critical action is still inside its cool-off window        |
/// | 5    | InvalidCooldown     | Proposed cooldown value is outside the accepted range        |
/// | 6    | NoPendingAdmin      | No admin transfer is currently pending                       |
/// | 7    | Overflow            | Arithmetic overflow detected                                 |
/// | 8    | AssetNotApproved    | Payment asset is not approved for escrow creation            |
/// | 9    | InvalidInput        | A supplied input is malformed or forbidden                    |
/// | 10   | EscrowExists        | Escrow already exists for the given payment asset and recipient |
/// | 11   | Paused              | Operation is rejected because the contract is paused         |
#[contracterror]
#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(u32)]
pub enum EscrowError {
    /// Contract has not been initialized yet (code 1).
    NotInitialized = 1,
    /// `init` was called more than once (code 2).
    AlreadyInitialized = 2,
    /// Caller is not authorized for this operation (code 3).
    Unauthorized = 3,
    /// A critical action is still inside its cool-off window (code 4).
    CooldownActive = 4,
    /// Proposed cooldown value is outside the accepted range (code 5).
    InvalidCooldown = 5,
    /// No admin transfer is currently pending (code 6).
    NoPendingAdmin = 6,
    /// Arithmetic overflow detected (code 7).
    Overflow = 7,
    /// Payment asset is not approved for escrow creation (code 8).
    AssetNotApproved = 8,
    /// A supplied input is malformed or forbidden (code 9).
    InvalidInput = 9,
    /// Escrow already exists for the given payment asset and recipient (code 10).
    EscrowExists = 10,
    /// Operation is rejected because the contract is paused (code 11).
    Paused = 11,
}
