//! Test helpers: capture the spans and metrics a test produces, and assert on them.
//!
//! Turn on the `testing` feature in `[dev-dependencies]` and call [`capture`] first thing in each
//! test, before any instrument is used. It takes the place of [`init`](crate::init), so a test
//! binary calls one or the other.
//!
//! ```
//! use sisa_telemetry::{instruments, testing};
//!
//! let captured = testing::capture();
//!
//! tracing::info_span!("place_order").in_scope(|| {
//!     let meter = instruments::meter("orders", "0.1.0");
//!     meter.u64_counter("orders.placed").build().add(1, &[]);
//! });
//!
//! assert!(captured.span("place_order").is_some());
//! assert_eq!(captured.metric("orders.placed").unwrap().total(), 1.0);
//! ```
//!
//! Tests of one binary share what is captured when they run in one process, as under
//! `cargo test`: give each test's spans and metrics names of their own, or run the tests with
//! cargo-nextest, which gives each test a process.

use std::sync::OnceLock;

use opentelemetry::{KeyValue, global};
pub use opentelemetry_sdk::trace::SpanData;
use opentelemetry_sdk::{
    metrics::{
        InMemoryMetricExporter, SdkMeterProvider,
        data::{AggregatedMetrics, Metric, MetricData},
    },
    trace::{InMemorySpanExporter, SdkTracerProvider},
};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use crate::init::{cardinality_view, propagator, trace_layer};

static CAPTURED: OnceLock<Captured> = OnceLock::new();

/// One series of a captured metric: one set of attributes and what was recorded with it.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct CapturedPoint {
    /// The attributes the values were recorded with.
    pub attributes: Vec<KeyValue>,
    /// A counter's sum, a gauge's last value, or the sum of a histogram's recordings.
    pub value: f64,
    /// How many values a histogram recorded; 0 for a counter or a gauge.
    pub count: u64,
    /// A histogram's bucket boundaries; empty for a counter or a gauge.
    pub bounds: Vec<f64>,
}

/// A metric as the SDK would export it: its unit, its description and one point per attribute
/// set.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct CapturedMetric {
    /// The instrument's name.
    pub name: String,
    /// The instrument's unit, such as `s` or `{order}`.
    pub unit: String,
    /// The instrument's description.
    pub description: String,
    /// One point per attribute set.
    pub points: Vec<CapturedPoint>,
}

impl CapturedMetric {
    /// The sum of every point's value: a counter's total over all attribute sets.
    pub fn total(&self) -> f64 {
        self.points.iter().map(|point| point.value).sum()
    }
}

// Values come back as `f64` whatever the instrument counts in. A test's counts are far below
// the 2^53 where an `f64` stops being exact.
trait Number: Copy {
    fn into_f64(self) -> f64;
}

// A float instrument's values, as they are.
impl Number for f64 {
    fn into_f64(self) -> f64 {
        self
    }
}

// A `u64` counter's values.
impl Number for u64 {
    fn into_f64(self) -> f64 {
        self as f64
    }
}

// An `i64` up-down counter's or gauge's values.
impl Number for i64 {
    fn into_f64(self) -> f64 {
        self as f64
    }
}

fn point(attributes: Vec<KeyValue>, value: f64) -> CapturedPoint {
    CapturedPoint {
        attributes,
        value,
        count: 0,
        bounds: Vec::new(),
    }
}

fn points<T: Number>(data: &MetricData<T>) -> Vec<CapturedPoint> {
    match data {
        MetricData::Gauge(gauge) => gauge
            .data_points()
            .map(|each| {
                point(
                    each.attributes().cloned().collect(),
                    each.value().into_f64(),
                )
            })
            .collect(),
        MetricData::Sum(sum) => sum
            .data_points()
            .map(|each| {
                point(
                    each.attributes().cloned().collect(),
                    each.value().into_f64(),
                )
            })
            .collect(),
        MetricData::Histogram(histogram) => histogram
            .data_points()
            .map(|each| CapturedPoint {
                attributes: each.attributes().cloned().collect(),
                value: each.sum().into_f64(),
                count: each.count(),
                bounds: each.bounds().collect(),
            })
            .collect(),
        MetricData::ExponentialHistogram(histogram) => histogram
            .data_points()
            .map(|each| CapturedPoint {
                attributes: each.attributes().cloned().collect(),
                value: each.sum().into_f64(),
                count: each.count() as u64,
                bounds: Vec::new(),
            })
            .collect(),
    }
}

