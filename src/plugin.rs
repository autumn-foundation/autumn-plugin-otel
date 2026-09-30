//! [`OtelPlugin`]: installs an OpenTelemetry pipeline in an Autumn app.
//!
//! # Contract
//!
//! - `build` reads the configuration. A bad configuration stops the boot in the startup hook.
//! - When `enabled = true`, `build` installs a tier-1 [`TelemetryProvider`](autumn_web::telemetry::TelemetryProvider)
//!   that owns the global tracing subscriber. The framework calls it during boot, before the startup hooks.
//! - The provider wires OTLP exporters for traces, metrics and logs to the configured endpoint.
//!   Traces flow through a `tracing-opentelemetry` layer, metrics through the global meter provider,
//!   and tracing events through a small bridge layer as OTLP logs.
//! - The sampler is parent-based over a trace-id ratio sampler with the configured `sample_ratio`.
//! - The resource carries `service.name`, `service.version`, `deployment.environment`
//!   and `service.namespace` when set.
//! - `TelemetryInitError` is `#[non_exhaustive]`, so the provider can not build one.
//!   An exporter that fails to build falls back to a logging-only subscriber with a loud
//!   warning on stderr, the same shape as the framework's non-strict path.
//! - The shutdown hook flushes every provider. The providers stay alive for the drain.
//! - The health check reports the exporter configuration. OTLP has no cheap probe,
//!   so the check never contacts the collector.

use std::borrow::Cow;
use std::sync::{
    Arc, OnceLock,
    atomic::{AtomicU8, Ordering},
};
use std::time::SystemTime;

use autumn_web::app::AppBuilder;
use autumn_web::config::{LogConfig, LogFormat, TelemetryConfig};
use autumn_web::plugin::Plugin;
use autumn_web::telemetry::{TelemetryGuard, TelemetryInitError, TelemetryProvider};
use autumn_web::{AppState, AutumnError};
use opentelemetry::{KeyValue, trace::TracerProvider as _};
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::logs::SdkLoggerProvider;
use opentelemetry_sdk::metrics::SdkMeterProvider;
use opentelemetry_sdk::propagation::TraceContextPropagator;
use opentelemetry_sdk::trace::{Sampler, SdkTracerProvider};
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

// `WithTonicConfig` needs no import: the `with_grpc_tls` bound carries the trait,
// which puts its methods in scope by itself.
#[cfg(any(feature = "grpc", feature = "http"))]
use opentelemetry_otlp::WithExportConfig as _;

use crate::client::OtelTelemetry;
use crate::config::{ConfigError, DEFAULT_SECTION, OtelConfig, OtelProtocol};
use crate::health::OtelHealthCheck;

/// The plugin name in Autumn diagnostics.
pub const PLUGIN_NAME: &str = "autumn-plugin-otel";

/// The instrumentation scope of every signal the plugin emits.
const SCOPE: &str = "autumn-plugin-otel";

/// The lifecycle of the shared state.
const NOT_STARTED: u8 = 0;
/// The lifecycle of the shared state.
const RUNNING: u8 = 1;
/// The lifecycle of the shared state.
const SHUT_DOWN: u8 = 2;
/// The lifecycle of the shared state: the subscriber runs, but the exporters
/// failed to build, so the plugin logs only.
const DEGRADED: u8 = 3;

/// The SDK providers that the plugin installed.
#[derive(Default)]
pub(crate) struct OtelHandles {
    pub(crate) tracer: Option<SdkTracerProvider>,
    pub(crate) meter: Option<SdkMeterProvider>,
    pub(crate) logger: Option<SdkLoggerProvider>,
}

impl OtelHandles {
    /// Flushes every provider. It ignores flush errors: shutdown is best-effort.
    pub(crate) fn shutdown(&self) {
        if let Some(provider) = &self.tracer {
            let _ = provider.shutdown();
        }
        if let Some(provider) = &self.meter {
            let _ = provider.shutdown();
        }
        if let Some(provider) = &self.logger {
            let _ = provider.shutdown();
        }
    }
}

