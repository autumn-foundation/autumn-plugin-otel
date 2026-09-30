//! Tests for the health indicator. They touch no collector.

use autumn_web::actuator::{HealthIndicator, HealthStatus, IndicatorGroup};

use super::*;
use crate::config::{ConfigError, OtelConfig};

fn running_shared() -> Arc<Shared> {
    let config = OtelConfig {
        enabled: true,
        // Deliberately secret-looking: health output must never carry it.
        endpoint: "https://collector-secret.internal:4317".into(),
        service_name: "shop".into(),
        sample_ratio: 0.25,
        ..OtelConfig::default()
    };
    let shared = Arc::new(Shared::new(Ok(config)));
    shared.mark_running();
    shared
}

#[test]
fn group_is_health_only() {
    let check = OtelHealthCheck::new(Arc::new(Shared::new(Ok(OtelConfig::default()))));
    assert_eq!(check.group(), IndicatorGroup::HealthOnly);
}

#[tokio::test]
async fn check_is_down_before_start() {
    let check = OtelHealthCheck::new(Arc::new(Shared::new(Ok(OtelConfig::default()))));
    let output = check.check().await;
    assert_eq!(output.status, HealthStatus::Down);
    assert_eq!(
        output.details.get("state").and_then(|v| v.as_str()),
        Some("not started")
    );
}

#[tokio::test]
async fn check_is_down_on_a_bad_config() {
    let shared = Arc::new(Shared::new(Err(ConfigError("otel.enabled nope".into()))));
    shared.mark_running();
    let check = OtelHealthCheck::new(shared);
    let output = check.check().await;
    assert_eq!(output.status, HealthStatus::Down);
}

#[tokio::test]
async fn check_is_up_while_running_and_reports_the_config() {
    let check = OtelHealthCheck::new(running_shared());
    let output = check.check().await;
    assert_eq!(output.status, HealthStatus::Up);
    let details = &output.details;
    assert_eq!(
        details.get("state").and_then(|v| v.as_str()),
        Some("configured")
    );
    // The collector address must never leak into health output.
    assert!(!details.contains_key("endpoint"));
    assert!(
        !details
            .values()
            .any(|v| v.as_str().is_some_and(|s| s.contains("collector-secret"))),
        "endpoint leaked: {details:?}"
    );
    assert_eq!(
        details.get("protocol").and_then(|v| v.as_str()),
        Some("grpc")
    );
    assert_eq!(
        details.get("sample_ratio").and_then(|v| v.as_str()),
        Some("0.25")
    );
    assert_eq!(
        details.get("signals").and_then(|v| v.as_str()),
        Some("traces,metrics,logs")
    );
    assert_eq!(
        details.get("service_name").and_then(|v| v.as_str()),
        Some("shop")
    );
}

#[tokio::test]
async fn check_is_down_after_shutdown() {
    let shared = running_shared();
    shared.shutdown();
    let check = OtelHealthCheck::new(shared);
    let output = check.check().await;
    assert_eq!(output.status, HealthStatus::Down);
    assert_eq!(
        output.details.get("state").and_then(|v| v.as_str()),
        Some("shut down")
    );
}

#[tokio::test]
async fn check_does_no_network_io() {
    // An unroutable address: if the check ever dialed the collector, this
    // test would hang or fail. It returns at once because the check only
    // reads the in-memory shared state.
    let config = OtelConfig {
        enabled: true,
        endpoint: "http://10.255.255.1:4317".into(),
        ..OtelConfig::default()
    };
    let shared = Arc::new(Shared::new(Ok(config)));
    shared.mark_running();
    let check = OtelHealthCheck::new(shared);
    let output = tokio::time::timeout(std::time::Duration::from_secs(5), check.check())
        .await
        .expect("the health check must not do network I/O");
    assert_eq!(output.status, HealthStatus::Up);
}

#[tokio::test]
async fn check_reports_degraded_when_the_exporters_failed() {
    let config = OtelConfig {
        enabled: true,
        ..OtelConfig::default()
    };
    let shared = Arc::new(Shared::new(Ok(config)));
    shared.mark_degraded();
    let check = OtelHealthCheck::new(shared);
    let output = check.check().await;
    assert_eq!(output.status, HealthStatus::Up);
    assert_eq!(
        output.details.get("state").and_then(|v| v.as_str()),
        Some("degraded (logging only)")
    );
}
