use anyhow::{Context, Result};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fs, path::PathBuf};

/// Jetons d'authentification stockés par site (clé = "thingiverse", "printables", "makerworld").
#[derive(Serialize, Deserialize, Default, Clone)]
pub struct Config {
    pub tokens: BTreeMap<String, String>,
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

    pub fn load() -> Result<Self> {
        let p = path()?;
        if !p.exists() {
            return Ok(Self::default());
        }
        Ok(serde_json::from_str(&fs::read_to_string(p)?)?)
    }

    pub fn save(&self) -> Result<()> {
        let p = path()?;
        fs::write(&p, serde_json::to_string_pretty(self)?)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&p, fs::Permissions::from_mode(0o600))?; // jetons = secrets
        }
        Ok(())
    }
}
