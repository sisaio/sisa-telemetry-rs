//! The tower layer for axum and tonic servers.

use std::{
    future::Future,
    pin::Pin,
    sync::LazyLock,
    task::{Context, Poll},
    time::{Duration, Instant},
};

use ::http::{Request, Response};
use axum::extract::MatchedPath;
use opentelemetry::{KeyValue, metrics::Histogram};
use sisa_telemetry_core::{instruments, propagation};
use tracing::{Instrument, field::Empty};

use crate::carrier::{HeaderExtractor, METER, method_name, rpc_path, rpc_system};

static HTTP_DURATION: LazyLock<Histogram<f64>> = LazyLock::new(|| {
    instruments::duration_histogram(
        &METER,
        "http.server.request.duration",
        "Duration of HTTP server requests",
    )
});

static RPC_DURATION: LazyLock<Histogram<f64>> = LazyLock::new(|| {
    instruments::duration_histogram(
        &METER,
        "rpc.server.call.duration",
        "Duration of RPC server calls",
    )
});

// The gRPC status codes that mean the server failed, per the OpenTelemetry gRPC conventions:
// UNKNOWN, DEADLINE_EXCEEDED, UNIMPLEMENTED, INTERNAL, UNAVAILABLE and DATA_LOSS.
const GRPC_SERVER_ERRORS: [u32; 6] = [2, 4, 12, 13, 14, 15];

// UNIMPLEMENTED: the service and method came from the caller and name nothing the server has.
const GRPC_UNIMPLEMENTED: u32 = 12;

/// A tower layer for servers: each request gets a server span that continues the caller's trace
/// from its `traceparent`, `tracestate` and `baggage` headers.
///
/// For HTTP, the span is named `{method} {route}`, with the route axum matched, and carries
/// `http.request.method`, `http.route`, `url.path` and `http.response.status_code`. A status of
/// 500 or more marks it as an error. Each request records `http.server.request.duration`.
///
/// A request with a `application/grpc` content type is a gRPC call, and one with a
/// `application/connect+` content type or a `connect-protocol-version` header is a Connect call.
/// Its span is named `{service}/{method}` and carries `rpc.system` (`grpc` or `connect_rpc`),
/// `rpc.service` and `rpc.method`, and, for gRPC, the `grpc-status` of a response that carries
/// it in its headers, as `rpc.grpc.status_code`. A status in trailers, after a streamed body,
/// isn't read. Each call records `rpc.server.call.duration`.
///
/// Durations are in seconds, until the response headers are ready. Without `init`, the span is a
/// plain `tracing` span.
///
/// Add it with `Router::layer` in axum, so the matched route is known, or with
/// `Server::builder().layer` in tonic.
///
/// ```
/// let app: axum::Router = axum::Router::new()
///     .route("/orders", axum::routing::get(|| async { "[]" }))
///     .layer(sisa_telemetry::http::TraceLayer::default());
/// ```
#[derive(Clone, Copy, Debug, Default)]
#[non_exhaustive]
pub struct TraceLayer;

impl<S> tower_layer::Layer<S> for TraceLayer {
    type Service = TraceService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        TraceService { inner }
    }
}

/// The service [`TraceLayer`] wraps around a server's service.
#[derive(Clone, Debug)]
pub struct TraceService<S> {
    inner: S,
}

// What kind of call a request is.
enum Call {
    Http {
        method: &'static str,
        route: Option<String>,
    },
    Rpc {
        system: &'static str,
        service: String,
        method: String,
    },
}

impl Call {
    fn of<B>(request: &Request<B>) -> Self {
        if let Some(system) = rpc_system(request.headers())
            && let Some((service, method)) = rpc_path(request.uri().path())
        {
            return Self::Rpc {
                system,
                service: service.to_owned(),
                method: method.to_owned(),
            };
        }
        let route = request.extensions().get::<MatchedPath>();
        Self::Http {
            method: method_name(request.method()),
            route: route.map(|route| route.as_str().to_owned()),
        }
    }

