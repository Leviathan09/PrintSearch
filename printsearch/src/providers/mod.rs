use anyhow::{bail, Result};
use async_trait::async_trait;
use serde_json::Value;

pub mod makerworld;
pub mod printables;
pub mod thingiverse;

#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Site {
    Thingiverse,
    Printables,
    Makerworld,
}

impl Site {
    pub const ALL: [Site; 3] = [Site::Thingiverse, Site::Printables, Site::Makerworld];
    pub fn from_key(k: &str) -> Option<Site> {
        Site::ALL.into_iter().find(|s| s.key() == k)
    }
    pub fn key(&self) -> &'static str {
        match self {
            Site::Thingiverse => "thingiverse",
            Site::Printables => "printables",
            Site::Makerworld => "makerworld",
        }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Model {
    pub site: Site,
    pub id: String,
    pub title: String,
    pub author: String,
    pub url: String,
    pub thumbnail: Option<String>,
    /// None = inconnu ; Some(0.0) = gratuit ; Some(x>0) = payant
    pub price: Option<f64>,
    pub likes: Option<u64>,
    pub downloads: Option<u64>,
}

#[async_trait]
pub trait Provider: Send + Sync {
    /// `limit` = résultats par page, `page` commence à 1.
    async fn search(&self, query: &str, limit: usize, page: usize, token: Option<&str>) -> Result<Vec<Model>>;
    async fn like(&self, _id: &str, _token: &str) -> Result<()> {
        bail!("« like » non supporté pour ce site")
    }
    async fn add_to_collection(&self, _collection: &str, _id: &str, _token: &str) -> Result<()> {
        bail!("ajout en collection non supporté pour ce site")
    }
}

pub fn provider(site: Site) -> Box<dyn Provider> {
    match site {
        Site::Thingiverse => Box::new(thingiverse::Thingiverse::new()),
        Site::Printables => Box::new(printables::Printables::new()),
        Site::Makerworld => Box::new(makerworld::MakerWorld::new()),
    }
}

pub(crate) fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent("printsearch/0.1 (+personal tool)")
        .build()
        .expect("client http")
}

pub(crate) fn text(v: &Value, key: &str) -> Option<String> {
    match v.get(key)? {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}
