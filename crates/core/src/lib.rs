//! The API-only part of Sisa Telemetry, for library crates: trace headers across messages and
//! HTTP, error recording, metric helpers and the shared attribute names.
//!
//! A library depends on this crate and never on `sisa-telemetry`: that crate installs the
//! process-wide providers, which only the application may do, once. This crate depends on the
//! OpenTelemetry API, [`tracing`](https://docs.rs/tracing) and `tracing-opentelemetry`, with no
//! SDK and no exporter, and re-exports the API as [`opentelemetry`], so a library and the
//! application it runs in share one version of it.
//!
//! Until the application calls `sisa_telemetry::init`, every function here does nothing:
//! [`propagation::inject`] writes no header, the links and parents are not set, and instruments
//! record into nothing. A library works the same with telemetry off.
//!
//! ```
//! use std::collections::HashMap;
//!
//! use sisa_telemetry_core::{propagation, record_error};
//!
//! // A producer: the span's trace context goes into the message's headers.
//! let span = tracing::info_span!("publish", otel.kind = "producer");
//! let mut headers = HashMap::new();
//! propagation::inject(&span, &mut headers);
//!
//! // A consumer: its span starts a new trace that links back to the producer's.
//! let span = tracing::info_span!("process", otel.kind = "consumer");
//! propagation::link_from(&span, &headers);
//!
//! let failure = std::io::Error::other("the handler failed");
//! record_error(&span, &failure);
//! ```

pub mod attributes;
mod error;
pub mod instruments;
pub mod propagation;

pub use error::record_error;
/// The OpenTelemetry API this crate is built on, for its metric instruments and `KeyValue`.
/// Libraries and services use it from here instead of depending on `opentelemetry` themselves,
/// so one binary holds one version.
pub use opentelemetry;
