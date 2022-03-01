use axum::{
    error_handling::HandleErrorLayer, extract::Extension, http::StatusCode, response::IntoResponse,
    routing::get, Json, Router,
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::net::SocketAddr;
use std::sync::{Arc, RwLock};
use std::time::Duration;
use tower::{BoxError, ServiceBuilder};
use tower_http::{add_extension::AddExtensionLayer, trace::TraceLayer};
use tracing::info;

#[derive(Debug, Serialize, Clone)]
struct Reponse {
    status: String,
    posted_items: Option<i32>,
}

#[derive(Deserialize, Debug, Clone)]
struct Config {
    twitter_api_bearer: String,
}

pub trait Shareable {
    fn title(&self) -> String;
    fn link(&self) -> String;
    fn message(&self) -> String {
        format!("{} - {}", self.title(), self.link())
    }
}

pub trait Cacheable {
    fn cache_key(&self) -> String;
}

pub trait Cache {
    fn add(&mut self, key: String) -> bool;
    fn contains(&self, key: String) -> bool;
}

#[derive(Debug, Deserialize, Clone)]
struct TwitterResponseItem {
    id: String,
    text: String,
}

impl Shareable for TwitterResponseItem {
    fn title(&self) -> String {
        self.text.clone()
    }
    fn link(&self) -> String {
        format!("https://twitter.com/twitter/status/{}", self.id)
    }
}

impl Cacheable for TwitterResponseItem {
    fn cache_key(&self) -> String {
        format!("{}", self.id)
    }
}

#[derive(Debug, Clone, Default)]
struct LocalCache {
    cache: HashSet<String>,
}

impl Cache for LocalCache {
    fn add(&mut self, key: String) -> bool {
        self.cache.insert(key);
        true
    }
    fn contains(&self, key: String) -> bool {
        self.cache.contains(&key)
    }
}

type LocalCacheAccessor = Arc<RwLock<LocalCache>>;

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

    // Setup a cache
    let cache = LocalCacheAccessor::default();

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
            .layer(AddExtensionLayer::new(cache))
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
    Extension(cache): Extension<LocalCacheAccessor>,
) -> impl IntoResponse {
    // TODO: paramaterize cdktf
    let resp = match reqwest::Client::new()
        .get("https://api.twitter.com/2/tweets/search/recent?max_results=100&query=cdktf")
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
                    posted_items: None,
                });
            }
        },
        Err(e) => {
            info!("{}", e);
            return Json(Reponse {
                status: "error".to_string(),
                posted_items: None,
            });
        }
    };

    let mut items_to_post: Vec<TwitterResponseItem> = Vec::new();

    // release the lock
    {
        let mut c = cache.write().unwrap();

        for item in resp.data {
            if !c.contains(item.cache_key()) {
                items_to_post.push(item.clone());
                c.add(item.cache_key());
            } else {
                info!("{} already posted", item.cache_key());
            }
        }
    }

    let content = items_to_post
        .iter()
        .map(|item| item.message())
        .collect::<Vec<String>>();
    info!("Fetched response {:?}", content.join("\n").to_string());

    let response = Reponse {
        status: String::from("ok"),
        posted_items: Some(items_to_post.len() as i32),
    };
    Json(response)
}
