# ADR-0001: `tracing` spans and events, and the OpenTelemetry metrics API

- Status: **Accepted**
- Date: 2026-11-07, in the design; recorded here on 2026-11-21
- Deciders: Sang Au

## Context

A service needs one way to write spans, events and metrics. The whole Rust ecosystem a service is
built on (axum, tonic, sqlx, reqwest) already emits `tracing`.

## Decision

Services and libraries write spans and events with `tracing`, with nothing sisa-specific, and
sisa-telemetry turns them into OpenTelemetry spans and log lines. Metrics are written with the
OpenTelemetry metrics API, which the core re-exports.

## Consequences

- A service's own spans join the ones its dependencies already emit, in one trace.
- Code stays free of sisa types: removing sisa-telemetry changes `main`, not the handlers.
- Two APIs to learn, one for spans and one for metrics.
- Span details OpenTelemetry has and `tracing` doesn't, such as the kind and the display name, go
  through the `otel.kind` and `otel.name` fields.

## Alternatives considered

None recorded: the design gives the reason for this choice, not what it was weighed against.
