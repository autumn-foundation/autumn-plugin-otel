//! The `[otel]` section of `autumn.toml`.
//!
//! # Contract
//!
//! Each layer overrides the layers before it:
//!
//! 1. The defaults.
//! 2. `[otel]` in `autumn.toml`.
//! 3. `[profile.<name>.otel]` in `autumn.toml`.
//! 4. `[otel]` in `autumn-<name>.toml`.
//! 5. `AUTUMN_OTEL__<KEY>` variables. `AUTUMN_OTEL__SAMPLE_RATIO` sets `sample_ratio`.
//!
//! The result must pass [`OtelConfig::validate`]. Unknown keys are errors.
//!
//! ```toml
//! [otel]
//! enabled = true
//! endpoint = "http://localhost:4317"
//! service_name = "my-app"
//! service_version = "1.2.3"
//! sample_ratio = 0.1
//! protocol = "grpc"
//! traces = true
//! metrics = true
//! logs = true
//! ```

use std::path::{Path, PathBuf};

use autumn_web::config::Env;
use serde::{Deserialize, Serialize};

/// The default section name.
pub const DEFAULT_SECTION: &str = "otel";

/// The default collector endpoint.
pub const DEFAULT_ENDPOINT: &str = "http://localhost:4317";

/// A configuration that is not valid.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct ConfigError(pub(crate) String);

/// The OTLP transport.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum OtelProtocol {
    /// OTLP over gRPC. It needs the `grpc` crate feature.
    #[default]
    Grpc,
    /// OTLP over HTTP protobuf. It needs the `http` crate feature.
    Http,
}

impl OtelProtocol {
    /// Returns `true` if the crate features build this transport.
    #[must_use]
    pub const fn is_available(self) -> bool {
        match self {
            Self::Grpc => cfg!(feature = "grpc"),
            Self::Http => cfg!(feature = "http"),
        }
    }

    /// The feature name that enables this transport.
    #[must_use]
    pub const fn feature(self) -> &'static str {
        match self {
            Self::Grpc => "grpc",
            Self::Http => "http",
        }
    }
}

/// The plugin settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
#[non_exhaustive]
#[allow(clippy::struct_excessive_bools, reason = "each bool is one TOML key")]
pub struct OtelConfig {
    /// If `true`, the plugin installs the OpenTelemetry pipeline.
    /// If `false`, the plugin installs nothing and Autumn keeps its default telemetry.
    pub enabled: bool,
    /// The collector endpoint, for example `http://localhost:4317`.
    pub endpoint: String,
    /// The `service.name` resource attribute. It must not be empty.
    pub service_name: String,
    /// The `service.version` resource attribute.
    pub service_version: String,
    /// The `service.namespace` resource attribute. `None` omits it.
    pub service_namespace: Option<String>,
    /// The `deployment.environment` resource attribute.
    pub environment: String,
    /// The fraction of traces to sample, from `0.0` to `1.0`.
    /// The sampler is parent-based: a sampled parent keeps the trace sampled.
    pub sample_ratio: f64,
    /// The OTLP transport.
    pub protocol: OtelProtocol,
    /// If `true`, spans export to the collector.
    pub traces: bool,
    /// If `true`, instruments export to the collector.
    pub metrics: bool,
    /// If `true`, tracing events export to the collector as logs.
    pub logs: bool,
    /// If `true`, the plugin adds a health indicator that reports the exporter state.
    pub health_check: bool,
}

impl Default for OtelConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            endpoint: DEFAULT_ENDPOINT.to_owned(),
            service_name: "autumn-app".to_owned(),
            service_version: "unknown".to_owned(),
            service_namespace: None,
            environment: "development".to_owned(),
            sample_ratio: 1.0,
            protocol: OtelProtocol::Grpc,
            traces: true,
            metrics: true,
            logs: true,
            health_check: true,
        }
    }
}

/// The type of a configuration leaf, for environment values.
#[derive(Clone, Copy)]
enum Kind {
    Text,
    Float,
    Bool,
}

