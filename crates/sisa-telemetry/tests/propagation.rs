//! Trace context across process boundaries: what `inject` writes, and what `link_from` and
//! `continue_from` make of it.

use std::{collections::HashMap, fmt};

use sisa_telemetry::{
    opentelemetry::trace::{SpanId, Status},
    propagation, record_error, testing,
};

#[test]
fn a_consumer_span_starts_a_new_trace_linked_to_the_producer() {
    let captured = testing::capture();
    let mut headers = HashMap::new();

    propagation::inject(&tracing::info_span!("link_publish"), &mut headers);
    // A consumer's poll loop may run inside a span of its own; the message's span still starts a
    // new trace.
    tracing::info_span!("link_poll").in_scope(|| {
        propagation::link_from(&tracing::info_span!("link_process"), &headers);
    });

    let producer = captured.span("link_publish").unwrap().span_context;
    let consumer = captured.span("link_process").unwrap();
    assert_ne!(consumer.span_context.trace_id(), producer.trace_id());
    assert_eq!(consumer.parent_span_id, SpanId::INVALID);
    assert_eq!(consumer.links.len(), 1);
    assert_eq!(
        consumer.links[0].span_context.trace_id(),
        producer.trace_id()
    );
    assert_eq!(consumer.links[0].span_context.span_id(), producer.span_id());
}

#[test]
fn a_server_span_continues_the_callers_trace() {
    let captured = testing::capture();
    let mut headers = HashMap::new();

    propagation::inject(&tracing::info_span!("continue_call"), &mut headers);
    propagation::continue_from(&tracing::info_span!("continue_handle"), &headers);

    let client = captured.span("continue_call").unwrap().span_context;
    let server = captured.span("continue_handle").unwrap();
    assert_eq!(server.span_context.trace_id(), client.trace_id());
    assert_eq!(server.parent_span_id, client.span_id());
    assert!(server.parent_span_is_remote);
    assert!(server.links.is_empty());
}

#[test]
fn inject_writes_the_w3c_headers() {
    let captured = testing::capture();
    let mut headers = HashMap::new();

    propagation::inject(&tracing::info_span!("w3c_publish"), &mut headers);

    let producer = captured.span("w3c_publish").unwrap().span_context;
    assert_eq!(
        headers["traceparent"],
        format!("00-{}-{}-01", producer.trace_id(), producer.span_id())
    );
    // `baggage` is written only when there is some; `tracestate` is always there.
    assert!(!headers.contains_key("baggage"));
    assert_eq!(headers.get("tracestate").map(String::as_str), Some(""));
}

#[test]
fn headers_without_a_trace_add_no_link_and_no_parent() {
    let captured = testing::capture();
    let headers = HashMap::new();

    propagation::link_from(&tracing::info_span!("empty_process"), &headers);
    tracing::info_span!("empty_outer").in_scope(|| {
        propagation::continue_from(&tracing::info_span!("empty_handle"), &headers);
    });

    assert!(captured.span("empty_process").unwrap().links.is_empty());
    let outer = captured.span("empty_outer").unwrap().span_context;
    let server = captured.span("empty_handle").unwrap();
    assert_eq!(server.parent_span_id, outer.span_id());
}

#[test]
fn baggage_carries_on_into_the_consumers_trace() {
    testing::capture();
    let received = HashMap::from([
        (
            "traceparent".to_owned(),
            "00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01".to_owned(),
        ),
        ("baggage".to_owned(), "tenant=acme".to_owned()),
    ]);
    let mut sent = HashMap::new();

    let consumer = tracing::info_span!("baggage_process");
    propagation::link_from(&consumer, &received);
    propagation::inject(&consumer, &mut sent);

    assert_eq!(sent["baggage"], "tenant=acme");
    assert!(!sent["traceparent"].contains("0af7651916cd43dd8448eb211c80319c"));
}

#[test]
fn a_span_that_has_started_keeps_its_parent() {
    let captured = testing::capture();
    let mut headers = HashMap::new();
    propagation::inject(&tracing::info_span!("late_call"), &mut headers);

    let entered = tracing::info_span!("late_handle");
    entered.in_scope(|| propagation::continue_from(&entered, &headers));
    entered.in_scope(|| propagation::link_from(&entered, &headers));
    drop(entered);

    let server = captured.span("late_handle").unwrap();
    assert_eq!(server.parent_span_id, SpanId::INVALID);
    assert!(server.links.is_empty());
}

#[derive(Debug)]
struct NotSaved(std::io::Error);

// The outer message of the chain.
impl fmt::Display for NotSaved {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("the order was not saved")
    }
}

// Gives the chain its source.
impl std::error::Error for NotSaved {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.0)
    }
}

#[test]
fn record_error_fails_the_span_with_the_source_chain() {
    let captured = testing::capture();

    let failing = tracing::info_span!("failing_insert");
    record_error(
        &failing,
        &NotSaved(std::io::Error::other("connection reset")),
    );
    drop(failing);

    let span = captured.span("failing_insert").unwrap();
    let message = "the order was not saved: connection reset";
    assert_eq!(span.status, Status::error(message));
    let event = &span.events[0];
    assert_eq!(event.name, "exception");
    assert_eq!(event.attributes[0].key.as_str(), "exception.message");
    assert_eq!(event.attributes[0].value.as_str(), message);
}
