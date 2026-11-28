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

use std::sync::LazyLock;

#[cfg(feature = "reqwest")]
#[cfg_attr(docsrs, doc(cfg(feature = "reqwest")))]
pub use client::TracingMiddleware;
use opentelemetry::metrics::Meter;
#[cfg(feature = "axum")]
#[cfg_attr(docsrs, doc(cfg(feature = "axum")))]
pub use server::{TraceLayer, TraceService};
use sisa_telemetry_core::{instruments, propagation};

// The meter of the HTTP helpers.
static METER: LazyLock<Meter> =
    LazyLock::new(|| instruments::meter(env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION")));

// Writes the trace headers into an `http::HeaderMap`. A value that isn't a valid header is left
// out.
#[cfg(feature = "reqwest")]
struct HeaderInjector<'headers>(&'headers mut ::http::HeaderMap);

#[cfg(feature = "reqwest")]
impl propagation::Injector for HeaderInjector<'_> {
    fn set(&mut self, key: &str, value: String) {
        if let (Ok(name), Ok(value)) = (
            ::http::HeaderName::from_bytes(key.as_bytes()),
            ::http::HeaderValue::from_str(&value),
        ) {
            self.0.insert(name, value);
        }
    }
}

// Reads the trace headers from an `http::HeaderMap`.
#[cfg(feature = "axum")]
struct HeaderExtractor<'headers>(&'headers ::http::HeaderMap);

#[cfg(feature = "axum")]
impl propagation::Extractor for HeaderExtractor<'_> {
    fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key).and_then(|value| value.to_str().ok())
    }

    fn keys(&self) -> Vec<&str> {
        self.0.keys().map(::http::HeaderName::as_str).collect()
    }
}

// The method as the conventions name it: one of the known methods, or `_OTHER`, so a client
// can't make an attribute value of its own.
fn method_name(method: &::http::Method) -> &'static str {
    use ::http::Method;
    match *method {
        Method::GET => "GET",
        Method::POST => "POST",
        Method::PUT => "PUT",
        Method::DELETE => "DELETE",
        Method::PATCH => "PATCH",
        Method::HEAD => "HEAD",
        Method::OPTIONS => "OPTIONS",
        Method::CONNECT => "CONNECT",
        Method::TRACE => "TRACE",
        _ => "_OTHER",
    }
}