/// Each leaf key and its type.
const LEAVES: &[(&str, Kind)] = &[
    ("enabled", Kind::Bool),
    ("endpoint", Kind::Text),
    ("service_name", Kind::Text),
    ("service_version", Kind::Text),
    ("service_namespace", Kind::Text),
    ("environment", Kind::Text),
    ("sample_ratio", Kind::Float),
    ("protocol", Kind::Text),
    ("traces", Kind::Bool),
    ("metrics", Kind::Bool),
    ("logs", Kind::Bool),
    ("health_check", Kind::Bool),
];

impl OtelConfig {
    /// Reads `[section]` from the app files and the environment.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError`] if a file is not valid TOML or a value is not valid.
    pub fn resolve(section: &str) -> Result<Self, ConfigError> {
        autumn_web::dotenv::os_env_with_dotenv().map_or_else(
            |_| Self::resolve_with_env(section, &autumn_web::config::OsEnv),
            |env| Self::resolve_with_env(section, &env),
        )
    }

    /// Reads `[section]` with `env` as the environment.
    ///
    /// # Errors
    ///
    /// See [`resolve`](Self::resolve).
    pub fn resolve_with_env(section: &str, env: &dyn Env) -> Result<Self, ConfigError> {
        let (selected, profile) = active_profile(env);
        let mut merged = toml::Table::new();
        if let Some(base) = read_toml(&config_file("autumn.toml", env))? {
            merge_section(&mut merged, base.get(section), section)?;
            for name in inline_profile_names(&profile) {
                let inline = base
                    .get("profile")
                    .and_then(|p| p.get(name))
                    .and_then(|p| p.get(section));
                merge_section(&mut merged, inline, section)?;
            }
        }
        for name in autumn_web::config::profile_override_file_lookup_names(&profile, &selected) {
            if let Some(file) = read_toml(&config_file(&format!("autumn-{name}.toml"), env))? {
                merge_section(&mut merged, file.get(section), section)?;
                break;
            }
        }
        apply_env(&mut merged, section, env)?;
        let config: Self = toml::Value::Table(merged)
            .try_into()
            .map_err(|err| ConfigError(format!("[{section}]: {err}")))?;
        config.validate_section(section)?;
        Ok(config)
    }

    /// Checks each value.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError`] that names the first key that is not valid.
    pub fn validate(&self) -> Result<(), ConfigError> {
        self.validate_section(DEFAULT_SECTION)
    }

    /// Checks each value. The errors name keys in `section`.
    pub(crate) fn validate_section(&self, section: &str) -> Result<(), ConfigError> {
        let fail = |key: &str, rule: &str| Err(ConfigError(format!("{section}.{key} {rule}")));
        if !self.enabled {
            return Ok(());
        }
        if self.service_name.trim().is_empty() {
            return fail("service_name", "must not be empty");
        }
        if let Err(reason) = check_endpoint(&self.endpoint) {
            return fail("endpoint", &reason);
        }
        if !(0.0..=1.0).contains(&self.sample_ratio) {
            return fail("sample_ratio", "must be from 0.0 to 1.0");
        }
        if !self.protocol.is_available() {
            return fail(
                "protocol",
                &format!(
                    "needs the `{}` crate feature: rebuild with `--features {}`",
                    self.protocol.feature(),
                    self.protocol.feature()
                ),
            );
        }
        if self.protocol == OtelProtocol::Grpc
            && !cfg!(feature = "tls")
            && is_https_endpoint(&self.endpoint)
        {
            return fail(
                "endpoint",
                "uses `https` with the gRPC transport: enable the `tls` crate feature",
            );
        }
        if !self.traces && !self.metrics && !self.logs {
            return fail(
                "traces",
                "enable at least one of `traces`, `metrics`, `logs`",
            );
        }
        Ok(())
    }

    /// The names of the enabled signals, for diagnostics.
    #[must_use]
    pub fn enabled_signals(&self) -> Vec<&'static str> {
        let mut signals = Vec::with_capacity(3);
        if self.traces {
            signals.push("traces");
        }
        if self.metrics {
            signals.push("metrics");
        }
        if self.logs {
            signals.push("logs");
        }
        signals
    }
}

