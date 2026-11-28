# Instrumenting guide

Short recipes for a service or a library on sisa-telemetry. Each is backed by a test, named under
it, so a recipe that stops working fails CI. A service uses `sisa_telemetry`, and a library the
same items from `sisa_telemetry_core`.

## Rules for custom spans and metrics

- Spans come from `tracing`, with nothing sisa-specific. Name a span for the operation
  (`place_order`), never for a value; values go in fields.
- `skip` request bodies, secrets and whole structs; record ids as fields.
- A task started with `tokio::spawn` keeps its parent span only with `.instrument(span)`.
- `init` runs before any instrument is first used: an instrument created before the provider is
  installed stays a no-op for the life of the process. In `main`, call `init` first; in tests,
  call `testing::capture()` first.
- Metric attributes take a small fixed set of values (channel, status, outcome). Ids go on
  spans, never on metrics; `metrics.cardinality_limit` folds accidents into an overflow series.
- Names: `<domain>.<thing>` in services, `sisa.<crate>.<thing>` in sisa crates; units in UCUM
  (`s`, `By`, `{order}`); durations are histograms in seconds.
- Long-running loops (consumers, pollers) open one span per unit of work, never one around the
  loop.
- Put DEBUG events on hot paths behind `tracing::enabled!`, and quiet a noisy dependency with a
  per-target filter such as `sqlx=warn`.

## 1. A custom span, a field recorded later, and `record_error`

`#[tracing::instrument]` opens a span named after the function, with its arguments as fields.
Declare a field as `Empty` to fill it in once its value is known, and mark the span as failed
where the error is handled.

```rust
#[tracing::instrument(skip(repo, cmd), fields(order.id = tracing::field::Empty))]
async fn place_order(repo: &OrderRepo, cmd: PlaceOrder) -> Result<Order, OrderError> {
    let order = repo.insert(&cmd).await.inspect_err(|error| {
        sisa_telemetry::record_error(&tracing::Span::current(), error);
    })?;
    tracing::Span::current().record("order.id", tracing::field::display(order.id));
    Ok(order)
}
```

`record_error` sets the span's status to `Error` and adds an `exception` event with the error
and its sources. It writes no log line: log where you handle the error.

Tests: `a_field_recorded_later_is_on_the_span`,
`record_error_fails_the_span_with_the_source_chain`.

## 2. A custom counter and duration histogram, and why `init` comes first

Instruments live in statics that are created on first use, which is after `init`.

```rust
use std::sync::LazyLock;

use sisa_telemetry::{
    instruments,
    opentelemetry::{KeyValue, metrics::{Counter, Histogram, Meter}},
};

static METER: LazyLock<Meter> =
    LazyLock::new(|| instruments::meter(env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION")));
static ORDERS_PLACED: LazyLock<Counter<u64>> = LazyLock::new(|| {
    METER.u64_counter("orders.placed").with_unit("{order}").with_description("Orders placed").build()
});
static PLACE_DURATION: LazyLock<Histogram<f64>> = LazyLock::new(|| {
    instruments::duration_histogram(&METER, "orders.place.duration", "Time to place an order")
});

let started = std::time::Instant::now();
// ... place the order ...
ORDERS_PLACED.add(1, &[KeyValue::new("channel", "web")]);
PLACE_DURATION.record(started.elapsed().as_secs_f64(), &[KeyValue::new("channel", "web")]);
```

An instrument asks the global meter provider for its storage when it is built. Before `init`
that provider is a no-op, and the instrument keeps the no-op for the life of the process, with
no error. So `init` is the first line of `main`, and nothing builds an instrument in a `const`
or before it.

Tests: `capture_sees_a_counter_added_after_it`, `a_duration_histogram_has_the_standard_buckets`.

## 3. Keeping a span across `tokio::spawn`

A spawned task starts with no span. Hand it one with `.instrument`:

```rust
use tracing::Instrument;

let span = tracing::info_span!("send_receipt");
tokio::spawn(send_receipt(order).instrument(span));

// Or keep the caller's span as the parent:
tokio::spawn(send_receipt(order).in_current_span());
```

Test: `a_spawned_task_keeps_its_span_only_when_instrumented`.

## 4. Trace headers over a message

sisa-messaging does this inside publish and consume, so handlers never touch headers. For a raw
NATS publish, adapt the header type once and call `inject` and `link_from`:

