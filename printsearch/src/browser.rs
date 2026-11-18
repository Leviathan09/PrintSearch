//! Lecture de pages web rendues en JavaScript avec un Chrome/Chromium sans interface.
//! Sert aux sites sans API accessible (MakerWorld) et de repli pour Thingiverse.
//!
//! Optimisations : un seul Chrome réutilisé entre les recherches (démarré à l'avance par `serve`),
//! images/médias désactivés, pas d'attente de l'événement « load », lecture de la page toutes les
//! 250 ms avec sortie dès que assez de cartes sont là, et cache de 10 min (la page suivante de
//! « Charger plus » est souvent servie sans recharger).
use anyhow::{anyhow, bail, Context, Result};
use headless_chrome::{Browser, LaunchOptions, Tab};
use serde::Deserialize;
use std::{
    collections::HashMap,
    ffi::OsStr,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

#[derive(Debug, Clone, Deserialize)]
pub struct Card {
    pub id: String,
    pub title: String,
    pub href: String,
    pub img: Option<String>,
}

#[derive(Deserialize, Default)]
struct Scan {
    #[serde(default)]
    cards: Vec<Card>,
    #[serde(default)]
    title: String,
    #[serde(default)]
    text: String,
}

const UA: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0 Safari/537.36";
const CACHE_TTL: Duration = Duration::from_secs(600);

type CacheMap = HashMap<String, (Instant, Vec<Card>, bool)>;
static BROWSER: Mutex<Option<Arc<Browser>>> = Mutex::new(None);
static CACHE: Mutex<Option<CacheMap>> = Mutex::new(None);

/// Extrait toutes les cartes dont un lien correspond à `__RE__` (1er groupe = id).
const JS: &str = r#"(() => {
  const re = new RegExp(__RE__);
  const map = new Map();
  const line = t => (t || '').trim().split('\n').map(s => s.trim()).find(s => s) || '';
  document.querySelectorAll('a[href]').forEach(a => {
    const m = a.getAttribute('href').match(re);
    if (!m) return;
    const img = a.querySelector('img');
    let title = a.getAttribute('title') || a.getAttribute('aria-label') || (img && img.alt) || line(a.innerText);
    if (!title) {
      let p = a.parentElement;
      for (let i = 0; i < 3 && p && !title; i++, p = p.parentElement) {
        if ((p.innerText || '').length < 300) title = line(p.innerText);
      }
    }
    const cur = map.get(m[1]) || { id: m[1], title: '', href: a.href, img: null };
    if (!cur.title && title) cur.title = title;
    if (!cur.img && img) {
      const s = img.currentSrc || img.getAttribute('data-src') || img.src || '';
      if (s && !s.startsWith('data:')) cur.img = s;
    }
    map.set(m[1], cur);
  });
  const cards = [...map.values()].map(c => ({ ...c, title: c.title || ('#' + c.id) }));
  return JSON.stringify({
    cards,
    title: document.title,
    text: (document.body ? document.body.innerText : '').replace(/\s+/g, ' ').slice(0, 120)
  });
})()"#;

pub fn page_slice(cards: Vec<Card>, limit: usize, page: usize) -> Vec<Card> {
    cards.into_iter().skip((page.max(1) - 1) * limit).take(limit).collect()
}

/// Démarre Chrome à l'avance pour que la première recherche soit rapide.
pub fn warm() {
    let _ = browser();
}

fn browser() -> Result<Arc<Browser>> {
    let mut guard = BROWSER.lock().unwrap();
    if let Some(b) = guard.as_ref() {
        if b.get_version().is_ok() {
            return Ok(b.clone());
        }
    }
    let opts = LaunchOptions::default_builder()
        .headless(true)
        .window_size(Some((1366, 900)))
        .idle_browser_timeout(Duration::from_secs(3600))
        .args(vec![
            OsStr::new("--disable-blink-features=AutomationControlled"),
            OsStr::new("--blink-settings=imagesEnabled=false"),
            OsStr::new("--mute-audio"),
            OsStr::new("--disable-extensions"),
            OsStr::new("--disable-background-networking"),
            OsStr::new("--no-first-run"),
        ])
        .ignore_default_args(vec![OsStr::new("--enable-automation")])
        .build()
        .map_err(|e| anyhow!("{e}"))?;
    let b = Arc::new(
        Browser::new(opts)
            .context("Chrome/Chromium introuvable ou impossible à lancer : installez-le (google-chrome, chromium…)")?,
    );
    *guard = Some(b.clone());
    Ok(b)
}

fn cache_get(url: &str, needed: usize) -> Option<Vec<Card>> {
    let g = CACHE.lock().unwrap();
    let (t, cards, exhausted) = g.as_ref()?.get(url)?;
    (t.elapsed() < CACHE_TTL && (cards.len() >= needed || *exhausted)).then(|| cards.clone())
}

fn cache_put(url: &str, cards: &[Card], exhausted: bool) {
    CACHE
        .lock()
        .unwrap()
        .get_or_insert_with(HashMap::new)
        .insert(url.to_string(), (Instant::now(), cards.to_vec(), exhausted));
}

/// Charge `url`, fait défiler la page jusqu'à avoir `needed` cartes (ou plus rien de nouveau).
pub async fn scrape_cards(url: String, id_re: &'static str, needed: usize) -> Result<Vec<Card>> {
    tokio::task::spawn_blocking(move || scrape_blocking(&url, id_re, needed)).await?
}

fn scrape_blocking(url: &str, id_re: &str, needed: usize) -> Result<Vec<Card>> {
    if let Some(c) = cache_get(url, needed) {
        return Ok(c);
    }
    let browser = browser()?;
    let tab = browser.new_tab()?;
    let _ = tab.set_user_agent(UA, None, None);
    let js = JS.replace("__RE__", &serde_json::to_string(id_re)?);
    let result = poll(&tab, url, &js, needed);
    let _ = tab.close(true);
    let (cards, exhausted) = result?;
    cache_put(url, &cards, exhausted);
    Ok(cards)
}

fn poll(tab: &Arc<Tab>, url: &str, js: &str, needed: usize) -> Result<(Vec<Card>, bool)> {
    tab.navigate_to(url)?; // pas d'attente de « load » : on lit la page au fil de son rendu
    let start = Instant::now();
    let (mut cards, mut stagnant, mut diag) = (Vec::<Card>::new(), 0u32, String::new());
    let mut exhausted = false;

    while start.elapsed() < Duration::from_secs(35) {
        std::thread::sleep(Duration::from_millis(250));
        let scan: Scan = tab
            .evaluate(js, false)
            .ok()
            .and_then(|o| o.value)
            .and_then(|v| v.as_str().map(String::from))
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();

        if scan.cards.is_empty() {
            diag = format!("titre « {} », début du texte : « {} »", scan.title, scan.text);
            continue;
        }
        stagnant = if scan.cards.len() > cards.len() { 0 } else { stagnant + 1 };
        cards = scan.cards;
        if cards.len() >= needed {
            break;
        }
        if stagnant >= 12 {
            exhausted = true; // ~3 s sans nouvelle carte après défilement : fin des résultats
            break;
        }
        let _ = tab.evaluate("window.scrollTo(0, document.body.scrollHeight)", false);
    }
    if cards.is_empty() {
        bail!("aucun résultat lu après {} s — {}", start.elapsed().as_secs(), diag);
    }
    Ok((cards, exhausted))
}
