//! Tests for the `[otel]` configuration. They use no collector. One test writes
//! scratch TOML files to prove profile-file layering.

use std::collections::HashMap;

use autumn_web::config::Env;
#[cfg(any(feature = "grpc", feature = "http"))]
use proptest::prelude::*;

use super::*;

/// An `Env` backed by a map, for deterministic tests.
#[derive(Default)]
struct MapEnv {
    vars: HashMap<String, String>,
}

impl MapEnv {
    fn with(mut self, key: &str, value: &str) -> Self {
        self.vars.insert(key.to_owned(), value.to_owned());
        self
    }
}

impl Env for MapEnv {
    fn var(&self, key: &str) -> Result<String, std::env::VarError> {
        self.vars
            .get(key)
            .cloned()
            .ok_or(std::env::VarError::NotPresent)
    }
}

/// A transport this build includes: gRPC (the default) when it can, else HTTP.
/// A build with neither transport rejects every enabled configuration.
const fn available_protocol() -> OtelProtocol {
    if cfg!(feature = "grpc") {
        OtelProtocol::Grpc
    } else {
        OtelProtocol::Http
    }
}

/// A configuration that is enabled and valid when the build has a transport.
fn enabled_config() -> OtelConfig {
    OtelConfig {
        enabled: true,
        protocol: available_protocol(),
        ..OtelConfig::default()
    }
}

#[test]
fn defaults_keep_the_plugin_off() {
    let config = OtelConfig::default();
    assert!(!config.enabled);
    assert!(config.validate().is_ok());
}

#[test]
fn empty_environment_resolves_to_defaults() {
    let config = OtelConfig::resolve_with_env("otel", &MapEnv::default()).unwrap();
    assert_eq!(config, OtelConfig::default());
}

#[test]
fn disabled_section_resolves_in_every_build() {
    // A disabled plugin needs no transport, so the `protocol` key never fails it.
    let env = MapEnv::default()
        .with("AUTUMN_OTEL__ENABLED", "false")
        .with("AUTUMN_OTEL__PROTOCOL", "http");
    let config = OtelConfig::resolve_with_env("otel", &env).unwrap();
    assert!(!config.enabled);
    assert_eq!(config.protocol, OtelProtocol::Http);
}

#[cfg(feature = "http")]
#[test]
fn environment_overrides_set_each_key() {
    let env = MapEnv::default()
        .with("AUTUMN_OTEL__ENABLED", "true")
        .with("AUTUMN_OTEL__ENDPOINT", "http://collector:4317")
        .with("AUTUMN_OTEL__SERVICE_NAME", "shop")
        .with("AUTUMN_OTEL__SERVICE_VERSION", "2.0.0")
        .with("AUTUMN_OTEL__SERVICE_NAMESPACE", "team-a")
        .with("AUTUMN_OTEL__ENVIRONMENT", "prod")
        .with("AUTUMN_OTEL__SAMPLE_RATIO", "0.25")
        .with("AUTUMN_OTEL__PROTOCOL", "http")
        .with("AUTUMN_OTEL__TRACES", "true")
        .with("AUTUMN_OTEL__METRICS", "false")
        .with("AUTUMN_OTEL__LOGS", "1")
        .with("AUTUMN_OTEL__HEALTH_CHECK", "0");
    let config = OtelConfig::resolve_with_env("otel", &env).unwrap();
    assert!(config.enabled);
    assert_eq!(config.endpoint, "http://collector:4317");
    assert_eq!(config.service_name, "shop");
    assert_eq!(config.service_version, "2.0.0");
    assert_eq!(config.service_namespace.as_deref(), Some("team-a"));
    assert_eq!(config.environment, "prod");
    assert!(
        (config.sample_ratio - 0.25).abs() < 1e-12,
        "got {}",
        config.sample_ratio
    );
    assert_eq!(config.protocol, OtelProtocol::Http);
    assert!(config.traces);
    assert!(!config.metrics);
    assert!(config.logs);
    assert!(!config.health_check);
}

#[test]
fn unknown_keys_are_errors() {
    let table: toml::Table = "bogus_key = true".parse().unwrap();
    toml::Value::Table(table)
        .try_into::<OtelConfig>()
        .expect_err("unknown keys must fail");
}

