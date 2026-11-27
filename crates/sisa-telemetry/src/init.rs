//! `init`: the subscriber, the providers and the propagators, installed once per process.

use std::{
    collections::HashMap,
    str::FromStr,
    sync::OnceLock,
    time::{Duration, Instant},
};

use http::{HeaderMap, HeaderName, HeaderValue};
use opentelemetry::{
    InstrumentationScope, global,
    propagation::{TextMapCompositePropagator, TextMapPropagator},
    trace::TracerProvider,
};
use opentelemetry_otlp::{
    MetricExporter, Protocol, SpanExporter, WithExportConfig, WithHttpConfig, WithTonicConfig,
    tonic_types::metadata::MetadataMap,
};
use opentelemetry_sdk::{
    metrics::{Instrument, PeriodicReader, SdkMeterProvider, Stream},
    propagation::{BaggagePropagator, TraceContextPropagator},
    trace::{Sampler, SdkTracer, SdkTracerProvider},
};
use tracing::Subscriber;
use tracing_subscriber::{
    EnvFilter, Layer, Registry,
    filter::{FilterFn, filter_fn},
    layer::SubscriberExt,
    registry::LookupSpan,
    reload,
    util::SubscriberInitExt,
};

use crate::{
    LogFormat, OtlpProtocol, OtlpSettings, ServiceResource, TelemetryError, TelemetrySettings,
    TracesSampler, TracesSettings,
};

// How long a guard that is dropped, not shut down, waits for the last export.
const DROP_BUDGET: Duration = Duration::from_secs(5);

// The crates the exporters themselves run on, beside the `opentelemetry` ones. Their spans stay
// out of the traces: exporting a span about an export would make another export.
const EXPORTER_TARGETS: [&str; 5] = ["tonic", "hyper", "hyper_util", "reqwest", "h2"];

// Whether a span or event comes from a crate the exporters run on, by its module path.
fn from_the_exporter(target: &str) -> bool {
    target.starts_with("opentelemetry")
        || EXPORTER_TARGETS.iter().any(|name| {
            target
                .strip_prefix(name)
                .is_some_and(|rest| rest.is_empty() || rest.starts_with("::"))
        })
}

// The handle to the filter `init` installed, which `reload_log_filter` swaps. Set once, by the
// `init` that installs the subscriber.
static LOG_FILTER: OnceLock<reload::Handle<EnvFilter, Registry>> = OnceLock::new();

fn parse_filter(filter: &str) -> Result<EnvFilter, TelemetryError> {
    EnvFilter::try_new(filter).map_err(|error| TelemetryError::LogFilter(error.to_string()))
}

// One signal's OTLP names: its endpoint variable, and its path under an HTTP endpoint.
struct Signal {
    endpoint_variable: &'static str,
    path: &'static str,
}

const TRACES: Signal = Signal {
    endpoint_variable: "OTEL_EXPORTER_OTLP_TRACES_ENDPOINT",
    path: "/v1/traces",
};

const METRICS: Signal = Signal {
    endpoint_variable: "OTEL_EXPORTER_OTLP_METRICS_ENDPOINT",
    path: "/v1/metrics",
};

// An environment variable that is set and not empty.
fn environment(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|value| !value.is_empty())
}

// `OTEL_EXPORTER_OTLP_PROTOCOL` wins over the settings.
fn protocol(
    otlp: &OtlpSettings,
    variable: impl Fn(&str) -> Option<String>,
) -> Result<OtlpProtocol, TelemetryError> {
    match variable("OTEL_EXPORTER_OTLP_PROTOCOL").as_deref() {
        None => Ok(otlp.protocol),
        Some("grpc") => Ok(OtlpProtocol::Grpc),
        Some("http/protobuf") => Ok(OtlpProtocol::HttpProtobuf),
        Some(other) => Err(TelemetryError::Protocol(other.to_owned())),
    }
}

