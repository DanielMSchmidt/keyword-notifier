use serde::{Deserialize, Serialize};

#[derive(Deserialize, Debug, Clone, Serialize, Eq, PartialEq,)]
pub struct Shareable {
    pub id: String,
    pub title: String,
    pub date: String,
    pub url: String,
    pub source: String,
}

impl PartialOrd for Shareable {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.date.cmp(&other.date))
    }
}

impl Ord for Shareable {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.date.cmp(&other.date)
    }
}