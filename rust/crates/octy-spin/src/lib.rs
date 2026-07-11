//! Shared plumbing for the Octy Spin (WASM) service components:
//!
//! * [`ctx`]       — config/secrets loading + Redis address building
//! * [`gateway`]   — HTTP client for the octy-data-gateway sidecar
//! * [`http_util`] — Octy response envelopes, header/pagination validation
//! * [`auth`]      — X-AUTH-JWT fat-token verification (`decode_account_jwt`)
//! * [`aws`]       — SigV4-signed outbound requests to AWS REST APIs
//!
//! This crate links spin-sdk and therefore only compiles for wasm32 targets.

pub mod auth;
pub mod aws;
pub mod ctx;
pub mod gateway;
pub mod http_util;