// The endpoint to hand an exporter, or none to leave it to the exporter: it reads the endpoint
// variables itself and falls back to localhost, but an endpoint given in code would win over
// them. An HTTP exporter takes a given endpoint as it is, so the signal's path is added here.
fn endpoint(
    otlp: &OtlpSettings,
    protocol: OtlpProtocol,
    signal: &Signal,
    variable: impl Fn(&str) -> Option<String>,
) -> Option<String> {
    if variable(signal.endpoint_variable).is_some()
        || variable("OTEL_EXPORTER_OTLP_ENDPOINT").is_some()
    {
        return None;
    }
    let base = otlp.endpoint.as_deref()?;
    match protocol {
        OtlpProtocol::Grpc => Some(base.to_owned()),
        OtlpProtocol::HttpProtobuf => {
            Some(format!("{}{}", base.trim_end_matches('/'), signal.path))
        }
    }
}

// The sampler to give the provider, or none to leave it to the SDK, which reads
// `OTEL_TRACES_SAMPLER` itself: a sampler given in code would win over it.
fn sampler(traces: &TracesSettings, variable: impl Fn(&str) -> Option<String>) -> Option<Sampler> {
    if variable("OTEL_TRACES_SAMPLER").is_some() {
        return None;
    }
    let ratio = Sampler::TraceIdRatioBased(traces.ratio);
    Some(match traces.sampler {
        TracesSampler::AlwaysOn => Sampler::AlwaysOn,
        TracesSampler::AlwaysOff => Sampler::AlwaysOff,
        TracesSampler::TraceIdRatio => ratio,
        TracesSampler::ParentBasedAlwaysOn => Sampler::ParentBased(Box::new(Sampler::AlwaysOn)),
        TracesSampler::ParentBasedAlwaysOff => Sampler::ParentBased(Box::new(Sampler::AlwaysOff)),
        TracesSampler::ParentBasedTraceIdRatio => Sampler::ParentBased(Box::new(ratio)),
    })
}

// The settings' headers as gRPC metadata, marked sensitive so they stay out of debug output.
fn metadata(otlp: &OtlpSettings) -> Result<MetadataMap, TelemetryError> {
    let mut headers = HeaderMap::new();
    for (name, value) in &otlp.headers {
        let invalid = || TelemetryError::Header { name: name.clone() };
        let mut value = HeaderValue::from_str(value).map_err(|_| invalid())?;
        value.set_sensitive(true);
        headers.insert(HeaderName::from_str(name).map_err(|_| invalid())?, value);
    }
    Ok(MetadataMap::from_headers(headers))
}

fn exporter_error(error: opentelemetry_otlp::ExporterBuildError) -> TelemetryError {
    TelemetryError::Exporter(error.to_string())
}

fn span_exporter(
    otlp: &OtlpSettings,
    protocol: OtlpProtocol,
) -> Result<SpanExporter, TelemetryError> {
    let endpoint = endpoint(otlp, protocol, &TRACES, environment);
    match protocol {
        OtlpProtocol::Grpc => {
            let builder = SpanExporter::builder()
                .with_tonic()
                .with_metadata(metadata(otlp)?);
            match endpoint {
                Some(endpoint) => builder.with_endpoint(endpoint).build(),
                None => builder.build(),
            }
        }
        OtlpProtocol::HttpProtobuf => {
            let builder = SpanExporter::builder()
                .with_http()
                .with_protocol(Protocol::HttpBinary)
                .with_headers(HashMap::from_iter(otlp.headers.clone()));
            match endpoint {
                Some(endpoint) => builder.with_endpoint(endpoint).build(),
                None => builder.build(),
            }
        }
    }
    .map_err(exporter_error)
}

fn metric_exporter(
    otlp: &OtlpSettings,
    protocol: OtlpProtocol,
) -> Result<MetricExporter, TelemetryError> {
    let endpoint = endpoint(otlp, protocol, &METRICS, environment);
    match protocol {
        OtlpProtocol::Grpc => {
            let builder = MetricExporter::builder()
                .with_tonic()
                .with_metadata(metadata(otlp)?);
            match endpoint {
                Some(endpoint) => builder.with_endpoint(endpoint).build(),
                None => builder.build(),
            }
        }
        OtlpProtocol::HttpProtobuf => {
            let builder = MetricExporter::builder()
                .with_http()
                .with_protocol(Protocol::HttpBinary)
                .with_headers(HashMap::from_iter(otlp.headers.clone()));
            match endpoint {
                Some(endpoint) => builder.with_endpoint(endpoint).build(),
                None => builder.build(),
            }
        }
    }
    .map_err(exporter_error)
}

