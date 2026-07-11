//! Shared, WASM-portable domain logic for the Octy Rust services.
//!
//! Everything in this crate compiles for both native targets (so it can be
//! unit-tested with plain `cargo test` and reused by the data gateway) and
//! `wasm32-wasip1` (so the Spin components can link it). Keep network I/O out
//! of this crate — HTTP/Redis access lives in the service crates.

pub mod config;
pub mod ejson;
pub mod errors;
pub mod jwt;
pub mod models;
pub mod secrets;
pub mod sigv4;
pub mod utils;