/// State that the plugin hooks share.
pub(crate) struct Shared {
    pub(crate) resolved: Result<OtelConfig, ConfigError>,
    handles: OnceLock<OtelHandles>,
    lifecycle: AtomicU8,
}

impl Shared {
    pub(crate) const fn new(resolved: Result<OtelConfig, ConfigError>) -> Self {
        Self {
            resolved,
            handles: OnceLock::new(),
            lifecycle: AtomicU8::new(NOT_STARTED),
        }
    }

    /// The installed providers, if the telemetry provider ran.
    pub(crate) fn handles(&self) -> Option<&OtelHandles> {
        self.handles.get()
    }

    /// The resolved configuration, or `None` when it failed to load.
    pub(crate) fn config(&self) -> Option<&OtelConfig> {
        self.resolved.as_ref().ok()
    }

    /// Returns `true` after the telemetry provider installed the subscriber.
    pub(crate) fn is_running(&self) -> bool {
        matches!(self.lifecycle.load(Ordering::Acquire), RUNNING | DEGRADED)
    }

    /// Returns `true` when the exporters failed and the plugin logs only.
    pub(crate) fn is_degraded(&self) -> bool {
        self.lifecycle.load(Ordering::Acquire) == DEGRADED
    }

    /// Returns `true` after the shutdown hook ran.
    pub(crate) fn is_shut_down(&self) -> bool {
        self.lifecycle.load(Ordering::Acquire) == SHUT_DOWN
    }

    pub(crate) fn mark_running(&self) {
        self.lifecycle.store(RUNNING, Ordering::Release);
    }

    pub(crate) fn mark_degraded(&self) {
        self.lifecycle.store(DEGRADED, Ordering::Release);
    }

    /// Flushes the providers once. It runs at most one time.
    pub(crate) fn shutdown(&self) {
        let previous = self.lifecycle.load(Ordering::Acquire);
        let claimed = matches!(previous, RUNNING | DEGRADED)
            && self
                .lifecycle
                .compare_exchange(previous, SHUT_DOWN, Ordering::AcqRel, Ordering::Acquire)
                .is_ok();
        if claimed && let Some(handles) = self.handles.get() {
            handles.shutdown();
        }
    }
}

enum ConfigSource {
    Section(String),
    Explicit(Box<OtelConfig>),
}

type Change = Box<dyn FnOnce(&mut OtelConfig) + Send + 'static>;

/// Installs an OpenTelemetry pipeline in an Autumn app.
///
/// ```rust,no_run
/// use autumn_plugin_otel::OtelPlugin;
///
/// # async fn run() {
/// autumn_web::app()
///     .plugin(OtelPlugin::new().configure(|c| {
///         c.enabled = true;
///         c.service_name = "my-app".into();
///     }))
///     .run()
///     .await;
/// # }
/// ```
#[must_use]
pub struct OtelPlugin {
    source: ConfigSource,
    changes: Vec<Change>,
}

impl Default for OtelPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl OtelPlugin {
    /// Makes a plugin that reads `[otel]`.
    pub fn new() -> Self {
        Self {
            source: ConfigSource::Section(DEFAULT_SECTION.to_owned()),
            changes: Vec::new(),
        }
    }

    /// Reads `[section]` instead of `[otel]`.
    pub fn config_section(mut self, section: impl Into<String>) -> Self {
        self.source = ConfigSource::Section(section.into());
        self
    }

    /// Uses `config` and reads no files or variables.
    pub fn config(mut self, config: OtelConfig) -> Self {
        self.source = ConfigSource::Explicit(Box::new(config));
        self
    }

    /// Changes the configuration after the plugin reads it.
    pub fn configure(mut self, change: impl FnOnce(&mut OtelConfig) + Send + 'static) -> Self {
        self.changes.push(Box::new(change));
        self
    }