```rust
use sisa_telemetry::propagation::{self, Extractor, Injector};

struct WriteHeaders<'a>(&'a mut async_nats::HeaderMap);

impl Injector for WriteHeaders<'_> {
    fn set(&mut self, key: &str, value: String) {
        self.0.insert(key, value.as_str());
    }
}

struct ReadHeaders<'a>(&'a async_nats::HeaderMap);

impl Extractor for ReadHeaders<'_> {
    fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key).map(|value| value.as_str())
    }

    fn keys(&self) -> Vec<&str> {
        self.0.iter().map(|(name, _)| name.as_ref()).collect()
    }
}

// publish
let span = tracing::info_span!("publish",
    otel.name = %format_args!("{subject} publish"), otel.kind = "producer",
    messaging.system = "nats", messaging.destination.name = %subject,
    messaging.operation.type = "send");
let mut headers = async_nats::HeaderMap::new();
propagation::inject(&span, &mut WriteHeaders(&mut headers));
client.publish_with_headers(subject, headers, payload).instrument(span).await?;

// consume, once per message
let span = tracing::info_span!("process",
    otel.name = %format_args!("{} process", message.subject), otel.kind = "consumer",
    messaging.system = "nats", messaging.destination.name = %message.subject,
    messaging.operation.type = "process");
if let Some(headers) = &message.headers {
    propagation::link_from(&span, &ReadHeaders(headers));
}
handler.handle(message).instrument(span).await
```

The consumer's span starts a new trace with a link to the producer's, because a message can
wait in a queue for hours ([the flow](flows.md#a-message-from-producer-to-consumer)). Call
`link_from` before the span is entered. With no `init`, `inject` writes nothing and `link_from`
adds nothing.

Tests: `a_consumer_span_starts_a_new_trace_linked_to_the_producer`,
`without_init_inject_writes_no_header`, and `a_consumed_message_links_to_its_publisher` in
[examples/axum-service](../examples/axum-service/src/main.rs), against a real NATS server.

## 5. Outbound and inbound HTTP

The same `inject`, with `continue_from` on the server: an HTTP call is one trace, and the
server's span is the caller's child.

```rust
// client: `http::HeaderMap` needs the same two small adapters as in recipe 4
let span = tracing::info_span!("GET", otel.kind = "client", http.request.method = "GET");
propagation::inject(&span, &mut WriteHeaders(request.headers_mut()));
let response = client.execute(request).instrument(span).await?;

// server: before the handler runs inside the span
let span = tracing::info_span!("request", otel.kind = "server", http.route = "/orders");
propagation::continue_from(&span, &ReadHeaders(request.headers()));
next.run(request).instrument(span).await
```

With the `reqwest` and `axum` features, `http::TracingMiddleware` and `http::TraceLayer` do this,
with the span names, attributes and duration metrics of the OpenTelemetry conventions.
`TraceLayer` also serves tonic, through `Server::builder().layer`, and gives gRPC and Connect
calls the RPC conventions.

```rust
let client = reqwest_middleware::ClientBuilder::new(reqwest::Client::new())
    .with(sisa_telemetry::http::TracingMiddleware::default())
    .build();
let app = axum::Router::new()
    .route("/orders", get(list_orders))
    .layer(sisa_telemetry::http::TraceLayer::default());
```

Test: `a_server_span_continues_the_callers_trace`.

## 6. Testing instrumentation

Add the `testing` feature as a dev-dependency, call `capture()` first, and assert on what the
code under test recorded.

```toml
[dev-dependencies]
sisa-telemetry = { version = "0.1", features = ["testing"] }
```

```rust
#[test]
fn placing_an_order_is_counted() {
    let captured = sisa_telemetry::testing::capture();

    place_order("web");

    let span = captured.span("place_order").unwrap();
    assert!(span.attributes.iter().any(|each| each.key.as_str() == "order.id"));
    let placed = captured.metric("orders.placed").unwrap();
    assert_eq!(placed.unit, "{order}");
    assert_eq!(placed.total(), 1.0);
}
```

- A span is captured when its last handle drops: leave its scope, or drop it, before looking.
- `capture()` installs once per test binary, and tests of one binary share it under
  `cargo test`. Use names of their own per test, or cargo-nextest, which runs each test in its
  own process.
- A binary calls `capture()` or `init`, never both.

- `clear()` empties what every test of the process captured: keep a test that calls it in a
  binary of its own.

Tests: `capture_sees_a_counter_added_after_it`, `clear_forgets_the_captured_spans`.

## 7. What never goes into telemetry

| Never | Do instead |
| --- | --- |
| Request and response bodies | `skip` them in `#[instrument]`; record an id or a size |
| Secrets: passwords, tokens, API keys, card numbers | `skip` them; wrap them in a type whose `Debug` is masked |
| Whole structs through `Debug` | `skip` the struct and record the fields you need |
| SQL bind values | Record the parameterized statement only, never the values |
| Personal data in baggage | Baggage crosses process boundaries in clear text; carry an opaque id |
| Ids as metric attributes | Put ids on spans and logs; metric attributes take a small fixed set of values |

`#[instrument]` records every argument it isn't told to skip, so `skip` is the default for
anything that isn't a short id. The Collector's redaction rules are a second line of defense,
not the first.

Test: `a_skipped_argument_is_not_on_the_span`.
