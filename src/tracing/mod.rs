use std::collections::HashMap;

use crate::config::Config;
pub use opentelemetry::global;
pub use opentelemetry::trace::Tracer;
pub use tokio_tracing::{
    debug, error, event, field, info, instrument, level_enabled, log, span, trace, warn,
    Instrument, Level,
};

use tracing_subscriber::EnvFilter;

pub fn initialize_tracing(c: &Config) {
    let opentelemetry = match c.honeycomb_api_key {
        Some(api_key) => {
            let tracer = opentelemetry_otlp::new_pipeline()
                .tracing()
                .with_exporter(opentelemetry_otlp::new_exporter().tonic());

            Some(tracing_opentelemetry::layer().with_tracer(tracer))
        }
        None => None,
    };

    let filter_layer = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new("info"))
        .unwrap();

    tracing_subscriber::registry()
        .with(filter_layer)
        .with(tracing_subscriber::fmt::layer().pretty())
        .with(opentelemetry)
        .init();
}