    fn resolve(source: &ConfigSource, changes: Vec<Change>) -> Result<OtelConfig, ConfigError> {
        let mut config = match source {
            ConfigSource::Section(section) => OtelConfig::resolve(section)?,
            ConfigSource::Explicit(config) => (**config).clone(),
        };
        for change in changes {
            change(&mut config);
        }
        config.validate()?;
        Ok(config)
    }
}

impl Plugin for OtelPlugin {
    fn name(&self) -> Cow<'static, str> {
        Cow::Borrowed(PLUGIN_NAME)
    }

    fn build(self, app: AppBuilder) -> AppBuilder {
        let Self { source, changes } = self;
        let mut app = app;
        if let ConfigSource::Section(section) = &source {
            app = app.config_section(section.clone());
        }
        let resolved = Self::resolve(&source, changes);
        let enabled = resolved.as_ref().is_ok_and(|config| config.enabled);
        let shared = Arc::new(Shared::new(resolved));
        if enabled {
            app = app.with_telemetry_provider(OtelTelemetryProvider::new(Arc::clone(&shared)));
        }
        if shared
            .resolved
            .as_ref()
            .is_ok_and(|config| config.enabled && config.health_check)
        {
            app = app.health_indicator("otel", Arc::new(OtelHealthCheck::new(Arc::clone(&shared))));
        }
        let on_start = Arc::clone(&shared);
        let on_stop = Arc::clone(&shared);
        app.on_startup(move |state: AppState| {
            let shared = Arc::clone(&on_start);
            async move {
                if let Err(err) = shared.resolved.as_ref() {
                    return Err(boot_error(&err.to_string()));
                }
                state.insert_extension(OtelTelemetry::new(shared));
                tracing::info!("the OpenTelemetry plugin is ready");
                Ok(())
            }
        })
        .on_shutdown(move || {
            let shared = Arc::clone(&on_stop);
            async move {
                shared.shutdown();
            }
        })
    }
}

impl std::fmt::Debug for OtelPlugin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let source = match &self.source {
            ConfigSource::Section(section) => section.as_str(),
            ConfigSource::Explicit(_) => "(explicit)",
        };
        f.debug_struct("OtelPlugin")
            .field("config", &source)
            .field("changes", &self.changes.len())
            .finish()
    }
}

/// The tier-1 telemetry provider that owns the global tracing subscriber.
struct OtelTelemetryProvider {
    shared: Arc<Shared>,
}

impl OtelTelemetryProvider {
    const fn new(shared: Arc<Shared>) -> Self {
        Self { shared }
    }
}

impl TelemetryProvider for OtelTelemetryProvider {
    fn init(
        &self,
        log: &LogConfig,
        _telemetry: &TelemetryConfig,
        profile: Option<&str>,
    ) -> Result<TelemetryGuard, TelemetryInitError> {
        // `TelemetryInitError` is non_exhaustive, so the provider can not build one.
        // A bad config stops the boot in the startup hook. An exporter that fails
        // to build falls back to a logging-only subscriber with a loud warning,
        // the same shape as the framework's non-strict path.
        let config = match &self.shared.resolved {
            Ok(config) => config.clone(),
            Err(err) => {
                eprintln!("{PLUGIN_NAME}: bad [otel] configuration: {err}");
                install_logging_only(log, profile);
                return Ok(TelemetryGuard::disabled());
            }
        };
        match install_pipeline(&config, log, profile) {
            Ok(handles) => {
                let _ = self.shared.handles.set(handles);
                self.shared.mark_running();
            }
            Err(warning) => {
                eprintln!("{PLUGIN_NAME}: {warning}");
                install_logging_only(log, profile);
                self.shared.mark_degraded();
                self.shared.mark_running();
            }
        }
        Ok(TelemetryGuard::disabled())
    }
}

