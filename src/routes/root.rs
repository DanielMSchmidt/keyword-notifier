use crate::fetcher::base::Shareable;
use askama::Template;
use axum::{
    extract::Extension,
    http::StatusCode,
    response::{Html, IntoResponse, Response},
};
use mysql::prelude::*;
use mysql::*;
use std::sync::Arc;
use tracing::{debug, error, info};

use crate::config::Config;

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
pub async fn root(
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
