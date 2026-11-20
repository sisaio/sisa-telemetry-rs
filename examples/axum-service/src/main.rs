//! An orders service with traces, metrics and logs from one `sisa_telemetry::init`.
//!
//! `POST /orders` places an order in a custom span, counts it, and publishes `orders.placed` to
//! NATS with the span's trace context in the message headers. A consumer in the same process
//! handles each message in a span that starts a new trace and links back to the publisher's.
//!
//! ```sh
//! docker run --rm -p 4222:4222 nats:2-alpine
//! docker run --rm -p 4317:4317 otel/opentelemetry-collector-contrib:0.149.0
//! cargo run -p sisa-telemetry-example-axum-service
//! curl -X POST localhost:3000/orders
//! ```
//!
//! `NATS_URL` (`nats://localhost:4222`) and `PORT` (`3000`) say where things are, and the
//! standard `OTEL_*` variables configure the export, such as `OTEL_EXPORTER_OTLP_ENDPOINT`.

use std::{
    error::Error,
    sync::{
        LazyLock,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use axum::{Json, Router, extract::State, http::StatusCode, routing::post};
use futures_util::StreamExt;
use sisa_telemetry::{
    ServiceResource, TelemetrySettings, instruments,
    opentelemetry::{
        KeyValue,
        metrics::{Counter, Meter},
    },
    propagation::{self, Extractor, Injector},
    record_error,
};
use tracing::Instrument;

const SUBJECT: &str = "orders.placed";

// Created on first use, after `init` has installed the meter provider.
static METER: LazyLock<Meter> =
    LazyLock::new(|| instruments::meter(env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION")));
static ORDERS_PLACED: LazyLock<Counter<u64>> = LazyLock::new(|| {
    METER
        .u64_counter("orders.placed")
        .with_unit("{order}")
        .with_description("Orders placed")
        .build()
});

static NEXT_ORDER_ID: AtomicU64 = AtomicU64::new(1);

struct WriteHeaders<'a>(&'a mut async_nats::HeaderMap);

// Lets `propagation::inject` write into a NATS message's headers.
impl Injector for WriteHeaders<'_> {
    fn set(&mut self, key: &str, value: String) {
        self.0.insert(key, value.as_str());
    }
}

struct ReadHeaders<'a>(&'a async_nats::HeaderMap);

// Lets `propagation::link_from` read a NATS message's headers.
impl Extractor for ReadHeaders<'_> {
    fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key).map(|value| value.as_str())
    }

    fn keys(&self) -> Vec<&str> {
        self.0.iter().map(|(name, _)| name.as_ref()).collect()
    }
}

/// Publishes one order to NATS, in a producer span whose trace context rides in the headers.
async fn publish(nats: &async_nats::Client, order_id: u64) -> Result<(), async_nats::Error> {
    let span = tracing::info_span!(
        "publish",
        otel.name = %format_args!("{SUBJECT} publish"),
        otel.kind = "producer",
        messaging.system = "nats",
        messaging.destination.name = SUBJECT,
        messaging.operation.type = "send",
    );
    let mut headers = async_nats::HeaderMap::new();
    propagation::inject(&span, &mut WriteHeaders(&mut headers));

    let sent = async {
        nats.publish_with_headers(SUBJECT, headers, order_id.to_string().into())
            .await?;
        nats.flush().await?;
        Ok(())
    };
    sent.instrument(span).await
}

/// Handles the orders a subscription delivers, one span per message, until it ends.
async fn consume(mut orders: async_nats::Subscriber) {
    while let Some(message) = orders.next().await {
        let span = tracing::info_span!(
            "process",
            otel.name = %format_args!("{} process", message.subject),
            otel.kind = "consumer",
            messaging.system = "nats",
            messaging.destination.name = %message.subject,
            messaging.operation.type = "process",
        );
        // A message can wait in a queue for hours, so its span starts a new trace that links to
        // the publisher's instead of becoming its child.
        if let Some(headers) = &message.headers {
            propagation::link_from(&span, &ReadHeaders(headers));
        }

        span.in_scope(|| {
            let order_id = String::from_utf8_lossy(&message.payload);
            tracing::info!(order.id = %order_id, "order received");
        });
    }
}