// Every instrument gets the cardinality limit and keeps the rest of what it asked for.
pub(crate) fn cardinality_view(limit: usize) -> impl Fn(&Instrument) -> Option<Stream> {
    move |_instrument| Stream::builder().with_cardinality_limit(limit).build().ok()
}

fn not_the_exporter() -> FilterFn<impl Fn(&tracing::Metadata<'_>) -> bool + Clone> {
    filter_fn(|metadata| !from_the_exporter(metadata.target()))
}

// The layer that turns `tracing` spans into OpenTelemetry spans; `testing::capture` uses it too.
pub(crate) fn trace_layer<S>(provider: &SdkTracerProvider) -> impl Layer<S> + use<S>
where
    S: Subscriber + for<'span> LookupSpan<'span>,
{
    let scope = InstrumentationScope::builder(env!("CARGO_PKG_NAME"))
        .with_version(env!("CARGO_PKG_VERSION"))
        .build();
    let tracer: SdkTracer = provider.tracer_with_scope(scope);
    tracing_opentelemetry::layer()
        .with_tracer(tracer)
        .with_filter(not_the_exporter())
}

// W3C trace context and baggage, the headers services in other languages read too.
pub(crate) fn propagator() -> impl TextMapPropagator + Send + Sync + 'static {
    TextMapCompositePropagator::new(vec![
        Box::new(TraceContextPropagator::new()),
        Box::new(BaggagePropagator::new()),
    ])
}

fn log_layer<S>(format: LogFormat) -> Box<dyn Layer<S> + Send + Sync>
where
    S: Subscriber + for<'span> LookupSpan<'span>,
{
    match format {
        LogFormat::Json => tracing_subscriber::fmt::layer().json().boxed(),
        LogFormat::Pretty => tracing_subscriber::fmt::layer().pretty().boxed(),
    }
}

/// Keeps telemetry running. Flushes and shuts down the providers on drop, or with
/// [`shutdown`](Self::shutdown), which picks how long to wait for the spans.
///
/// A drop waits up to 5 seconds for the span queue and up to 5 more for the last metric export,
/// on the thread that drops it.
///
/// Hold it until the end of `main`: dropping it earlier stops the export.
#[derive(Debug)]
#[must_use = "dropping the guard shuts telemetry down"]
pub struct TelemetryGuard {
    tracer_provider: Option<SdkTracerProvider>,
    meter_provider: Option<SdkMeterProvider>,
}

impl TelemetryGuard {
    /// Flushes what the providers still hold and shuts them down. Call it after the servers
    /// have stopped and before the pools close.
    ///
    /// `budget` is how long it waits for the span queue to be sent. The last metric export then
    /// gets up to 5 seconds whatever the budget is: the SDK's meter provider ignores the timeout
    /// it is given. So the call can take `budget` plus 5 seconds.
    ///
    /// It blocks the calling thread. With the gRPC exporter the last export runs on the Tokio
    /// runtime, which a multi-thread runtime does on another thread; on a current-thread
    /// runtime, call it from `tokio::task::spawn_blocking`.
    ///
    /// # Errors
    ///
    /// [`TelemetryError::Shutdown`] when a provider reports that its shutdown failed or timed
    /// out, as when the Collector is down; what it still held is lost.
    pub fn shutdown(mut self, budget: Duration) -> Result<(), TelemetryError> {
        self.close(budget)
    }

    fn close(&mut self, budget: Duration) -> Result<(), TelemetryError> {
        let deadline = Instant::now() + budget;
        let mut failures = Vec::new();

        if let Some(provider) = self.tracer_provider.take()
            && let Err(error) = provider.shutdown_with_timeout(budget)
        {
            failures.push(format!("traces: {error}"));
        }
        if let Some(provider) = self.meter_provider.take()
            && let Err(error) =
                provider.shutdown_with_timeout(deadline.saturating_duration_since(Instant::now()))
        {
            failures.push(format!("metrics: {error}"));
        }

        if failures.is_empty() {
            Ok(())
        } else {
            Err(TelemetryError::Shutdown(failures.join("; ")))
        }
    }
}

// A guard that goes out of scope still sends what the providers hold.
impl Drop for TelemetryGuard {
    fn drop(&mut self) {
        _ = self.close(DROP_BUDGET);
    }
}

