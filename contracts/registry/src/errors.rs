use soroban_sdk::contracterror;

/// Stable error codes for the Callora offering registry contract.
#[contracterror]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum RegistryError {
    /// Contract has not been initialized (code 1).
    NotInitialized = 1,
    /// `init` was called more than once (code 2).
    AlreadyInitialized = 2,
    /// Caller is not the configured admin (code 3).
    Unauthorized = 3,
    /// Offering id is empty or invalid (code 4).
    InvalidOfferingId = 4,
    /// Offering is already registered (code 5).
    OfferingAlreadyRegistered = 5,
    /// Developer balance is below the required floor (code 6).
    InsufficientDeveloperBalance = 6,
    /// Registered offering count overflow (code 7).
    Overflow = 7,
    /// Offering is not registered (code 8).
    OfferingNotFound = 8,
    /// Admin action cooldown has not yet expired (code 9).
    AdminCooldownActive = 9,
    /// Metadata is empty, exceeds the byte bound, or contains invalid
    /// (non-visible-ASCII / control / whitespace-delimited) encodings (code 10).
    InvalidMetadata = 10,
    /// New developer address is invalid (e.g. zero address) (code 11).
    InvalidDeveloper = 11,
}
