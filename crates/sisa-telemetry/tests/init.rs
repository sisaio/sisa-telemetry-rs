//! What `init` refuses, and what it sets up with telemetry off. Only the last test installs the
//! global subscriber, so they share a binary.

use std::{collections::HashMap, time::Duration};

use sisa_telemetry::{ServiceResource, TelemetryError, TelemetrySettings, propagation};

fn resource() -> ServiceResource {
    ServiceResource::new("init-test", "0.1.0")
}

#[test]
fn grpc_outside_a_tokio_runtime_is_an_error() {
    let result = sisa_telemetry::init(&TelemetrySettings::default(), resource());

    assert!(matches!(result, Err(TelemetryError::NoRuntime)));
}

#[test]
fn an_invalid_log_filter_is_an_error() {
    let mut settings = TelemetrySettings::default();
    settings.log.filter = "info,sqlx=loud".to_owned();

    let result = sisa_telemetry::init(&settings, resource());

    assert!(matches!(result, Err(TelemetryError::LogFilter(_))));
}

#[tokio::test]
async fn an_endpoint_that_is_no_url_is_an_error() {
    let mut settings = TelemetrySettings::default();
    settings.otlp.endpoint = Some("not a url".to_owned());

    let result = sisa_telemetry::init(&settings, resource());

    assert!(matches!(result, Err(TelemetryError::Exporter(_))));
}

#[tokio::test]
async fn an_https_endpoint_is_an_error_over_grpc() {
    let mut settings = TelemetrySettings::default();
    settings.otlp.endpoint = Some("https://collector.example:4317".to_owned());

    let result = sisa_telemetry::init(&settings, resource());

    assert!(matches!(result, Err(TelemetryError::Exporter(_))));
}

#[test]
fn disabled_sets_up_logs_only_and_a_second_init_is_an_error() {
    let mut settings = TelemetrySettings::default();
    settings.enabled = false;

    let telemetry = sisa_telemetry::init(&settings, resource()).unwrap();

    // No propagator is installed, so nothing is written.
    let mut headers = HashMap::new();
    propagation::inject(&tracing::info_span!("publish"), &mut headers);
    assert!(headers.is_empty());

    let second = sisa_telemetry::init(&settings, resource());
    assert!(matches!(second, Err(TelemetryError::AlreadyInitialized(_))));

    telemetry.shutdown(Duration::from_secs(1)).unwrap();
}
