//! An instrumented Autumn app.
//!
//! Run it, then curl `http://localhost:3000/work`. The handler runs inside a
//! span that the plugin exports to the configured OTLP endpoint.
//!
//! Without a collector the spans drop quietly in the background. With one,
//! for example `docker run -p 4317:4317 otel/opentelemetry-collector`,
//! they show up in the collector logs.

use autumn_plugin_otel::OtelPlugin;
use autumn_web::prelude::*;

#[get("/work")]
#[tracing::instrument]
async fn work() -> &'static str {
    tracing::info!("the handler runs inside a span");
    "done"
}

#[autumn_web::main]
async fn main() {
    autumn_web::app()
        .plugin(OtelPlugin::new().configure(|c| {
            c.enabled = true;
            c.service_name = "instrumented-example".into();
            c.sample_ratio = 1.0;
        }))
        .routes(routes![work])
        .run()
        .await;
}
