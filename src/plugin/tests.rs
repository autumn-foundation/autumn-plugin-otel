//! Tests for the sampler, the resource, the plugin builder and the shared state.
//! They install no subscriber and touch no collector.

// `Plugin` comes in through `super::*`, which picks up the parent's import.

use super::*;
use crate::config::OtelProtocol;

fn enabled_config() -> OtelConfig {
    OtelConfig {
        enabled: true,
        ..OtelConfig::default()
    }
}

#[test]
fn plugin_name_is_stable() {
    assert_eq!(OtelPlugin::new().name(), PLUGIN_NAME);
    assert_eq!(PLUGIN_NAME, "autumn-plugin-otel");
}

#[test]
fn debug_output_never_carries_an_explicit_endpoint() {
    let config = OtelConfig {
        endpoint: "https://collector-secret.internal:4317".into(),
        ..OtelConfig::default()
    };
    let plugin = OtelPlugin::new().config(config);
    let debug = format!("{plugin:?}");
    assert!(debug.contains("(explicit)"));
    assert!(
        !debug.contains("collector-secret"),
        "endpoint leaked: {debug}"
    );
    assert!(!debug.contains("4317"), "endpoint leaked: {debug}");
}

#[test]
fn configure_records_each_change() {
    let plugin = OtelPlugin::new()
        .configure(|c| c.service_name = "first".into())
        .configure(|c| c.sample_ratio = 0.5);
    assert!(format!("{plugin:?}").contains("changes: 2"));
}

#[test]
fn resolve_applies_configure_changes() {
    let plugin = OtelPlugin::new().configure(|c| {
        c.enabled = true;
        c.service_name = "resolved".into();
        c.sample_ratio = 0.25;
    });
    let config = OtelPlugin::resolve(&plugin.source, plugin.changes).unwrap();
    assert!(config.enabled);
    assert_eq!(config.service_name, "resolved");
    assert!(
        (config.sample_ratio - 0.25).abs() < 1e-12,
        "got {}",
        config.sample_ratio
    );
}

#[test]
fn resolve_fails_on_a_bad_explicit_config() {
    let config = OtelConfig {
        enabled: true,
        endpoint: "not a uri".into(),
        ..OtelConfig::default()
    };
    let plugin = OtelPlugin::new().config(config);
    assert!(OtelPlugin::resolve(&plugin.source, plugin.changes).is_err());
}

#[test]
fn build_sampler_is_parent_based() {
    let sampler = build_sampler(0.25);
    let debug = format!("{sampler:?}");
    assert!(debug.contains("ParentBased"), "got: {debug}");
}

#[test]
fn build_resource_sets_the_service_attributes() {
    let mut config = enabled_config();
    config.service_name = "shop".into();
    config.service_version = "1.2.3".into();
    config.service_namespace = Some("team-a".into());
    config.environment = "prod".into();
    let resource = build_resource(&config);
    let get = |key: &'static str| {
        resource
            .get(&opentelemetry::Key::from(key))
            .map(|value| value.to_string())
    };
    assert_eq!(get("service.name").as_deref(), Some("shop"));
    assert_eq!(get("service.version").as_deref(), Some("1.2.3"));
    assert_eq!(get("service.namespace").as_deref(), Some("team-a"));
    assert_eq!(get("deployment.environment").as_deref(), Some("prod"));
}

#[test]
fn build_resource_omits_an_empty_namespace() {
    let config = enabled_config();
    let resource = build_resource(&config);
    assert!(
        resource
            .get(&opentelemetry::Key::from("service.namespace"))
            .is_none()
    );
    let mut with_empty = enabled_config();
    with_empty.service_namespace = Some(String::new());
    let resource = build_resource(&with_empty);
    assert!(
        resource
            .get(&opentelemetry::Key::from("service.namespace"))
            .is_none()
    );
}

#[test]
fn resolve_format_matches_the_framework() {
    assert_eq!(
        resolve_format(LogFormat::Pretty, None),
        ResolvedFormat::Pretty
    );
    assert_eq!(resolve_format(LogFormat::Json, None), ResolvedFormat::Json);
    assert_eq!(
        resolve_format(LogFormat::Auto, None),
        ResolvedFormat::Pretty
    );
    assert_eq!(
        resolve_format(LogFormat::Auto, Some("dev")),
        ResolvedFormat::Pretty
    );
    assert_eq!(
        resolve_format(LogFormat::Auto, Some("prod")),
        ResolvedFormat::Json
    );
    assert_eq!(
        resolve_format(LogFormat::Auto, Some("production")),
        ResolvedFormat::Json
    );
}

#[test]
fn protocol_availability_matches_the_features() {
    assert_eq!(OtelProtocol::Grpc.is_available(), cfg!(feature = "grpc"));
    assert_eq!(OtelProtocol::Http.is_available(), cfg!(feature = "http"));
}

#[test]
fn shared_lifecycle_moves_one_way() {
    let shared = Shared::new(Ok(enabled_config()));
    assert!(!shared.is_running());
    assert!(!shared.is_shut_down());
    shared.mark_running();
    assert!(shared.is_running());
    // Shutdown flushes once and is idempotent.
    shared.shutdown();
    assert!(shared.is_shut_down());
    assert!(!shared.is_running());
    shared.shutdown();
    assert!(shared.is_shut_down());
}

#[test]
fn shared_shutdown_without_start_stays_not_started() {
    let shared = Shared::new(Ok(enabled_config()));
    shared.shutdown();
    assert!(!shared.is_shut_down());
    assert!(!shared.is_running());
}

#[test]
fn shared_degraded_is_running_but_degraded() {
    let shared = Shared::new(Ok(enabled_config()));
    shared.mark_degraded();
    assert!(shared.is_running());
    assert!(shared.is_degraded());
    assert!(!shared.is_shut_down());
    shared.shutdown();
    assert!(shared.is_shut_down());
    assert!(!shared.is_running());
}
