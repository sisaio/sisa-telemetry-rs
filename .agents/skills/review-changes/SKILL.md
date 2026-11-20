---
name: review-changes
description: Reviews a diff or pull request in sisa-telemetry-rs, code and docs alike, against CI's checks, the design rules, the settled decisions and the docs conventions, and reports findings by severity. Use when asked to review, evaluate or check a change, a branch, a PR or a docs page, even if the word "review" isn't used.
---

# Review changes

Review what changed, not the whole crate, unless asked to review the whole repository (below).
Report findings; don't push fixes unless asked.

## Workflow

- [ ] Get the diff: the PR's diff, or `git diff main...HEAD` on a branch.
- [ ] Run the checks in [CONTRIBUTING.md](../../../CONTRIBUTING.md#build-and-check). Note each
      failure with its first error.
- [ ] Check the diff against the
      [rules](../../../docs/architecture.md#rules-that-hold-the-design-together), the
      [decisions](../../../docs/adr/README.md) and the
      [hard rules](../../../AGENTS.md#hard-rules) in AGENTS.md. The ones most often broken:
  - the core gains a dependency, or reaches for the SDK or an exporter;
  - a function other than `init` and `testing::capture` installs something process-wide;
  - a core function panics, logs or writes a header when no application called `init`;
  - an id or another unbounded value becomes a metric attribute;
  - an OpenTelemetry crate moves to another minor without the others.
- [ ] Check what goes with the change: a test for each new behavior, doc comments on new public
      items, the crate's [spec](../../../docs/specs/README.md) when what it does changed,
      [Formats](../../../docs/formats.md) when a header, a name or a setting changed, the recipe
      in [Instrumenting](../../../docs/instrumenting.md) that shows it, an [ADR](../../../docs/adr/README.md) for a choice that is hard
      to reverse, and `breaking` on the PR if the public API or a format changed.
- [ ] Check the docs in the diff: doc comments are short and plain with an example where it
      helps, and never link a Markdown file; `docs/` pages use Mermaid for diagrams and tables
      over long lists; every relative link and `#anchor` resolves.
- [ ] Check each commit subject and the PR title against
      [Commit messages](../../../CONTRIBUTING.md#commit-messages): the type matches the change
      (`feat` only for something users can do now, `!` for a breaking change).
- [ ] Look for bugs: a parent or a link set on a span after it was entered, which does nothing;
      a setting passed to the SDK in code where an `OTEL_*` variable should win; an error
      swallowed without a log; a header value in a log or an error; and the patterns past
      reviews found (below).
- [ ] Look for what only shows at scale or during an outage: anything on the request path that
      waits on the Collector, a queue without a bound, a span around a loop instead of one per
      unit of work, and how many log lines an export failure makes while the Collector is
      down.

## Bugs past reviews found

Each was real and passed CI in sisa-caching-rs or while this repository was first built. Look for
the same shape anywhere in a change.

- A flag released before a final recheck, with no ordering that makes the other side's writes
  visible. Pair the swaps with `AcqRel` and recheck after releasing.
- A last-owner check (`Arc::strong_count`) made outside the lock that hands out clones.
- A time read before waiting for a lock, then compared with what was written during the wait.
- Callers that wait on a shared operation and take its success for granted. Hand them its error,
  and let them redo it if its owner was cancelled.
- A future dropped halfway (a timeout, a `select!`) that leaves shared state behind: a held lock
  or a result never stored.
- A consumer span given a parent with an invalid span in it, to make it a root: the
  parent-based sampler read that parent as not sampled and dropped the span. A root comes from a
  parent context with no span at all.
- A test against a container that started but wasn't ready yet: wait for the server's own
  "ready" line.
- A watermark that vouches for a run of finished items while an earlier one is still running.
- A span or log line on a helper another path also calls.
- A test that can't fail: a wait for a value the test never set, or a sleep standing in for proof
  that something happened. Break the code the test guards and watch it fail.

## Whole-repository review

- [ ] Start from `main`'s head and note its commit. Read
      [Guarantees and limits](../../../docs/architecture.md#guarantees-and-limits) and the
      [rules](../../../docs/architecture.md#rules-that-hold-the-design-together) first, then
      each crate.
- [ ] For each invariant below, check a test still guards it: break the code and see it fail.
      An invariant the docs promise with no test is a finding. Name the test in the table once
      it exists.
- [ ] Measure before calling something slow, in alternating rounds of the old and the new code;
      for a change under 5%, compare callgrind instruction counts instead.
- [ ] Write the findings as one document: severity, evidence, and the fix grouped into proposed
      PRs, each needing its own issue.

| Invariant | Test |
| --- | --- |
| A consumed message starts a new trace with one link to its producer | `a_consumer_span_starts_a_new_trace_linked_to_the_producer` |
| A server span is the caller's child | `a_server_span_continues_the_callers_trace` |
| `inject` writes the W3C headers | `inject_writes_the_w3c_headers` |
| Without `init`, the core writes nothing and never panics | `without_init_inject_writes_no_header`, `without_init_reading_headers_does_nothing`, `without_init_recording_an_error_does_nothing` |
| Baggage carries on into a consumer's trace | `baggage_carries_on_into_the_consumers_trace` |
| `record_error` fails the span with the source chain | `record_error_fails_the_span_with_the_source_chain` |
| A duration histogram is in seconds, 5 ms to 10 s | `a_duration_histogram_has_the_standard_buckets` |
| Past the cardinality limit, new attribute sets share one overflow point | `attribute_sets_past_the_limit_fold_into_one_overflow_point` |
| A span that has started takes no parent and no link | `a_span_that_has_started_keeps_its_parent` |
| The `OTEL_*` variables win over the settings | `the_protocol_variable_wins_over_the_settings`, `an_endpoint_variable_wins_over_the_settings`, `the_sampler_variable_wins_over_the_settings` |
| A header value never shows | `debug_hides_header_values`, `a_header_that_cannot_be_sent_is_named_without_its_value` |
| The exporters' own spans stay out of the traces | `the_exporters_own_spans_stay_out_of_the_traces` |
| `init` exports spans and metrics to a Collector | `init_exports_spans_and_metrics_over_grpc`, `init_exports_spans_and_metrics_over_http` |
| `init` runs once, and off means logs only | `disabled_sets_up_logs_only_and_a_second_init_is_an_error` |

## Gotchas

- A span reaches the exporter when its last handle drops. A test that looks for a span it still
  holds finds nothing.
- `cargo +1.89 check` can fail where stable passes. Read the exit code, not a piped tail.
- Tests can't set environment variables: the workspace forbids unsafe code. The code that reads
  one takes the lookup as a parameter.
- Not bugs, by design: `link_from` makes its span a root even when the headers carry no trace,
  and adds no link to a span that has started;
  `continue_from` leaves the span alone then; `record_error` writes no log line; `init` succeeds
  while the Collector is down.

## Report

```markdown
**Blocking**
- `crates/core/src/propagation.rs:60`: what is wrong. Fix: what would fix it.

**Not blocking**
- `file:line`: what is wrong. Fix: what would fix it.

Checks: all passed, or the failing check and its first error.
```

If nothing blocks, say so in one line instead of the Blocking section.