/// Places an order: a span named for the operation, with the order's id recorded once it is
/// known, and a counter whose only attribute has a small fixed set of values.
#[tracing::instrument(skip(nats), fields(order.id = tracing::field::Empty))]
async fn place_order(nats: &async_nats::Client, channel: &'static str) -> Result<u64, StatusCode> {
    let order_id = NEXT_ORDER_ID.fetch_add(1, Ordering::Relaxed);
    tracing::Span::current().record("order.id", order_id);

    publish(nats, order_id).await.map_err(|error| {
        record_error(&tracing::Span::current(), error.as_ref());
        tracing::error!(error, "the order was not published");
        StatusCode::SERVICE_UNAVAILABLE
    })?;

    ORDERS_PLACED.add(1, &[KeyValue::new("channel", channel)]);
    Ok(order_id)
}

async fn create_order(State(nats): State<async_nats::Client>) -> Result<Json<u64>, StatusCode> {
    place_order(&nats, "web").await.map(Json)
}

async fn shutdown_signal() {
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .expect("the process can listen for SIGTERM");
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        _ = terminate.recv() => {}
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // First, so every instrument created later records into the installed provider.
    let telemetry = sisa_telemetry::init(
        &TelemetrySettings::default(),
        ServiceResource::new(env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION")),
    )?;

    let nats_url = std::env::var("NATS_URL").unwrap_or_else(|_| "nats://localhost:4222".into());
    let port = std::env::var("PORT").unwrap_or_else(|_| "3000".into());
    let nats = async_nats::connect(nats_url).await?;
    let consumer = tokio::spawn(consume(nats.subscribe(SUBJECT).await?));

    let router = Router::new()
        .route("/orders", post(create_order))
        .with_state(nats);
    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{port}")).await?;
    tracing::info!(port, "serving");
    axum::serve(listener, router)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    // After the server has stopped: send what the providers still hold. A Collector that is
    // down costs the last spans and metrics, not a failed exit.
    consumer.abort();
    if let Err(error) = telemetry.shutdown(Duration::from_secs(2)) {
        tracing::warn!(%error, "the last telemetry was not sent");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use sisa_telemetry::testing;
    use testcontainers::{
        GenericImage, ImageExt,
        core::{IntoContainerPort, WaitFor},
        runners::AsyncRunner,
    };

    use super::*;

    #[tokio::test]
    async fn a_consumed_message_links_to_its_publisher() {
        let captured = testing::capture();
        let named = std::env::var("NATS_IMAGE").unwrap_or_else(|_| "nats:2-alpine".to_owned());
        let (name, tag) = named.split_once(':').unwrap_or((named.as_str(), "latest"));
        let server = GenericImage::new(name, tag)
            .with_exposed_port(4222.tcp())
            .with_wait_for(WaitFor::message_on_stderr("Server is ready"))
            .with_startup_timeout(Duration::from_secs(60))
            .start()
            .await
            .expect("Docker must be running to start NATS for the test");
        let port = server.get_host_port_ipv4(4222).await.unwrap();
        // The server says it is ready a moment before Docker forwards the port.
        let nats = async_nats::ConnectOptions::new()
            .retry_on_initial_connect()
            .connect(format!("nats://127.0.0.1:{port}"))
            .await
            .unwrap();
        let orders = nats.subscribe(SUBJECT).await.unwrap();
        nats.flush().await.unwrap();
        let consumer = tokio::spawn(consume(orders));

        let order_id = place_order(&nats, "web").await.unwrap();

        let processed = async {
            loop {
                if let Some(span) = captured.span("orders.placed process") {
                    return span;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        };
        let consumed = tokio::time::timeout(Duration::from_secs(10), processed)
            .await
            .expect("the consumer handles the message");
        consumer.abort();

        let published = captured.span("orders.placed publish").unwrap();
        let placed = captured.span("place_order").unwrap();
        assert_eq!(order_id, 1);
        assert_eq!(published.parent_span_id, placed.span_context.span_id());
        assert_ne!(
            consumed.span_context.trace_id(),
            published.span_context.trace_id()
        );
        assert_eq!(consumed.links.len(), 1);
        assert_eq!(
            consumed.links[0].span_context.span_id(),
            published.span_context.span_id()
        );
        assert_eq!(captured.metric("orders.placed").unwrap().total(), 1.0);
    }
}
