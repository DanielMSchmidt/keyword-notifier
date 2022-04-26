use mysql::params;
use mysql::prelude::*;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tokio::task::JoinError;
use tokio::{task, time};
use tracing::{debug, error, info};

#[derive(Serialize, Deserialize, Debug, Clone)]
struct Config {
    database_url: String,
    twitter_api_bearer: String,
    keyword: String,
    interval_in_sec: u64,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct KnownShareable {
    id: String,
}

#[derive(Deserialize, Debug, Clone, Serialize)]
struct Shareable {
    id: String,
    title: String,
    date: String,
    url: String,
    source: String,
}

#[derive(Debug, Deserialize, Clone)]
struct TwitterResponseItem {
    id: String,
    text: String,
    created_at: String,
}

#[derive(Debug, Deserialize)]
struct TwitterResponse {
    data: Vec<TwitterResponseItem>,
}

async fn fetch_twitter_api(token: String, query: String) -> Result<TwitterResponse, String> {
    let url = format!(
        "https://api.twitter.com/2/tweets/search/recent?max_results=100&tweet.fields=created_at&query={}",
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

pub async fn fetch(
    mut conn: mysql::PooledConn,
    twitter_api_bearer: String,
    keyword: String,
) -> mysql::Result<()> {
    info!("Fetching tweets");
    let known_shareables =
        conn.query_map("SELECT id from shareables", |id| KnownShareable { id })?;
    debug!("Found these known shareables {:?}", known_shareables);
    debug!("Fetching data from twitter");
    let tweet_result = fetch_twitter_api(twitter_api_bearer.clone(), format!("{}", keyword)).await;

    let mut shareables: Vec<Shareable> = vec![];
    match tweet_result {
        Ok(data) => {
            info!("Found {} tweets, filtering", data.data.len());
            data.data.iter().for_each(|item| {
                let item_id = format!("twitter-{}", item.id.clone());
                debug!("Checking if {} is known", item_id);
                debug!("{:?}", known_shareables.iter().map(|item| item.id.clone()));

                if item.text.contains("RT") {
                    debug!("Skipping tweet {} because it is a retweet", item_id);
                    return;
                }

                if known_shareables.iter().find(|x| x.id == item_id).is_none() {
                    shareables.push(Shareable {
                        id: item_id,
                        title: item.text.clone(),
                        date: item.created_at.clone(),
                        url: format!("https://twitter.com/twitter/status/{}", item.id),
                        source: String::from("twitter"),
                    });
                }
            });
        }
        Err(e) => {
            error!("Could not fetch tweets, aborting{}", e);
            return Ok(());
        }
    }

    info!(
        "Found previously unkown {} shareables, inserting into the DB",
        shareables.len()
    );

    conn.exec_batch(
        r"INSERT INTO shareables (id, title, url, date, source)
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

    info!("Done fetching  tweets");
    Ok(())
}

pub async fn spawn_fetcher(
    interval_in_sec: u64,
    pool: Arc<mysql::Pool>,
    twitter_api_bearer: String,
    keyword: String,
) -> Result<(), JoinError> {
    let forever = task::spawn(async move {
        let mut interval = time::interval(Duration::from_secs(interval_in_sec.clone()));

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