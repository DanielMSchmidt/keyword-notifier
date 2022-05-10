mod config;
mod fetcher;
mod routes;

use axum::{error_handling::HandleErrorLayer, http::StatusCode, routing::get, Router};
use mysql::*;
use serde::Serialize;
use std::time::Duration;
use std::{net::SocketAddr, sync::Arc};
use tower::{BoxError, ServiceBuilder};
use tower_http::{add_extension::AddExtensionLayer, trace::TraceLayer};
use tracing::{error, info, initialize_tracing, shutdown_tracer_provider, tracing_config};

use crate::config::Config;

use self::fetcher::stackoverflow::spawn_fetcher as fetch_stackoverflow;
use self::fetcher::twitter::spawn_fetcher as fetch_twitter;
use self::routes::root::root as root_route;

#[derive(Debug, Serialize, Clone)]
struct Reponse {
    status: String,
    count: Option<i32>,
}

#[tokio::main]
async fn main() {
    let tracing_config = tracing_config("KEYWORD_NOTIFIER_");

    initialize_tracing(&tracing_config);
    // load config
    let config = envy::from_env::<Config>().expect("Failed to load config");

    let builder =
        mysql::OptsBuilder::from_opts(mysql::Opts::from_url(&config.database_url).unwrap());
    let pool = mysql::Pool::new(builder.ssl_opts(mysql::SslOpts::default()))
        .expect("Failed to initialize mysql");
    let pool_arc = Arc::new(pool);

    let app = Router::new().route("/", get(root_route)).layer(
        ServiceBuilder::new()
            .layer(HandleErrorLayer::new(|error: BoxError| async move {
                if error.is::<tower::timeout::error::Elapsed>() {
                    Ok(StatusCode::REQUEST_TIMEOUT)
                } else {
                    Err((
                        StatusCode::INTERNAL_SERVER_ERROR,
                        format!("Unhandled internal error: {}", error),
                    ))
                }
            }))
            .timeout(Duration::from_secs(5))
            .layer(TraceLayer::new_for_http())
            .layer(AddExtensionLayer::new(config.clone()))
            .layer(AddExtensionLayer::new(pool_arc.clone()))
            .into_inner(),
    );

    let addr = SocketAddr::from(([0, 0, 0, 0], config.port));
    tracing::debug!("listening on {}", addr);
    let web_task = axum::Server::bind(&addr).serve(app.into_make_service());

    match tokio::join!(
        web_task,
        fetch_twitter(
            config.interval_in_sec,
            pool_arc.clone(),
            config.keyword.clone(),
            config.twitter_api_bearer.clone()
        ),
        fetch_stackoverflow(
            config.interval_in_sec,
            pool_arc.clone(),
            config.keyword.clone()
        )
    ) {
        (Ok(_), Ok(_), Ok(_)) => info!("Done without errors"),
        (a, b, c) => error!(
            "Error found, web: {:#?}, twitter: {:#?}, stackoverflow: {:#?}",
            a, b, c
        ),
    }

    shutdown_tracer_provider()
}
