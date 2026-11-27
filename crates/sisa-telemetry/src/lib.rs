//! OpenTelemetry for Rust services: one [`init`] for traces, metrics and logs over OTLP, plus
//! trace headers that carry across messaging.
//!
//! A service calls [`init`] once, first thing in `main`, and from then on writes plain
//! [`tracing`](https://docs.rs/tracing) spans and events and uses the OpenTelemetry metrics API.
//! `init` turns them into JSON logs on stdout and OTLP traces and metrics, sent to an
//! OpenTelemetry Collector, which decides where each signal goes.
//!
//! ```no_run
//! use std::{sync::LazyLock, time::Duration};
//!
//! use sisa_telemetry::{
//!     ServiceResource, TelemetrySettings, instruments,
//!     opentelemetry::{KeyValue, metrics::Counter},
//! };
//!
//! static ORDERS_PLACED: LazyLock<Counter<u64>> = LazyLock::new(|| {
//!     instruments::meter(env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"))
//!         .u64_counter("orders.placed")
//!         .with_unit("{order}")
//!         .with_description("Orders placed")
//!         .build()
//! });
//!
//! #[tracing::instrument]
//! fn place_order(channel: &'static str) {
//!     ORDERS_PLACED.add(1, &[KeyValue::new("channel", channel)]);
//! }
//!
//! #[tokio::main]
//! async fn main() -> Result<(), sisa_telemetry::TelemetryError> {
//!     let telemetry = sisa_telemetry::init(
//!         &TelemetrySettings::default(),
//!         ServiceResource::new(env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION")),
//!     )?;
//!
//!     place_order("web");
//!
//!     telemetry.shutdown(Duration::from_secs(2))
//! }
//! ```
//!
//! Everything in `sisa-telemetry-core` is re-exported here: [`propagation`] for trace headers
//! over messages and HTTP, [`record_error`], [`instruments`], [`attributes`] and the
//! [`opentelemetry`] API itself, so a service depends on no OpenTelemetry crate directly.
//! Library crates depend on `sisa-telemetry-core` alone, never on this crate: it installs
//! process-wide providers, and only the application may do that, once.
//!
//! | Feature | Adds |
//! | --- | --- |
//! | `prometheus` | [`prometheus_text`]: the metrics in the Prometheus text format, for an admin port's `/metrics` |
//! | `testing` | The `testing` module: `capture()` installs in-memory exporters, to assert on spans and metrics in tests |

#![cfg_attr(docsrs, feature(doc_cfg))]

mod error;
mod init;
mod resource;
mod settings;
#[cfg(feature = "testing")]
#[cfg_attr(docsrs, doc(cfg(feature = "testing")))]
pub mod testing;

pub use error::TelemetryError;
#[cfg(feature = "prometheus")]
pub use init::prometheus_text;
pub use init::{TelemetryGuard, init, reload_log_filter};
pub use resource::ServiceResource;
pub use settings::{
    LogFormat, LogSettings, MetricsSettings, OtlpProtocol, OtlpSettings, ResourceSettings,
    TelemetrySettings, TracesSampler, TracesSettings,
};
pub use sisa_telemetry_core::*;
