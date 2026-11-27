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
use serde::Deserialize;
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

// The attributes every catalog entry allows besides its own: the tenant, which
// `metrics.tenant_label` adds, and the one the SDK puts on the point past the cardinality limit.
const ALWAYS_ALLOWED: [&str; 2] = ["sisa.tenant", "otel.metric.overflow"];

// One metric of `catalog/metrics.json`.
#[derive(Deserialize)]
struct CatalogEntry {
    name: String,
    unit: String,
    attributes: Vec<String>,
}

fn catalog() -> Vec<CatalogEntry> {
    serde_json::from_str(include_str!("../catalog/metrics.json"))
        .expect("catalog/metrics.json is valid, as a unit test checks")
}

// What is wrong with one captured metric against the catalog, if it is one the catalog governs:
// listed in it, or named `sisa.*`.
fn catalog_problems(metric: &CapturedMetric, catalog: &[CatalogEntry]) -> Vec<String> {
    let name = &metric.name;
    let Some(entry) = catalog.iter().find(|entry| &entry.name == name) else {
        return if name.starts_with("sisa.") {
            vec![format!("`{name}` is not in the catalog")]
        } else {
            Vec::new()
        };
    };

    let mut problems = Vec::new();
    if metric.unit != entry.unit {
        problems.push(format!(
            "`{name}` has the unit `{}`, and the catalog says `{}`",
            metric.unit, entry.unit
        ));
    }
    let mut unknown: Vec<&str> = metric
        .points
        .iter()
        .flat_map(|point| &point.attributes)
        .map(|attribute| attribute.key.as_str())
        .filter(|key| {
            !ALWAYS_ALLOWED.contains(key) && !entry.attributes.iter().any(|allowed| allowed == key)
        })
        .collect();
    unknown.sort_unstable();
    unknown.dedup();
    for key in unknown {
        problems.push(format!(
            "`{name}` has the attribute `{key}`, which the catalog doesn't allow"
        ));
    }
    problems
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

    /// Checks every metric recorded so far against the sisa metric catalog, the file
    /// `catalog/metrics.json` in this crate, so a sisa crate's test catches a metric that
    /// dashboards wouldn't find.
    ///
    /// A metric named `sisa.*`, or one the catalog lists, must be in the catalog, with its unit,
    /// and carry only the attributes it allows, plus `sisa.tenant` and `otel.metric.overflow`.
    /// Other metrics, such as a service's own, are left alone.
    ///
    /// ```
    /// use sisa_telemetry::{instruments, opentelemetry::KeyValue, testing};
    ///
    /// let captured = testing::capture();
    /// let meter = instruments::meter("sisa-lock", "0.1.0");
    /// let lost = meter.u64_counter("sisa.lock.lost").with_unit("{lock}").build();
    /// lost.add(1, &[KeyValue::new("sisa.lock.name", "billing")]);
    ///
    /// captured.check_catalog().unwrap();
    /// ```
    ///
    /// # Errors
    ///
    /// Every problem found, one per line.
    pub fn check_catalog(&self) -> Result<(), String> {
        _ = self.meter_provider.force_flush();
        let exports = self.metrics.get_finished_metrics().unwrap_or_default();
        let catalog = catalog();
        let problems: Vec<String> = exports
            .last()
            .into_iter()
            .flat_map(|export| export.scope_metrics())
            .flat_map(|scope| scope.metrics())
            .flat_map(|metric| catalog_problems(&captured_metric(metric), &catalog))
            .collect();
        if problems.is_empty() {
            Ok(())
        } else {
            Err(problems.join("\n"))
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn metric(
        name: &str,
        unit: &str,
        attributes: &[(&'static str, &'static str)],
    ) -> CapturedMetric {
        CapturedMetric {
            name: name.to_owned(),
            unit: unit.to_owned(),
            description: String::new(),
            points: vec![point(
                attributes
                    .iter()
                    .map(|(key, value)| KeyValue::new(*key, *value))
                    .collect(),
                1.0,
            )],
        }
    }

    #[test]
    fn the_catalog_parses_and_names_each_metric_once() {
        let catalog = catalog();
        let mut names: Vec<&str> = catalog.iter().map(|entry| entry.name.as_str()).collect();
        names.sort_unstable();
        names.dedup();

        assert_eq!(names.len(), catalog.len());
    }

    #[test]
    fn a_metric_as_the_catalog_lists_it_passes() {
        let lost = metric(
            "sisa.lock.lost",
            "{lock}",
            &[("sisa.lock.name", "billing"), ("sisa.tenant", "acme")],
        );

        assert!(catalog_problems(&lost, &catalog()).is_empty());
    }

    #[test]
    fn a_sisa_metric_missing_from_the_catalog_fails() {
        let unknown = metric("sisa.lock.stolen", "{lock}", &[]);

        assert_eq!(catalog_problems(&unknown, &catalog()).len(), 1);
    }

    #[test]
    fn another_unit_or_an_unknown_attribute_fails() {
        let wrong = metric(
            "sisa.lock.wait_duration",
            "ms",
            &[("sisa.lock.holder", "42")],
        );

        let problems = catalog_problems(&wrong, &catalog());

        assert_eq!(problems.len(), 2);
        assert!(problems[0].contains("`ms`"));
        assert!(problems[1].contains("sisa.lock.holder"));
    }

    #[test]
    fn a_service_metric_is_left_alone() {
        let placed = metric("orders.placed", "{order}", &[("order.id", "42")]);

        assert!(catalog_problems(&placed, &catalog()).is_empty());
    }
}
