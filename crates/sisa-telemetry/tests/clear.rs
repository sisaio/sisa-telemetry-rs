//! `Captured::clear` forgets what every test of the process captured, so its test has a binary
//! of its own.

use sisa_telemetry::testing;

// Recipe 6 of docs/instrumenting.md.
#[test]
fn clear_forgets_the_captured_spans() {
    let captured = testing::capture();
    tracing::info_span!("cleared_span").in_scope(|| {});
    assert!(captured.span("cleared_span").is_some());

    captured.clear();

    assert!(captured.span("cleared_span").is_none());
}
