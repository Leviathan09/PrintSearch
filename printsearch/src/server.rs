//! Interface web locale. Écoute uniquement sur 127.0.0.1 : les jetons ne quittent pas votre machine.
use crate::commands::{self, Ctx};
use crate::config::Config;
use crate::log::{self, log_info, log_warn};
use crate::providers::{provider, thingiverse, Site};
use crate::search::{search_all, Filters, PriceFilter, Sort};
use axum::{
    extract::{DefaultBodyLimit, Path, Query, Request, State},
    http::{header, StatusCode},
    middleware::{self, Next},
    response::{Html, IntoResponse, Redirect, Response},
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

/// Protection contre les pages web tierces (CSRF, « DNS rebinding ») : seules les requêtes adressées
/// à localhost:8787 et, si elles ont une origine, venant de localhost:8787 sont acceptées.
async fn local_only(req: Request, next: Next) -> Response {
    const OK: [&str; 2] = ["localhost:8787", "127.0.0.1:8787"];
    let host_ok = req.headers().get(header::HOST).and_then(|h| h.to_str().ok()).map_or(false, |h| OK.contains(&h));
    let origin_ok = match req.headers().get(header::ORIGIN).and_then(|o| o.to_str().ok()) {
        None => true,
        Some(o) => o.strip_prefix("http://").map_or(false, |h| OK.contains(&h)),
    };
    if host_ok && origin_ok {
        next.run(req).await
    } else {
        (StatusCode::FORBIDDEN, "forbidden").into_response()
    }
}

pub async fn run(listener: std::net::TcpListener, open_browser: bool) -> anyhow::Result<()> {
    let cfg = Config::load();
    crate::browser::set_choice(cfg.browser.clone());
    let state: Shared = Arc::new(Mutex::new(cfg));
    let app = Router::new()
        .route("/", get(|| async { Html(include_str!("../web/index.html")) }))
        .route("/icons/:name", get(icon))
        .route("/api/search", get(search))
        .route("/api/status", get(status))
        .route("/api/login", post(login))
        .route("/api/logout", post(logout))
        .route("/api/browser", get(browser_info).post(browser_set))
        .route("/api/update", get(update_info))
        .route("/api/like", post(like))
        .route("/api/collect", post(collect))
        .route("/api/command", post(command))
        .route("/api/logs", get(logs_text))
        .route("/api/logs/export", get(logs_export))
        .route("/api/quit", post(quit))
        .route("/api/lang", post(set_lang))
        .route("/auth/thingiverse", get(auth_thingiverse))
        .route("/callback", get(callback))
        .layer(DefaultBodyLimit::max(64 * 1024)) // aucune requête de l'interface n'a besoin de plus
        .layer(middleware::from_fn(local_only))
        .with_state(state);
    listener.set_nonblocking(true)?;
    let listener = tokio::net::TcpListener::from_std(listener)?;
    log_info!("web interface ready on {}", crate::app::URL);
    println!("Web interface: {} (Ctrl+C to stop)", crate::app::URL);
    if open_browser {
        let _ = open::that(crate::app::URL);
    }
    axum::serve(listener, app).with_graceful_shutdown(crate::app::wait_quit()).await?;
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
    m.insert("version".into(), json!(crate::update::current()));
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
    log_info!("token saved for {}", site.key());
    Ok(Json(json!({ "ok": true })))
}

async fn logout(State(cfg): State<Shared>, Json(b): Json<LoginBody>) -> ApiResult {
    let site = site_of(&b.site)?;
    let mut c = cfg.lock().await;
    c.tokens.remove(site.key());
    c.save().map_err(err)?;
    log_info!("token removed for {}", site.key());
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

/// Identifiants envoyés tels quels dans une URL d'API : lettres, chiffres, « - » et « _ » seulement.
fn valid_id(s: &str) -> bool {
    !s.is_empty() && s.len() <= 64 && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

async fn like(State(cfg): State<Shared>, Json(b): Json<ActionBody>) -> ApiResult {
    let site = site_of(&b.site)?;
    if !site.can_like() {
        return Err((StatusCode::BAD_REQUEST, format!("Like is not available for {}", site.key())));
    }
    if !valid_id(&b.id) {
        return Err((StatusCode::BAD_REQUEST, "invalid model id".into()));
    }
    let t = token_of(&cfg, site).await?;
    match provider(site).like(&b.id, &t).await {
        Ok(()) => log_info!("liked {} {}", site.key(), b.id),
        Err(e) => {
            log_warn!("like failed on {} {}: {e:#}", site.key(), b.id);
            return Err(err(e));
        }
    }
    Ok(Json(json!({ "ok": true })))
}

async fn collect(State(cfg): State<Shared>, Json(b): Json<ActionBody>) -> ApiResult {
    let site = site_of(&b.site)?;
    if !site.can_collect() {
        return Err((StatusCode::BAD_REQUEST, format!("Collections are not available for {}", site.key())));
    }
    let t = token_of(&cfg, site).await?;
    let col = b.collection.ok_or((StatusCode::BAD_REQUEST, "collection manquante".into()))?;
    if !valid_id(&col) || !valid_id(&b.id) {
        return Err((StatusCode::BAD_REQUEST, "invalid id".into()));
    }
    match provider(site).add_to_collection(&col, &b.id, &t).await {
        Ok(()) => log_info!("added {} {} to collection {col}", site.key(), b.id),
        Err(e) => {
            log_warn!("add to collection failed on {} {}: {e:#}", site.key(), b.id);
            return Err(err(e));
        }
    }
    Ok(Json(json!({ "ok": true })))
}

/// Instant où une connexion OAuth a été lancée depuis l'interface : une page tierce ne peut pas
/// glisser son propre code d'autorisation dans /callback sans que l'utilisateur ait cliqué « Se connecter ».
static OAUTH_PENDING: std::sync::Mutex<Option<std::time::Instant>> = std::sync::Mutex::new(None);

async fn auth_thingiverse() -> Result<Redirect, (StatusCode, String)> {
    let url = thingiverse::authorize_url().map_err(err)?;
    *OAUTH_PENDING.lock().unwrap_or_else(|e| e.into_inner()) = Some(std::time::Instant::now());
    Ok(Redirect::to(&url))
}

#[derive(Deserialize)]
struct CallbackParams {
    code: String,
}

async fn callback(State(cfg): State<Shared>, Query(p): Query<CallbackParams>) -> Result<Redirect, (StatusCode, String)> {
    let pending = OAUTH_PENDING.lock().unwrap_or_else(|e| e.into_inner()).take();
    if !pending.map_or(false, |t| t.elapsed() < std::time::Duration::from_secs(600)) {
        log_warn!("OAuth callback refused: no login in progress");
        return Err((StatusCode::FORBIDDEN, "no Thingiverse login in progress".into()));
    }
    let token = thingiverse::exchange_code(&p.code).await.map_err(|e| {
        log_warn!("Thingiverse OAuth failed: {e:#}");
        err(e)
    })?;
    log_info!("Thingiverse connected through OAuth");
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
        "thangs.svg" => include_str!("../web/icons/thangs.svg"),
        "cults3d.svg" => include_str!("../web/icons/cults3d.svg"),
        "myminifactory.svg" => include_str!("../web/icons/myminifactory.svg"),
        _ => return Err(StatusCode::NOT_FOUND),
    };
    Ok(([(header::CONTENT_TYPE, "image/svg+xml")], svg))
}

fn browser_json(choice: Option<String>) -> Value {
    json!({
        "choice": choice.clone().unwrap_or_else(|| "auto".into()),
        "detected": crate::browser::detect(),
        "effective": crate::browser::effective(choice.as_deref()),
    })
}

async fn browser_info(State(cfg): State<Shared>) -> Json<Value> {
    let choice = cfg.lock().await.browser.clone();
    Json(browser_json(choice))
}

#[derive(Deserialize)]
struct BrowserBody {
    choice: String,
}

async fn browser_set(State(cfg): State<Shared>, Json(b): Json<BrowserBody>) -> ApiResult {
    let choice = b.choice.trim().to_string();
    crate::browser::validate_choice(&choice).map_err(|e| (StatusCode::BAD_REQUEST, format!("{e:#}")))?;
    let stored = if choice.is_empty() || choice == "auto" { None } else { Some(choice) };
    {
        let mut c = cfg.lock().await;
        c.browser = stored.clone();
        c.save().map_err(err)?;
    }
    log_info!("search browser set to {}", stored.as_deref().unwrap_or("auto"));
    crate::browser::set_choice(stored.clone());
    Ok(Json(browser_json(stored)))
}

#[derive(Deserialize)]
struct UpdateParams {
    force: Option<u8>,
}

async fn update_info(Query(p): Query<UpdateParams>) -> ApiResult {
    let info = crate::update::check(p.force == Some(1)).await.map_err(err)?;
    Ok(Json(json!(info)))
}

// ---------------------------------------------------------------------------------------------
// Console, journal, arrêt, langue
// ---------------------------------------------------------------------------------------------

#[derive(Deserialize)]
struct CommandBody {
    line: String,
}

async fn command(State(cfg): State<Shared>, Json(b): Json<CommandBody>) -> ApiResult {
    if b.line.len() > 2000 {
        return Err((StatusCode::BAD_REQUEST, "command too long".into()));
    }
    let output = commands::run_line(&Ctx { cfg, in_app: true }, &b.line).await;
    Ok(Json(json!({ "output": output })))
}

#[derive(Deserialize)]
struct LogParams {
    n: Option<usize>,
    level: Option<log::Level>,
}

async fn logs_text(Query(p): Query<LogParams>) -> impl IntoResponse {
    let lines = log::tail(p.n.unwrap_or(200).min(5000), p.level.unwrap_or(log::Level::Info));
    ([(header::CONTENT_TYPE, "text/plain; charset=utf-8")], lines.join("\n"))
}

async fn logs_export(State(cfg): State<Shared>) -> impl IntoResponse {
    let body = log::export_text(&commands::system_info(&Ctx { cfg, in_app: true }).await);
    (
        [
            (header::CONTENT_TYPE, "text/plain; charset=utf-8".to_string()),
            (header::CONTENT_DISPOSITION, format!("attachment; filename=\"printsearch-logs-{}.txt\"", log::stamp_for_filename())),
        ],
        body,
    )
}

async fn quit() -> Json<Value> {
    crate::app::request_quit();
    Json(json!({ "ok": true }))
}

#[derive(Deserialize)]
struct LangBody {
    lang: String,
}

/// Mémorise la langue de l'interface (pour le menu de l'icône de la barre d'état au prochain démarrage).
async fn set_lang(State(cfg): State<Shared>, Json(b): Json<LangBody>) -> ApiResult {
    if !matches!(b.lang.as_str(), "en" | "de" | "fr") {
        return Err((StatusCode::BAD_REQUEST, "unsupported language".into()));
    }
    let mut c = cfg.lock().await;
    if c.lang.as_deref() != Some(b.lang.as_str()) {
        c.lang = Some(b.lang);
        c.save().map_err(err)?;
    }
    Ok(Json(json!({ "ok": true })))
}
