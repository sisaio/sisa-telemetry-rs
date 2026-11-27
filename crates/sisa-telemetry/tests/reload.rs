//! `reload_log_filter` before and after `init`, which installs the global subscriber, so the
//! steps share one test.

use sisa_telemetry::{ServiceResource, TelemetryError, TelemetrySettings};
use tracing::Level;

#[test]
fn reload_log_filter_changes_the_level_after_init() {
    let before = sisa_telemetry::reload_log_filter("debug");
    assert!(matches!(before, Err(TelemetryError::NotInitialized)));

    let mut settings = TelemetrySettings::default();
    settings.enabled = false;
    settings.log.filter = "info".to_owned();
    let _telemetry =
        sisa_telemetry::init(&settings, ServiceResource::new("reload-test", "0.1.0")).unwrap();
    assert!(!tracing::enabled!(Level::DEBUG));

    let invalid = sisa_telemetry::reload_log_filter("info,sqlx=loud");
    assert!(matches!(invalid, Err(TelemetryError::LogFilter(_))));
    assert!(!tracing::enabled!(Level::DEBUG));

    sisa_telemetry::reload_log_filter("debug").unwrap();
    assert!(tracing::enabled!(Level::DEBUG));
}
