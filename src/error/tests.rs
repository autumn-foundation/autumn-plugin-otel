//! Tests for the error type. They touch no network and no collector.

use http::StatusCode;

use super::*;
use crate::config::ConfigError;

#[test]
fn not_installed_maps_to_500() {
    let error = OtelError::NotInstalled;
    assert_eq!(error.kind(), ErrorKind::NotInstalled);
    assert_eq!(error.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(error.detail(), None);
    assert!(error.to_string().contains("not installed"));
}

#[test]
fn config_error_maps_to_500() {
    let error = OtelError::Config(ConfigError(
        "otel.sample_ratio must be from 0.0 to 1.0".into(),
    ));
    assert_eq!(error.kind(), ErrorKind::Config);
    assert_eq!(error.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        error.detail(),
        Some("otel.sample_ratio must be from 0.0 to 1.0")
    );
}

#[test]
fn export_error_maps_to_503_and_hides_detail() {
    let error = OtelError::export("connection refused at http://collector:4317");
    assert_eq!(error.kind(), ErrorKind::Export);
    assert_eq!(error.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        error.detail(),
        Some("connection refused at http://collector:4317")
    );
    // The Display text shows the kind only, never the endpoint.
    assert!(!error.to_string().contains("collector"));
}

#[test]
fn or_http_converts_to_autumn_errors() {
    let ok: Result<(), OtelError> = Ok(());
    assert!(ok.or_http().is_ok());
    let err: Result<(), OtelError> = Err(OtelError::NotInstalled);
    assert!(err.or_http().is_err());
}

#[test]
fn debug_output_is_the_error_text() {
    let error = OtelError::export("secret detail");
    assert_eq!(format!("{error:?}"), format!("{error}"));
    assert!(!format!("{error:?}").contains("secret detail"));
}