fn captured_metric(metric: &Metric) -> CapturedMetric {
    CapturedMetric {
        name: metric.name().to_owned(),
        unit: metric.unit().to_owned(),
        description: metric.description().to_owned(),
        points: match metric.data() {
            AggregatedMetrics::F64(data) => points(data),
            AggregatedMetrics::U64(data) => points(data),
            AggregatedMetrics::I64(data) => points(data),
        },
    }
}

/// A handle to what the test binary has recorded since [`capture`] or the last
/// [`clear`](Self::clear).
#[derive(Debug, Clone)]
pub struct Captured {
    spans: InMemorySpanExporter,
    metrics: InMemoryMetricExporter,
    meter_provider: SdkMeterProvider,
}

impl Captured {
    /// Every span that has ended, in the order they ended. A span ends when its last handle is
    /// dropped, so drop a span, or leave its scope, before looking for it.
    pub fn spans(&self) -> Vec<SpanData> {
        self.spans.get_finished_spans().unwrap_or_default()
    }

    /// The span with this name that ended last, if any. The name is the span's `otel.name` field
    /// when it has one.
    pub fn span(&self, name: &str) -> Option<SpanData> {
        self.spans().into_iter().rfind(|span| span.name == name)
    }

    /// The metric with this name as it stands now, if anything was recorded on it.
    pub fn metric(&self, name: &str) -> Option<CapturedMetric> {
        _ = self.meter_provider.force_flush();
        let exports = self.metrics.get_finished_metrics().unwrap_or_default();
        // Metrics are cumulative, so the last export holds the current values.
        exports.iter().rev().find_map(|export| {
            export
                .scope_metrics()
                .flat_map(|scope| scope.metrics())
                .find(|metric| metric.name() == name)
                .map(captured_metric)
        })
    }

    /// Forgets the spans captured so far, and the metric exports. Instruments keep their running
    /// totals: a counter doesn't go back to zero. It empties what every test of the process
    /// captured, so under `cargo test` keep a test that calls it in a binary of its own.
    pub fn clear(&self) {
        self.spans.reset();
        self.metrics.reset();
    }
}

fn install() -> Captured {
    let spans = InMemorySpanExporter::default();
    let metrics = InMemoryMetricExporter::default();
    let tracer_provider = SdkTracerProvider::builder()
        .with_simple_exporter(spans.clone())
        .build();
    let meter_provider = SdkMeterProvider::builder()
        .with_periodic_exporter(metrics.clone())
        .with_view(cardinality_view(2000))
        .build();

    tracing_subscriber::registry()
        .with(trace_layer(&tracer_provider))
        .try_init()
        .expect("capture() runs before anything else installs a global tracing subscriber");
    global::set_tracer_provider(tracer_provider);
    global::set_meter_provider(meter_provider.clone());
    global::set_text_map_propagator(propagator());

    Captured {
        spans,
        metrics,
        meter_provider,
    }
}

/// Installs in-memory exporters once per test binary and returns a handle to read them.
///
/// The first call installs the global `tracing` subscriber, the tracer and meter providers and
/// the W3C propagators, as [`init`](crate::init) does, with nothing exported and no log
/// filter; later calls return the same handle. Every new trace is kept; a span continued from
/// headers that say "not sampled" is not, as with the default sampler.
///
/// # Panics
///
/// When a global `tracing` subscriber is installed already, since no span would be captured.
pub fn capture() -> Captured {
    CAPTURED.get_or_init(install).clone()
}
