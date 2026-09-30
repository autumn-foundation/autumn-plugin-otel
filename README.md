# autumn-plugin-otel

OpenTelemetry traces, metrics and logs for the [Autumn](https://autumn-web.app) web framework,
exported over OTLP to a configurable collector endpoint.

## Quickstart

Add the plugin, then opt in. The plugin reads `[otel]` in `autumn.toml`:

```toml
[otel]
enabled = true
endpoint = "http://localhost:4317"
service_name = "my-app"
sample_ratio = 0.1
protocol = "grpc" # or "http"
```

```rust,no_run
use autumn_plugin_otel::OtelPlugin;

# async fn run() {
autumn_web::app()
    .plugin(OtelPlugin::new().configure(|c| {
        c.enabled = true;
        c.service_name = "my-app".into();
    }))
    .run()
    .await;
# }
```

Or set it with environment variables: `AUTUMN_OTEL__ENABLED=true`,
`AUTUMN_OTEL__ENDPOINT`, `AUTUMN_OTEL__SERVICE_NAME`, `AUTUMN_OTEL__SAMPLE_RATIO`,
`AUTUMN_OTEL__PROTOCOL`.

Run the example:

```bash
cargo run --example instrumented
curl http://localhost:3000/work
```

## What it does

- Installs a tier-1 `TelemetryProvider` that owns the global tracing subscriber.
  This is the framework's sanctioned hook for exporter setup — the plugin does
  not fight the default initializer, it replaces it through `with_telemetry_provider`.
- Exports three signals to the OTLP collector: traces (via a `tracing-opentelemetry`
  layer), metrics (via the global OTel meter provider) and logs (tracing events
  forwarded as OTLP log records).
- Samples with a parent-based sampler over the configured `sample_ratio` (0.0 to 1.0).
- Tags every signal with `service.name`, `service.version`, `deployment.environment`
  and `service.namespace` when set.
- Reports exporter configuration on `/actuator/health` under `otel` (health-only:
  a sick collector never blocks a deploy). The details name the transport, the
  sample ratio, the enabled signals and the service — never the collector
  endpoint.
- Flushes all providers at shutdown.

## Configuration

| Key | Env var | Default | Notes |
| --- | --- | --- | --- |
| `enabled` | `AUTUMN_OTEL__ENABLED` | `false` | The plugin installs nothing when off. |
| `endpoint` | `AUTUMN_OTEL__ENDPOINT` | `http://localhost:4317` | Absolute URI with scheme and authority. |
| `service_name` | `AUTUMN_OTEL__SERVICE_NAME` | `autumn-app` | The `service.name` resource attribute. |
| `service_version` | `AUTUMN_OTEL__SERVICE_VERSION` | `unknown` | The `service.version` resource attribute. |
| `service_namespace` | `AUTUMN_OTEL__SERVICE_NAMESPACE` | unset | The `service.namespace` resource attribute. |
| `environment` | `AUTUMN_OTEL__ENVIRONMENT` | `development` | The `deployment.environment` resource attribute. |
| `sample_ratio` | `AUTUMN_OTEL__SAMPLE_RATIO` | `1.0` | 0.0 to 1.0, parent-based. |
| `protocol` | `AUTUMN_OTEL__PROTOCOL` | `grpc` | `grpc` or `http` (protobuf). |
| `traces` | `AUTUMN_OTEL__TRACES` | `true` | Export spans. |
| `metrics` | `AUTUMN_OTEL__METRICS` | `true` | Export instruments. |
| `logs` | `AUTUMN_OTEL__LOGS` | `true` | Export tracing events as logs. |
| `health_check` | `AUTUMN_OTEL__HEALTH_CHECK` | `true` | The `otel` health indicator. |

Layering: defaults < `[otel]` in `autumn.toml` < `[profile.<name>.otel]` <
`[otel]` in `autumn-<name>.toml` < `AUTUMN_OTEL__*` variables.

## Metrics in handlers

```rust,no_run
use autumn_plugin_otel::{OtelMetrics, OtelTelemetry};
use autumn_web::AppState;

fn record(state: &AppState) {
    if let Some(otel) = OtelTelemetry::from_state(state) {
        let metrics = OtelMetrics::new();
        let counter = metrics.counter("orders_total", "orders placed");
        counter.add(1, &[]);
    }
}
```

## Cargo features

- `grpc` (default): OTLP over gRPC via tonic.
- `http` (default): OTLP over HTTP protobuf via reqwest.
- `tls`: TLS trust roots for `https://` gRPC endpoints. HTTP uses rustls.

## Known issues

- `TelemetryInitError` is `#[non_exhaustive]`, so the provider can not return a
  typed init error. An exporter that fails to build falls back to a logging-only
  subscriber with a warning on stderr (the framework's non-strict shape). A bad
  `[otel]` configuration still stops the boot, via the startup hook.
- The plugin replaces Autumn's default telemetry initializer, so the framework's
  log-capture buffer and the `/actuator/loggers` reload handle are not installed.
- OTLP has no cheap health probe. The `otel` health indicator reports the exporter
  configuration only; it never contacts the collector, and it never exposes the
  collector endpoint.
- gRPC export needs a Tokio runtime at provider build time (tonic requirement);
  Autumn always boots on Tokio, so this holds in practice.
- The tracing-to-OTLP log bridge forwards each event's message and target; it
  does not forward every structured field.
