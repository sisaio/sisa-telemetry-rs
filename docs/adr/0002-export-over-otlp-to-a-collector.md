# ADR-0002: Export over OTLP to an OpenTelemetry Collector

- Status: **Accepted**
- Date: 2026-11-07, in the design; recorded here on 2026-11-21
- Deciders: Sang Au

## Context

Telemetry has to reach a backend, and the choice of backend differs per team and changes over time.
sisa-telemetry is not a backend; Grafana's stack, Jaeger or Tempo, Honeycomb, Datadog and
OpenObserve all take OTLP.

## Decision

The only exporters are OTLP ones, over gRPC or HTTP with protobuf, pointed at an OpenTelemetry
Collector. The Collector handles batching, tail sampling and routing to any backend, and holds the
redaction rules. No exporter for one vendor is added.

## Consequences

- Swapping a backend is a Collector change, with no new build of the service.
- Tail sampling can keep every error and slow trace while the service samples by ratio.
- A deployment needs a Collector, as a sidecar or a DaemonSet.

## Alternatives considered

None recorded: the design gives the reason for this choice, not what it was weighed against.
