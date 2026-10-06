//! Tests for the telemetry handle. They install no subscriber.

use std::sync::Arc;

use axum::extract::FromRequestParts as _;

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

fn request_parts() -> http::request::Parts {
    http::Request::new(()).into_parts().0
}

#[tokio::test]
async fn extractor_returns_the_installed_handle() {
    let state = AppState::detached();
    state.insert_extension(OtelTelemetry::new(shared()));
    let telemetry = OtelTelemetry::from_request_parts(&mut request_parts(), &state)
        .await
        .unwrap();
    assert_eq!(
        telemetry
            .config()
            .map(|config| config.service_name.as_str()),
        Some("autumn-app")
    );
}

#[tokio::test]
async fn extractor_rejects_when_the_plugin_is_not_installed() {
    let state = AppState::detached();
    let rejection = OtelTelemetry::from_request_parts(&mut request_parts(), &state)
        .await
        .unwrap_err();
    assert_eq!(rejection.status(), http::StatusCode::INTERNAL_SERVER_ERROR);
}
