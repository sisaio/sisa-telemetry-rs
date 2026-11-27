# Roadmap

What shipped is in the [changelog](CHANGELOG.md). The issues and their milestones show the work in
progress. Each milestone is a release, and the milestones on GitHub come from
[.github/milestones.yml](.github/milestones.yml).

| Milestone | Status | Goal |
| --- | --- | --- |
| 0.1.0 | Next | The core: `propagation`, `record_error`, the meter and duration-histogram helpers, the attribute names. The main crate: `init` with JSON or pretty logs, OTLP traces and metrics, the W3C propagators, the guard, and the `testing` feature. The [instrumenting guide](docs/instrumenting.md). All of it is built; the release is what is left |
| 0.2.0 | Planned | The metric catalog and the test that checks every sisa metric against it, the HTTP helpers (a reqwest middleware and an axum layer), and the Prometheus `/metrics` option |
| 0.3.0 | Planned | OTLP logs, changing the log filter at runtime, and the sisa-context fields on every event |
| 0.4.0 | Planned | Grafana dashboards, SLO and alert rules, and a reference Collector configuration |
| 1.0.0 | Planned | The stable API, after OpenTelemetry Rust tracing leaves beta, so the public API needs no other break |

After 0.1.0 is out, sisa-messaging adopts `sisa-telemetry-core`, and the examples of sisa-caching
and sisa-health-checks call `init` instead of their own OTLP setup.

## Not in 0.1.0

What the design names and a later milestone ships:

| Item | Milestone |
| --- | --- |
| A reqwest middleware and an axum layer over `inject` and `continue_from` | 0.2.0 |
| The `sisa.telemetry.export_failures` counter | Not set yet; export failures are log lines until then |

## Open questions

- **TLS to the Collector.** 0.1.0 builds the exporters without TLS, which suits a Collector
  beside the service. An `https` endpoint, as a SaaS backend has, needs a crypto provider (ring
  or aws-lc) picked for both the gRPC and the HTTP exporter.

## Later

Ideas that wait until users ask for them:

- Profiling, through Pyroscope.
