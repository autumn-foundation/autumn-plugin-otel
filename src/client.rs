//! The telemetry handle that the app uses.
//!
//! # Contract
//!
//! - [`OtelTelemetry`] is cheap to clone. It shares the SDK providers.
//! - `from_state` returns `None` until the startup hook runs.
//! - `shutdown` flushes every provider. It is idempotent.
//! - The handle never exposes the collector endpoint in error text.

use std::sync::Arc;

use autumn_web::{AppState, AutumnError};
use opentelemetry::metrics::Meter;
use opentelemetry_sdk::logs::SdkLoggerProvider;
use opentelemetry_sdk::metrics::SdkMeterProvider;
use opentelemetry_sdk::trace::SdkTracerProvider;

use crate::config::OtelConfig;
use crate::error::OtelError;
use crate::plugin::Shared;

/// The running OpenTelemetry pipeline.
///
/// Get it from the app state in a handler, a job or a task:
///
/// ```rust,no_run
/// use autumn_plugin_otel::OtelTelemetry;
/// use autumn_web::AppState;
///
/// fn from_state(state: &AppState) -> Option<OtelTelemetry> {
///     OtelTelemetry::from_state(state)
/// }
/// ```
#[derive(Clone)]
pub struct OtelTelemetry {
    shared: Arc<Shared>,
}

impl OtelTelemetry {
    /// Makes a handle from shared plugin state.
    pub(crate) const fn new(shared: Arc<Shared>) -> Self {
        Self { shared }
    }

    /// Gets the handle from the app state, for example in a job or a task.
    #[must_use]
    pub fn from_state(state: &AppState) -> Option<Self> {
        state.extension::<Self>().map(|handle| (*handle).clone())
    }

    /// The resolved plugin configuration, or `None` when it failed to load.
    ///
    /// The startup hook only installs the handle after a successful load,
    /// so this is `Some` for every handle that came from the app state.
    #[must_use]
    pub fn config(&self) -> Option<&OtelConfig> {
        self.shared.config()
    }

    /// Returns `true` after the telemetry provider installed the pipeline.
    #[must_use]
    pub fn is_running(&self) -> bool {
        self.shared.is_running()
    }

    /// The tracer provider, when the `traces` signal is on.
    #[must_use]
    pub fn tracer_provider(&self) -> Option<SdkTracerProvider> {
        self.shared.handles().and_then(|h| h.tracer.clone())
    }

    /// The meter provider, when the `metrics` signal is on.
    #[must_use]
    pub fn meter_provider(&self) -> Option<SdkMeterProvider> {
        self.shared.handles().and_then(|h| h.meter.clone())
    }

    /// The logger provider, when the `logs` signal is on.
    #[must_use]
    pub fn logger_provider(&self) -> Option<SdkLoggerProvider> {
        self.shared.handles().and_then(|h| h.logger.clone())
    }

    /// Makes a meter with the plugin's instrumentation scope.
    ///
    /// The meter records into the global meter provider that the plugin installed.
    #[must_use]
    pub fn meter(&self, name: &'static str) -> Meter {
        crate::metrics::meter_named(name)
    }

    /// Flushes every provider. It is idempotent.
    pub fn shutdown(&self) {
        self.shared.shutdown();
    }
}

impl std::fmt::Debug for OtelTelemetry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OtelTelemetry")
            .field("running", &self.is_running())
            .field(
                "service_name",
                &self.config().map(|config| config.service_name.as_str()),
            )
            .field("signals", &self.config().map(OtelConfig::enabled_signals))
            .finish_non_exhaustive()
    }
}

impl axum::extract::FromRequestParts<AppState> for OtelTelemetry {
    type Rejection = AutumnError;

    #[allow(
        clippy::unused_async_trait_impl,
        reason = "the trait requires `async fn`"
    )]
    async fn from_request_parts(
        _parts: &mut http::request::Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        Self::from_state(state).ok_or_else(|| OtelError::NotInstalled.into_autumn())
    }
}

#[cfg(test)]
mod tests;
