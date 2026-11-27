# Crate: sisa-telemetry

Status: **Implemented** in 0.1.0 · Published: yes · Label: `area:telemetry` · Commit scope: `telemetry`

## Purpose

The crate applications depend on. One `init` sets up logs on stdout and OTLP traces and metrics,
installs the W3C propagators, and returns a guard that flushes on shutdown. It re-exports
[the core](core.md), so a service depends on no OpenTelemetry crate directly. It is not a
backend, and library crates never depend on it: it installs process-wide providers, which only
the application may do, once.

## Public contract

```rust
pub use sisa_telemetry_core::*;

pub fn init(settings: &TelemetrySettings, resource: ServiceResource)
    -> Result<TelemetryGuard, TelemetryError>;

pub fn reload_log_filter(filter: &str) -> Result<(), TelemetryError>;

pub struct ServiceResource { /* ... */ }
impl ServiceResource {
    pub fn new(name: &'static str, version: &'static str) -> Self;
    pub fn with_namespace(self, namespace: impl Into<String>) -> Self;
    pub fn with_environment(self, environment: impl Into<String>) -> Self;
    pub fn with_attribute(self, key: impl Into<String>, value: impl Into<String>) -> Self;
}

pub struct TelemetryGuard { /* ... */ }
impl TelemetryGuard {
    pub fn shutdown(self, budget: Duration) -> Result<(), TelemetryError>;
}

#[cfg(feature = "testing")]
pub mod testing {
    pub fn capture() -> Captured;
    // Captured::spans(), ::span(name), ::metric(name), ::clear()
}
```

