//! Interface web locale. Écoute uniquement sur 127.0.0.1 : les jetons ne quittent pas votre machine.
use crate::config::Config;
use crate::providers::{provider, thingiverse, Site};
use crate::search::{search_all, Filters, PriceFilter, Sort};
use axum::{
    extract::{Path, Query, State},
    http::{header, StatusCode},
    response::{Html, Redirect},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::sync::Mutex;

type Shared = Arc<Mutex<Config>>;
type ApiResult = Result<Json<Value>, (StatusCode, String)>;

fn err(e: impl std::fmt::Display) -> (StatusCode, String) {
    (StatusCode::BAD_GATEWAY, format!("{e:#}"))
}
fn site_of(k: &str) -> Result<Site, (StatusCode, String)> {
    Site::from_key(k).ok_or((StatusCode::BAD_REQUEST, format!("site inconnu : {k}")))
}

pub async fn run(open_browser: bool) -> anyhow::Result<()> {
    let state: Shared = Arc::new(Mutex::new(Config::load()?));
    let app = Router::new()
        .route("/", get(|| async { Html(include_str!("../web/index.html")) }))
        .route("/icons/:name", get(icon))
        .route("/api/search", get(search))
        .route("/api/status", get(status))
        .route("/api/login", post(login))
        .route("/api/logout", post(logout))
        .route("/api/like", post(like))
        .route("/api/collect", post(collect))
        .route("/auth/thingiverse", get(auth_thingiverse))
        .route("/callback", get(callback))
        .with_state(state);
    tokio::task::spawn_blocking(crate::browser::warm); // Chrome prêt avant la 1re recherche
    let listener = tokio::net::TcpListener::bind("127.0.0.1:8787").await?;
    println!("Interface disponible sur http://localhost:8787 (fermez cette fenêtre ou Ctrl+C pour arrêter)");
    if open_browser {
        let _ = open::that("http://localhost:8787");
    }
    axum::serve(listener, app).await?;
    Ok(())
}

#[derive(Deserialize)]
struct SearchParams {
    q: String,
    sites: Option<String>,
    limit: Option<usize>,
    page: Option<usize>,
    price: Option<PriceFilter>,
    sort: Option<Sort>,
    min_likes: Option<u64>,
    min_downloads: Option<u64>,
}

async fn search(State(cfg): State<Shared>, Query(p): Query<SearchParams>) -> Json<Value> {
    let sites: Vec<Site> = match &p.sites {
        Some(s) if !s.is_empty() => s.split(',').filter_map(Site::from_key).collect(),
        _ => Site::ALL.to_vec(),
    };
    let filters = Filters {
        price: p.price.unwrap_or_default(),
        sort: p.sort.unwrap_or_default(),
        min_likes: p.min_likes.unwrap_or(0),
        min_downloads: p.min_downloads.unwrap_or(0),
    };
    let snapshot = cfg.lock().await.clone();
    let out = search_all(&snapshot, &sites, &p.q, p.limit.unwrap_or(12).clamp(1, 100), p.page.unwrap_or(1).max(1), &filters).await;
    Json(json!({ "results": out.results, "errors": out.errors, "raw": out.raw }))
}

async fn status(State(cfg): State<Shared>) -> Json<Value> {
    let c = cfg.lock().await;
    let m: serde_json::Map<String, Value> = Site::ALL
        .iter()
        .map(|s| (s.key().to_string(), json!(c.token_for(*s).is_some())))
        .collect();
    let mut m = m;
    let oauth = std::env::var("THINGIVERSE_CLIENT_ID").is_ok() && std::env::var("THINGIVERSE_CLIENT_SECRET").is_ok();
    m.insert("thingiverse_oauth".into(), json!(oauth));
    Json(Value::Object(m))
}

#[derive(Deserialize)]
struct LoginBody {
    site: String,
    token: Option<String>,
}

async fn login(State(cfg): State<Shared>, Json(b): Json<LoginBody>) -> ApiResult {
    let site = site_of(&b.site)?;
    let token = b.token.filter(|t| !t.trim().is_empty()).ok_or((StatusCode::BAD_REQUEST, "jeton vide".into()))?;
    let mut c = cfg.lock().await;
    c.tokens.insert(site.key().into(), token.trim().to_string());
    c.save().map_err(err)?;
    Ok(Json(json!({ "ok": true })))
}

async fn logout(State(cfg): State<Shared>, Json(b): Json<LoginBody>) -> ApiResult {
    let site = site_of(&b.site)?;
    let mut c = cfg.lock().await;
    c.tokens.remove(site.key());
    c.save().map_err(err)?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct ActionBody {
    site: String,
    id: String,
    collection: Option<String>,
}

async fn token_of(cfg: &Shared, site: Site) -> Result<String, (StatusCode, String)> {
    cfg.lock().await.token_for(site).ok_or((StatusCode::UNAUTHORIZED, "non connecté à ce site".into()))
}

async fn like(State(cfg): State<Shared>, Json(b): Json<ActionBody>) -> ApiResult {
    let site = site_of(&b.site)?;
    let t = token_of(&cfg, site).await?;
    provider(site).like(&b.id, &t).await.map_err(err)?;
    Ok(Json(json!({ "ok": true })))
}

async fn collect(State(cfg): State<Shared>, Json(b): Json<ActionBody>) -> ApiResult {
    let site = site_of(&b.site)?;
    let t = token_of(&cfg, site).await?;
    let col = b.collection.ok_or((StatusCode::BAD_REQUEST, "collection manquante".into()))?;
    provider(site).add_to_collection(&col, &b.id, &t).await.map_err(err)?;
    Ok(Json(json!({ "ok": true })))
}

async fn auth_thingiverse() -> Result<Redirect, (StatusCode, String)> {
    Ok(Redirect::to(&thingiverse::authorize_url().map_err(err)?))
}

#[derive(Deserialize)]
struct CallbackParams {
    code: String,
}

async fn callback(State(cfg): State<Shared>, Query(p): Query<CallbackParams>) -> Result<Redirect, (StatusCode, String)> {
    let token = thingiverse::exchange_code(&p.code).await.map_err(err)?;
    let mut c = cfg.lock().await;
    c.tokens.insert("thingiverse".into(), token);
    c.save().map_err(err)?;
    Ok(Redirect::to("/#parametres"))
}

/// Icônes des sites, intégrées au binaire (remplaçables : voir web/icons/).
async fn icon(Path(name): Path<String>) -> Result<([(header::HeaderName, &'static str); 1], &'static str), StatusCode> {
    let svg = match name.as_str() {
        "thingiverse.svg" => include_str!("../web/icons/thingiverse.svg"),
        "printables.svg" => include_str!("../web/icons/printables.svg"),
        "makerworld.svg" => include_str!("../web/icons/makerworld.svg"),
        _ => return Err(StatusCode::NOT_FOUND),
    };
    Ok(([(header::CONTENT_TYPE, "image/svg+xml")], svg))
}
