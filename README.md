# sisa-telemetry-rs

[![CI](https://github.com/sisaio/sisa-telemetry-rs/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/sisaio/sisa-telemetry-rs/actions/workflows/ci.yml)
[![OpenSSF Scorecard](https://api.scorecard.dev/projects/github.com/sisaio/sisa-telemetry-rs/badge)](https://scorecard.dev/viewer/?uri=github.com/sisaio/sisa-telemetry-rs)
[![crates.io](https://img.shields.io/crates/v/sisa-telemetry.svg)](https://crates.io/crates/sisa-telemetry)
[![docs.rs](https://img.shields.io/docsrs/sisa-telemetry)](https://docs.rs/sisa-telemetry)
[![MSRV](https://img.shields.io/badge/MSRV-1.89-blue)](https://github.com/sisaio/sisa-telemetry-rs/blob/main/Cargo.toml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](https://github.com/sisaio/sisa-telemetry-rs/blob/main/LICENSE)

OpenTelemetry for Rust services: one `init()` for traces, metrics and logs over OTLP, plus trace
headers that carry across messaging, for the sisa crates.

## What you get

| Crate | For | Gives |
| --- | --- | --- |
| `sisa-telemetry` | Applications | `init`: JSON or pretty logs on stdout, OTLP traces and metrics to a Collector, the W3C propagators, and a guard that flushes on shutdown. Re-exports the core |
| `sisa-telemetry`, feature `testing` | Tests, as a dev-dependency | `testing::capture()`: in-memory exporters, to assert on spans and metrics |
| `sisa-telemetry-core` | Library crates | `propagation` for trace headers over messages and HTTP, `record_error`, the meter and duration-histogram helpers, the shared attribute names, and the `opentelemetry` API |

A service keeps writing plain [`tracing`](https://docs.rs/tracing) spans and events and uses the
OpenTelemetry metrics API; nothing in its code is sisa-specific. A library depends on the core
only, which has no SDK and no exporter, and does nothing until an application calls `init`.

## Quick start

```sh
cargo add sisa-telemetry
```

```rust,ignore
use std::{sync::LazyLock, time::Duration};

use sisa_telemetry::{
    ServiceResource, TelemetrySettings, instruments,
    opentelemetry::{KeyValue, metrics::Counter},
};

static ORDERS_PLACED: LazyLock<Counter<u64>> = LazyLock::new(|| {
    instruments::meter(env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"))
        .u64_counter("orders.placed")
        .with_unit("{order}")
        .with_description("Orders placed")
        .build()
});

#[tracing::instrument(skip(repo, cmd), fields(order.id = tracing::field::Empty))]
async fn place_order(repo: &OrderRepo, cmd: PlaceOrder) -> Result<Order, OrderError> {
    let order = repo.insert(&cmd).await.inspect_err(|error| {
        sisa_telemetry::record_error(&tracing::Span::current(), error);
    })?;
    tracing::Span::current().record("order.id", tracing::field::display(order.id));
    ORDERS_PLACED.add(1, &[KeyValue::new("channel", cmd.channel.as_str())]);
    Ok(order)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // First thing in `main`, before any instrument is used.
    let telemetry = sisa_telemetry::init(
        &TelemetrySettings::default(),
        ServiceResource::new(env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION")),
    )?;

    // ... serve ...

    // After the servers have stopped: send what is left.
    telemetry.shutdown(Duration::from_secs(2))?;
    Ok(())
}
```

By default this exports over gRPC to a Collector at `http://localhost:4317`, samples every trace,
pushes metrics every 30 seconds and prints JSON logs at `info`. The settings deserialize from a
`[telemetry]` section of a config file, and the standard `OTEL_*` environment variables win over
them; [docs/formats.md](https://github.com/sisaio/sisa-telemetry-rs/blob/main/docs/formats.md#settings)
lists every key.
[examples/axum-service](https://github.com/sisaio/sisa-telemetry-rs/tree/main/examples/axum-service)
is a whole service.

## Trace headers across messaging

A library that sends or receives messages depends on `sisa-telemetry-core` and adapts its header
type to `Injector` and `Extractor`. The producer writes the W3C headers; each consumed message
starts a new trace with a link to the producer, because a message can wait in a queue for hours.

```rust,ignore
use sisa_telemetry_core::propagation::{self, Extractor, Injector};

// publish
let span = tracing::info_span!("publish", otel.kind = "producer");
propagation::inject(&span, &mut WriteHeaders(&mut headers));

// consume, once per message
let span = tracing::info_span!("process", otel.kind = "consumer");
propagation::link_from(&span, &ReadHeaders(&message.headers));
handler.handle(message).instrument(span).await
```

`inject` with `continue_from` does the same for HTTP, where the caller's span becomes the parent.

## Testing instrumentation

```toml
[dev-dependencies]
sisa-telemetry = { version = "0.1", features = ["testing"] }
```

```rust,ignore
let captured = sisa_telemetry::testing::capture();

place_order("web");

assert!(captured.span("place_order").is_some());
assert_eq!(captured.metric("orders.placed").unwrap().total(), 1.0);
```

## More

- [Docs](https://github.com/sisaio/sisa-telemetry-rs/blob/main/docs/README.md): the
  architecture, each crate's spec, the formats, the flows, the instrumenting guide and the
  decisions.
- [Roadmap](https://github.com/sisaio/sisa-telemetry-rs/blob/main/ROADMAP.md) and
  [changelog](https://github.com/sisaio/sisa-telemetry-rs/blob/main/CHANGELOG.md).
- [Contributing](https://github.com/sisaio/sisa-telemetry-rs/blob/main/CONTRIBUTING.md):
  building, testing and conventions.
- [Security](https://github.com/sisaio/sisa-telemetry-rs/blob/main/SECURITY.md): reporting
  a vulnerability.

Rust 1.89 or newer. MIT licensed.
