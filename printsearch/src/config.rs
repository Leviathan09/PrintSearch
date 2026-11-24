use anyhow::{Context, Result};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fs, path::PathBuf};

/// Réglages enregistrés : jetons par site ("thingiverse", "printables", "makerworld"; les autres sites n'ont pas de compte)
/// et navigateur utilisé pour la lecture des pages ("auto", un identifiant ou un chemin).
#[derive(Serialize, Deserialize, Default, Clone)]
pub struct Config {
    #[serde(default)]
    pub tokens: BTreeMap<String, String>,
    #[serde(default)]
    pub browser: Option<String>,
    /// Langue de l'interface ("en" ou "fr"), utilisée pour le menu de l'icône de la barre d'état
    #[serde(default)]
    pub lang: Option<String>,
}

fn path() -> Result<PathBuf> {
    let dirs = ProjectDirs::from("dev", "printsearch", "printsearch").context("dossier de config introuvable")?;
    fs::create_dir_all(dirs.config_dir())?;
    Ok(dirs.config_dir().join("config.json"))
}

impl Config {
    pub fn token_for(&self, site: crate::providers::Site) -> Option<String> {
        use crate::providers::Site;
        self.tokens.get(site.key()).cloned().or_else(|| match site {
            Site::Thingiverse => std::env::var("THINGIVERSE_APP_TOKEN").ok(),
            _ => None,
        })
    }

    /// Charge la configuration. Un fichier illisible ou corrompu est mis de côté (`config.json.bad`)
    /// et l'application démarre avec une configuration vide, au lieu de refuser de démarrer.
    pub fn load() -> Self {
        let Ok(p) = path() else { return Self::default() };
        if !p.exists() {
            return Self::default();
        }
        match fs::read_to_string(&p).map_err(anyhow::Error::from).and_then(|t| serde_json::from_str::<Self>(&t).map_err(anyhow::Error::from)) {
            Ok(c) => c,
            Err(e) => {
                let bad = p.with_extension("json.bad");
                let _ = fs::rename(&p, &bad);
                crate::log::log_warn!("config.json unreadable ({e}); moved to {} and starting with defaults", bad.display());
                Self::default()
            }
        }
    }

    /// Écriture atomique (fichier temporaire puis renommage) : pas de config à moitié écrite.
    pub fn save(&self) -> Result<()> {
        let p = path()?;
        let tmp = p.with_extension("json.tmp");
        fs::write(&tmp, serde_json::to_string_pretty(self)?)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&tmp, fs::Permissions::from_mode(0o600))?; // jetons = secrets
        }
        fs::rename(&tmp, &p)?;
        Ok(())
    }
}
