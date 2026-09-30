//! Autumn plugin for OpenTelemetry.
//!
//! Add [`OtelPlugin`] to the app. Spans, instruments and tracing events export
//! to the configured OTLP collector over gRPC or HTTP.
//!
//! ```rust,no_run
//! use autumn_plugin_otel::OtelPlugin;
//!
//! # async fn run() {
//! autumn_web::app()
//!     .plugin(OtelPlugin::new().configure(|c| {
//!         c.enabled = true;
//!         c.service_name = "my-app".into();
//!         c.sample_ratio = 0.1;
//!     }))
//!     .run()
//!     .await;
//! # }
//! ```
//!
//! The plugin reads `[otel]` in `autumn.toml`. See [`config`] for the keys.
//!
//! # What the plugin gives
//!
//! - A tier-1 [`TelemetryProvider`](autumn_web::telemetry::TelemetryProvider) that owns
//!   the global tracing subscriber and wires OTLP exporters for three signals.
//! - Traces: a `tracing-opentelemetry` layer with a parent-based sampler.
//! - Metrics: an SDK meter provider installed as the global provider, plus [`metrics`] helpers.
//! - Logs: a bridge layer that forwards tracing events as OTLP log records.
//! - [`OtelTelemetry`]: the handle in the app state, with a handler extractor.
//! - A health indicator that reports the exporter configuration (never the
//!   collector endpoint).
//! - A shutdown hook that flushes every provider.
//!
//! # Limits
//!
//! - The plugin replaces Autumn's default telemetry initializer. The framework's
//!   log-capture buffer and the `/actuator/loggers` reload handle belong to the
//!   default provider and are not installed.
//! - `TelemetryInitError` is `#[non_exhaustive]`, so the provider can not build one.
//!   An exporter that fails to build falls back to a logging-only subscriber with
//!   a warning on stderr. A bad `[otel]` configuration stops the boot in the startup hook.
//! - OTLP has no cheap health probe. The health check reports the exporter
//!   configuration. It never contacts the collector and never exposes the
//!   collector endpoint.
//! - `https` gRPC endpoints need the `tls` crate feature. HTTP uses rustls through reqwest.
//! - The tracing-to-OTLP log bridge forwards each event's message and target;
//!   it does not forward every structured field.
//!
//! # Shutdown
//!
//! Autumn marks the shutdown before it drains the requests. The shutdown hook flushes
//! the trace, metric and log providers. The exporters stay alive for the drain.

mod client;
pub mod config;
mod error;
mod health;
pub mod metrics;
mod plugin;

pub use client::OtelTelemetry;
pub use config::{ConfigError, DEFAULT_SECTION, OtelConfig, OtelProtocol};
pub use error::{ErrorKind, OtelError, OtelResultExt};
pub use metrics::{OtelMetrics, meter, meter_named};
pub use plugin::{OtelPlugin, PLUGIN_NAME};
