//! The health indicator.
//!
//! # Contract
//!
//! - The check is `HealthOnly`: a sick collector never blocks a deploy.
//! - Before the telemetry provider runs, the check is down with `state = "not started"`.
//! - After the shutdown hook runs, the check is down with `state = "shut down"`.
//! - While running, the check is up. The details name the transport, the sample
//!   ratio, the enabled signals and the service name. They never name the
//!   collector endpoint: health output is visible to operators and must not
//!   leak deployment addresses. When the exporters failed to build and the
//!   plugin logs only, `state` is `"degraded (logging only)"`.
//! - OTLP has no cheap probe. The check reports the exporter configuration.
//!   It never contacts the collector.

use std::collections::HashMap;
use std::sync::Arc;

use autumn_web::actuator::{HealthCheckOutput, HealthIndicator, IndicatorGroup};

use crate::plugin::Shared;

/// The future type of the health check.
type BoxFuture<'a, T> = std::pin::Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Reports the OpenTelemetry exporter configuration.
pub(crate) struct OtelHealthCheck {
    shared: Arc<Shared>,
}

impl OtelHealthCheck {
    pub(crate) const fn new(shared: Arc<Shared>) -> Self {
        Self { shared }
    }
}

impl HealthIndicator for OtelHealthCheck {
    fn group(&self) -> IndicatorGroup {
        IndicatorGroup::HealthOnly
    }

    fn check(&self) -> BoxFuture<'_, HealthCheckOutput> {
        Box::pin(async move {
            if self.shared.is_shut_down() {
                return HealthCheckOutput::down().with_details(detail(&[("state", "shut down")]));
            }
            let Ok(config) = self.shared.resolved.as_ref() else {
                return HealthCheckOutput::down().with_details(detail(&[("state", "not started")]));
            };
            if !self.shared.is_running() {
                return HealthCheckOutput::down().with_details(detail(&[("state", "not started")]));
            }
            let state = if self.shared.is_degraded() {
                "degraded (logging only)"
            } else {
                "configured"
            };
            let protocol = match config.protocol {
                crate::config::OtelProtocol::Grpc => "grpc",
                crate::config::OtelProtocol::Http => "http",
            };
            let signals = config.enabled_signals().join(",");
            let ratio = config.sample_ratio.to_string();
            HealthCheckOutput::up().with_details(detail(&[
                ("state", state),
                ("protocol", protocol),
                ("sample_ratio", ratio.as_str()),
                ("signals", signals.as_str()),
                ("service_name", config.service_name.as_str()),
            ]))
        })
    }
}

fn detail(fields: &[(&str, &str)]) -> HashMap<String, serde_json::Value> {
    fields
        .iter()
        .map(|(key, value)| ((*key).to_owned(), serde_json::Value::from(*value)))
        .collect()
}

#[cfg(test)]
mod tests;
