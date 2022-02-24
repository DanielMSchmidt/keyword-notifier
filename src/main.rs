use axum::{
    error_handling::HandleErrorLayer, extract::Extension, http::StatusCode, response::IntoResponse,
    routing::get, Json, Router,
};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::time::Duration;
use tower::{BoxError, ServiceBuilder};
use tower_http::{add_extension::AddExtensionLayer, trace::TraceLayer};
use tracing::info;

#[derive(Debug, Serialize, Clone)]
struct Reponse {
    status: String,
}

#[derive(Deserialize, Debug, Clone)]
struct Config {
    twitter_api_bearer: String,
}

#[derive(Debug, Deserialize)]
struct TwitterResponseItem {
    // id: String,
    text: String,
}

#[derive(Debug, Deserialize)]
struct TwitterResponse {
    data: Vec<TwitterResponseItem>,
}

#[tokio::main]
async fn main() {
    // initialize tracing
    tracing_subscriber::fmt::init();

    // load config
    let config = envy::from_env::<Config>().expect("Failed to load config");

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
            .timeout(Duration::from_secs(10))
            .layer(TraceLayer::new_for_http())
            .layer(AddExtensionLayer::new(config))
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
async fn root(Extension(config): Extension<Config>) -> impl IntoResponse {
    // TODO: paramaterize cdktf
    let resp = match reqwest::Client::new()
        .get("https://api.twitter.com/2/tweets/search/recent?query=cdktf")
        .bearer_auth(config.twitter_api_bearer)
        .send()
        .await
    {
        Ok(resp) => match resp.json::<TwitterResponse>().await {
            Ok(json) => json,
            Err(err) => {
                info!("{}", err);
                return Json(Reponse {
                    status: "error".to_string(),
                });
            }
        },
        Err(e) => {
            info!("{}", e);
            return Json(Reponse {
                status: "error".to_string(),
            });
        }
    };

    let content = resp
        .data
        .iter()
        .map(|item| item.text.clone())
        .collect::<Vec<String>>()
        .join("\n")
        .to_string();
    info!("Fetched response {:?}", content);

    let response = Reponse {
        status: String::from("ok"),
    };
    Json(response)
}
