# ADR-0008: The minimum Rust version is 1.89

- Status: **Accepted**
- Date: 2026-11-21
- Deciders: Sang Au

## Context

The workspace has one `rust-version` for every package. The OpenTelemetry crates at 0.33 need Rust
1.75, and edition 2024 needs 1.85. The example service uses async-nats 0.52, which needs 1.89, as
checked on 2026-11-21. sisa-health-checks is on 1.94 and sisa-messaging on 1.95, so 1.89 holds no
sisa crate back.

## Decision

The workspace's `rust-version` is 1.89, and CI builds every package at it.

## Consequences

- An application on a compiler older than 1.89 can't use either crate, though the two libraries
  alone would build on 1.85.
- Moving the minimum up is called out in the changelog.

## Alternatives considered

- **A lower minimum for the two library crates:** one version for the workspace is simpler to test
  and to state.