#[cfg(any(feature = "grpc", feature = "http"))]
#[test]
fn profile_file_overrides_the_base_file() {
    // A scratch dir with `autumn.toml` and `autumn-dev.toml`, wired through
    // `AUTUMN_MANIFEST_DIR`. Only this test uses it, so the process id is a
    // unique-enough name.
    let dir = std::env::temp_dir().join(format!(
        "autumn-plugin-otel-config-test-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("autumn.toml"),
        format!(
            "[otel]\nenabled = true\nservice_name = \"base\"\nsample_ratio = 0.1\nprotocol = \"{}\"\n",
            available_protocol().feature()
        ),
    )
    .unwrap();
    std::fs::write(
        dir.join("autumn-dev.toml"),
        "[otel]\nservice_name = \"from-profile-file\"\n",
    )
    .unwrap();

    let env = MapEnv::default()
        .with("AUTUMN_MANIFEST_DIR", dir.to_str().unwrap())
        .with("AUTUMN_PROFILE", "dev");
    let config = OtelConfig::resolve_with_env("otel", &env).unwrap();

    assert!(config.enabled, "kept from the base file");
    assert_eq!(
        config.service_name, "from-profile-file",
        "the profile file wins"
    );
    assert!(
        (config.sample_ratio - 0.1).abs() < 1e-12,
        "kept from the base file, got {}",
        config.sample_ratio
    );

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn empty_service_name_is_not_valid() {
    let mut config = enabled_config();
    config.service_name = "   ".into();
    assert!(config.validate().is_err());
}

#[cfg(any(feature = "grpc", feature = "http"))]
#[test]
fn endpoint_must_be_an_absolute_uri() {
    for endpoint in ["", "localhost:4317", "://missing-scheme", "not a uri"] {
        let mut config = enabled_config();
        config.endpoint = endpoint.into();
        assert!(config.validate().is_err(), "{endpoint:?} must not validate");
    }
    // `https` depends on the transport and the `tls` feature: see the tests below.
    for endpoint in ["http://localhost:4317", "grpc://collector:4317"] {
        let mut config = enabled_config();
        config.endpoint = endpoint.into();
        assert!(config.validate().is_ok(), "{endpoint:?} must validate");
    }
}

#[cfg(any(feature = "grpc", feature = "http"))]
#[test]
fn ratio_below_zero_or_above_one_is_not_valid() {
    for ratio in [-0.5, -0.1, 1.1, 2.0, f64::NAN, f64::INFINITY] {
        let mut config = enabled_config();
        config.sample_ratio = ratio;
        assert!(config.validate().is_err(), "{ratio} must not validate");
    }
    for ratio in [0.0, 0.25, 1.0] {
        let mut config = enabled_config();
        config.sample_ratio = ratio;
        assert!(config.validate().is_ok(), "{ratio} must validate");
    }
}

#[test]
fn all_signals_off_is_not_valid() {
    let mut config = enabled_config();
    config.traces = false;
    config.metrics = false;
    config.logs = false;
    assert!(config.validate().is_err());
}

#[test]
fn disabled_config_skips_value_checks() {
    let config = OtelConfig {
        endpoint: "not a uri".into(),
        service_name: String::new(),
        sample_ratio: 42.0,
        ..OtelConfig::default()
    };
    assert!(config.validate().is_ok());
}

#[test]
fn enabled_signals_lists_each_on_signal() {
    let config = enabled_config();
    assert_eq!(config.enabled_signals(), vec!["traces", "metrics", "logs"]);
    let mut partial = enabled_config();
    partial.metrics = false;
    assert_eq!(partial.enabled_signals(), vec!["traces", "logs"]);
}

#[cfg(feature = "grpc")]
#[test]
fn https_grpc_endpoint_needs_the_tls_feature() {
    let mut config = enabled_config();
    config.protocol = OtelProtocol::Grpc;
    config.endpoint = "https://collector:4317".into();
    if cfg!(feature = "tls") {
        assert!(config.validate().is_ok());
    } else {
        assert!(config.validate().is_err());
    }
}

#[cfg(feature = "http")]
#[test]
fn https_http_endpoint_needs_no_tls_feature() {
    let mut config = enabled_config();
    config.protocol = OtelProtocol::Http;
    config.endpoint = "https://collector:4318".into();
    assert!(config.validate().is_ok());
}

#[test]
fn unavailable_protocol_names_the_feature() {
    for protocol in [OtelProtocol::Grpc, OtelProtocol::Http] {
        let mut config = enabled_config();
        config.protocol = protocol;
        let result = config.validate();
        if protocol.is_available() {
            assert!(result.is_ok(), "{protocol:?}: {result:?}");
        } else {
            let error = result.unwrap_err().to_string();
            assert!(error.starts_with("otel.protocol "), "{error}");
            assert!(
                error.contains(&format!("--features {}", protocol.feature())),
                "{error}"
            );
        }
    }
}

#[test]
fn is_https_endpoint_detects_the_scheme() {
    assert!(is_https_endpoint("https://collector:4317"));
    assert!(!is_https_endpoint("http://localhost:4317"));
    assert!(!is_https_endpoint("localhost:4317"));
    assert!(!is_https_endpoint("not a uri"));
}

#[cfg(any(feature = "grpc", feature = "http"))]
proptest! {
    #[test]
    fn sample_ratio_validation_matches_the_range(ratio in proptest::num::f64::ANY) {
        let mut config = enabled_config();
        config.sample_ratio = ratio;
        let expected = (0.0..=1.0).contains(&ratio);
        prop_assert_eq!(config.validate().is_ok(), expected);
    }

    #[test]
    fn service_name_validation_matches_non_empty(name in "\\PC*") {
        let mut config = enabled_config();
        config.service_name = name.clone();
        let expected = !name.trim().is_empty();
        prop_assert_eq!(config.validate().is_ok(), expected);
    }
}
