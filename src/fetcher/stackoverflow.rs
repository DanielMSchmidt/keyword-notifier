use chrono::{TimeZone, Utc};
use mysql::params;
use mysql::prelude::*;
use serde::Deserialize;
use std::sync::Arc;
use std::time::Duration;
use tokio::task::JoinError;
use tokio::{task, time};
use tracing::{debug, error, info};

use crate::fetcher::base::Shareable;

#[derive(Debug, Deserialize)]
struct StackOverflowQuestion {
    is_answered: bool,
    link: String,
    title: String,
    answer_count: i32,
    creation_date: i64,
}

#[derive(Debug, Deserialize)]
struct StackOverflowResponse {
    items: Vec<StackOverflowQuestion>,
}

// TODO: walk through pagination if needed
async fn fetch_stackoverflow_api(query: String) -> Result<StackOverflowResponse, String> {
    let url = format!(
        "https://api.stackexchange.com/2.3/search/advanced?order=desc&sort=activity&site=stackoverflow&q={}",
        query
    );
    let resp = match reqwest::Client::builder()
        .gzip(true)
        .build()
        .unwrap()
        .get(url)
        .header("Accept", "application/json; charset=utf-8")
        .send()
        .await
    {
        Ok(resp) => {
            // debug!("Response: {:?}", resp.json().await.unwrap());
            match resp.json::<StackOverflowResponse>().await {
                Ok(json) => json,
                Err(err) => {
                    error!("Could not parse stackoverflow API: {}", err);
                    return Err(format!("{}", err));
                }
            }
        }
        Err(e) => {
            error!("Stackoverflow resopnded with an Error exit code: {}", e);
            return Err(format!("{}", e));
        }
    };

    debug!("Stackoverflow response: {:?}", resp);
    Ok(resp)
}

async fn fetch(mut conn: mysql::PooledConn, keyword: String) -> mysql::Result<()> {
    info!("Fetching StackOverflow Questions");

    let so_result = fetch_stackoverflow_api(keyword.to_string()).await;

    let mut shareables: Vec<Shareable> = vec![];
    match so_result {
        Ok(data) => {
            info!("Found {} StackOverflow Questions", data.items.len());
            data.items.iter().for_each(|item| {
                let item_id = format!("stackoverflow-{}", item.link.clone());

                let date = Utc.timestamp(item.creation_date, 0);
                let state = if item.is_answered {
                    ":white_check_mark:"
                } else if item.answer_count > 0 {
                    ":waiting-spin:"
                } else {
                    ":question:"
                };

                shareables.push(Shareable {
                    id: item_id,
                    title: format!("{} - {}", state, item.title),
                    date: date.date().to_string(),
                    url: item.link.clone(),
                    source: String::from("stackoverflow"),
                });
            });
        }
        Err(e) => {
            error!("Could not fetch StackOverflow Questions, aborting{}", e);
            return Ok(());
        }
    }

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

    info!("Done fetching  StackOverflow Questions");
    Ok(())
}

pub async fn spawn_fetcher(
    interval_in_sec: u64,
    pool: Arc<mysql::Pool>,
    keyword: String,
) -> Result<(), JoinError> {
    let forever = task::spawn(async move {
        let mut interval = time::interval(Duration::from_secs(interval_in_sec));

        loop {
            let conn = pool.get_conn().expect("Failed to get connection");
            let res = fetch(conn, keyword.clone()).await;
            match res {
                Ok(_) => {
                    info!("Fetched StackOverflow Questions, waiting...");
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
