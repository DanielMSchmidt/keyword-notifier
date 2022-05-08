use serde::Deserialize;

fn default_port() -> u16 {
    3000
}
#[derive(Deserialize, Debug, Clone)]
pub struct Config {
    pub database_url: String,
    pub twitter_api_bearer: String,
    pub keyword: String,
    pub interval_in_sec: u64,
    #[serde(default = "default_port")]
    pub port: u16,
    pub honeycomb_api_key: Option<String>,
    pub honeycomb_dataset: Option<String>,
}
