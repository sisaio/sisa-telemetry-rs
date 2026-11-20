# ADR-0003: Logs are JSON on stdout, always; OTLP logs are optional

- Status: **Accepted**
- Date: 2026-11-07, in the design; recorded here on 2026-11-21
- Deciders: Sang Au

## Context

Logs are what is left when everything else fails, so they can't depend on the exporter. Kubernetes
ships a container's stdout anyway.

## Decision

`init` always installs a layer that writes logs to stdout, as JSON, or pretty for development.
Exporting logs over OTLP as well, through `opentelemetry-appender-tracing`, is an option
(`log.otlp`) that is off by default and planned for 0.3.0.

## Consequences

- Logs survive an exporter outage.
- With `log.otlp` on, logs reach the backend twice, so an environment picks one path.

## Alternatives considered

None recorded: the design gives the reason for this choice, not what it was weighed against.