    fn span<B>(&self, request: &Request<B>) -> tracing::Span {
        let path = request.uri().path();
        match self {
            Self::Http { method, route } => {
                let name = match route {
                    Some(route) => format!("{method} {route}"),
                    None => (*method).to_owned(),
                };
                tracing::info_span!(
                    "HTTP request",
                    otel.kind = "server",
                    otel.name = name,
                    otel.status_code = Empty,
                    http.request.method = method,
                    http.route = route.as_deref(),
                    url.path = path,
                    http.response.status_code = Empty,
                    "error.type" = Empty,
                )
            }
            Self::Rpc {
                system,
                service,
                method,
            } => tracing::info_span!(
                "RPC call",
                otel.kind = "server",
                otel.name = format!("{service}/{method}"),
                otel.status_code = Empty,
                rpc.system = system,
                rpc.service = service,
                rpc.method = method,
                rpc.grpc.status_code = Empty,
                http.response.status_code = Empty,
                "error.type" = Empty,
            ),
        }
    }

    // Records the outcome on the span and the duration on the histogram.
    fn finish<R, E>(self, span: &tracing::Span, result: Result<&Response<R>, &E>, took: Duration) {
        let mut attributes = Vec::new();
        let (histogram, error_type) = match self {
            Self::Http { method, route } => {
                attributes.push(KeyValue::new("http.request.method", method));
                if let Some(route) = route {
                    attributes.push(KeyValue::new("http.route", route));
                }
                let error_type = result.ok().map_or(Some("_OTHER".to_owned()), |response| {
                    let status = response.status().as_u16();
                    span.record("http.response.status_code", status);
                    attributes.push(KeyValue::new(
                        "http.response.status_code",
                        i64::from(status),
                    ));
                    (status >= 500).then(|| status.to_string())
                });
                (&*HTTP_DURATION, error_type)
            }
            Self::Rpc {
                system,
                service,
                method,
            } => {
                attributes.push(KeyValue::new("rpc.system", system));
                let response = result.ok();
                let status = response.map(|response| response.status().as_u16());
                if let Some(status) = status {
                    span.record("http.response.status_code", status);
                }
                let grpc_status = response.and_then(|response| {
                    let header = response.headers().get("grpc-status")?;
                    header.to_str().ok()?.parse::<u32>().ok()
                });
                if let Some(code) = grpc_status {
                    span.record("rpc.grpc.status_code", code);
                    attributes.push(KeyValue::new("rpc.grpc.status_code", i64::from(code)));
                }
                if grpc_status != Some(GRPC_UNIMPLEMENTED) {
                    attributes.push(KeyValue::new("rpc.service", service));
                    attributes.push(KeyValue::new("rpc.method", method));
                }
                let error_type = match (status, grpc_status) {
                    (None, _) => Some("_OTHER".to_owned()),
                    (_, Some(code)) if GRPC_SERVER_ERRORS.contains(&code) => Some(code.to_string()),
                    (Some(status), _) if status >= 500 => Some(status.to_string()),
                    _ => None,
                };
                (&*RPC_DURATION, error_type)
            }
        };
        if let Some(error_type) = error_type {
            span.record("otel.status_code", "error");
            span.record("error.type", error_type.as_str());
            attributes.push(KeyValue::new("error.type", error_type));
        }
        histogram.record(took.as_secs_f64(), &attributes);
    }
}

impl<S, B, R> tower_service::Service<Request<B>> for TraceService<S>
where
    S: tower_service::Service<Request<B>, Response = Response<R>>,
    S::Future: Send + 'static,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<S::Response, S::Error>> + Send>>;

    fn poll_ready(&mut self, context: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(context)
    }

    fn call(&mut self, request: Request<B>) -> Self::Future {
        let started = Instant::now();
        let call = Call::of(&request);
        let span = call.span(&request);
        propagation::continue_from(&span, &HeaderExtractor(request.headers()));
        let future = span.in_scope(|| self.inner.call(request));
        Box::pin(async move {
            let result = future.instrument(span.clone()).await;
            call.finish(&span, result.as_ref(), started.elapsed());
            result
        })
    }
}
