//! Trace context across process boundaries, as the W3C headers `traceparent`, `tracestate` and
//! `baggage`.
//!
//! A crate adapts its own header type to [`Injector`] and [`Extractor`]; both are implemented
//! for `HashMap<String, String>` already. Which headers are written is the application's choice:
//! `sisa_telemetry::init` installs the W3C propagators, and without it nothing is written or
//! read.
//!
//! Call [`link_from`] and [`continue_from`] on a span before it is entered: once a span has
//! started, its parent can't change.
//!
//! Baggage crosses process boundaries in clear text: never put secrets or personal data in it.

pub use opentelemetry::propagation::{Extractor, Injector};
use opentelemetry::{
    Context,
    baggage::{BaggageExt, KeyValueMetadata},
    global,
    trace::TraceContextExt,
};
use tracing_opentelemetry::{OpenTelemetrySpanExt, SetParentError};

// What the headers carry: the sender's span, as a remote span, and its baggage.
fn extract(headers: &dyn Extractor) -> Context {
    global::get_text_map_propagator(|propagator| {
        propagator.extract_with_context(&Context::new(), headers)
    })
}

// Without `init` there is no OpenTelemetry layer, and a span the filter turned off has no data:
// neither is worth a log line. A span that has started keeps its parent, which is a mistake in
// the calling code.
fn set_parent(span: &tracing::Span, parent: Context) {
    if let Err(SetParentError::AlreadyStarted) = span.set_parent(parent) {
        tracing::warn!(
            "the span had already started, so its trace context was not set: call link_from or \
             continue_from before entering the span"
        );
    }
}

/// Producer and client side: writes the trace context of `span` (`traceparent`, `tracestate`,
/// `baggage`) into the headers. Writes nothing when no propagator is installed.
///
/// ```
/// use std::collections::HashMap;
///
/// use sisa_telemetry_core::propagation;
///
/// let span = tracing::info_span!("publish", otel.kind = "producer");
/// let mut headers = HashMap::new();
/// propagation::inject(&span, &mut headers);
///
/// // No application called `sisa_telemetry::init` here.
/// assert!(headers.is_empty());
/// ```
pub fn inject(span: &tracing::Span, headers: &mut dyn Injector) {
    let context = span.context();
    global::get_text_map_propagator(|propagator| propagator.inject_context(&context, headers));
}

/// Consumer side: `span` starts a new trace and links to the producer's span.
///
/// A message can wait in a queue for hours, and a parent and child would make a trace that long.
/// The baggage in the headers carries on into the new trace. Headers with no trace context add
/// no link.
pub fn link_from(span: &tracing::Span, headers: &dyn Extractor) {
    let producer = extract(headers);
    let producer_span = producer.span().span_context().clone();

    // A parent with no span in it makes `span` the root of a new trace; the baggage is kept.
    let baggage = producer.baggage().iter().map(|(key, (value, metadata))| {
        KeyValueMetadata::new(key.clone(), value.clone(), metadata.clone())
    });
    set_parent(span, Context::new().with_baggage(baggage));
    span.add_link(producer_span);
}

/// Server side (HTTP, gRPC): the caller's span becomes the parent of `span`.
///
/// Headers with no trace context leave the span as it is.
pub fn continue_from(span: &tracing::Span, headers: &dyn Extractor) {
    let caller = extract(headers);
    if caller.span().span_context().is_valid() {
        set_parent(span, caller);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    #[test]
    fn without_init_inject_writes_no_header() {
        let span = tracing::info_span!("publish");
        let mut headers = HashMap::new();

        inject(&span, &mut headers);

        assert!(headers.is_empty());
    }

    #[test]
    fn without_init_reading_headers_does_nothing() {
        let headers = HashMap::from([(
            "traceparent".to_owned(),
            "00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01".to_owned(),
        )]);

        link_from(&tracing::info_span!("process"), &headers);
        continue_from(&tracing::info_span!("handle"), &headers);
    }
}
