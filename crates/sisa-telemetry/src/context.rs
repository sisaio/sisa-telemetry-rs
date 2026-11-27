//! The sisa-context fields and the trace id at the top level of every JSON log line.

use std::fmt::{self, Write as _};

use opentelemetry::trace::TraceContextExt;
use sisa_telemetry_core::attributes;
use tracing::{
    Event, Subscriber,
    field::{Field, Visit},
    span,
};
use tracing_subscriber::{
    Layer,
    fmt::{
        FmtContext, FormatEvent, FormatFields,
        format::{self, Format, Json, Writer},
    },
    layer::Context,
    registry::LookupSpan,
};

// The span fields every JSON log line repeats at its top level, from the nearest span that
// recorded each.
const CONTEXT_FIELDS: [&str; 5] = [
    attributes::REQUEST_ID,
    attributes::CORRELATION_ID,
    attributes::TENANT,
    attributes::ACTOR_TYPE,
    "enduser.id",
];

// The context fields one span recorded, kept in its extensions.
#[derive(Default)]
struct ContextFields(Vec<(&'static str, String)>);

impl ContextFields {
    fn keep(&mut self, field: &Field, value: String) {
        let Some(name) = CONTEXT_FIELDS
            .into_iter()
            .find(|name| *name == field.name())
        else {
            return;
        };
        self.0.retain(|(kept, _)| *kept != name);
        self.0.push((name, value));
    }
}

// Keeps the last value of each context field a span records.
impl Visit for ContextFields {
    fn record_str(&mut self, field: &Field, value: &str) {
        self.keep(field, value.to_owned());
    }

    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        self.keep(field, format!("{value:?}"));
    }
}

/// Records the context fields of each span, for [`ContextJson`] to read.
pub(crate) struct ContextLayer;

// Stores the context fields a span is created with, and the ones recorded on it later.
impl<S> Layer<S> for ContextLayer
where
    S: Subscriber + for<'span> LookupSpan<'span>,
{
    fn on_new_span(
        &self,
        attributes: &span::Attributes<'_>,
        id: &span::Id,
        context: Context<'_, S>,
    ) {
        let mut fields = ContextFields::default();
        attributes.record(&mut fields);
        if let Some(span) = context.span(id)
            && !fields.0.is_empty()
        {
            span.extensions_mut().insert(fields);
        }
    }

    fn on_record(&self, id: &span::Id, values: &span::Record<'_>, context: Context<'_, S>) {
        let Some(span) = context.span(id) else {
            return;
        };
        let mut extensions = span.extensions_mut();
        if let Some(fields) = extensions.get_mut::<ContextFields>() {
            values.record(fields);
        } else {
            let mut fields = ContextFields::default();
            values.record(&mut fields);
            if !fields.0.is_empty() {
                extensions.insert(fields);
            }
        }
    }
}

// Writes `text` as a JSON string.
fn write_json_string(output: &mut String, text: &str) {
    output.push('"');
    for character in text.chars() {
        match character {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            control if control < ' ' => {
                _ = write!(output, "\\u{:04x}", u32::from(control));
            }
            other => output.push(other),
        }
    }
    output.push('"');
}

/// `tracing-subscriber`'s JSON lines, with the trace and span ids of the event's span and the
/// context fields of its spans added at the top level.
pub(crate) struct ContextJson(Format<Json>);

impl ContextJson {
    pub(crate) fn new() -> Self {
        Self(format::format().json())
    }
}

// The JSON line as the plain format writes it, with the extra keys spliced in before its last
// brace. The plain format's own top-level keys never clash with them.
impl<S, N> FormatEvent<S, N> for ContextJson
where
    S: Subscriber + for<'span> LookupSpan<'span>,
    N: for<'writer> FormatFields<'writer> + 'static,
{
    fn format_event(
        &self,
        context: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &Event<'_>,
    ) -> fmt::Result {
        let mut line = String::new();
        self.0
            .format_event(context, Writer::new(&mut line), event)?;

        let mut extra = String::new();
        if let Some(scope) = context.event_scope() {
            let mut seen: Vec<&'static str> = Vec::new();
            for span in scope {
                let extensions = span.extensions();
                let Some(fields) = extensions.get::<ContextFields>() else {
                    continue;
                };
                for (name, value) in &fields.0 {
                    if !seen.contains(name) {
                        seen.push(name);
                        extra.push(',');
                        write_json_string(&mut extra, name);
                        extra.push(':');
                        write_json_string(&mut extra, value);
                    }
                }
            }
            // The OpenTelemetry layer makes the entered span's context the current one.
            let otel_context = opentelemetry::Context::current();
            let span = otel_context.span();
            let span_context = span.span_context();
            if span_context.is_valid() {
                _ = write!(
                    extra,
                    r#","trace_id":"{}","span_id":"{}""#,
                    span_context.trace_id(),
                    span_context.span_id()
                );
            }
        }

        match line.strip_suffix("}\n") {
            Some(object) if !extra.is_empty() => writeln!(writer, "{object}{extra}}}"),
            _ => writer.write_str(&line),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        io,
        sync::{Arc, Mutex},
    };

    use opentelemetry_sdk::trace::SdkTracerProvider;
    use tracing_subscriber::layer::SubscriberExt;

    use super::*;
    use crate::init::trace_layer;

    // Collects what the fmt layer writes.
    #[derive(Clone, Default)]
    struct Output(Arc<Mutex<Vec<u8>>>);

    // Appends each write to the shared buffer.
    impl io::Write for Output {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    // The JSON lines `log` writes, with the OpenTelemetry layer on when `traced`.
    fn lines(traced: bool, log: impl FnOnce()) -> Vec<serde_json::Value> {
        let output = Output::default();
        let writer = output.clone();
        let provider = SdkTracerProvider::builder().build();
        let subscriber = tracing_subscriber::registry()
            .with(ContextLayer)
            .with(
                tracing_subscriber::fmt::layer()
                    .json()
                    .event_format(ContextJson::new())
                    .with_writer(move || writer.clone()),
            )
            .with(traced.then(|| trace_layer(&provider)));
        tracing::subscriber::with_default(subscriber, log);

        let written = output.0.lock().unwrap().clone();
        String::from_utf8(written)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }

    #[test]
    fn a_line_carries_the_nearest_context_fields_and_the_trace_id() {
        let lines = lines(true, || {
            let request = tracing::info_span!(
                "request",
                sisa.request_id = "req-1",
                sisa.tenant = "acme",
                enduser.id = tracing::field::Empty
            );
            request.record("enduser.id", 42);
            request.in_scope(|| {
                tracing::info_span!("job", sisa.tenant = "beta \"quoted\"").in_scope(|| {
                    tracing::info!("placed");
                });
            });
        });

        let line = &lines[0];
        assert_eq!(line["sisa.request_id"], "req-1");
        assert_eq!(line["sisa.tenant"], "beta \"quoted\"");
        assert_eq!(line["enduser.id"], "42");
        assert_eq!(line["fields"]["message"], "placed");
        assert_eq!(line["trace_id"].as_str().unwrap().len(), 32);
        assert_eq!(line["span_id"].as_str().unwrap().len(), 16);
    }

    #[test]
    fn without_traces_a_line_carries_the_fields_but_no_trace_id() {
        let lines = lines(false, || {
            tracing::info_span!("request", sisa.correlation_id = "flow-7").in_scope(|| {
                tracing::info!("placed");
            });
        });

        assert_eq!(lines[0]["sisa.correlation_id"], "flow-7");
        assert!(lines[0].get("trace_id").is_none());
    }

    #[test]
    fn a_line_outside_any_span_is_unchanged() {
        let lines = lines(true, || tracing::info!(sisa.tenant = "acme", "started"));

        assert!(lines[0].get("sisa.tenant").is_none());
        assert_eq!(lines[0]["fields"]["sisa.tenant"], "acme");
        assert!(lines[0].get("trace_id").is_none());
    }
}
