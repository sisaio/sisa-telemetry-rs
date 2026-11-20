//! The `[telemetry]` section of a service's configuration.

use std::{collections::BTreeMap, fmt, time::Duration};

use serde::{Deserialize, Deserializer, de::Error as _};

// A duration as config files write it: a whole number and `ms`, `s`, `m` or `h`, such as `30s`.
fn parse_duration(text: &str) -> Option<Duration> {
    let digits = text.trim_end_matches(|character: char| character.is_ascii_alphabetic());
    let amount: u64 = digits.trim().parse().ok()?;
    match &text[digits.len()..] {
        "ms" => Some(Duration::from_millis(amount)),
        "s" => Some(Duration::from_secs(amount)),
        "m" => amount.checked_mul(60).map(Duration::from_secs),
        "h" => amount.checked_mul(3600).map(Duration::from_secs),
        _ => None,
    }
}

fn deserialize_duration<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Duration, D::Error> {
    let text = String::deserialize(deserializer)?;
    parse_duration(&text).ok_or_else(|| {
        D::Error::custom(format!(
            "invalid duration `{text}`: write a whole number and ms, s, m or h, such as `30s`"
        ))
    })
}

/// The telemetry settings of one service, with the defaults a local Collector needs.
///
/// Build them from `default()` and set fields on it, or deserialize them from the `[telemetry]`
/// section of a config file; a key left out keeps its default. The standard `OTEL_*` environment
/// variables win over these settings, as each field says.
///
/// ```
/// use sisa_telemetry::{LogFormat, TelemetrySettings};
///
/// let mut settings = TelemetrySettings::default();
/// settings.log.format = LogFormat::Pretty;
/// settings.traces.ratio = 0.1;
/// ```
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
#[non_exhaustive]
pub struct TelemetrySettings {
    /// Whether traces and metrics are exported. Off leaves the logs on stdout only: no provider,
    /// no propagator and no exporter. On by default.
    pub enabled: bool,
    /// Where the OTLP exporters send to.
    pub otlp: OtlpSettings,
    /// Trace sampling.
    pub traces: TracesSettings,
    /// The metric export.
    pub metrics: MetricsSettings,
    /// The logs on stdout.
    pub log: LogSettings,
    /// What is added to the service's resource.
    pub resource: ResourceSettings,
}

// On, with every section at its defaults.
impl Default for TelemetrySettings {
    fn default() -> Self {
        Self {
            enabled: true,
            otlp: OtlpSettings::default(),
            traces: TracesSettings::default(),
            metrics: MetricsSettings::default(),
            log: LogSettings::default(),
            resource: ResourceSettings::default(),
        }
    }
}

/// The wire protocol of the OTLP exporters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[non_exhaustive]
pub enum OtlpProtocol {
    /// OTLP over gRPC, to port 4317 by default. [`init`](crate::init) must then run inside a
    /// Tokio runtime.
    #[default]
    #[serde(rename = "grpc")]
    Grpc,
    /// OTLP over HTTP with protobuf bodies, to port 4318 by default.
    #[serde(rename = "http/protobuf")]
    HttpProtobuf,
}

/// `[telemetry.otlp]`: where the OTLP exporters send to, usually a Collector sidecar or
/// DaemonSet.
#[derive(Clone, PartialEq, Eq, Default, Deserialize)]
#[serde(default)]
#[non_exhaustive]
pub struct OtlpSettings {
    /// The Collector's base URL. Left out, it is `http://localhost:4317` for gRPC and
    /// `http://localhost:4318` for HTTP, where `/v1/traces` and `/v1/metrics` are added to it.
    /// `OTEL_EXPORTER_OTLP_ENDPOINT` and its per-signal forms win.
    pub endpoint: Option<String>,
    /// gRPC by default. `OTEL_EXPORTER_OTLP_PROTOCOL` wins.
    pub protocol: OtlpProtocol,
    /// Headers sent with every export, such as a SaaS backend's API key. `Debug` shows their
    /// names only. `OTEL_EXPORTER_OTLP_HEADERS` adds to them; set a name in one place only.
    pub headers: BTreeMap<String, String>,
}

// Header values are secrets, so only their names show.
impl fmt::Debug for OtlpSettings {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("OtlpSettings")
            .field("endpoint", &self.endpoint)
            .field("protocol", &self.protocol)
            .field("headers", &self.headers.keys().collect::<Vec<_>>())
            .finish()
    }
}

/// Which traces are recorded and exported, named as `OTEL_TRACES_SAMPLER` names them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[non_exhaustive]
pub enum TracesSampler {
    /// Every trace.
    #[serde(rename = "always_on")]
    AlwaysOn,
    /// No trace.
    #[serde(rename = "always_off")]
    AlwaysOff,
    /// A share of the traces, set by `ratio`, whatever the caller decided.
    #[serde(rename = "traceidratio")]
    TraceIdRatio,
    /// The caller's decision; every trace that starts here.
    #[serde(rename = "parentbased_always_on")]
    ParentBasedAlwaysOn,
    /// The caller's decision; no trace that starts here.
    #[serde(rename = "parentbased_always_off")]
    ParentBasedAlwaysOff,
    /// The caller's decision; a share of the traces that start here, set by `ratio`.
    #[default]
    #[serde(rename = "parentbased_traceidratio")]
    ParentBasedTraceIdRatio,
}

