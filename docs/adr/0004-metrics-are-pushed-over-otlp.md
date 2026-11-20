# ADR-0004: Metrics are pushed over OTLP; a Prometheus endpoint is an option

- Status: **Accepted**
- Date: 2026-11-07, in the design; recorded here on 2026-11-21
- Deciders: Sang Au

## Context

Some teams push metrics and some scrape them.

## Decision

Metrics are pushed over OTLP every `metrics.interval`, 30 seconds by default. A `prometheus` feature
that serves the text format for an admin port's `/metrics` is planned for 0.2.0, if
`opentelemetry-prometheus` is still released on the same minor; otherwise the feature is dropped and
scraping goes through the Collector.

## Consequences

- Fits both push and pull shops.
- Counter names carry no `_total` suffix in code: the Prometheus exporter adds it.

## Alternatives considered

None recorded: the design gives the reason for this choice, not what it was weighed against.
