use axum::{
    error_handling::HandleErrorLayer, extract::Extension, http::StatusCode, response::IntoResponse,
    routing::get, Json, Router,
};
use mysql::prelude::*;
use mysql::*;
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::time::Duration;
use tower::{BoxError, ServiceBuilder};
use tower_http::{add_extension::AddExtensionLayer, trace::TraceLayer};
use tracing::{debug, error, info};

#[derive(Debug, Serialize, Clone)]
struct Reponse {
    status: String,
    count: Option<i32>,
}

#[derive(Deserialize, Debug, Clone)]
struct Config {
    database_url: String,
}

#[derive(Deserialize, Debug, Clone, Serialize)]
struct Shareable {
    id: String,
    title: String,
    date: String,
    url: String,
    source: String,
}

#[tokio::main]
async fn main() {
    // initialize tracing
    tracing_subscriber::fmt::init();

    // load config
    let config = envy::from_env::<Config>().expect("Failed to load config");

    let builder =
        mysql::OptsBuilder::from_opts(mysql::Opts::from_url(&config.database_url).unwrap());
    let pool = mysql::Pool::new(builder.ssl_opts(mysql::SslOpts::default()))
        .expect("Failed to initialize mysql");

    let app = Router::new().route("/", get(root)).layer(
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
            .layer(AddExtensionLayer::new(config))
            .layer(AddExtensionLayer::new(pool))
            .into_inner(),
    );

    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    tracing::debug!("listening on {}", addr);
    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await
        .unwrap();
}

// basic handler that responds with a static string
#[tracing::instrument]
async fn root(
    Extension(config): Extension<Config>,
    Extension(pool): Extension<Pool>,
) -> impl IntoResponse {
    let mut conn = pool.get_conn().expect("Failed to get connection");
    let query_result = conn.query_map(
        "SELECT id, title, url, date, source from shareables",
        |(id, title, url, date, source)| Shareable {
            id,
            title,
            date,
            url,
            source,
        },
    );

    match query_result {
        Ok(shareables) => {
            info!("Fetched {} items", shareables.len());
            debug!("Items: {:?}", shareables);
            let response = Reponse {
                status: String::from("ok"),
                count: Some(shareables.len() as i32),
            };
            Json(response)
        }
        Err(e) => {
            error!("Error loading data: {}", e);
            Json(Reponse {
                status: String::from("error"),
                count: None,
            })
        }
    }
}
