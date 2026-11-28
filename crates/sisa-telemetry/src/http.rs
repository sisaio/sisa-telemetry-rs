//! The trace headers on HTTP and gRPC calls: [`TracingMiddleware`] on the reqwest client side,
//! [`TraceLayer`] on the server side.
//!
//! Both follow the OpenTelemetry HTTP conventions: a span per call, named `{method}` on the client
//! and `{method} {route}` on the server, with the method, the status code and an error status
//! for a failed call, and a duration histogram in seconds. [`TraceLayer`] follows the RPC
//! conventions instead for gRPC and Connect calls.

#[cfg(feature = "axum")]
mod server;

#[cfg(feature = "reqwest")]
mod client;

#[cfg(feature = "reqwest")]
#[cfg_attr(docsrs, doc(cfg(feature = "reqwest")))]
pub use client::TracingMiddleware;
#[cfg(feature = "axum")]
#[cfg_attr(docsrs, doc(cfg(feature = "axum")))]
pub use server::{TraceLayer, TraceService};
