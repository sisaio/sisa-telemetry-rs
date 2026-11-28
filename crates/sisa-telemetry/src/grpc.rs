//! The trace headers on outgoing gRPC and Connect calls: [`TraceClientLayer`].
//!
//! Incoming calls are `http::TraceLayer`'s, behind the `axum` feature.

use std::{
    future::Future,
    pin::Pin,
    sync::LazyLock,
    task::{Context, Poll},
    time::Instant,
};

use ::http::{Request, Response};
use opentelemetry::{KeyValue, metrics::Histogram};
use sisa_telemetry_core::{instruments, propagation};
use tracing::{Instrument, field::Empty};

use crate::carrier::{HeaderInjector, METER, rpc_path, rpc_system};

static DURATION: LazyLock<Histogram<f64>> = LazyLock::new(|| {
    instruments::duration_histogram(
        &METER,
        "rpc.client.call.duration",
        "Duration of RPC client calls",
    )
});

/// A tower layer for gRPC and Connect clients: each call gets a client span, and the trace
/// headers (`traceparent`, `tracestate`, `baggage`) of that span.
///
/// It wraps any tower service over `http::Request`, such as a tonic `Channel` or a tower-based
/// Connect client. The span is named `{service}/{method}`, from the request path, and carries
/// `rpc.system`, `rpc.service`, `rpc.method`, `server.address` and `server.port`. `rpc.system`
/// is `connect_rpc` for a request with an `application/connect+` content type or a
/// `connect-protocol-version` header, and `grpc` otherwise.
///
/// A `grpc-status` in the response headers is recorded as `rpc.grpc.status_code`, and any
/// status but OK marks the span as an error, as does a Connect response of 400 or more, or no
/// response. A status in trailers, after a streamed body, isn't read. Each call records
/// `rpc.client.call.duration`, in seconds, until the response headers arrive.
///
/// Without `init`, the span is a plain `tracing` span and no header is written.
///
/// ```ignore
/// let channel = tower::ServiceBuilder::new()
///     .layer(sisa_telemetry::grpc::TraceClientLayer::default())
///     .service(tonic::transport::Channel::from_static("http://orders:50051").connect_lazy());
/// let client = OrdersClient::new(channel);
/// ```
#[derive(Clone, Copy, Debug, Default)]
#[non_exhaustive]
pub struct TraceClientLayer;

impl<S> tower_layer::Layer<S> for TraceClientLayer {
    type Service = TraceClientService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        TraceClientService { inner }
    }
}

/// The service [`TraceClientLayer`] wraps around a client's service.
#[derive(Clone, Debug)]
pub struct TraceClientService<S> {
    inner: S,
}

impl<S, B, R> tower_service::Service<Request<B>> for TraceClientService<S>
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

    fn call(&mut self, mut request: Request<B>) -> Self::Future {
        let started = Instant::now();
        let system = rpc_system(request.headers()).unwrap_or("grpc");
        let path = request.uri().path().to_owned();
        let (service, method) = rpc_path(&path).unwrap_or((&path, ""));
        let (service, method) = (service.to_owned(), method.to_owned());
        let address = request.uri().host().unwrap_or_default().to_owned();
        let port = request.uri().port_u16();

        let span = tracing::info_span!(
            "RPC call",
            otel.kind = "client",
            otel.name = format!("{service}/{method}"),
            otel.status_code = Empty,
            rpc.system = system,
            rpc.service = service,
            rpc.method = method,
            server.address = address,
            server.port = port,
            rpc.grpc.status_code = Empty,
            "error.type" = Empty,
        );
        propagation::inject(&span, &mut HeaderInjector(request.headers_mut()));
        let future = span.in_scope(|| self.inner.call(request));

        Box::pin(async move {
            let result = future.instrument(span.clone()).await;

            let mut attributes = vec![
                KeyValue::new("rpc.system", system),
                KeyValue::new("rpc.service", service),
                KeyValue::new("rpc.method", method),
                KeyValue::new("server.address", address),
            ];
            if let Some(port) = port {
                attributes.push(KeyValue::new("server.port", i64::from(port)));
            }
            let error_type = match &result {
                Ok(response) => {
                    let grpc_status = response
                        .headers()
                        .get("grpc-status")
                        .and_then(|header| header.to_str().ok()?.parse::<u32>().ok());
                    if let Some(code) = grpc_status {
                        span.record("rpc.grpc.status_code", code);
                        attributes.push(KeyValue::new("rpc.grpc.status_code", i64::from(code)));
                    }
                    let status = response.status().as_u16();
                    match grpc_status {
                        Some(0) => None,
                        Some(code) => Some(code.to_string()),
                        None if status >= 400 => Some(status.to_string()),
                        None => None,
                    }
                }
                Err(_) => Some("_OTHER".to_owned()),
            };
            if let Some(error_type) = error_type {
                span.record("otel.status_code", "error");
                span.record("error.type", error_type.as_str());
                attributes.push(KeyValue::new("error.type", error_type));
            }
            DURATION.record(started.elapsed().as_secs_f64(), &attributes);
            result
        })
    }
}
