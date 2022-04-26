use async_trait::async_trait;
use mysql::params;
use mysql::prelude::*;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tokio::task::JoinError;
use tokio::{task, time};
use tracing::{debug, error, info};

#[derive(Serialize, Deserialize, Debug, Clone)]
struct KnownShareable {
    id: String,
}

#[derive(Deserialize, Debug, Clone, Serialize)]
pub struct Shareable {
    id: String,
    title: String,
    date: String,
    url: String,
    source: String,
}

fn stringify(err: mysql::Error) -> String {
    format!("{}", err)
}

#[async_trait]
pub trait Fetcher {
    fn name(&self) -> &str;
    async fn fetch(&self, keyword: String) -> Result<Vec<Shareable>, String>;

    async fn run(&self, mut conn: mysql::PooledConn, keyword: String) -> Result<(), String> {
        debug!("Fetching {} with keyword {}", self.name(), keyword);
        let all_items = self.fetch(keyword).await?;

        // MYSQL will find the duplicates and ignore them
        conn.exec_batch(
            r"INSERT IGNORE INTO shareables (id, title, url, date, source)
          VALUES (:id, :title, :url, :date, :source)",
            all_items.iter().map(|p| {
                params! {
                    "id" => p.id.clone(),
                    "title" => p.title.clone(),
                    "url" => p.url.clone(),
                    "date" => p.date.clone(),
                    "source" => p.source.clone()
                }
            }),
        )
        .map_err(stringify)?;

        Ok(())
    }

    async fn run_in_loop(
        &self,
        interval_in_sec: u64,
        pool: Arc<mysql::Pool>,
        keyword: String,
    ) -> Result<(), JoinError> {
        let forever = task::spawn(async move {
            let mut interval = time::interval(Duration::from_secs(interval_in_sec));

            loop {
                let conn = pool.get_conn().expect("Failed to get connection");
                let res = self.run(conn, keyword.clone()).await;
                match res {
                    Ok(_) => {
                        info!("Fetched {}, waiting...", self.name());
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
}
