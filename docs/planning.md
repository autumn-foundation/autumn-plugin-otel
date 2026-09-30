# Planning: autumn-plugin-otel

## Goal

One new Autumn plugin crate that exports OpenTelemetry traces, metrics and
logs over OTLP, with layered `[otel]` configuration, parent-based sampling,
resource attributes, health reporting, a runnable example and docs.

## API grounding (from the Autumn docs MCP, 2026-09-29)

- `Plugin` lives at `autumn_web::plugin::Plugin` with
  `fn name(&self) -> Cow<'static, str>` and
  `fn build(self, app: AppBuilder) -> AppBuilder`.
- Tier-1 telemetry replacement: `AppBuilder::with_telemetry_provider(T)` where
  `T: autumn_web::telemetry::TelemetryProvider`, with
  `fn init(&self, log: &LogConfig, telemetry: &TelemetryConfig, profile: Option<&str>)
  -> Result<TelemetryGuard, TelemetryInitError>`.
- Health: `autumn_web::actuator::{HealthCheckOutput, HealthIndicator, IndicatorGroup}`,
  registered with `app.health_indicator("otel", ...)`. OTLP has no cheap probe,
  so the check is `HealthOnly`.
- Custom config roots register with `AppBuilder::config_section`.
- Lifecycle: `on_startup`, `on_shutdown`, state via `AppState`.

## Design decisions

- The plugin replaces the framework's default telemetry initializer. The
  framework's log-capture buffer and `/actuator/loggers` reload handle are not
  installed (documented as a known issue).
- `TelemetryInitError` is `#[non_exhaustive]`: exporter build failures fall back
  to a logging-only subscriber with a stderr warning (the framework's
  non-strict shape). Bad `[otel]` configuration stops the boot in the startup hook.
- Parent-based sampler: `Sampler::ParentBased(Box::new(Sampler::TraceIdRatioBased(ratio)))`.
- `https://` gRPC endpoints need the `tls` feature; HTTP uses rustls via reqwest.
- gRPC exporters need a Tokio runtime at build time (tonic); Autumn boots on Tokio.

## Module layout

- `config.rs`: `[otel]` section, defaults, layered loading, validation.
- `error.rs`: `OtelError` → HTTP status mapping, no endpoint leakage.
- `plugin.rs`: `OtelPlugin` builder, shared state, `TelemetryProvider` impl,
  exporter builders, the log bridge, startup/shutdown hooks.
- `client.rs`: `OtelTelemetry` handle in the app state + axum extractor.
- `health.rs`: `HealthOnly` indicator reporting configuration only, never the
  collector endpoint.
- `metrics.rs`: meter helpers over the global provider.

## Test strategy

No live collector. Unit tests per module (`src/<module>/tests.rs`):

- Config: defaults, env layering, validation edge cases, proptests for
  `sample_ratio` and `service_name`.
- Plugin: sampler is parent-based, resource attributes, lifecycle one-way.
- Health: down before start, up while running with details, down after shutdown,
  `HealthOnly` group.
- Error: status mapping, detail hiding.
- Metrics/client: instruments build on the no-op provider, extractors work.

Gates: `cargo fmt --check`, `cargo clippy --locked --all-targets --all-features -D warnings`,
`cargo test --locked --all-targets --all-features`.

## Known issues (also in the README)

1. Log-capture buffer and `/actuator/loggers` reload handle are not installed.
2. Exporter build failure degrades to logging-only instead of a typed init error.
3. Health reports configuration, never collector reachability, and never the
   collector endpoint.
4. Tokio runtime required at provider build time (holds in practice under Autumn).
