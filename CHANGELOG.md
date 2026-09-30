# Changelog

All notable changes to this project are documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `OtelPlugin`: registers `[otel]` config, installs a tier-1
  `TelemetryProvider` via `with_telemetry_provider`, adds the `otel` health
  indicator, and flushes all providers on shutdown.
- OTLP export for traces, metrics and logs over gRPC (tonic) and HTTP protobuf.
- Parent-based sampler with a configurable `sample_ratio`.
- Resource attributes: `service.name`, `service.version`, `service.namespace`
  (when set) and `deployment.environment`.
- `OtelTelemetry`: the handle in the app state, with an axum extractor and
  provider accessors.
- `OtelMetrics`: meter helpers (`counter`, `histogram`, `gauge`,
  `up_down_counter`).
- Layered `[otel]` configuration: defaults < `autumn.toml` <
  `[profile.<name>.otel]` < `autumn-<name>.toml` < `AUTUMN_OTEL__*` variables.
- `instrumented` example.
- Health indicator is `HealthOnly` and reports exporter configuration; it never
  contacts the collector.
- `tls` feature: TLS trust roots for `https://` gRPC endpoints.
