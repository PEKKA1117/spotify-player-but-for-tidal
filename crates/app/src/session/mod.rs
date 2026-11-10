//! Where the login session is kept (spec 0002 "Storage"): the keyring or
//! the age-encrypted file ([`store`]), its passphrase ([`passphrase`]), the
//! choice between them ([`setup`]) and `tidal-player login` ([`login`]).

pub mod login;
pub mod passphrase;
pub mod setup;
pub mod store;
