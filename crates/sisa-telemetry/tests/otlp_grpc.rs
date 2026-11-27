//! `init` exports to a real OpenTelemetry Collector over gRPC. It installs the global
//! providers, so it has a binary of its own.

mod support;

use std::time::Duration;

use sisa_telemetry::{OtlpProtocol, ServiceResource, TelemetrySettings, instruments};

use support::Collector;

#[tokio::test(flavor = "multi_thread")]
async fn init_exports_spans_metrics_and_logs_over_grpc() {
    let collector = Collector::start().await;
    let mut settings = TelemetrySettings::default();
    settings.otlp.protocol = OtlpProtocol::Grpc;
    settings.otlp.endpoint = Some(collector.endpoint(4317).await);
    settings
        .resource
        .attributes
        .insert("region".to_owned(), "test-region".to_owned());
    settings.log.otlp = true;

    let telemetry =
        sisa_telemetry::init(&settings, ServiceResource::new("otlp-grpc-test", "0.1.0")).unwrap();
    tracing::info_span!("exported_over_grpc").in_scope(|| {
        let meter = instruments::meter("otlp-grpc-test", "0.1.0");
        meter.u64_counter("exports.counted").build().add(1, &[]);
        tracing::info!("logged_over_grpc");
    });
    // The last export runs on this runtime, so the wait for it goes on a blocking thread.
    tokio::task::spawn_blocking(move || telemetry.shutdown(Duration::from_secs(10)))
        .await
        .unwrap()
        .unwrap();

    collector
        .received(&[
            "exported_over_grpc",
            "logged_over_grpc",
            "exports.counted",
            "otlp-grpc-test",
            "test-region",
        ])
        .await;
}
