use chrono::{TimeZone, Utc};
use mysql::params;
use mysql::prelude::*;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio::task::JoinError;
use tokio::{task, time};
use tracing::{debug, error, info};

#[derive(Serialize, Deserialize, Debug, Clone)]
struct Config {
    database_url: String,
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
    let known_shareables =
        conn.query_map("SELECT id from shareables", |id| KnownShareable { id })?;
    debug!("Found these known shareables {:?}", known_shareables);
    debug!("Fetching data from twitter");
    let so_result = fetch_stackoverflow_api(format!("{}", keyword)).await;

    let mut shareables: Vec<Shareable> = vec![];
    match so_result {
        Ok(data) => {
            info!(
                "Found {} StackOverflow Questions, filtering",
                data.items.len()
            );
            data.items.iter().for_each(|item| {
                let item_id = format!("stackoverflow-{}", item.link.clone());
                debug!("Checking if {} is known", item_id);
                debug!("{:?}", known_shareables.iter().map(|item| item.id.clone()));

                if known_shareables.iter().find(|x| x.id == item_id).is_none() {
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
                }
            });
        }
        Err(e) => {
            error!("Could not fetch StackOverflow Questions, aborting{}", e);
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

    info!("Done fetching  StackOverflow Questions");
    Ok(())
}

pub async fn spawn_fetcher() -> Result<(), JoinError> {
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
            let res = fetch(conn, config.keyword.clone()).await;
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