/// Checks that the endpoint is an absolute URI with a scheme and an authority.
fn check_endpoint(endpoint: &str) -> Result<(), String> {
    let uri: http::Uri = endpoint
        .parse()
        .map_err(|err: http::uri::InvalidUri| err.to_string())?;
    // `http::Uri` accepts an empty scheme (for example `"://missing-scheme"`),
    // so the scheme must be present and non-empty.
    if uri.scheme_str().is_none_or(str::is_empty) {
        return Err("must be an absolute URI with a scheme".to_owned());
    }
    if uri.authority().is_none() {
        return Err("must have an authority, for example a host and a port".to_owned());
    }
    Ok(())
}

/// Returns `true` when the endpoint uses the `https` scheme.
pub(crate) fn is_https_endpoint(endpoint: &str) -> bool {
    endpoint
        .parse::<http::Uri>()
        .ok()
        .and_then(|uri| uri.scheme_str().map(|scheme| scheme == "https"))
        .unwrap_or(false)
}

/// Gives the selected profile text and the normalized profile, as Autumn does.
fn active_profile(env: &dyn Env) -> (String, String) {
    let selected = ["AUTUMN_ENV", "AUTUMN_PROFILE"]
        .iter()
        .filter_map(|key| env.var(key).ok())
        .map(|value| value.trim().to_owned())
        .find(|value| !value.is_empty())
        .unwrap_or_else(|| {
            let release = env.var("AUTUMN_IS_DEBUG").is_ok_and(|v| v == "0");
            if release { "prod" } else { "dev" }.to_owned()
        });
    let profile =
        autumn_web::config::normalize_profile_name(&selected).unwrap_or_else(|| "dev".to_owned());
    (selected, profile)
}

/// The inline profile names to read, in order. The canonical name is last.
fn inline_profile_names(profile: &str) -> Vec<&str> {
    match profile {
        "prod" => vec!["production", "prod"],
        "dev" => vec!["development", "dev"],
        other => vec![other],
    }
}

/// Finds a config file in `AUTUMN_MANIFEST_DIR`, or else in the working directory.
fn config_file(name: &str, env: &dyn Env) -> PathBuf {
    env.var("AUTUMN_MANIFEST_DIR")
        .ok()
        .map(|dir| Path::new(&dir).join(name))
        .filter(|path| path.exists())
        .unwrap_or_else(|| PathBuf::from(name))
}

fn read_toml(path: &Path) -> Result<Option<toml::Table>, ConfigError> {
    match std::fs::read_to_string(path) {
        Ok(text) => text
            .parse::<toml::Table>()
            .map(Some)
            .map_err(|err| ConfigError(format!("{}: {err}", path.display()))),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(ConfigError(format!("{}: {err}", path.display()))),
    }
}

fn merge_section(
    into: &mut toml::Table,
    layer: Option<&toml::Value>,
    section: &str,
) -> Result<(), ConfigError> {
    match layer {
        None => Ok(()),
        Some(toml::Value::Table(table)) => {
            deep_merge(into, table);
            Ok(())
        }
        Some(_) => Err(ConfigError(format!("[{section}] must be a table"))),
    }
}

fn deep_merge(into: &mut toml::Table, layer: &toml::Table) {
    for (key, value) in layer {
        match (into.get_mut(key), value) {
            (Some(toml::Value::Table(old)), toml::Value::Table(new)) => deep_merge(old, new),
            _ => {
                into.insert(key.clone(), value.clone());
            }
        }
    }
}

fn apply_env(into: &mut toml::Table, section: &str, env: &dyn Env) -> Result<(), ConfigError> {
    let name: String = section
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_uppercase()
            } else {
                '_'
            }
        })
        .collect();
    for (key, kind) in LEAVES {
        let name = format!("AUTUMN_{name}__{}", key.to_ascii_uppercase());
        let Ok(raw) = env.var(&name) else {
            continue;
        };
        let bad = || ConfigError(format!("{name}: can not read {raw:?}"));
        let value = match kind {
            Kind::Text => toml::Value::String(raw.clone()),
            Kind::Float => {
                let value: f64 = raw.trim().parse().map_err(|_| bad())?;
                toml::Value::Float(value)
            }
            Kind::Bool => match raw.trim() {
                "true" | "1" => toml::Value::Boolean(true),
                "false" | "0" => toml::Value::Boolean(false),
                _ => return Err(bad()),
            },
        };
        into.insert((*key).to_owned(), value);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
