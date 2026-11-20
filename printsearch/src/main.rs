mod browser;
mod config;
mod providers;
mod search;
mod server;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use config::Config;
use providers::{provider, Site};
use search::{search_all, Filters, PriceFilter, Sort};

#[derive(Parser)]
#[command(name = "printsearch", version, about = "Recherche unifiée : MakerWorld, Thingiverse, Printables")]
struct Cli {
    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Rechercher sur un ou plusieurs sites
    Search {
        query: String,
        #[arg(short, long, value_enum, num_args = 1.., value_delimiter = ',')]
        sites: Vec<Site>,
        /// Résultats demandés par site
        #[arg(short, long, default_value_t = 10)]
        limit: usize,
        /// Page de résultats (commence à 1)
        #[arg(long, default_value_t = 1)]
        page: usize,
        #[arg(long, value_enum, default_value_t = PriceFilter::All)]
        price: PriceFilter,
        #[arg(long, value_enum, default_value_t = Sort::Relevance)]
        sort: Sort,
        #[arg(long, default_value_t = 0)]
        min_likes: u64,
        #[arg(long, default_value_t = 0)]
        min_downloads: u64,
    },
    /// Se connecter (Thingiverse : OAuth ou --token ; autres : --token collé depuis le navigateur)
    Login {
        #[arg(value_enum)]
        site: Site,
        #[arg(long)]
        token: Option<String>,
    },
    /// Lancer l'interface web locale (http://localhost:8787). Sans commande : lance ceci et ouvre le navigateur
    Serve {
        /// Ouvrir le navigateur automatiquement
        #[arg(long)]
        open: bool,
    },
    /// Liste les navigateurs compatibles, ou en choisit un : `browser auto|chrome|edge|brave|vivaldi|chromium|<chemin>`
    Browser { choice: Option<String> },
    /// Liker un modèle
    Like {
        #[arg(value_enum)]
        site: Site,
        id: String,
    },
    /// Ajouter un modèle à une collection
    Collect {
        #[arg(value_enum)]
        site: Site,
        collection: String,
        id: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let mut cfg = Config::load()?;
    browser::set_choice(cfg.browser.clone());

    match cli.cmd.unwrap_or(Cmd::Serve { open: true }) {
        Cmd::Search { query, sites, limit, page, price, sort, min_likes, min_downloads } => {
            let sites = if sites.is_empty() { Site::ALL.to_vec() } else { sites };
            let filters = Filters { price, sort, min_likes, min_downloads };
            let out = search_all(&cfg, &sites, &query, limit, page.max(1), &filters).await;
            for (s, e) in &out.errors {
                eprintln!("[{s}] erreur : {e}");
            }
            for m in &out.results {
                let price = match m.price {
                    Some(p) if p > 0.0 => format!("{p}"),
                    Some(_) => "gratuit".into(),
                    None => "?".into(),
                };
                println!(
                    "[{:<11}] {} — {}  ♥{} ⬇{}  prix:{}\n              {}  (id: {})",
                    m.site.key(),
                    m.title,
                    m.author,
                    m.likes.map_or("?".into(), |x| x.to_string()),
                    m.downloads.map_or("?".into(), |x| x.to_string()),
                    price,
                    m.url,
                    m.id
                );
            }
            eprintln!("{} résultat(s)", out.results.len());
        }
        Cmd::Login { site, token } => {
            let t = match (site, token) {
                (_, Some(t)) => t,
                (Site::Thingiverse, None) => providers::thingiverse::oauth_login().await?,
                _ => anyhow::bail!("pour ce site, fournissez --token (voir README)"),
            };
            cfg.tokens.insert(site.key().into(), t);
            cfg.save()?;
            println!("Connecté à {}.", site.key());
        }
        Cmd::Serve { open } => server::run(open).await?,
        Cmd::Browser { choice } => match choice {
            None => {
                let current = cfg.browser.clone();
                println!("Selected: {}", current.as_deref().unwrap_or("auto"));
                for f in browser::detect() {
                    println!("  {:<9} {}  ({})", f.id, f.name, f.path);
                }
                match browser::effective(current.as_deref()) {
                    Some(f) => println!("In use: {} ({})", f.name, f.path),
                    None => println!("No compatible browser found (Chrome, Edge, Brave, Vivaldi, Chromium)."),
                }
            }
            Some(c) => {
                browser::validate_choice(&c)?;
                cfg.browser = if c == "auto" { None } else { Some(c.clone()) };
                cfg.save()?;
                println!("Browser set to {c}.");
            }
        },
        Cmd::Like { site, id } => {
            let t = cfg.token_for(site).context("non connecté : lancez `login` d'abord")?;
            provider(site).like(&id, &t).await?;
            println!("Modèle {id} liké sur {}.", site.key());
        }
        Cmd::Collect { site, collection, id } => {
            let t = cfg.token_for(site).context("non connecté : lancez `login` d'abord")?;
            provider(site).add_to_collection(&collection, &id, &t).await?;
            println!("Modèle {id} ajouté à la collection {collection}.");
        }
    }
    Ok(())
}