| Item | For |
| --- | --- |
| `init` | Sets everything up, once, first thing in `main` |
| `reload_log_filter` | Replaces the `log.filter` that `init` installed, as for an admin endpoint that turns on `debug` while an incident lasts |
| `TelemetrySettings` and `OtlpSettings`, `TracesSettings`, `MetricsSettings`, `LogSettings`, `ResourceSettings` | The `[telemetry]` section of a config file, as [Formats](../formats.md#settings) lists it. All deserialize, all have defaults, all are `#[non_exhaustive]` |
| `OtlpProtocol`, `TracesSampler`, `LogFormat` | The values of `otlp.protocol`, `traces.sampler` and `log.format` |
| `ServiceResource` | The service's name and version, and optionally its namespace, environment and extra attributes |
| `TelemetryGuard` | Flushes and shuts the providers down on drop, or with `shutdown(budget)`, where the budget bounds the wait for the spans |
| `TelemetryError` | Why `init` or `shutdown` failed |
| Feature `testing`: `testing::capture`, `Captured`, `CapturedMetric`, `CapturedPoint`, `SpanData` | In-memory exporters for tests, as a dev-dependency |

## Behaviour

`init` does these in order, and fails before installing anything:

1. Parses `log.filter`.
2. With `enabled = true`: picks the protocol, builds the [resource](../formats.md#resource), and
   builds the two exporters and their providers.
   - Traces: a batch span processor, and the sampler from `traces.sampler` and `traces.ratio`.
   - Metrics: a periodic reader that pushes every `metrics.interval`, and a view that gives
     every instrument the `metrics.cardinality_limit`.
3. Installs the global `tracing` subscriber: the filter, the fmt layer (JSON or pretty, on
   stdout), and the layer that turns spans into OpenTelemetry spans. That layer leaves out the
   spans of the crates the exporters run on (`opentelemetry*`, `tonic`, `hyper`, `hyper_util`,
   `reqwest`, `h2`), so an export doesn't trace itself.
4. Sets the global tracer provider, meter provider and propagators: W3C trace context, then
   baggage.

With `enabled = false` only the filter and the fmt layer are installed: no provider, no
propagator, no exporter.

`init` keeps a handle to the filter it installs, and `reload_log_filter` parses a new one and
swaps it in for the whole process, spans included. A filter that doesn't parse leaves the
current one in place. Under `testing::capture`, or before `init`, there is no filter to swap.

Where an `OTEL_*` variable matches a setting, the variable wins
([Formats](../formats.md#settings)). The SDK and the exporters read most of them themselves, but
a value given in code would win over them, so `init` passes a setting on only when its variable
is unset. `OTEL_EXPORTER_OTLP_PROTOCOL` and `OTEL_SERVICE_NAME` it reads itself.

An HTTP exporter takes an endpoint given in code as it is, so `init` adds `/v1/traces` and
`/v1/metrics` to `otlp.endpoint`. The headers go out as gRPC metadata, marked sensitive, or as
HTTP headers.

`TelemetryGuard::shutdown(budget)` calls the tracer provider's `shutdown_with_timeout(budget)`,
which flushes the span queue, and then the meter provider's, which exports once more. In
OpenTelemetry 0.33 the meter provider ignores the timeout and waits up to a fixed 5 seconds, so
the call can take the budget plus 5 seconds. Dropping the guard does the same with a budget of 5
seconds. Whatever isn't sent by then is lost, and `shutdown` says so with `Shutdown`.

`shutdown` blocks the calling thread. With the gRPC exporter the last export runs on the Tokio
runtime: calling it from a multi-thread runtime works, directly or through `spawn_blocking`,
which is what the example and the tests do. On a current-thread runtime it isn't tested, and a
direct call is expected to wait out its budget, since it blocks the only thread that could run
the export.

`testing::capture()` installs, once per test binary, a tracer provider with an in-memory
exporter that takes each span as it ends, a meter provider with an in-memory exporter and a
cardinality limit of 2000, the same OpenTelemetry layer and the same propagators. The sampler is
the SDK's default: every new trace is kept, and a span continued from headers that say "not
sampled" is not. It installs no log filter and no fmt layer, so every span is created and no
log line is printed. Later calls return
the same handle. `Captured::metric` flushes the meter provider and reads the last export.

## Guarantees and limits

The library's guarantees are in [Architecture](../architecture.md#guarantees-and-limits). This
crate's own:

- `init` either installs everything or nothing: every step that can fail runs before the
  subscriber is installed. A failed `init` may have built exporters, which are dropped.
- A header value never appears in `Debug`, in an error or in a log line of this crate.
- The settings' field names and defaults are part of the public API.
- The exporters are built without TLS in 0.1.0, so an `https` endpoint doesn't work.
- Only `tracing` events reach the logs: no bridge from the `log` crate is installed.
- The log lines carry the fields of the spans they are in, not the trace id.
- `testing::capture` and `init` can't share a process: each installs the global subscriber.
- Tests in one binary share what `capture` holds when they run in one process, as under
  `cargo test`. Give each test's spans and metrics names of their own.

## Errors

| Type | Says |
| --- | --- |
| `TelemetryError::LogFilter` | `log.filter`, or the filter given to `reload_log_filter`, isn't valid filter syntax |
| `TelemetryError::Header` | A name or value in `otlp.headers` can't be sent; it names the header and leaves the value out |
| `TelemetryError::Protocol` | `OTEL_EXPORTER_OTLP_PROTOCOL` names a protocol other than `grpc` and `http/protobuf` |
| `TelemetryError::NoRuntime` | The gRPC protocol was picked and `init` was called outside a Tokio runtime |
| `TelemetryError::Exporter` | An exporter couldn't be built, as from an endpoint that isn't a URL |
| `TelemetryError::AlreadyInitialized` | A global `tracing` subscriber is installed already |
| `TelemetryError::NotInitialized` | `reload_log_filter` was called with no subscriber from `init` installed |
| `TelemetryError::Shutdown` | A provider reported a failed or timed-out shutdown, as when the Collector is down; what it still held is lost |

The enum is `#[non_exhaustive]`. A Collector that is down is not an error from `init`: export
failures are log lines from the `opentelemetry` crates.

## Formats

The [settings](../formats.md#settings), the [resource](../formats.md#resource) and the
[trace headers](../formats.md#trace-headers) the installed propagators write.

## Open questions

- Which crypto provider the exporters use for TLS, so an `https` endpoint works
  ([roadmap](../../ROADMAP.md#open-questions)).