/// Installs the filter and the format layer only. It never fails loudly:
/// a bad filter directive falls back to `info` with a warning.
fn install_logging_only(log: &LogConfig, profile: Option<&str>) {
    let filter = build_filter(log);
    let result = match resolve_format(log.format, profile) {
        ResolvedFormat::Json => tracing_subscriber::registry()
            .with(filter)
            .with(fmt::layer().json())
            .try_init(),
        ResolvedFormat::Pretty => tracing_subscriber::registry()
            .with(filter)
            .with(fmt::layer().pretty())
            .try_init(),
    };
    if let Err(error) = result {
        eprintln!("{PLUGIN_NAME}: the logging-only subscriber failed to install: {error}");
    }
}

/// Builds the exporters and installs the full subscriber. The `Err` is a warning for stderr.
#[cfg(any(feature = "grpc", feature = "http"))]
fn install_pipeline(
    config: &OtelConfig,
    log: &LogConfig,
    profile: Option<&str>,
) -> Result<OtelHandles, String> {
    let resource = build_resource(config);
    let mut handles = OtelHandles::default();

    let trace_layer = if config.traces {
        let exporter = build_span_exporter(config)?;
        let provider = SdkTracerProvider::builder()
            .with_resource(resource.clone())
            .with_sampler(build_sampler(config.sample_ratio))
            .with_batch_exporter(exporter)
            .build();
        opentelemetry::global::set_text_map_propagator(TraceContextPropagator::new());
        let tracer = provider.tracer(SCOPE);
        handles.tracer = Some(provider);
        // The layer goes directly on the registry: `OpenTelemetryLayer<S>` only
        // implements `Layer<S>` for one `S`, and the span context must exist
        // before the format layer renders it.
        let layer: tracing_opentelemetry::OpenTelemetryLayer<tracing_subscriber::Registry, _> =
            tracing_opentelemetry::layer().with_tracer(tracer);
        Some(layer)
    } else {
        None
    };

    if config.metrics {
        let exporter = build_metric_exporter(config)?;
        let provider = SdkMeterProvider::builder()
            .with_resource(resource.clone())
            .with_periodic_exporter(exporter)
            .build();
        opentelemetry::global::set_meter_provider(provider.clone());
        handles.meter = Some(provider);
    }

    let log_bridge = if config.logs {
        let exporter = build_log_exporter(config)?;
        let provider = SdkLoggerProvider::builder()
            .with_resource(resource)
            .with_batch_exporter(exporter)
            .build();
        let logger = {
            use opentelemetry::logs::LoggerProvider as _;
            provider.logger(SCOPE)
        };
        handles.logger = Some(provider);
        Some(OtelLogBridge::new(logger))
    } else {
        None
    };

    let filter = build_filter(log);
    let result = match resolve_format(log.format, profile) {
        ResolvedFormat::Json => tracing_subscriber::registry()
            .with(trace_layer)
            .with(filter)
            .with(fmt::layer().json())
            .with(log_bridge)
            .try_init(),
        ResolvedFormat::Pretty => tracing_subscriber::registry()
            .with(trace_layer)
            .with(filter)
            .with(fmt::layer().pretty())
            .with(log_bridge)
            .try_init(),
    };
    result.map_err(|error| format!("the tracing subscriber failed to install: {error}"))?;
    Ok(handles)
}

/// Without an OTLP transport there is nothing to export to. The caller warns
/// and falls back to the logging-only subscriber, the same degraded path as a
/// failed exporter build.
#[cfg(not(any(feature = "grpc", feature = "http")))]
fn install_pipeline(
    _config: &OtelConfig,
    _log: &LogConfig,
    _profile: Option<&str>,
) -> Result<OtelHandles, String> {
    Err("no OTLP transport is enabled: rebuild with the `grpc` or `http` crate feature".to_owned())
}

/// The parent-based sampler for the configured ratio.
pub(crate) fn build_sampler(ratio: f64) -> Sampler {
    Sampler::ParentBased(Box::new(Sampler::TraceIdRatioBased(ratio)))
}

