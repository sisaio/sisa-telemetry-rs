//! What the HTTP and gRPC helpers share: the header adapters, the method and RPC names, and
//! their meter.

use std::sync::LazyLock;

use opentelemetry::metrics::Meter;
use sisa_telemetry_core::instruments;
#[cfg(any(feature = "reqwest", feature = "tonic", feature = "axum"))]
use sisa_telemetry_core::propagation;

// The meter of the HTTP and gRPC helpers.
pub(crate) static METER: LazyLock<Meter> =
    LazyLock::new(|| instruments::meter(env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION")));

// Writes the trace headers into an `http::HeaderMap`. A value that isn't a valid header is left
// out.
#[cfg(any(feature = "reqwest", feature = "tonic"))]
pub(crate) struct HeaderInjector<'headers>(pub(crate) &'headers mut ::http::HeaderMap);

#[cfg(any(feature = "reqwest", feature = "tonic"))]
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
pub(crate) struct HeaderExtractor<'headers>(pub(crate) &'headers ::http::HeaderMap);

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
#[cfg(any(feature = "reqwest", feature = "axum"))]
pub(crate) fn method_name(method: &::http::Method) -> &'static str {
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

// `grpc` or `connect_rpc` for an RPC request, from its content type and headers.
#[cfg(any(feature = "axum", feature = "tonic"))]
pub(crate) fn rpc_system(headers: &::http::HeaderMap) -> Option<&'static str> {
    let content_type = headers
        .get(::http::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    if content_type.starts_with("application/grpc") {
        Some("grpc")
    } else if content_type.starts_with("application/connect+")
        || headers.contains_key("connect-protocol-version")
    {
        Some("connect_rpc")
    } else {
        None
    }
}

// The service and the method of an RPC path, `/{service}/{method}`.
#[cfg(any(feature = "axum", feature = "tonic"))]
pub(crate) fn rpc_path(path: &str) -> Option<(&str, &str)> {
    path.strip_prefix('/')
        .and_then(|path| path.rsplit_once('/'))
}
