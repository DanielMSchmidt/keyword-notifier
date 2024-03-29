mod fetcher;
use askama::Template;
use axum::{
    error_handling::HandleErrorLayer,
    extract::Extension,
    http::StatusCode,
    response::{Html, IntoResponse, Response},
    routing::get,
    Router,
};

use fetcher::base::Shareable;
use mysql::prelude::*;
use mysql::*;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use std::{net::SocketAddr, sync::Arc};
use tower::{BoxError, ServiceBuilder};
use tower_http::{add_extension::AddExtensionLayer, trace::TraceLayer};
use tracing::{debug, error, info};

use self::fetcher::stackoverflow::spawn_fetcher as fetch_stackoverflow;
use self::fetcher::twitter::spawn_fetcher as fetch_twitter;

#[derive(Debug, Serialize, Clone)]
struct Reponse {
    status: String,
    count: Option<i32>,
}

fn default_port() -> u16 {
    3000
}
#[derive(Deserialize, Debug, Clone)]
struct Config {
    database_url: String,
    twitter_api_bearer: String,
    keyword: String,
    interval_in_sec: u64,
    #[serde(default = "default_port")]
    port: u16,
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
    let pool_arc = Arc::new(pool);

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
}

#[derive(Template)]
#[template(path = "base.html", escape = "none")]
struct BaseTemplate {
    title: String,
}

#[derive(Template)]
#[template(path = "index.html", escape = "none")]
struct IndexTemplate {
    items: Vec<Shareable>,
}

#[derive(Template)]
#[template(path = "error.html")]
struct ErrorTemplate {
    message: String,
}

struct HtmlTemplate<T>(T);

impl<T> IntoResponse for HtmlTemplate<T>
where
    T: Template,
{
    fn into_response(self) -> Response {
        match self.0.render() {
            Ok(html) => Html(html).into_response(),
            Err(err) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to render template. Error: {}", err),
            )
                .into_response(),
        }
    }
}

#[tracing::instrument]
async fn root(
    Extension(config): Extension<Config>,
    Extension(pool): Extension<Arc<Pool>>,
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

            let mut sanitized_shareable = shareables
                .into_iter()
                .map(|item| Shareable {
                    id: item.id,
                    title: item
                        .title
                        .replace(":question:", "❓")
                        .replace(":white_check_mark:", "✅")
                        .replace(":waiting-spin:", "🔄"),
                    date: item.date,
                    url: item.url,
                    source: item.source,
                })
                .filter(|item| !item.title.contains("[Dependency Updated]"))
                .collect::<Vec<Shareable>>();

                sanitized_shareable.sort_by(|a, b| b.cmp(a));



            HtmlTemplate(IndexTemplate {
                items: sanitized_shareable,
            })
            .into_response()
        }
        Err(e) => {
            error!("Error loading data: {}", e);
            HtmlTemplate(ErrorTemplate {
                message: format!("{}", e),
            })
            .into_response()
        }
    }
}
