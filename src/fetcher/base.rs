use serde::{Deserialize, Serialize};

#[derive(Deserialize, Debug, Clone, Serialize)]
pub struct Shareable {
    pub id: String,
    pub title: String,
    pub date: String,
    pub url: String,
    pub source: String,
}
