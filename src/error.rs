//! The public error type.
//!
//! # Contract
//!
//! - An error keeps its kind. The error text shows the kind and a short reason.
//! - [`OtelError::detail`] gives the full message when the variant carries one.
//! - A bad configuration gives HTTP 500. The boot stops before it serves requests.
//! - An exporter failure gives HTTP 503. The collector is a downstream dependency.
//! - A missing plugin install gives HTTP 500. The app forgot `.plugin(OtelPlugin::new())`.

use autumn_web::AutumnError;
use http::StatusCode;

use crate::config::ConfigError;

/// An error from the plugin.
#[derive(Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum OtelError {
    /// The `[otel]` configuration is not valid.
    #[error(transparent)]
    Config(#[from] ConfigError),
    /// The OTLP exporter failed.
    ///
    /// The text does not show `detail`, because it can hold an endpoint.
    /// [`OtelError::detail`] gives the full message.
    #[error("the OTLP exporter failed")]
    #[non_exhaustive]
    Export {
        /// The exporter error kind.
        kind: ErrorKind,
        /// The full exporter message.
        detail: String,
    },
    /// The plugin is not installed in the app.
    #[error("the otel plugin is not installed: add `.plugin(OtelPlugin::new())`")]
    NotInstalled,
}

/// The kind of an [`OtelError`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ErrorKind {
    /// The `[otel]` configuration is not valid.
    Config,
    /// The OTLP exporter failed.
    Export,
    /// The plugin is not installed in the app.
    NotInstalled,
}

impl OtelError {
    /// The kind of the error.
    #[must_use]
    pub const fn kind(&self) -> ErrorKind {
        match self {
            Self::Config(_) => ErrorKind::Config,
            Self::Export { .. } => ErrorKind::Export,
            Self::NotInstalled => ErrorKind::NotInstalled,
        }
    }

    /// The full message, when the variant carries one.
    #[must_use]
    pub const fn detail(&self) -> Option<&str> {
        match self {
            Self::Config(err) => Some(err.0.as_str()),
            Self::Export { detail, .. } => Some(detail.as_str()),
            Self::NotInstalled => None,
        }
    }

    /// The HTTP status code for the error.
    #[must_use]
    pub const fn status(&self) -> StatusCode {
        match self {
            Self::Config(_) | Self::NotInstalled => StatusCode::INTERNAL_SERVER_ERROR,
            Self::Export { .. } => StatusCode::SERVICE_UNAVAILABLE,
        }
    }

    /// Converts the error into an Autumn error with the right status code.
    #[must_use]
    pub fn into_autumn(self) -> AutumnError {
        let message = format!("{self}");
        match self {
            Self::Export { .. } => AutumnError::service_unavailable_msg(message),
            Self::Config(_) | Self::NotInstalled => AutumnError::internal_server_error_msg(message),
        }
    }

    /// Makes an exporter error. The text keeps `detail` out of logs.
    #[must_use]
    pub fn export(detail: impl Into<String>) -> Self {
        Self::Export {
            kind: ErrorKind::Export,
            detail: detail.into(),
        }
    }
}

impl std::fmt::Debug for OtelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self}")
    }
}

/// Extends `Result<T, OtelError>` with an HTTP conversion.
pub trait OtelResultExt<T> {
    /// Converts `Err` into an Autumn error with the right status code.
    ///
    /// # Errors
    ///
    /// Returns the Autumn error when `self` is `Err`.
    fn or_http(self) -> Result<T, AutumnError>;
}

impl<T> OtelResultExt<T> for Result<T, OtelError> {
    fn or_http(self) -> Result<T, AutumnError> {
        self.map_err(OtelError::into_autumn)
    }
}

#[cfg(test)]
mod tests;
