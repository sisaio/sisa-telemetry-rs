# Architecture decision records

Short records of significant decisions: why, not only what. The rules:

1. Copy [0000-template.md](0000-template.md) to the next number, such as `0009-short-title.md`.
2. Open it as **Proposed** in the PR that needs it. It becomes **Accepted** when the PR merges.
3. Never rewrite an accepted ADR. Write a new one and mark the old one **Superseded by
   ADR-XXXX**.
4. Write an ADR when a choice is hard to reverse, changes a rule, a data format or a dependency,
   or someone will ask "why did we do this?" in six months.

Don't re-propose an accepted decision without new evidence.

| # | Decision | Status |
| --- | --- | --- |
| [0001](0001-tracing-spans-and-the-opentelemetry-metrics-api.md) | `tracing` spans and events are the instrumentation API for traces and logs; the OpenTelemetry metrics API is the one for metrics | Accepted |
| [0002](0002-export-over-otlp-to-a-collector.md) | Traces and metrics leave the process over OTLP, gRPC or HTTP, to an OpenTelemetry Collector | Accepted |
| [0003](0003-logs-as-json-on-stdout.md) | Logs are always written to stdout, as JSON by default; exporting them over OTLP too is optional | Accepted |
| [0004](0004-metrics-are-pushed-over-otlp.md) | Metrics are pushed over OTLP by default; a Prometheus `/metrics` endpoint on the admin port is an option | Accepted |
| [0005](0005-one-place-pins-the-opentelemetry-version.md) | The core pins the OpenTelemetry API, sisa-telemetry pins the SDK and the exporters, and services depend on neither OpenTelemetry crate directly | Accepted |
| [0006](0006-two-crates-core-and-main.md) | Two crates: `sisa-telemetry-core`, API only, for libraries, and `sisa-telemetry` for applications; the test helpers are a feature of the main crate | Accepted |
| [0007](0007-a-consumed-message-links-to-its-producer.md) | The producer writes W3C trace headers; each consumed message starts a new trace with a link to the producer | Accepted |
| [0008](0008-minimum-rust-version.md) | The workspace's `rust-version` is 1.89 | Accepted |
