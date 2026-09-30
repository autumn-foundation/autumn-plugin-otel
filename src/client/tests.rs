//! Tests for the telemetry handle. They install no subscriber.

use std::sync::Arc;

use super::*;
use crate::config::OtelConfig;
use crate::plugin::Shared;

fn shared() -> Arc<Shared> {
    Arc::new(Shared::new(Ok(OtelConfig::default())))
}

#[test]
fn handle_reports_config_and_lifecycle() {
    let telemetry = OtelTelemetry::new(shared());
    assert!(!telemetry.is_running());
    assert_eq!(
        telemetry
            .config()
            .map(|config| config.service_name.as_str()),
        Some("autumn-app")
    );
    assert!(telemetry.tracer_provider().is_none());
    assert!(telemetry.meter_provider().is_none());
    assert!(telemetry.logger_provider().is_none());
}

#[test]
fn shutdown_is_idempotent_before_start() {
    let telemetry = OtelTelemetry::new(shared());
    telemetry.shutdown();
    telemetry.shutdown();
    assert!(!telemetry.is_running());
}

#[test]
fn debug_output_names_the_service() {
    let telemetry = OtelTelemetry::new(shared());
    let debug = format!("{telemetry:?}");
    assert!(debug.contains("autumn-app"));
}

#[test]
fn debug_output_never_carries_the_endpoint() {
    let config = OtelConfig {
        endpoint: "https://collector-secret.internal:4317".into(),
        ..OtelConfig::default()
    };
    let telemetry = OtelTelemetry::new(Arc::new(Shared::new(Ok(config))));
    let debug = format!("{telemetry:?}");
    assert!(
        !debug.contains("collector-secret"),
        "endpoint leaked: {debug}"
    );
    assert!(!debug.contains("4317"), "endpoint leaked: {debug}");
}

#[test]
fn meter_builds_on_the_global_provider() {
    let telemetry = OtelTelemetry::new(shared());
    let meter = telemetry.meter("test-scope");
    let counter = meter
        .u64_counter("test_client_total".to_owned())
        .with_description("client test".to_owned())
        .build();
    counter.add(1, &[]);
}
