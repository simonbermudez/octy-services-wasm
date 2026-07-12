//! Folded-in port of `configurations/` — merged into the admin service to
//! reduce deployable count (both were small, single-purpose services with no
//! functional overlap; see rust/README.md's architecture notes for why).
//! Nothing here changed behavior from the standalone `configurations-service`
//! crate — routes, config/secrets variable names, and the AMQP/JWT
//! dependencies below are unchanged, just re-homed under one Spin component.

pub mod handlers;
pub mod http_util;
pub mod models;
pub mod repos;
