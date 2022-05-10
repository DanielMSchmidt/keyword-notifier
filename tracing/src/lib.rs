pub use opentelemetry::global;
use opentelemetry::sdk::export::trace;
use opentelemetry::sdk::trace::config;
use opentelemetry::sdk::Resource;
pub use opentelemetry::trace::Tracer;
use opentelemetry::KeyValue;
use serde::Deserialize;
pub use tokio_tracing::{
    debug, error, event, field, info, instrument, level_enabled, log, span, warn, Instrument, Level,
};
use tonic::metadata::*;

use opentelemetry_otlp::*;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

#[derive(Deserialize, Debug)]
#[serde(rename_all = "lowercase")]
pub enum StdoutFmt {
    Pretty,
    Json,
    Compact,
}

// https://github.com/open-telemetry/opentelemetry-rust/blob/main/examples/basic-otlp/src/main.rs

#[derive(Deserialize, Debug)]
pub struct Config {
    /// Truthy value for the OTEL collector endpoint. NOTE in
    /// this pre-release crate this is expected to be a JÄGER
    /// endpoint, in the format like http://localhost:14268/api/traces
    /// With no value here (None) then opentelemetry collection
    /// is disabled.
    #[serde(default = "default_otel")]
    pub otel: Option<String>,
    #[serde(default = "default_stdout")]
    pub stdout: StdoutFmt,
    #[serde(default = "default_level")]
    pub level: String,
    #[serde(default = "default_service_name")]
    pub service_name: String,
    pub tracing_api_key: String,
}

fn default_otel() -> Option<String> {
    None
}

fn default_stdout() -> StdoutFmt {
    StdoutFmt::Pretty
}

fn default_level() -> String {
    tokio_tracing::Level::INFO.to_string()
}

fn default_service_name() -> String {
    std::env::args()
        .next()
        .as_ref()
        .map(std::path::Path::new)
        .and_then(std::path::Path::file_name)
        .and_then(std::ffi::OsStr::to_str)
        .map(String::from)
        .unwrap_or("unnamed service (fault)".into())
}

pub fn tracing_config<P: AsRef<str>>(prefix: P) -> Config {
    match envy::prefixed(prefix.as_ref().to_string()).from_env::<Config>() {
        Ok(config) => config,
        Err(error) => panic!("{:#?}", error),
    }
}

pub fn initialize_tracing(c: &Config) {
    let (json, compact, pretty) = match c.stdout {
        StdoutFmt::Json => (Some(tracing_subscriber::fmt::layer().json()), None, None),
        StdoutFmt::Compact => (None, Some(tracing_subscriber::fmt::layer().compact()), None),
        StdoutFmt::Pretty => (None, None, Some(tracing_subscriber::fmt::layer().pretty())),
    };

    let opentelemetry = match &c.otel {
        Some(collector_endpoint) => {
            // let tracer = opentelemetry_jaeger::new_pipeline()
            //     .with_collector_endpoint(collector_endpoint)
            //
            //     .install_simple()
            //     .unwrap();

            let mut map = MetadataMap::with_capacity(1);

            map.insert("x-honeycomb-team", c.tracing_api_key.parse().unwrap());

            let tracer = new_pipeline()
                .tracing()
                .with_exporter(
                    new_exporter()
                        .tonic()
                        .with_metadata(map)
                        .with_endpoint(collector_endpoint.to_string()),
                )
                .with_trace_config(opentelemetry::sdk::trace::config().with_resource(
                    Resource::new(vec![KeyValue::new("service.name", c.service_name.clone())]),
                ))
                .install_simple()
                .unwrap();
            Some(tracing_opentelemetry::layer().with_tracer(tracer))
        }
        None => None,
    };

    let filter_layer = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new("info"))
        .unwrap();

    tracing_subscriber::registry()
        .with(filter_layer)
        .with(json)
        .with(pretty)
        .with(compact)
        .with(opentelemetry)
        .init();
}

pub fn shutdown_tracer_provider() {
    opentelemetry::global::shutdown_tracer_provider()
}
