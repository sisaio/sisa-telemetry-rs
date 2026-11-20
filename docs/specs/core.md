# Crate: sisa-telemetry-core

Status: **Implemented** in 0.1.0 · Published: yes · Label: `area:core` · Commit scope: `core`

## Purpose

The small API-only part of Sisa Telemetry, for library crates such as sisa-messaging,
sisa-caching and sisa-health-checks: trace headers across messages and HTTP, error recording,
metric helpers and the shared attribute names. It depends on the OpenTelemetry API, `tracing`
and `tracing-opentelemetry`. It has no SDK, no exporter, no broker and no HTTP client, and it
installs nothing: that is the application's job, through
[`sisa-telemetry`](telemetry.md).

## Public contract

```rust
pub mod propagation {
    pub use opentelemetry::propagation::{Extractor, Injector};

    pub fn inject(span: &tracing::Span, headers: &mut dyn Injector);
    pub fn link_from(span: &tracing::Span, headers: &dyn Extractor);
    pub fn continue_from(span: &tracing::Span, headers: &dyn Extractor);
}

pub fn record_error(span: &tracing::Span, error: &dyn std::error::Error);

pub mod instruments {
    pub fn meter(scope: &'static str, version: &'static str) -> Meter;
    pub fn duration_histogram(meter: &Meter, name: &'static str, description: &'static str)
        -> Histogram<f64>;
}

pub mod attributes {
    pub const REQUEST_ID: &str = "sisa.request_id";
    pub const CORRELATION_ID: &str = "sisa.correlation_id";
    pub const TENANT: &str = "sisa.tenant";
    pub const ACTOR_TYPE: &str = "sisa.actor.type";
}

pub use opentelemetry;
```

| Item | For |
| --- | --- |
| `propagation::inject` | Producer and client side: writes the trace context of a span into headers |
| `propagation::link_from` | Consumer side: the span starts a new trace and links to the producer's span |
| `propagation::continue_from` | Server side (HTTP, gRPC): the caller's span becomes the span's parent |
| `propagation::{Injector, Extractor}` | The two traits a crate implements for its own header type. `HashMap<String, String>` has both already |
| `record_error` | Marks a span as failed and records the error with its source chain |
| `instruments::meter` | The meter of one crate; its scope is the crate's name and version |
| `instruments::duration_histogram` | A duration histogram in seconds with the standard buckets |
| `attributes` | The attribute names every sisa crate shares |
| `opentelemetry` | The OpenTelemetry API, re-exported, so a library and the application it runs in hold one version |

A crate with its own header type adapts it without the core knowing the broker:

```rust
struct Headers<'a>(&'a mut async_nats::HeaderMap);

impl Injector for Headers<'_> {
    fn set(&mut self, key: &str, value: String) {
        self.0.insert(key, value.as_str());
    }
}
```

## Behaviour

`inject` takes the span's OpenTelemetry context and hands it to the global propagator, which
writes the [trace headers](../formats.md#trace-headers). Asking for the context starts the span,
so set its parent or link first.

`link_from` reads the headers with the global propagator. It gives the span a parent context that
holds the headers' baggage and no span, which makes the span the root of a new trace, and adds a
link to the producer's span when the headers carry a valid one.

`continue_from` reads the headers the same way and, when they carry a valid span, makes it the
span's parent, baggage included. Otherwise it leaves the span as it is.

Both must run before the span is entered. On a span that has started they log one warning and
change nothing. [Flows](../flows.md) draws both.

`record_error` joins the error and each `source()` under it with `: `, sets the span's status to
`Error` with that text, and adds an [`exception` event](../formats.md#errors-on-spans). It writes
no log line.

`meter` asks the global meter provider for a meter with the given scope and version.
`duration_histogram` builds an `f64` histogram with the unit `s` and the
[standard buckets](../formats.md#metrics), 5 ms to 10 s.

## Guarantees and limits

The library's guarantees are in [Architecture](../architecture.md#guarantees-and-limits). This
crate's own:

- Without `init`, nothing is written, linked, recorded or logged, and nothing panics.
- No function here installs a provider, a propagator or a subscriber.
- A span the log filter turned off is skipped silently: it has no OpenTelemetry data to change.
- An instrument made by `meter` before the application's `init` records nothing for the life of
  the process. Create instruments lazily, on first use.

## Errors

None: every function returns nothing and can't fail.

## Formats

The [trace headers](../formats.md#trace-headers), the
[attribute names](../formats.md#attribute-names), the
[`exception` event](../formats.md#errors-on-spans) and the
[duration buckets](../formats.md#metrics).