/// Sets up logs, traces and metrics for the process. Call it once, first thing in `main`, and
/// keep the guard until the end.
///
/// It installs the global `tracing` subscriber: the filter from `log.filter`, JSON or pretty logs
/// on stdout, and a layer that turns spans into OpenTelemetry spans. It sets the global tracer
/// and meter providers, which export over OTLP with the service's resource, and the W3C
/// propagators for `traceparent`, `tracestate` and `baggage`. With `enabled = false` only the
/// logs are set up.
///
/// With the gRPC protocol, the default, call it inside a Tokio runtime. Create metric instruments
/// after it: one created before stays a no-op for the life of the process.
///
/// ```no_run
/// use std::time::Duration;
///
/// use sisa_telemetry::{ServiceResource, TelemetrySettings};
///
/// #[tokio::main]
/// async fn main() -> Result<(), sisa_telemetry::TelemetryError> {
///     let telemetry = sisa_telemetry::init(
///         &TelemetrySettings::default(),
///         ServiceResource::new(env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION")),
///     )?;
///
///     tracing::info!("serving");
///
///     telemetry.shutdown(Duration::from_secs(2))
/// }
/// ```
///
/// # Errors
///
/// A setting that can't be used ([`LogFilter`](TelemetryError::LogFilter),
/// [`Header`](TelemetryError::Header), [`Protocol`](TelemetryError::Protocol),
/// [`Exporter`](TelemetryError::Exporter)), the gRPC protocol outside a Tokio runtime
/// ([`NoRuntime`](TelemetryError::NoRuntime)), or a second call
/// ([`AlreadyInitialized`](TelemetryError::AlreadyInitialized)). Nothing is installed when it
/// fails. A Collector that is down is not an error: the exporters connect when they first send,
/// and a failed export is a log line from the `opentelemetry` crates.
pub fn init(
    settings: &TelemetrySettings,
    resource: ServiceResource,
) -> Result<TelemetryGuard, TelemetryError> {
    let (filter, filter_handle) = reload::Layer::new(parse_filter(&settings.log.filter)?);

    let providers = if settings.enabled {
        let protocol = protocol(&settings.otlp, environment)?;
        if protocol == OtlpProtocol::Grpc && tokio::runtime::Handle::try_current().is_err() {
            return Err(TelemetryError::NoRuntime);
        }
        let resource = resource.build(
            &settings.resource.attributes,
            environment("OTEL_SERVICE_NAME"),
        );

        let mut tracer_provider = SdkTracerProvider::builder()
            .with_resource(resource.clone())
            .with_batch_exporter(span_exporter(&settings.otlp, protocol)?);
        if let Some(sampler) = sampler(&settings.traces, environment) {
            tracer_provider = tracer_provider.with_sampler(sampler);
        }

        let mut reader = PeriodicReader::builder(metric_exporter(&settings.otlp, protocol)?);
        if environment("OTEL_METRIC_EXPORT_INTERVAL").is_none() {
            reader = reader.with_interval(settings.metrics.interval);
        }
        let meter_provider = SdkMeterProvider::builder()
            .with_resource(resource)
            .with_reader(reader.build())
            .with_view(cardinality_view(settings.metrics.cardinality_limit))
            .build();

        Some((tracer_provider.build(), meter_provider))
    } else {
        None
    };

    tracing_subscriber::registry()
        .with(filter)
        .with(log_layer(settings.log.format))
        .with(providers.as_ref().map(|(tracer, _)| trace_layer(tracer)))
        .try_init()
        .map_err(|error| TelemetryError::AlreadyInitialized(error.to_string()))?;
    _ = LOG_FILTER.set(filter_handle);

    let (tracer_provider, meter_provider) = providers.unzip();
    if let Some(provider) = &tracer_provider {
        global::set_tracer_provider(provider.clone());
        global::set_text_map_propagator(propagator());
    }
    if let Some(provider) = &meter_provider {
        global::set_meter_provider(provider.clone());
    }

    Ok(TelemetryGuard {
        tracer_provider,
        meter_provider,
    })
}

