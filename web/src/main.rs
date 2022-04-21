use askama::Template;
use axum::{
    error_handling::HandleErrorLayer,
    extract::Extension,
    http::StatusCode,
    response::{Html, IntoResponse, Response},
    routing::get,
    Router,
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

#[derive(Template)]
#[template(path = "base.html", escape = "none")]
struct BaseTemplate {
    content: String,
}

#[derive(Template)]
#[template(path = "index.html", escape = "none")]
struct IndexTemplate {
    twitter_items: String,
    stackoverflow_items: String,
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

            let twitter_items = shareables
                .iter()
                .filter(|shareable| shareable.source == "twitter")
                .map(|shareable| {
                    format!(
                        "<li><a href=\"{}\">{}</a></li>",
                        shareable.url, shareable.title
                    )
                })
                .collect::<Vec<String>>()
                .join("");

            let stackoverflow_items = shareables
                .iter()
                .filter(|shareable| shareable.source == "stackoverflow")
                .map(|shareable| {
                    format!(
                        "<li><a href=\"{}\">{}</a></li>",
                        shareable.url,
                        shareable
                            .title
                            .replace(":question:", "❓")
                            .replace(":white_check_mark:", "✅")
                            .replace(":waiting-spin:", "🔄")
                    )
                })
                .collect::<Vec<String>>()
                .join("");

            let content = IndexTemplate {
                twitter_items,
                stackoverflow_items,
            };
            let str = content.render().expect("Could not render template");
            HtmlTemplate(BaseTemplate { content: str })
        }
        Err(e) => {
            error!("Error loading data: {}", e);
            let content = ErrorTemplate {
                message: format!("{}", e),
            };
            let str = content.render().expect("Could not render template");
            HtmlTemplate(BaseTemplate { content: str })
        }
    }
}