/// The resource attributes for the service.
pub(crate) fn build_resource(config: &OtelConfig) -> Resource {
    let mut attributes = vec![
        KeyValue::new("service.version", config.service_version.clone()),
        KeyValue::new("deployment.environment", config.environment.clone()),
    ];
    if let Some(namespace) = config
        .service_namespace
        .as_deref()
        .filter(|name| !name.is_empty())
    {
        attributes.push(KeyValue::new("service.namespace", namespace.to_owned()));
    }
    Resource::builder()
        .with_service_name(config.service_name.clone())
        .with_attributes(attributes)
        .build()
}

/// The concrete log format for the process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResolvedFormat {
    Pretty,
    Json,
}

/// Resolves the log format, as the framework does: `Auto` is JSON in prod, pretty elsewhere.
fn resolve_format(format: LogFormat, profile: Option<&str>) -> ResolvedFormat {
    match format {
        LogFormat::Json => ResolvedFormat::Json,
        LogFormat::Auto => {
            let production = profile.is_some_and(|name| {
                name.eq_ignore_ascii_case("prod") || name.eq_ignore_ascii_case("production")
            });
            if production {
                ResolvedFormat::Json
            } else {
                ResolvedFormat::Pretty
            }
        }
        // `Pretty`, and any future variant: human-readable output is the safe default.
        _ => ResolvedFormat::Pretty,
    }
}

fn build_filter(log: &LogConfig) -> EnvFilter {
    EnvFilter::try_new(&log.level).unwrap_or_else(|error| {
        eprintln!(
            "{PLUGIN_NAME}: invalid log filter {:?}: {error}; falling back to \"info\"",
            log.level
        );
        EnvFilter::new("info")
    })
}

#[cfg(any(feature = "grpc", feature = "http"))]
fn build_span_exporter(config: &OtelConfig) -> Result<opentelemetry_otlp::SpanExporter, String> {
    match config.protocol {
        OtelProtocol::Grpc => {
            #[cfg(feature = "grpc")]
            {
                let builder = opentelemetry_otlp::SpanExporter::builder()
                    .with_tonic()
                    .with_endpoint(config.endpoint.clone());
                with_grpc_tls(builder, &config.endpoint)
                    .build()
                    .map_err(|error| error.to_string())
            }
            #[cfg(not(feature = "grpc"))]
            {
                Err("the gRPC transport needs the `grpc` crate feature".to_owned())
            }
        }
        OtelProtocol::Http => {
            #[cfg(feature = "http")]
            {
                opentelemetry_otlp::SpanExporter::builder()
                    .with_http()
                    .with_endpoint(config.endpoint.clone())
                    .build()
                    .map_err(|error| error.to_string())
            }
            #[cfg(not(feature = "http"))]
            {
                Err("the HTTP transport needs the `http` crate feature".to_owned())
            }
        }
    }
}

#[cfg(any(feature = "grpc", feature = "http"))]
fn build_metric_exporter(
    config: &OtelConfig,
) -> Result<opentelemetry_otlp::MetricExporter, String> {
    match config.protocol {
        OtelProtocol::Grpc => {
            #[cfg(feature = "grpc")]
            {
                let builder = opentelemetry_otlp::MetricExporter::builder()
                    .with_tonic()
                    .with_endpoint(config.endpoint.clone());
                with_grpc_tls(builder, &config.endpoint)
                    .build()
                    .map_err(|error| error.to_string())
            }
            #[cfg(not(feature = "grpc"))]
            {
                Err("the gRPC transport needs the `grpc` crate feature".to_owned())
            }
        }
        OtelProtocol::Http => {
            #[cfg(feature = "http")]
            {
                opentelemetry_otlp::MetricExporter::builder()
                    .with_http()
                    .with_endpoint(config.endpoint.clone())
                    .build()
                    .map_err(|error| error.to_string())
            }
            #[cfg(not(feature = "http"))]
            {
                Err("the HTTP transport needs the `http` crate feature".to_owned())
            }
        }
    }
}

