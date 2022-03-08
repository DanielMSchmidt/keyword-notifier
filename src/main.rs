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
use tracing::{info, warn};

#[derive(Debug, Serialize, Clone)]
struct Reponse {
    status: String,
    posted_items: Option<i32>,
}

#[derive(Debug, Serialize, Clone)]
struct SlackMessage {
    text: String,
}

#[derive(Deserialize, Debug, Clone)]
struct Config {
    twitter_api_bearer: String,
    keyword: String,
    slack_webhook: String,
}

pub trait Shareable {
    fn title(&self) -> String;
    fn link(&self) -> String;
    fn link_name(&self) -> String;
    fn message(&self) -> String {
        format!("<{}|{}>: {}", self.link(), self.link_name(), self.title())
    }
}

pub trait Cacheable {
    fn cache_key(&self) -> String;
    fn never_share(&self) -> bool {
        false
    }
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
    fn link_name(&self) -> String {
        ":bird: Twitter".to_string()
    }
}

impl Cacheable for TwitterResponseItem {
    fn cache_key(&self) -> String {
        format!("twitter-{}", self.id)
    }

    fn never_share(&self) -> bool {
        self.text.contains("RT") || self.text.starts_with("@")
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

#[derive(Debug, Deserialize)]
struct StackOverflowQuestion {
    is_answered: bool,
    link: String,
    title: String,
    answer_count: i32,
}

impl Shareable for StackOverflowQuestion {
    fn title(&self) -> String {
        self.title.clone()
    }
    fn link(&self) -> String {
        self.link.clone()
    }
    fn link_name(&self) -> String {
        let state = if self.is_answered {
            ":white_check_mark:"
        } else if self.answer_count > 0 {
            ":waiting-spin:"
        } else {
            ":question:"
        };

        format!("{} StackOverflow", state)
    }
}

impl Cacheable for StackOverflowQuestion {
    fn cache_key(&self) -> String {
        format!("stack-overflow-{}", self.link)
    }
}

#[derive(Debug, Deserialize)]
struct StackOverflowResponse {
    items: Vec<StackOverflowQuestion>,
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

async fn fetch_twitter_api(token: String, query: String) -> Result<TwitterResponse, String> {
    let url = format!(
        "https://api.twitter.com/2/tweets/search/recent?max_results=100&query={}",
        query
    );
    let resp = match reqwest::Client::new()
        .get(url)
        .bearer_auth(token)
        .send()
        .await
    {
        Ok(resp) => match resp.json::<TwitterResponse>().await {
            Ok(json) => json,
            Err(err) => {
                info!("{}", err);
                return Err(format!("{}", err));
            }
        },
        Err(e) => {
            info!("{}", e);
            return Err(format!("{}", e));
        }
    };

    Ok(resp)
}

async fn fetch_stackoverflow_api(query: String) -> Result<StackOverflowResponse, String> {
    let url = format!(
        "https://api.stackexchange.com/search/advanced?site=stackoverflow.com&q={}",
        query
    );
    let resp = match reqwest::Client::new().get(url).send().await {
        Ok(resp) => match resp.json::<StackOverflowResponse>().await {
            Ok(json) => json,
            Err(err) => {
                info!("{}", err);
                return Err(format!("{}", err));
            }
        },
        Err(e) => {
            info!("{}", e);
            return Err(format!("{}", e));
        }
    };

    Ok(resp)
}

fn filter_duplicate_twitter_items(
    cache: &LocalCacheAccessor,
    resp: TwitterResponse,
) -> Vec<Box<dyn Shareable>> {
    let mut items_to_post: Vec<Box<dyn Shareable>> = Vec::new();
    let mut c = cache.write().unwrap();

    for item in resp.data {
        if c.contains(item.cache_key()) {
            info!("{} already posted", item.cache_key());
        } else if item.never_share() {
            info!("{} should not be shared", item.link());
        } else {
            items_to_post.push(Box::new(item));
            c.add(item.cache_key());
        }
    }

    items_to_post
}

fn filter_duplicate_stack_overflow_items(
    cache: &LocalCacheAccessor,
    resp: StackOverflowResponse,
) -> Vec<Box<dyn Shareable>> {
    let mut items_to_post: Vec<Box<dyn Shareable>> = Vec::new();
    let mut c = cache.write().unwrap();

    for item in resp.items {
        if c.contains(item.cache_key()) {
            info!("{} already posted", item.cache_key());
        } else if item.never_share() {
            info!("{} should not be shared", item.link());
        } else {
            items_to_post.push(Box::new(item));
            c.add(item.cache_key());
        }
    }

    items_to_post
}

// basic handler that responds with a static string
#[tracing::instrument]
async fn root(
    Extension(config): Extension<Config>,
    Extension(cache): Extension<LocalCacheAccessor>,
) -> impl IntoResponse {
    let mut items_to_post: Vec<Box<dyn Shareable>> = Vec::new();
    let reponses = tokio::join!(
        fetch_twitter_api(
            config.twitter_api_bearer.clone(),
            format!("{}", config.keyword)
        ),
        fetch_twitter_api(
            config.twitter_api_bearer.clone(),
            format!("%23{}", config.keyword)
        ),
        fetch_stackoverflow_api(format!("{}", config.keyword))
    );

    for item in [reponses.0, reponses.1] {
        match item {
            Ok(resp) => {
                items_to_post.extend(filter_duplicate_twitter_items(&cache, resp));
            }
            Err(err) => {
                warn!("{}", err);
            }
        }
    }

    match reponses.2 {
        Ok(resp) => {
            items_to_post.extend(filter_duplicate_stack_overflow_items(&cache, resp));
        }
        Err(err) => {
            warn!("{}", err);
        }
    }

    let content = items_to_post
        .iter()
        .map(|item| format!("• {}", item.message()))
        .collect::<Vec<String>>();
    info!("Fetched response {:?}", content.join("\n").to_string());

    let slack_webhook_url = config.slack_webhook;

    let resp = SlackMessage {
        text: content.join("\n"),
    };

    let res = reqwest::Client::new()
        .post(slack_webhook_url)
        .body(serde_json::to_string(&resp).unwrap())
        .send()
        .await;
    match res {
        Ok(_) => info!("ok"),
        Err(x) => warn!("Error sending slack message: {:?}", x),
    }

    let response = Reponse {
        status: String::from("ok"),
        posted_items: Some(items_to_post.len() as i32),
    };
    Json(response)
}