/// Replaces the filter from `log.filter` for the rest of the process, or until the next call, as
/// for an admin endpoint that turns on `debug` for one target while an incident lasts.
///
/// It takes the same `EnvFilter` syntax as `log.filter`, and filters the traces too: a span
/// below the new level is no longer created. Spans already open keep running.
///
/// ```no_run
/// sisa_telemetry::reload_log_filter("info,my_service=debug")?;
/// # Ok::<(), sisa_telemetry::TelemetryError>(())
/// ```
///
/// # Errors
///
/// [`TelemetryError::LogFilter`] when the filter isn't valid syntax, and
/// [`TelemetryError::NotInitialized`] when [`init`] hasn't installed the subscriber, as before
/// it or under `testing::capture`. The current filter stays in place either way.
pub fn reload_log_filter(filter: &str) -> Result<(), TelemetryError> {
    let filter = parse_filter(filter)?;
    LOG_FILTER
        .get()
        .ok_or(TelemetryError::NotInitialized)?
        .reload(filter)
        .map_err(|_| TelemetryError::NotInitialized)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unset(_name: &str) -> Option<String> {
        None
    }

    fn only(name: &'static str, value: &'static str) -> impl Fn(&str) -> Option<String> {
        move |asked| (asked == name).then(|| value.to_owned())
    }

    fn collector() -> OtlpSettings {
        OtlpSettings {
            endpoint: Some("http://collector:4318/".to_owned()),
            ..OtlpSettings::default()
        }
    }

    #[test]
    fn the_protocol_variable_wins_over_the_settings() {
        let otlp = OtlpSettings::default();

        assert_eq!(protocol(&otlp, unset).unwrap(), OtlpProtocol::Grpc);
        assert_eq!(
            protocol(&otlp, only("OTEL_EXPORTER_OTLP_PROTOCOL", "http/protobuf")).unwrap(),
            OtlpProtocol::HttpProtobuf
        );
        assert!(matches!(
            protocol(&otlp, only("OTEL_EXPORTER_OTLP_PROTOCOL", "http/json")),
            Err(TelemetryError::Protocol(name)) if name == "http/json"
        ));
    }

    #[test]
    fn an_http_endpoint_gets_the_signal_path() {
        assert_eq!(
            endpoint(&collector(), OtlpProtocol::HttpProtobuf, &TRACES, unset).as_deref(),
            Some("http://collector:4318/v1/traces")
        );
        assert_eq!(
            endpoint(&collector(), OtlpProtocol::Grpc, &METRICS, unset).as_deref(),
            Some("http://collector:4318/")
        );
    }

    #[test]
    fn an_endpoint_variable_wins_over_the_settings() {
        for name in [
            "OTEL_EXPORTER_OTLP_ENDPOINT",
            "OTEL_EXPORTER_OTLP_TRACES_ENDPOINT",
        ] {
            let variable = only(name, "http://other:4317");
            assert_eq!(
                endpoint(&collector(), OtlpProtocol::Grpc, &TRACES, variable),
                None
            );
        }
    }

    #[test]
    fn no_endpoint_leaves_the_default_to_the_exporter() {
        let otlp = OtlpSettings::default();

        assert_eq!(endpoint(&otlp, OtlpProtocol::Grpc, &TRACES, unset), None);
    }

    #[test]
    fn the_sampler_variable_wins_over_the_settings() {
        let traces = TracesSettings::default();

        assert!(sampler(&traces, unset).is_some());
        assert!(sampler(&traces, only("OTEL_TRACES_SAMPLER", "always_off")).is_none());
    }

    #[test]
    fn a_header_that_cannot_be_sent_is_named_without_its_value() {
        let mut otlp = OtlpSettings::default();
        otlp.headers
            .insert("x-api-key".to_owned(), "canary\nsecret".to_owned());

        let error = metadata(&otlp).unwrap_err();

        assert!(matches!(&error, TelemetryError::Header { name } if name == "x-api-key"));
        assert!(!error.to_string().contains("canary"));
    }

    #[test]
    fn the_exporters_own_spans_stay_out_of_the_traces() {
        assert!(from_the_exporter("opentelemetry_sdk::trace"));
        assert!(from_the_exporter("h2::proto"));
        assert!(from_the_exporter("hyper"));
        assert!(!from_the_exporter("tower_http::trace"));
        assert!(!from_the_exporter("reqwest_tracing::middleware"));
        assert!(!from_the_exporter("orders::handlers"));
    }
}
