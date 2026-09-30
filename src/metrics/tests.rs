//! Tests for the meter helpers. They use the global no-op provider.

use super::*;

#[test]
fn instruments_build_and_record_without_a_pipeline() {
    let metrics = OtelMetrics::new();
    let counter = metrics.counter("test_requests_total", "test requests");
    counter.add(1, &[]);
    let histogram = metrics.histogram("test_duration_seconds", "test durations");
    histogram.record(0.5, &[]);
    let gauge = metrics.gauge("test_queue_depth", "test depth");
    gauge.record(3.0, &[]);
    let up_down = metrics.up_down_counter("test_connections", "test connections");
    up_down.add(1, &[]);
    up_down.add(-1, &[]);
}

#[test]
fn named_meter_builds_instruments() {
    let metrics = OtelMetrics::with_meter(meter_named("my-scope"));
    let counter = metrics.counter("test_named_total", "named");
    counter.add(2, &[]);
}

#[test]
fn default_is_the_plugin_meter() {
    let metrics = OtelMetrics::default();
    let counter = metrics.counter("test_default_total", "default");
    counter.add(1, &[]);
}

#[test]
fn debug_output_does_not_panic() {
    let metrics = OtelMetrics::new();
    let debug = format!("{metrics:?}");
    assert!(debug.contains("OtelMetrics"));
}
