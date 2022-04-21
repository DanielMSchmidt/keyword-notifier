use mysql::params;
use mysql::prelude::*;
use serde::{Deserialize, Serialize};
use std::time::Duration;
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

async fn fetch(
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

#[tokio::main]
async fn main() {
    // initialize tracing
    tracing_subscriber::fmt::init();

    let forever = task::spawn(async {
        // load config
        let config = envy::from_env::<Config>().expect("Failed to load config");

        let builder =
            mysql::OptsBuilder::from_opts(mysql::Opts::from_url(&config.database_url).unwrap());
        let mut interval = time::interval(Duration::from_secs(config.interval_in_sec));

        let pool = mysql::Pool::new(builder.ssl_opts(mysql::SslOpts::default()))
            .expect("Failed to initialize mysql");
        loop {
            let conn = pool.get_conn().expect("Failed to get connection");
            let res = fetch(
                conn,
                config.twitter_api_bearer.clone(),
                config.keyword.clone(),
            )
            .await;
            match res {
                Ok(_) => {
                    info!("Fetched tweets, waiting...");
                }
                Err(e) => {
                    error!("Error: {}", e);
                }
            }
            interval.tick().await;
        }
    });

    match forever.await {
        Ok(_) => {
            info!("Done");
        }
        Err(e) => {
            error!("Error: {}", e);
        }
    }
}
