//! MakerWorld n'a pas d'API publique et bloque les requêtes HTTP « nues » (protection anti-bot).
//! On lit donc la page de recherche avec un Chrome piloté (voir browser.rs) : Chrome/Chromium
//! doit être installé. Likes/téléchargements/prix ne sont pas lus dans ce mode.
use super::*;
use crate::browser;

pub struct MakerWorld;

impl MakerWorld {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Provider for MakerWorld {
    async fn search(&self, query: &str, limit: usize, page: usize, _token: Option<&str>) -> Result<Vec<Model>> {
        let url = format!("https://makerworld.com/en/search/models?keyword={}", urlencoding::encode(query));
        let cards = browser::scrape_cards(url, r"/models/(\d+)", page * limit).await?;
        Ok(browser::page_slice(cards, limit, page)
            .into_iter()
            .map(|c| Model {
                site: Site::Makerworld,
                id: c.id,
                title: c.title,
                author: String::new(),
                url: c.href,
                thumbnail: c.img,
                price: None,
                likes: None,
                downloads: None,
            })
            .collect())
    }
    // like / collections : à implémenter après inspection des appels réseau du site connecté.
}
