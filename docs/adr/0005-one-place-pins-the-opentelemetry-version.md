# ADR-0005: One place pins the OpenTelemetry version

- Status: **Accepted**
- Date: 2026-11-07, in the design; recorded here on 2026-11-21
- Deciders: Sang Au

## Context

The first-party OpenTelemetry crates release together and must share one minor version. Two
semver-incompatible versions in one binary are two crates with two sets of global providers, and a
crate on the one the application didn't initialize records into nothing, with no error. The tracing
API is still beta, so breaking minors keep coming. Checked on 2026-11-21: `opentelemetry` 0.33.1 is
current, and `tracing-opentelemetry` 0.34.0 pairs with it.

## Decision

`sisa-telemetry-core` pins the OpenTelemetry API for libraries and re-exports it. `sisa-telemetry`
pins the SDK and the exporters on the same minor and re-exports the core. A service depends on
neither OpenTelemetry crate directly. An upgrade is one coordinated change, sisa-telemetry first,
and CI in every sisa repository fails when a build holds two `opentelemetry` versions.

## Consequences

- Upgrading the beta tracing API happens in one place.
- Every OpenTelemetry minor is a breaking release of both crates, and each sisa repository follows
  it.
- The `opentelemetry` API is part of this library's public API.

## Alternatives considered

None recorded: the design gives the reason for this choice, not what it was weighed against.
