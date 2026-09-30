# CLAUDE.md — autumn-plugin-otel

The OpenTelemetry traces/metrics/logs exporter for the Autumn web framework.

## Build gates (run all three, in this order)

Every Cargo command runs with:

```bash
export PATH="$HOME/.cargo/bin:$PATH"
export CARGO_BUILD_JOBS=2
export CARGO_TARGET_DIR=~/workspace/autumn-arena/target
```

Tests additionally use `TMPDIR=~/workspace/.tmp-cargo`. Never `cargo clean` the
shared arena target dir. Do not run concurrent Cargo builds.

1. `cargo fmt --all -- --check`
2. `cargo clippy --locked --all-targets --all-features -- -D warnings`
3. `cargo test --locked --all-targets --all-features`

## Design notes

- Autumn APIs are grounded in the docs MCP (`~/workspace/skills/autumn-mcp/`),
  never from training memory. The tier-1 hook is
  `AppBuilder::with_telemetry_provider`, with the exact trait signature
  `fn init(&self, log: &LogConfig, telemetry: &TelemetryConfig, profile: Option<&str>)
  -> Result<TelemetryGuard, TelemetryInitError>`.
- The provider replaces the framework's default initializer, so the framework's
  log-capture buffer and `/actuator/loggers` reload handle are not installed.
  This is a documented known issue, not an oversight.
- `TelemetryInitError` is `#[non_exhaustive]`: exporter build failures fall back
  to a logging-only subscriber with a stderr warning. Bad `[otel]` configuration
  stops the boot in the startup hook instead.
- OTLP has no cheap health probe: the health check is `HealthOnly` and reports
  configuration only.
- gRPC exporters need a Tokio runtime at build time (tonic); Autumn always
  boots on Tokio.
- Tests use `src/<module>/tests.rs` and must not touch a collector or the
  network. `allow-unwrap-in-tests` is on; production code never unwraps.
- The commit message convention is `feat: initial autumn-plugin-otel`.
