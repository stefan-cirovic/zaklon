//! Shared building blocks for the Zaklon hub and apps: configuration,
//! storage, pairing and TLS identity. Everything here is offline-only.

pub mod config;
pub mod db;
pub mod pairing;
pub mod tls;

pub use config::Config;
pub use db::Db;
