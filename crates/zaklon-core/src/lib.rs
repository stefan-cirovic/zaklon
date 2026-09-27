//! Shared building blocks for the Zaklon hub and apps: configuration,
//! storage, pairing and TLS identity. Everything here is offline-only.

pub mod catalog;
pub mod config;
pub mod dates;
pub mod db;
pub mod lang;
pub mod maps;
pub mod memory;
pub mod pairing;
pub mod supplies;
pub mod tls;
pub mod translit;

pub use config::Config;
/// The database library, so callers can tell database failures apart.
pub use rusqlite;
pub use db::Db;
