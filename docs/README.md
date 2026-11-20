# sisa-telemetry docs

sisa-telemetry is two crates. `sisa-telemetry` gives a service one `init()` that sets up traces,
metrics and logs over OpenTelemetry. `sisa-telemetry-core` gives library crates the small
API-only part: trace headers across messages and HTTP, error recording, metric helpers and the
shared attribute names. Together they let a request be followed from the edge through the
outbox, broker, consumer and job with the same ids and metric names.

## Layout

```text
docs/
├── README.md          this page: the index and the words used everywhere
├── architecture.md    how the crates fit: crate map, signal pipeline, rules, version alignment
├── formats.md         the headers, the attribute and metric names, the settings
├── flows.md           diagrams of a message and a request crossing processes
├── instrumenting.md   recipes for spans, metrics, trace headers and tests
├── specs/             what each crate does: one page per crate
├── adr/               architecture decision records: why it is this way
└── templates/         the crate spec and the flow; the ADR template is in adr/
```

At the repository root: [ROADMAP.md](../ROADMAP.md) for what comes next,
[CHANGELOG.md](../CHANGELOG.md) for what shipped, [CONTRIBUTING.md](../CONTRIBUTING.md) for the
checks, code style and Git rules, [SECURITY.md](../SECURITY.md), and [AGENTS.md](../AGENTS.md) for
coding agents. What each type does for users is in its Rust docs, not here.

## Where to start

| You want to | Read |
| --- | --- |
| Instrument a service or a library | [instrumenting.md](instrumenting.md) |
| See how the parts fit and what the library promises | [architecture.md](architecture.md) |
| Change or use one crate | Its page in [specs/](specs/README.md) |
| Read the headers, the names or the settings from another tool or language | [formats.md](formats.md) |
| Follow a message or a request across processes | [flows.md](flows.md) |
| Know why something was chosen, before proposing another way | [adr/](adr/README.md) |
| Know what's next | [ROADMAP.md](../ROADMAP.md) |
| Contribute, commit, release | [CONTRIBUTING.md](../CONTRIBUTING.md) |

## Words used everywhere

| Word | Meaning |
| --- | --- |
| **Signal** | One kind of telemetry: traces, metrics or logs |
| **Span** | One operation in a trace, written with `tracing` and turned into an OpenTelemetry span by the layer `init` installs |
| **Trace context** | A span's trace id, span id and sampling flag, with the baggage beside it: what crosses a process boundary |
| **Propagator** | What writes a trace context into headers and reads it back. `init` installs the W3C ones; without them nothing is written |
| **Baggage** | Key and value pairs that travel with the trace context, in clear text |
| **Link** | A pointer from one span to a span of another trace. A consumed message links to its producer instead of being its child |
| **Provider** | The SDK object that makes tracers or meters and exports what they record. There is one of each per process, installed by `init` |
| **Instrument** | A counter, histogram or gauge, made by a meter. One created before the provider is installed records nothing, for good |
| **Meter** | What makes instruments. Its scope is a crate's name and version |
| **Resource** | What the service is: `service.name`, `service.version` and the rest, on every span and metric it sends |
| **Collector** | The OpenTelemetry Collector the service exports to, which batches, samples and routes each signal to a backend |
| **OTLP** | The OpenTelemetry protocol the exporters speak, over gRPC or HTTP |
| **Guard** | `TelemetryGuard`, which `init` returns. It flushes and shuts the providers down |

## How docs grow

Docs change in the same PR as the code they describe.

1. **A change to what a crate does:** update its page in [specs/](specs/README.md), and the
   diagram in [flows.md](flows.md) if a step changes.
2. **A change to a header, an attribute or metric name, a unit, or a setting:** update
   [formats.md](formats.md). It is part of the public API, so the change is breaking.
3. **A new way to instrument something:** add a recipe to [instrumenting.md](instrumenting.md),
   with a test that backs it.
4. **A new crate:** copy [templates/crate-spec.md](templates/crate-spec.md) to
   `specs/<crate>.md`, and add it to the [specs index](specs/README.md) and the
   [crate map](architecture.md#crates).
5. **A new flow across crates:** add a section to [flows.md](flows.md), in the shape of
   [templates/flow-spec.md](templates/flow-spec.md).
6. **A choice that is hard to reverse, or that changes a rule or a dependency:** write an
   [ADR](adr/README.md). An accepted ADR is never rewritten; a new one supersedes it.

A spec's `Status` moves from Draft to Accepted to Implemented.
