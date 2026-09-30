//! Meter helpers for application instruments.
//!
//! # Contract
//!
//! - The helpers record into the global meter provider that the plugin installed.
//! - Before the plugin runs, the global provider is the no-op provider.
//!   Instruments still build and record; the measurements go nowhere.
//! - The scope of every meter is `autumn-plugin-otel` unless the caller names its own.

use opentelemetry::metrics::{Counter, Gauge, Histogram, Meter, UpDownCounter};

/// Makes a meter with the plugin's instrumentation scope.
#[must_use]
pub fn meter() -> Meter {
    meter_named("autumn-plugin-otel")
}

/// Makes a meter with a custom instrumentation scope.
#[must_use]
pub fn meter_named(name: &'static str) -> Meter {
    opentelemetry::global::meter(name)
}

/// Builds the standard instruments for one meter.
pub struct OtelMetrics {
    meter: Meter,
}

impl OtelMetrics {
    /// Makes helpers over the plugin's meter.
    #[must_use]
    pub fn new() -> Self {
        Self::with_meter(meter())
    }

    /// Makes helpers over `meter`.
    #[must_use]
    pub const fn with_meter(meter: Meter) -> Self {
        Self { meter }
    }

    /// Builds a monotonic counter, for example `http_requests_total`.
    #[must_use]
    pub fn counter(&self, name: &str, description: &str) -> Counter<u64> {
        self.meter
            .u64_counter(name.to_owned())
            .with_description(description.to_owned())
            .build()
    }

    /// Builds a histogram, for example `http_request_duration_seconds`.
    #[must_use]
    pub fn histogram(&self, name: &str, description: &str) -> Histogram<f64> {
        self.meter
            .f64_histogram(name.to_owned())
            .with_description(description.to_owned())
            .build()
    }

    /// Builds a gauge, for example `queue_depth`.
    #[must_use]
    pub fn gauge(&self, name: &str, description: &str) -> Gauge<f64> {
        self.meter
            .f64_gauge(name.to_owned())
            .with_description(description.to_owned())
            .build()
    }

    /// Builds an up-down counter, for example `active_connections`.
    #[must_use]
    pub fn up_down_counter(&self, name: &str, description: &str) -> UpDownCounter<i64> {
        self.meter
            .i64_up_down_counter(name.to_owned())
            .with_description(description.to_owned())
            .build()
    }
}

impl Default for OtelMetrics {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for OtelMetrics {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OtelMetrics").finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests;
