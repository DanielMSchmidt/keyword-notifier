use async_recursion::async_recursion;
use mysql::params;
use mysql::prelude::*;
use serde::Deserialize;
use std::sync::Arc;
use std::time::Duration;
use tokio::task::JoinError;
use tokio::{task, time};
use tracing::{debug, error, info};

use crate::fetcher::base::Shareable;

#[derive(Debug, Deserialize, Clone)]
struct TwitterResponseItem {
    id: String,
    text: String,
    created_at: String,
}

#[derive(Debug, Deserialize, Clone)]
struct TwitterResponseMeta {
    next_token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TwitterResponse {
    data: Vec<TwitterResponseItem>,
    meta: TwitterResponseMeta,
}

#[async_recursion]
async fn fetch_twitter_api(
    token: String,
    query: String,
    next_token: Option<String>,
) -> Result<Vec<Shareable>, String> {
    let mut shareables: Vec<Shareable> = vec![];
    let url = if next_token.is_none() {
        format!(
        "https://api.twitter.com/2/tweets/search/recent?max_results=100&tweet.fields=created_at&query={}",
        query
    )
    } else {
        format!(
        "https://api.twitter.com/2/tweets/search/recent?max_results=100&tweet.fields=created_at&query={}&next_token={}",
        query,
        next_token.unwrap()
    )
    };
    let resp = match reqwest::Client::new()
        .get(url)
        .bearer_auth(token.clone())
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

    resp.data.iter().for_each(|item| {
        let item_id = format!("twitter-{}", item.id.clone());

        if item.text.contains("RT") {
            debug!("Skipping tweet {} because it is a retweet", item_id);
            return;
        }

        shareables.push(Shareable {
            id: item_id,
            title: item.text.clone(),
            date: item.created_at.clone(),
            url: format!("https://twitter.com/twitter/status/{}", item.id),
            source: String::from("twitter"),
        });
    });

    if resp.meta.next_token.is_some() {
        let pagination_result =
            fetch_twitter_api(token.clone(), query, resp.meta.next_token).await?;

        shareables.extend(pagination_result);
    }

    Ok(shareables)
}

#[tracing::instrument]
pub async fn fetch(
    mut conn: mysql::PooledConn,
    twitter_api_bearer: String,
    keyword: String,
) -> mysql::Result<()> {
    info!("Fetching tweets");
    let result = fetch_twitter_api(twitter_api_bearer.clone(), keyword.to_string(), None).await;

    match result {
        Ok(shareables) => {
            info!("Found {} tweets", shareables.len());
            conn.exec_batch(
                r"INSERT IGNORE INTO shareables (id, title, url, date, source)
              VALUES (:id, :title, :url, :date, :source)",
                shareables.iter().map(|p| {
                    params! {
                        "id" => p.id.clone(),
                        "title" => p.title.clone(),
                        "url" => p.url.clone(),
                        "date" => p.date.clone(),
                        "source" => p.source.clone()
                    }
                }),
            )?;
        }
        Err(e) => {
            error!("Could not fetch tweets, aborting{}", e);
            return Ok(());
        }
    }

    info!("Done fetching  tweets");
    Ok(())
}

pub async fn spawn_fetcher(
    interval_in_sec: u64,
    pool: Arc<mysql::Pool>,

    keyword: String,
    twitter_api_bearer: String,
) -> Result<(), JoinError> {
    let forever = task::spawn(async move {
        let mut interval = time::interval(Duration::from_secs(interval_in_sec));

        loop {
            let conn = pool.get_conn().expect("Failed to get connection");
            let res = fetch(conn, twitter_api_bearer.clone(), keyword.clone()).await;
            match res {
                Ok(_) => {
                    info!("Fetched Tweets, waiting...");
                }
                Err(e) => {
                    error!("Error: {}", e);
                }
            }
            interval.tick().await;
        }
    });

    forever.await
}