/// `[telemetry.traces]`: trace sampling.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
#[non_exhaustive]
pub struct TracesSettings {
    /// `parentbased_traceidratio` by default: respect the caller's decision, and sample the
    /// traces that start here by `ratio`. `OTEL_TRACES_SAMPLER` wins, with its ratio in
    /// `OTEL_TRACES_SAMPLER_ARG`; `ratio` is then not read.
    pub sampler: TracesSampler,
    /// The share of traces the ratio samplers keep, from 0.0 to 1.0; a value below keeps none
    /// and one above keeps all. 1.0 by default, which suits development; set about 0.1 in production, and let the Collector's tail sampling keep every
    /// error and slow trace.
    pub ratio: f64,
}

// The caller's decision, and every trace that starts here.
impl Default for TracesSettings {
    fn default() -> Self {
        Self {
            sampler: TracesSampler::default(),
            ratio: 1.0,
        }
    }
}

/// `[telemetry.metrics]`: the metric export.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default)]
#[non_exhaustive]
pub struct MetricsSettings {
    /// How often metrics are pushed over OTLP, written as `30s`, `500ms`, `1m` or `1h`. 30
    /// seconds by default. `OTEL_METRIC_EXPORT_INTERVAL`, in milliseconds, wins; when it is set
    /// to something that isn't a number, the SDK's 60 seconds apply, not this.
    #[serde(deserialize_with = "deserialize_duration")]
    pub interval: Duration,
    /// The most attribute sets one instrument keeps, 2000 by default. Past it, new sets share
    /// one point with the attribute `otel.metric.overflow`: the total stays right, and the
    /// attributes of the extra sets are lost.
    pub cardinality_limit: usize,
}

// A push every 30 seconds, and 2000 series per instrument.
impl Default for MetricsSettings {
    fn default() -> Self {
        Self {
            interval: Duration::from_secs(30),
            cardinality_limit: 2000,
        }
    }
}

/// How the logs on stdout are written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum LogFormat {
    /// One JSON object per line, for a log collector.
    #[default]
    Json,
    /// Colored lines over several rows, for development.
    Pretty,
}

/// `[telemetry.log]`: the logs on stdout, which keep flowing when the Collector is down.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default)]
#[non_exhaustive]
pub struct LogSettings {
    /// JSON by default.
    pub format: LogFormat,
    /// Which spans and events are recorded, in `tracing-subscriber`'s `EnvFilter` syntax:
    /// a default level, then `target=level` pairs. `info,sqlx=warn,hyper=warn` by default. It
    /// filters the traces too: a span below its level is never created.
    pub filter: String,
}

// JSON at `info`, with the noisiest dependencies at `warn`.
impl Default for LogSettings {
    fn default() -> Self {
        Self {
            format: LogFormat::default(),
            filter: "info,sqlx=warn,hyper=warn".to_owned(),
        }
    }
}

/// `[telemetry.resource]`: what is added to the service's resource.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
#[serde(default)]
#[non_exhaustive]
pub struct ResourceSettings {
    /// Extra attributes for every span and metric, such as the cluster and the region. None by
    /// default. They win over the ones given in code, and `OTEL_RESOURCE_ATTRIBUTES` wins over
    /// both.
    pub attributes: BTreeMap<String, String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_defaults_are_the_documented_ones() {
        let settings = TelemetrySettings::default();

        assert!(settings.enabled);
        assert_eq!(settings.otlp.endpoint, None);
        assert_eq!(settings.otlp.protocol, OtlpProtocol::Grpc);
        assert_eq!(
            settings.traces.sampler,
            TracesSampler::ParentBasedTraceIdRatio
        );
        assert_eq!(settings.metrics.interval, Duration::from_secs(30));
        assert_eq!(settings.metrics.cardinality_limit, 2000);
        assert_eq!(settings.log.format, LogFormat::Json);
        assert_eq!(settings.log.filter, "info,sqlx=warn,hyper=warn");
    }

    #[test]
    fn a_config_file_overrides_only_the_keys_it_names() {
        let settings: TelemetrySettings = serde_json::from_value(serde_json::json!({
            "otlp": { "endpoint": "http://collector:4318", "protocol": "http/protobuf" },
            "traces": { "sampler": "always_on" },
            "metrics": { "interval": "500ms" },
            "log": { "format": "pretty" },
            "resource": { "attributes": { "region": "eu-west-1" } },
        }))
        .unwrap();

        assert_eq!(
            settings.otlp.endpoint.as_deref(),
            Some("http://collector:4318")
        );
        assert_eq!(settings.otlp.protocol, OtlpProtocol::HttpProtobuf);
        assert_eq!(settings.traces.sampler, TracesSampler::AlwaysOn);
        assert_eq!(settings.traces.ratio, 1.0);
        assert_eq!(settings.metrics.interval, Duration::from_millis(500));
        assert_eq!(settings.metrics.cardinality_limit, 2000);
        assert_eq!(settings.log.format, LogFormat::Pretty);
        assert_eq!(settings.resource.attributes["region"], "eu-west-1");
    }

    #[test]
    fn a_duration_needs_a_unit() {
        assert_eq!(parse_duration("2m"), Some(Duration::from_secs(120)));
        assert_eq!(parse_duration("1h"), Some(Duration::from_secs(3600)));
        assert_eq!(parse_duration("30"), None);
        assert_eq!(parse_duration("soon"), None);
    }

    #[test]
    fn debug_hides_header_values() {
        let mut settings = OtlpSettings::default();
        settings
            .headers
            .insert("x-api-key".to_owned(), "canary-secret".to_owned());

        let shown = format!("{settings:?}");

        assert!(shown.contains("x-api-key"));
        assert!(!shown.contains("canary-secret"));
    }
}
