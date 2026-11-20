# ADR-0007: A consumed message links to its producer

- Status: **Accepted**
- Date: 2026-11-07, in the design; recorded here on 2026-11-21
- Deciders: Sang Au

## Context

A request that publishes a message should be traceable to the work the message causes. A message can
wait in a queue for hours. This follows the OpenTelemetry messaging conventions.

## Decision

The producer writes the W3C headers `traceparent`, `tracestate` and `baggage` into each message. The
consumer opens one span per message, which starts a new trace and carries a link to the producer's
span. The consumer's span also carries `sisa.correlation_id`, so a business flow can be found across
traces. HTTP and gRPC are different: a server span is the caller's child.

## Consequences

- Traces stay as long as the work, not as long as the queue.
- Following a flow end to end means following links, or searching by `sisa.correlation_id`, not
  reading one trace.
- Services in other languages join the same traces, since the headers are the W3C ones.

## Alternatives considered

- **The consumer's span as a child of the producer's:** a message that waits for hours would make a
  trace that long.
