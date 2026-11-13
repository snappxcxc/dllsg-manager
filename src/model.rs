use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Proxy DLL names supported by the upstream project, in the recommended
/// trial order. Exactly one of them may sit next to the game exe.
pub const PROXIES: [&str; 6] = [
    "version.dll",
    "winmm.dll",
    "dbghelp.dll",
    "dinput8.dll",
    "dxgi.dll",
    "d3d12.dll",
];

/// Marker string embedded in every proxy DLL of the project. Used to tell our
/// proxies apart from a foreign `version.dll` that must be backed up instead of
/// deleted.
pub const PROXY_MARKER: &[u8] = b"dlssg_sm86";

pub const MARKER_DIR: &str = ".dlssg-manager";
pub const MARKER_FILE: &str = "install.json";
pub const INI_NAME: &str = "dlssg_sm86.ini";
pub const PAYLOAD_DIR: &str = "dlssg_sm86";

pub const DEFAULT_REPO: &str = "sdli1995/dlssg_for_sm86";
pub const DRIVER_FLOOR: u32 = 580;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Level {
    Ok,
    Warn,
    Bad,
    Info,
}

impl Level {
    pub fn label(self) -> &'static str {
        match self {
            Level::Ok => "OK",
            Level::Warn => crate::i18n::t("ВНИМАНИЕ"),
            Level::Bad => crate::i18n::t("ПРОБЛЕМА"),
            Level::Info => crate::i18n::t("ИНФО"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Check {
    pub title: String,
    pub detail: String,
    pub level: Level,
}

impl Check {
    pub fn new(level: Level, title: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            detail: detail.into(),
            level,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Ready,
    Attention,
    Unsupported,
}

impl Verdict {
    pub fn title(self) -> &'static str {
        match self {
            Verdict::Ready => crate::i18n::t("Можно устанавливать"),
            Verdict::Attention => crate::i18n::t("Можно, но есть замечания"),
            Verdict::Unsupported => crate::i18n::t("Установка не рекомендуется"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ExeInfo {
    pub path: PathBuf,
    pub name: String,
    pub size: u64,
    pub imports_d3d12: bool,
    pub imports_dxgi: bool,
    pub has_dlssg: bool,
    pub score: i32,
}

#[derive(Debug, Clone, Default)]
pub struct GameScan {
    pub dir: PathBuf,
    pub exes: Vec<ExeInfo>,
    pub selected: usize,
    pub checks: Vec<Check>,
    pub verdict: Verdict,
    pub installed: Option<InstallRecord>,
    pub dlssg_dll: Option<PathBuf>,
    pub sr_dll: Option<PathBuf>,
    pub conflicts: Vec<String>,
    pub our_proxies: Vec<String>,
    pub running: Vec<String>,
    pub writable: bool,
    pub d3d12: bool,
}

impl Default for Verdict {
    fn default() -> Self {
        Verdict::Attention
    }
}

impl GameScan {
    pub fn selected_exe(&self) -> Option<&ExeInfo> {
        self.exes.get(self.selected)
    }

    pub fn bad_count(&self) -> usize {
        self.checks.iter().filter(|c| c.level == Level::Bad).count()
    }

    pub fn warn_count(&self) -> usize {
        self.checks.iter().filter(|c| c.level == Level::Warn).count()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PackageFile {
    pub path: String,
    pub size: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Package {
    pub version: String,
    /// Root folder of the extracted package (contains `version.dll`).
    pub dir: PathBuf,
    pub files: Vec<PackageFile>,
    pub source: String,
}

impl Package {
    pub fn payload_bytes(&self) -> u64 {
        self.files.iter().map(|f| f.size).sum()
    }

    pub fn has_file(&self, rel: &str) -> bool {
        self.files.iter().any(|f| f.path.eq_ignore_ascii_case(rel))
    }

    pub fn sha_of(&self, rel: &str) -> Option<&str> {
        self.files
            .iter()
            .find(|f| f.path.eq_ignore_ascii_case(rel))
            .map(|f| f.sha256.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Release {
    pub tag: String,
    pub name: String,
    pub asset_name: String,
    pub asset_url: String,
    pub size: u64,
    pub published: String,
    pub notes: String,
}

impl Release {
    pub fn version(&self) -> String {
        normalize_version(&self.tag)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InstallRecord {
    /// Folder that contains the renderer exe.
    pub path: PathBuf,
    /// Exe file name used for the install (for the "is it running" check).
    pub exe: String,
    pub proxy: String,
    pub version: String,
    pub installed_at: String,
}

/// `v0.4.1` / `0.4.1-x64` / `0.4.1` -> `0.4.1`
pub fn normalize_version(raw: &str) -> String {
    let s = raw.trim().trim_start_matches(['v', 'V']);
    let cut = s
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(s.len());
    s[..cut].trim_end_matches('.').to_string()
}

/// Numeric comparison of dotted versions: `0.4.10` > `0.4.9`.
pub fn version_gt(a: &str, b: &str) -> bool {
    fn parts(v: &str) -> Vec<u64> {
        normalize_version(v)
            .split('.')
            .map(|p| p.parse::<u64>().unwrap_or(0))
            .collect()
    }
    let (a, b) = (parts(a), parts(b));
    for i in 0..a.len().max(b.len()) {
        let x = *a.get(i).unwrap_or(&0);
        let y = *b.get(i).unwrap_or(&0);
        if x != y {
            return x > y;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions() {
        assert_eq!(normalize_version("v0.4.1-x64"), "0.4.1");
        assert!(version_gt("0.4.10", "0.4.9"));
        assert!(!version_gt("0.4.1", "0.4.1"));
        assert!(version_gt("0.5", "0.4.9"));
    }
}
