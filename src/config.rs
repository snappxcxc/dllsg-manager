use crate::model::{InstallRecord, Release};
use crate::util;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub schema: u32,
    pub installs: Vec<InstallRecord>,
    pub auto_check_updates: bool,
    pub auto_download: bool,
    pub auto_apply: bool,
    /// Look for installed games when the program starts.
    pub auto_discover: bool,
    /// UI language: "ru" or "en".
    pub lang: String,
    pub last_seen_release: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            schema: 2,
            installs: Vec::new(),
            auto_check_updates: true,
            auto_download: true,
            auto_apply: false,
            auto_discover: true,
            lang: crate::i18n::system_lang().code().to_string(),
            last_seen_release: None,
        }
    }
}

impl Config {
    pub fn load() -> Self {
        let path = util::config_path();
        let Ok(text) = fs::read_to_string(&path) else {
            return Self::default();
        };
        match serde_json::from_str::<Config>(&text) {
            Ok(mut cfg) => {
                cfg.schema = 2;
                if crate::i18n::Lang::from_code(&cfg.lang).is_none() {
                    cfg.lang = crate::i18n::system_lang().code().to_string();
                }
                cfg
            }
            Err(_) => Self::default(),
        }
    }

    /// Language actually used by the UI.
    pub fn lang(&self) -> crate::i18n::Lang {
        crate::i18n::Lang::from_code(&self.lang).unwrap_or_else(crate::i18n::system_lang)
    }

    pub fn save(&self) -> Result<(), String> {
        let path = util::config_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        fs::write(&path, text).map_err(|e| format!("Не записать конфиг: {e}"))
    }

    pub fn upsert_install(&mut self, record: InstallRecord) {
        if let Some(existing) = self
            .installs
            .iter_mut()
            .find(|i| same_path(&i.path, &record.path))
        {
            *existing = record;
        } else {
            self.installs.push(record);
        }
    }

    pub fn remove_install(&mut self, path: &PathBuf) {
        self.installs.retain(|i| !same_path(&i.path, path));
    }

    pub fn install_for(&self, path: &PathBuf) -> Option<&InstallRecord> {
        self.installs.iter().find(|i| same_path(&i.path, path))
    }

    pub fn latest_installed_version(&self) -> Option<String> {
        self.installs
            .iter()
            .map(|i| i.version.clone())
            .max_by(|a, b| {
                if crate::model::version_gt(a, b) {
                    std::cmp::Ordering::Less
                } else if a == b {
                    std::cmp::Ordering::Equal
                } else {
                    std::cmp::Ordering::Greater
                }
            })
    }

    pub fn remember_release(&mut self, release: &Release) {
        self.last_seen_release = Some(release.version());
    }
}

pub fn same_path(a: &PathBuf, b: &PathBuf) -> bool {
    let norm = |p: &PathBuf| {
        p.to_string_lossy()
            .replace('/', "\\")
            .trim_end_matches('\\')
            .to_ascii_lowercase()
    };
    norm(a) == norm(b)
}
