//! What can go wrong while telemetry starts or stops.

/// Why [`init`](crate::init) or [`TelemetryGuard::shutdown`](crate::TelemetryGuard::shutdown)
/// failed.
///
/// Recording returns no error: once telemetry runs, a Collector that is down costs the spans
/// that don't fit the export queue and a log line per failed export, not an error in the
/// service.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum TelemetryError {
    /// `log.filter` isn't valid filter syntax, such as `info,sqlx=warn`.
    #[error("the log filter is invalid: {0}")]
    LogFilter(String),

    /// A name or value in `otlp.headers` can't be sent as a header. The value is left out, since
    /// headers carry API keys.
    #[error("the OTLP header `{name}` is not a valid header name and value")]
    Header {
        /// The header's name.
        name: String,
    },

    /// `OTEL_EXPORTER_OTLP_PROTOCOL` names a protocol other than `grpc` and `http/protobuf`.
    #[error("the OTLP protocol `{0}` is not supported: use `grpc` or `http/protobuf`")]
    Protocol(String),

    /// The gRPC exporter needs a Tokio runtime, and `init` was called outside one.
    #[error("the OTLP gRPC exporter needs a Tokio runtime: call init inside one")]
    NoRuntime,

    /// An OTLP exporter couldn't be built, for example from an endpoint that isn't a URL.
    #[error("the OTLP exporter could not be built: {0}")]
    Exporter(String),

    /// A global `tracing` subscriber is installed already: `init` runs once per process.
    #[error("telemetry is initialized already: {0}")]
    AlreadyInitialized(String),

    /// A provider reported that its shutdown failed or timed out; what it still held is lost.
    #[error("telemetry did not shut down cleanly: {0}")]
    Shutdown(String),
}
