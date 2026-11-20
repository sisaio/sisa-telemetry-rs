//! Marking a span as failed.

use std::error::Error;

use opentelemetry::{KeyValue, trace::Status};
use tracing_opentelemetry::OpenTelemetrySpanExt;

// The error and every source under it, outermost first, as one line.
fn chain(error: &dyn Error) -> String {
    let mut message = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        message.push_str(": ");
        message.push_str(&cause.to_string());
        source = cause.source();
    }
    message
}

/// Marks `span` as failed (OpenTelemetry status `Error`) and records the error with its source
/// chain, as an `exception` event with `exception.message`.
///
/// It writes no log line: log the error where it is handled. Without `sisa_telemetry::init` it
/// does nothing.
///
/// ```
/// use sisa_telemetry_core::record_error;
///
/// fn parse(input: &str) -> Result<u32, std::num::ParseIntError> {
///     input.parse().inspect_err(|error| record_error(&tracing::Span::current(), error))
/// }
///
/// assert!(parse("forty-two").is_err());
/// ```
pub fn record_error(span: &tracing::Span, error: &dyn Error) {
    let message = chain(error);
    span.add_event(
        "exception",
        vec![KeyValue::new("exception.message", message.clone())],
    );
    span.set_status(Status::error(message));
}

#[cfg(test)]
mod tests {
    use std::fmt;

    use super::*;

    #[derive(Debug)]
    struct Outer(std::io::Error);

    // What the chain prints for the outer error.
    impl fmt::Display for Outer {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("the order was not saved")
        }
    }

    // Gives the chain its source.
    impl Error for Outer {
        fn source(&self) -> Option<&(dyn Error + 'static)> {
            Some(&self.0)
        }
    }

    #[test]
    fn the_chain_lists_every_source() {
        let error = Outer(std::io::Error::other("connection reset"));

        assert_eq!(chain(&error), "the order was not saved: connection reset");
    }

    #[test]
    fn without_init_recording_an_error_does_nothing() {
        let span = tracing::info_span!("work");

        record_error(&span, &Outer(std::io::Error::other("connection reset")));
    }
}
