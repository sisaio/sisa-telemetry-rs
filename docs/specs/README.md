# Specs

What each crate does, written so it can be built, reviewed and tested against. One page per
crate, all with the same sections: purpose, public contract, behaviour, guarantees and limits,
errors, and formats.

| Crate | Spec | Holds |
| --- | --- | --- |
| `sisa-telemetry-core` | [core.md](core.md) | Trace headers across messages and HTTP, error recording, the meter and duration-histogram helpers, the attribute names |
| `sisa-telemetry` | [telemetry.md](telemetry.md) | `init`, the settings, the service's resource, the guard, and the `testing` feature |

What spans the crates is one level up: how they fit in [Architecture](../architecture.md), the
headers, names and settings in [Formats](../formats.md), the steps in [Flows](../flows.md), and
how to use it all in the [instrumenting guide](../instrumenting.md).

Changing what a crate does: update its page in the same PR. A new crate starts from
[templates/crate-spec.md](../templates/crate-spec.md). A choice that is hard to reverse gets an
[ADR](../adr/README.md). Planned work goes in the [roadmap](../../ROADMAP.md), not here: a spec
says what the code does, and marks what is still open as an open question.
