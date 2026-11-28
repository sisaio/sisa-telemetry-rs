//! The reqwest middleware.

use std::{sync::LazyLock, time::Instant};

use opentelemetry::{KeyValue, metrics::Histogram};
use reqwest_middleware::{
    Middleware, Next, Result,
    reqwest::{Request, Response},
};
use sisa_telemetry_core::{instruments, propagation, record_error};
use tracing::{Instrument, field::Empty};

use super::{HeaderInjector, METER, method_name};

static DURATION: LazyLock<Histogram<f64>> = LazyLock::new(|| {
    instruments::duration_histogram(
        &METER,
        "http.client.request.duration",
        "Duration of HTTP client requests",
    )
});

/// A [`reqwest_middleware`] middleware: each request gets a client span, and the trace headers
/// (`traceparent`, `tracestate`, `baggage`) of that span.
///
/// The span is named after the method and carries `http.request.method`, `server.address`,
/// `server.port`, `url.full` without its credentials, query or fragment, and
/// `http.response.status_code`. A status of 400 or more, or no response, marks it as an error,
/// with `error.type`. Each request also records `http.client.request.duration`, in seconds,
/// until the response headers arrive.
///
/// Without `init`, the span is a plain `tracing` span and no header is written.
///
/// ```
/// let client = reqwest_middleware::ClientBuilder::new(reqwest_middleware::reqwest::Client::new())
///     .with(sisa_telemetry::http::TracingMiddleware::default())
///     .build();
/// ```
#[derive(Clone, Copy, Debug, Default)]
#[non_exhaustive]
pub struct TracingMiddleware;

#[async_trait::async_trait]
impl Middleware for TracingMiddleware {
    async fn handle(
        &self,
        mut request: Request,
        extensions: &mut ::http::Extensions,
        next: Next<'_>,
    ) -> Result<Response> {
        let method = method_name(request.method());
        let url = request.url();
        let address = url.host_str().unwrap_or_default().to_owned();
        let port = url.port_or_known_default();
        let mut full = url.clone();
        _ = full.set_username("");
        _ = full.set_password(None);
        full.set_query(None);
        full.set_fragment(None);

        let span = tracing::info_span!(
            "HTTP request",
            otel.kind = "client",
            otel.name = method,
            otel.status_code = Empty,
            http.request.method = method,
            server.address = address,
            server.port = port,
            url.full = full.as_str(),
            http.response.status_code = Empty,
            "error.type" = Empty,
        );
        propagation::inject(&span, &mut HeaderInjector(request.headers_mut()));

        let started = Instant::now();
        let result = next.run(request, extensions).instrument(span.clone()).await;
        let seconds = started.elapsed().as_secs_f64();

        let mut attributes = vec![
            KeyValue::new("http.request.method", method),
            KeyValue::new("server.address", address),
        ];
        if let Some(port) = port {
            attributes.push(KeyValue::new("server.port", i64::from(port)));
        }
        let error_type = match &result {
            Ok(response) => {
                let status = response.status().as_u16();
                span.record("http.response.status_code", status);
                attributes.push(KeyValue::new(
                    "http.response.status_code",
                    i64::from(status),
                ));
                let failed = status >= 400;
                if failed {
                    span.record("otel.status_code", "error");
                }
                failed.then(|| status.to_string())
            }
            Err(error) => {
                record_error(&span, error);
                Some("_OTHER".to_owned())
            }
        };
        if let Some(error_type) = error_type {
            span.record("error.type", error_type.as_str());
            attributes.push(KeyValue::new("error.type", error_type));
        }
        DURATION.record(seconds, &attributes);
        result
    }
}
