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

### Fixed

- Builds with `--no-default-features`, or `grpc` without `tls`, no longer fail
  with dead-code and unused-variable errors under `-D warnings`.
- The test suite passes with every feature combination, not only
  `--all-features`; plain `cargo test` failed on an `https` gRPC endpoint
  without `tls`.
- The `OtelTelemetry` extractor returns a ready future, which satisfies the
  `unused_async_trait_impl` lint on new clippy releases without an `allow`
  that older ones reject.

### Changed

- CI: Rust 1.98.1, `actions/checkout@v7` (Node 24), a `features` job that runs
  clippy and tests over the feature powerset, and a weekly scheduled run for
  the latest-deps job.