#[cfg(any(feature = "grpc", feature = "http"))]
fn build_log_exporter(config: &OtelConfig) -> Result<opentelemetry_otlp::LogExporter, String> {
    match config.protocol {
        OtelProtocol::Grpc => {
            #[cfg(feature = "grpc")]
            {
                let builder = opentelemetry_otlp::LogExporter::builder()
                    .with_tonic()
                    .with_endpoint(config.endpoint.clone());
                with_grpc_tls(builder, &config.endpoint)
                    .build()
                    .map_err(|error| error.to_string())
            }
            #[cfg(not(feature = "grpc"))]
            {
                Err("the gRPC transport needs the `grpc` crate feature".to_owned())
            }
        }
        OtelProtocol::Http => {
            #[cfg(feature = "http")]
            {
                opentelemetry_otlp::LogExporter::builder()
                    .with_http()
                    .with_endpoint(config.endpoint.clone())
                    .build()
                    .map_err(|error| error.to_string())
            }
            #[cfg(not(feature = "http"))]
            {
                Err("the HTTP transport needs the `http` crate feature".to_owned())
            }
        }
    }
}

/// Attaches TLS roots to a tonic exporter builder for `https` endpoints.
/// Without the `tls` feature the builder passes through unchanged; the
/// configuration validator rejects `https` gRPC endpoints in that case.
#[cfg(feature = "grpc")]
fn with_grpc_tls<B>(builder: B, endpoint: &str) -> B
where
    B: opentelemetry_otlp::WithTonicConfig,
{
    #[cfg(feature = "tls")]
    {
        if crate::config::is_https_endpoint(endpoint) {
            return builder.with_tls_config(
                opentelemetry_otlp::tonic_types::transport::ClientTlsConfig::new()
                    .with_enabled_roots(),
            );
        }
    }
    #[allow(clippy::let_and_return, reason = "the tls branch returns early")]
    builder
}

/// A tracing layer that forwards events to the OpenTelemetry log pipeline.
struct OtelLogBridge {
    logger: opentelemetry_sdk::logs::SdkLogger,
}

impl OtelLogBridge {
    const fn new(logger: opentelemetry_sdk::logs::SdkLogger) -> Self {
        Self { logger }
    }
}

impl<S> tracing_subscriber::Layer<S> for OtelLogBridge
where
    S: tracing::Subscriber,
{
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        use opentelemetry::logs::{AnyValue, LogRecord as _, Logger as _, Severity};
        use tracing::field::{Field, Visit};

        #[derive(Default)]
        struct Fields {
            message: Option<String>,
            target: Option<String>,
        }

        impl Visit for Fields {
            fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
                if field.name() == "message" {
                    self.message = Some(format!("{value:?}"));
                }
            }
            fn record_str(&mut self, field: &Field, value: &str) {
                if field.name() == "message" {
                    self.message = Some(value.to_owned());
                }
            }
        }

        let metadata = event.metadata();
        let mut fields = Fields {
            target: Some(metadata.target().to_owned()),
            ..Fields::default()
        };
        event.record(&mut fields);

        let (severity, text) = match *metadata.level() {
            tracing::Level::TRACE => (Severity::Trace, "TRACE"),
            tracing::Level::DEBUG => (Severity::Debug, "DEBUG"),
            tracing::Level::INFO => (Severity::Info, "INFO"),
            tracing::Level::WARN => (Severity::Warn, "WARN"),
            tracing::Level::ERROR => (Severity::Error, "ERROR"),
        };
        let mut record = self.logger.create_log_record();
        record.set_severity_number(severity);
        record.set_severity_text(text);
        record.set_timestamp(SystemTime::now());
        if let Some(message) = fields.message {
            record.set_body(AnyValue::String(message.into()));
        }
        if let Some(target) = fields.target {
            record.add_attribute("target", AnyValue::String(target.into()));
        }
        self.logger.emit(record);
    }
}

/// A boot error for the startup hook. The detail goes to the operator's own
/// startup logs only; the HTTP-facing `Display` of `OtelError` never shows it.
fn boot_error(detail: &str) -> AutumnError {
    AutumnError::internal_server_error_msg(format!("{PLUGIN_NAME}: {detail}"))
}

#[cfg(test)]
mod tests;
