# ADR-0006: Two crates: a core for libraries and a main crate for applications

- Status: **Accepted**
- Date: 2026-11-07, in the design; recorded here on 2026-11-21
- Deciders: Sang Au

## Context

Library crates such as sisa-messaging need trace headers, error recording and metric helpers.
Applications need the SDK, the exporters and the setup. Only an application may install global
providers, once.

## Decision

`sisa-telemetry-core` holds the API-only part and depends on the OpenTelemetry API, `tracing` and
`tracing-opentelemetry`: no SDK, no exporter, no broker, no HTTP client. `sisa-telemetry` holds
`init` and everything it needs, and re-exports the core. Libraries depend on the core only, never on
`sisa-telemetry`. The test helpers are the `testing` feature of the main crate, not a third crate.

## Consequences

- Libraries never pull in the SDK or the exporters.
- A breaking bump on the SDK side doesn't force a release of sisa-messaging.
- The two crates share one version and are released together.

## Alternatives considered

None recorded: the design gives the reason for this choice, not what it was weighed against.
