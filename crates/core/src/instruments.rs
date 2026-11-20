//! Meters and the standard duration histogram, so every crate's metrics have the same shape.
//!
//! Create instruments after the application has called `sisa_telemetry::init`: an instrument
//! created before the provider is installed stays a no-op for the life of the process. A
//! `static` in a [`LazyLock`](std::sync::LazyLock), first used when the first request is handled,
//! does that by itself.

use opentelemetry::{
    InstrumentationScope, global,
    metrics::{Histogram, Meter},
};

// 5 ms to 10 s, the buckets the OpenTelemetry HTTP conventions recommend for durations in seconds.
const DURATION_BUCKETS: [f64; 14] = [
    0.005, 0.01, 0.025, 0.05, 0.075, 0.1, 0.25, 0.5, 0.75, 1.0, 2.5, 5.0, 7.5, 10.0,
];

/// The meter of one crate; its scope is the crate's name and version.
///
/// ```
/// use std::sync::LazyLock;
///
/// use sisa_telemetry_core::{instruments, opentelemetry::metrics::Meter};
///
/// static METER: LazyLock<Meter> =
///     LazyLock::new(|| instruments::meter(env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION")));
/// ```
pub fn meter(scope: &'static str, version: &'static str) -> Meter {
    global::meter_with_scope(
        InstrumentationScope::builder(scope)
            .with_version(version)
            .build(),
    )
}

/// A duration histogram in seconds with the standard buckets (5 ms .. 10 s).
///
/// ```
/// use sisa_telemetry_core::instruments;
///
/// let meter = instruments::meter("orders", "0.1.0");
/// let duration =
///     instruments::duration_histogram(&meter, "orders.place.duration", "Time to place an order");
/// duration.record(0.042, &[]);
/// ```
pub fn duration_histogram(
    meter: &Meter,
    name: &'static str,
    description: &'static str,
) -> Histogram<f64> {
    meter
        .f64_histogram(name)
        .with_unit("s")
        .with_description(description)
        .with_boundaries(DURATION_BUCKETS.to_vec())
        .build()
}
