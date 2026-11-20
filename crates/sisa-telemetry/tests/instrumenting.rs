//! The recipes of docs/instrumenting.md, one test each.

use std::sync::LazyLock;

use sisa_telemetry::{
    instruments,
    opentelemetry::{
        KeyValue,
        metrics::{Counter, Histogram, Meter},
        trace::SpanId,
    },
    testing,
};
use tracing::Instrument;

static METER: LazyLock<Meter> =
    LazyLock::new(|| instruments::meter(env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION")));
static ORDERS_PLACED: LazyLock<Counter<u64>> = LazyLock::new(|| {
    METER
        .u64_counter("orders.placed")
        .with_unit("{order}")
        .with_description("Orders placed")
        .build()
});
static PLACE_DURATION: LazyLock<Histogram<f64>> = LazyLock::new(|| {
    instruments::duration_histogram(&METER, "orders.place.duration", "Time to place an order")
});

fn attribute(span: &testing::SpanData, key: &str) -> Option<String> {
    let found = span.attributes.iter().find(|each| each.key.as_str() == key);
    found.map(|each| each.value.as_str().into_owned())
}

// Recipe 1: a custom span, and a field recorded once its value is known.
#[tracing::instrument(skip(card_number), fields(order.id = tracing::field::Empty))]
fn place_order(channel: &'static str, card_number: &str) -> u64 {
    let order_id = 42 + card_number.len() as u64;
    tracing::Span::current().record("order.id", order_id);
    order_id
}

#[test]
fn a_field_recorded_later_is_on_the_span() {
    let captured = testing::capture();

    place_order("web", "4111-canary");

    let span = captured.span("place_order").unwrap();
    assert_eq!(attribute(&span, "order.id").as_deref(), Some("53"));
    assert_eq!(attribute(&span, "channel").as_deref(), Some("web"));
}

// Recipe 7: what is skipped never reaches the span.
#[test]
fn a_skipped_argument_is_not_on_the_span() {
    let captured = testing::capture();

    place_order("web", "4111-canary");

    let span = captured.span("place_order").unwrap();
    assert_eq!(attribute(&span, "card_number"), None);
    assert!(!format!("{span:?}").contains("4111-canary"));
}

// Recipes 2 and 6: a counter added after `capture()` is seen, with its unit.
#[test]
fn capture_sees_a_counter_added_after_it() {
    let captured = testing::capture();

    ORDERS_PLACED.add(2, &[KeyValue::new("channel", "web")]);
    ORDERS_PLACED.add(1, &[KeyValue::new("channel", "app")]);

    let metric = captured.metric("orders.placed").unwrap();
    assert_eq!(metric.unit, "{order}");
    assert_eq!(metric.description, "Orders placed");
    assert_eq!(metric.points.len(), 2);
    assert_eq!(metric.total(), 3.0);
}

// Recipe 2: the standard duration histogram is in seconds, with buckets from 5 ms to 10 s.
#[test]
fn a_duration_histogram_has_the_standard_buckets() {
    let captured = testing::capture();

    PLACE_DURATION.record(0.042, &[]);

    let metric = captured.metric("orders.place.duration").unwrap();
    let point = &metric.points[0];
    assert_eq!(metric.unit, "s");
    assert_eq!(point.count, 1);
    assert_eq!(point.value, 0.042);
    assert_eq!(point.bounds.first(), Some(&0.005));
    assert_eq!(point.bounds.last(), Some(&10.0));
    assert_eq!(point.bounds.len(), 14);
}

// The cardinality limit: past 2000 attribute sets, new ones share one overflow point.
#[test]
fn attribute_sets_past_the_limit_fold_into_one_overflow_point() {
    let captured = testing::capture();
    let counter = METER.u64_counter("orders.by_customer").build();

    for customer in 0..2500_i64 {
        counter.add(1, &[KeyValue::new("customer.id", customer)]);
    }

    let metric = captured.metric("orders.by_customer").unwrap();
    let overflow = |point: &&testing::CapturedPoint| {
        let mut keys = point.attributes.iter().map(|each| each.key.as_str());
        keys.any(|key| key == "otel.metric.overflow")
    };
    assert_eq!(metric.total(), 2500.0);
    assert!(metric.points.len() <= 2001);
    assert_eq!(metric.points.iter().filter(overflow).count(), 1);
}

#[test]
fn a_metric_nobody_recorded_is_missing() {
    let captured = testing::capture();

    assert!(captured.metric("orders.never_recorded").is_none());
}

// Recipe 3: a spawned task keeps its parent span only with `.instrument(span)`.
#[tokio::test]
async fn a_spawned_task_keeps_its_span_only_when_instrumented() {
    let captured = testing::capture();

    let span = tracing::info_span!("spawn_parent");
    let kept_work = async { tracing::info_span!("spawn_kept").in_scope(|| {}) };
    tokio::spawn(kept_work.instrument(span.clone()))
        .await
        .unwrap();
    let lost_work = async { tracing::info_span!("spawn_lost").in_scope(|| {}) };
    span.in_scope(|| tokio::spawn(lost_work)).await.unwrap();
    drop(span);

    let parent = captured.span("spawn_parent").unwrap().span_context;
    let kept = captured.span("spawn_kept").unwrap();
    let lost = captured.span("spawn_lost").unwrap();
    assert_eq!(kept.parent_span_id, parent.span_id());
    assert_eq!(lost.parent_span_id, SpanId::INVALID);
}
