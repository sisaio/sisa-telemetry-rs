# Formats

What other tools, languages and config files see: the headers `inject` writes, the attribute
names, the resource, the naming conventions for spans and metrics, and the settings. They are
part of the public API, and a change to one is breaking.

## Trace headers

`propagation::inject` writes the W3C headers, and `link_from` and `continue_from` read them, so
services in other languages join the same traces.

| Header | Holds | Standard |
| --- | --- | --- |
| `traceparent` | `00-<trace id>-<span id>-<flags>`; flag `01` means sampled | [W3C Trace Context](https://www.w3.org/TR/trace-context/) |
| `tracestate` | Vendor-specific trace state, passed along unchanged | W3C Trace Context |
| `baggage` | `key=value` pairs, comma-separated | [W3C Baggage](https://www.w3.org/TR/baggage/) |

`baggage` is written only when it holds something; `tracestate` is always written, and is empty
when there is none. Baggage crosses process boundaries in clear text: never put secrets or
personal data in it.

## Attribute names

Follow the OpenTelemetry semantic conventions where one exists (HTTP, database, messaging, RPC).
Everything sisa-specific uses the `sisa.<crate>.*` prefix. The names every sisa crate shares are
constants in `sisa_telemetry_core::attributes`:

| Constant | Attribute | Holds |
| --- | --- | --- |
| `REQUEST_ID` | `sisa.request_id` | The id of one inbound request |
| `CORRELATION_ID` | `sisa.correlation_id` | The id of one business flow, across every message it causes |
| `TENANT` | `sisa.tenant` | The tenant the work is done for |
| `ACTOR_TYPE` | `sisa.actor.type` | The kind of actor behind the request |

Their values are unbounded, so they go on spans and logs, never on metrics. The actor's id uses
the OpenTelemetry name `enduser.id`.

## Errors on spans

`record_error` sets the span's status to `Error`, with the error and its sources joined by `: `
as the description, and adds one event:

| Event | Attribute | Holds |
| --- | --- | --- |
| `exception` | `exception.message` | The same text as the status description |

## Resource

`init` puts these on every span and metric. Later rows win over earlier ones for the same key.

| Source | Attributes |
| --- | --- |
| The SDK | `telemetry.sdk.name`, `telemetry.sdk.language`, `telemetry.sdk.version` |
| `ServiceResource::new(name, version)` | `service.name`, `service.version` |
| `with_namespace`, `with_environment`, `with_attribute` | `service.namespace`, `deployment.environment.name`, and the extra ones |
| `resource.attributes` in the settings | Any |
| `OTEL_RESOURCE_ATTRIBUTES` | Any, as `key=value,key=value` |
| `OTEL_SERVICE_NAME` | `service.name` |

Spans carry the instrumentation scope `sisa-telemetry` with its version. A metric's scope is the
name and version given to `instruments::meter`.

## Metrics

| Rule | Example |
| --- | --- |
| Names are `<domain>.<thing>` in services and `sisa.<crate>.<thing>` in sisa crates | `orders.placed`, `sisa.outbox.pending` |
| Units are UCUM | `s`, `By`, `{order}` |
| Durations are histograms in seconds | `messaging.process.duration` |
| Counters end in a plural noun, with no `_total` suffix in code; a Prometheus exporter adds it | `sisa.jobs.started` |
| Attributes take a small fixed set of values; ids never | `channel`, `status`, `outcome` |
| The tenant is a metric attribute only where the number of tenants is bounded | |

`instruments::duration_histogram` makes a histogram with the unit `s` and these bucket
boundaries, 5 ms to 10 s, the ones the OpenTelemetry HTTP conventions recommend:

```text
0.005 0.01 0.025 0.05 0.075 0.1 0.25 0.5 0.75 1 2.5 5 7.5 10
```

## Span and metric names per crate

The conventions the sisa crates follow, so one dashboard reads all of them. Each crate emits its
own; this repository holds the list. The metrics are also a file,
[crates/sisa-telemetry/catalog/metrics.json](../crates/sisa-telemetry/catalog/metrics.json), with
each metric's name, unit and allowed attributes. A sisa crate's tests call
`testing::Captured::check_catalog()`, which fails on a `sisa.*` metric missing from the file, a
unit that differs, or an attribute the file doesn't list. `sisa.tenant` and
`otel.metric.overflow` are allowed on every metric. The table is the summary; the file is what a
crate must match, and a new metric goes into both in the PR that adds it.

| Crate | Span (name) | Key span attributes | Metrics (unit) | Metric attributes |
| --- | --- | --- | --- | --- |
| HTTP server (axum) | `{method} {route}`, per the OpenTelemetry HTTP conventions | `http.request.method`, `http.route`, `http.response.status_code` | `http.server.request.duration` (s, histogram) | method, route, status class |
| sqlx | `db {operation} {table}` | `db.system.name=postgresql`, `db.operation.name`, `db.collection.name`; the statement only with `capture_sql` | `db.client.operation.duration` (s); `sisa.db.pool.in_use`, `.idle`, `.wait_duration` | operation, pool |
| sisa-idempotency | `idempotency.claim` | `sisa.idempotency.outcome` (`acquired`, `replay`, `in_progress`, `mismatch`) | `sisa.idempotency.outcomes` (count) | outcome, route |
| sisa-ratelimit | `ratelimit.check` | policy, decision, cost | `sisa.ratelimit.decisions` (count), `.check.duration` (s) | policy, decision |
| sisa-resilience | `resilience.attempt` (a child per attempt) | pipeline, attempt, outcome | `sisa.resilience.retries`, `.breaker.state` (gauge), `.rejected`, `.timeouts` | pipeline, strategy, state |
| sisa-messaging | `{destination} publish` and `{destination} process`, per the OpenTelemetry messaging conventions, linked across the broker | `messaging.system`, `messaging.destination.name`, `messaging.message.id` | `sisa.outbox.pending` (gauge), `.oldest_age` (s), `messaging.process.duration` (s), `sisa.inbox.duplicates` | destination, outcome |
| sisa-scheduler | `job {kind}` | job id, attempt, queue | `sisa.jobs.started`, `.succeeded`, `.failed`, `.dead`, `.duration` (s), `.queue_lag` (s, gauge) | kind, queue |
| sisa-lock | `lock.acquire {name}` | name, fence, waited | `sisa.lock.wait_duration` (s), `.lost` | lock name (a bounded set) |
| sisa-audit | `audit.write` | entity type, action | `sisa.audit.records` | entity type |
| sisa-cache | `cache.get` (sampled) | hit, store | `sisa.cache.requests` | store, result (hit, miss, error) |
| sisa-runtime | `worker.run {name}` | restarts | `sisa.worker.restarts`, `sisa.runtime.event_loop_lag` (s) | worker |

## Settings

`TelemetrySettings` deserializes from a `[telemetry]` section; a key left out keeps its default.
A standard `OTEL_*` environment variable, when set and not empty, wins over the key beside it,
even when its value is one the SDK can't use: the SDK then falls back to its own default, not to
the key.

| Key | Default | Meaning | Environment variable that wins |
| --- | --- | --- | --- |
| `enabled` | `true` | Off leaves the logs on stdout only: no provider, no propagator, no exporter | |
| `otlp.endpoint` | `http://localhost:4317` for gRPC, `http://localhost:4318` for HTTP | The Collector's base URL, usually a sidecar or a DaemonSet. Over HTTP, `/v1/traces` and `/v1/metrics` are added to it | `OTEL_EXPORTER_OTLP_ENDPOINT`, `OTEL_EXPORTER_OTLP_TRACES_ENDPOINT`, `OTEL_EXPORTER_OTLP_METRICS_ENDPOINT` |
| `otlp.protocol` | `grpc` | `grpc` or `http/protobuf` | `OTEL_EXPORTER_OTLP_PROTOCOL` |
| `otlp.headers` | none | Headers on every export, for a backend's API key. Their values are secret | `OTEL_EXPORTER_OTLP_HEADERS` adds to them. Set a name in one place only: for a name in both, the HTTP exporter sends the variable's value, and what the gRPC one sends isn't checked |
| `traces.sampler` | `parentbased_traceidratio` | `always_on`, `always_off`, `traceidratio`, `parentbased_always_on`, `parentbased_always_off` or `parentbased_traceidratio`. The default respects the caller's decision and samples new traces by ratio | `OTEL_TRACES_SAMPLER`, with `OTEL_TRACES_SAMPLER_ARG` |
| `traces.ratio` | `1.0` | The share of traces the ratio samplers keep; below 0 keeps none and above 1 keeps all. Set about `0.1` in production, and use the Collector's tail sampling to keep every error and slow trace | `OTEL_TRACES_SAMPLER_ARG`, when `OTEL_TRACES_SAMPLER` is set |
| `traces.capture_sql` | `false` | Whether a database span carries its parameterized statement, never its bind values. The sisa crates that make database spans read it; `init` doesn't | |
| `metrics.interval` | `30s` | How often metrics are pushed: a whole number and `ms`, `s`, `m` or `h` | `OTEL_METRIC_EXPORT_INTERVAL`, in milliseconds |
| `metrics.cardinality_limit` | `2000` | The most attribute sets one instrument keeps; the rest share one point with the attribute `otel.metric.overflow` | |
| `metrics.tenant_label` | `false` | Whether the metrics of the sisa crates carry `sisa.tenant`. Turn it on only where the number of tenants is small and bounded. The sisa crates that record those metrics read it; `init` doesn't | |
| `log.format` | `json` | `json`, or `pretty` for development | |
| `log.filter` | `info,sqlx=warn,hyper=warn` | `EnvFilter` syntax: a default level, then `target=level` pairs. It filters the traces too: a span below its level is never created. `reload_log_filter` replaces it at runtime. `RUST_LOG` is not read | |
| `resource.attributes` | none | Extra resource attributes, such as the cluster and the region | `OTEL_RESOURCE_ATTRIBUTES`, key by key |

```toml
[telemetry]
enabled = true

[telemetry.otlp]
endpoint = "http://otel-collector:4317"
protocol = "grpc"

[telemetry.traces]
ratio = 0.1

[telemetry.metrics]
interval = "30s"

[telemetry.log]
format = "json"
filter = "info,sqlx=warn,hyper=warn"

[telemetry.resource.attributes]
"k8s.cluster.name" = "prod-eu"
```

A key the design names that isn't read yet, since what uses it comes later: `log.otlp`
([roadmap](../ROADMAP.md#not-in-010)).
