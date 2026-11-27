//! `prometheus_text` before and after `init`, which installs process-wide providers, so the steps
//! share one test.

use sisa_telemetry::{
    OtlpProtocol, ServiceResource, TelemetrySettings, instruments, opentelemetry::KeyValue,
};

#[test]
fn prometheus_text_shows_a_counter_after_init() {
    assert_eq!(sisa_telemetry::prometheus_text(), "");

    // Over HTTP no Tokio runtime is needed; nothing listens there, and a failed push is a log
    // line.
    let mut settings = TelemetrySettings::default();
    settings.otlp.protocol = OtlpProtocol::HttpProtobuf;
    settings.otlp.endpoint = Some("http://127.0.0.1:9".to_owned());
    let _telemetry =
        sisa_telemetry::init(&settings, ServiceResource::new("prometheus-test", "0.1.0")).unwrap();

    instruments::meter("prometheus-test", "0.1.0")
        .u64_counter("orders.placed")
        .with_unit("{order}")
        .build()
        .add(3, &[KeyValue::new("channel", "web")]);

    let text = sisa_telemetry::prometheus_text();
    assert!(
        text.contains("orders_placed_total{channel=\"web\""),
        "{text}"
    );
}
