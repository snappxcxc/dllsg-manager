//! Automatic discovery of installed games.
//!
//! Two kinds of sources are combined:
//!
//! * launcher manifests — Steam libraries (`libraryfolders.vdf` + `appmanifest_*.acf`),
//!   Epic Games, GOG Galaxy, Xbox/Game Pass, EA, Ubisoft, Battle.net. These are
//!   trustworthy: every entry is a game, so any folder with the game exe is listed.
//! * the Windows "installed programs" registry — every entry is searched, but only
//!   folders that actually contain `nvngx_dlss*.dll` are reported, so tools and
//!   drivers do not pollute the list.
//!
//! For each game the renderer folder is located — the one that holds the game exe
//! next to `nvngx_dlssg.dll` when possible — and the exe icon is cached for the UI.

use crate::icons;
use crate::installer;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Debug, Clone)]
pub struct FoundGame {
    pub name: String,
    /// Folder that contains the game exe (what the installer needs).
    pub dir: PathBuf,
    pub exe: String,
    pub exe_size: u64,
    pub source: String,
    /// `nvngx_dlssg.dll` sits next to the exe: the mod has something to replace.
    pub dlssg: bool,
    /// Only `nvngx_dlss.dll` (DLSS 2, no frame generation) was found.
    pub has_dlss: bool,
    pub installed_version: Option<String>,
    /// Cached raw-RGBA icon file (`icons::load`).
    pub icon: Option<PathBuf>,
}

impl FoundGame {
    pub fn exe_path(&self) -> PathBuf {
        self.dir.join(&self.exe)
    }

    /// `D:` for `D:\SteamLibrary\...` — helps telling two copies of a game apart.
    pub fn drive(&self) -> String {
        let text = self.dir.to_string_lossy();
        match text.find(':') {
            Some(pos) if pos <= 2 => text[..pos].to_ascii_uppercase(),
            _ => String::new(),
        }
    }
}

struct Root {
    name: String,
    path: PathBuf,
    source: &'static str,
    /// From a launcher manifest: no need for DLSS evidence.
    trusted: bool,
}

struct Hit {
    dir: PathBuf,
    exe: String,
    size: u64,
    dlssg: bool,
    has_dlss: bool,
    score: i32,
}

const MAX_DEPTH: usize = 5;
const DIR_BUDGET_TRUSTED: usize = 6000;
const DIR_BUDGET_OTHER: usize = 600;
/// Deep search for entries that are not known games (registry lookups).
const MAX_DEPTH_OTHER: usize = 3;

/// Folders that never hold the renderer exe and can be huge.
const SKIP_DIRS: &[&str] = &[
    "content",
    "data",
    "paks",
    "pak",
    "audio",
    "sound",
    "sounds",
    "movies",
    "video",
    "videos",
    "localization",
    "loc",
    "redist",
    "redistributable",
    "redistributables",
    "vc_redist",
    "_commonredist",
    "installer",
    "installers",
    "install",
    "directx",
    "vcredist",
    "support",
    "crashpad",
    "crashreport",
    "help",
    "docs",
    "documentation",
    "manual",
    "screenshots",
    "cache",
    "shadercache",
    "temp",
    "tempfiles",
    "workshop",
    "html",
    "web",
    "deeplinking",
    "streamingassets",
    "plugins",
    "thirdparty",
    "third_party",
    "resources",
    "managed",
    "mono",
    "il2cpp",
    "__pycache__",
    "dlssg_sm86",
    ".dlssg-manager",
    "savegames",
    "saves",
    "logs",
];

/// Folder names that usually contain the renderer exe.
const RENDERER_DIRS: &[&str] = &[
    "win64",
    "win32",
    "binaries",
    "binary",
    "bin",
    "x64",
    "x86",
    "x64_dx12",
    "x64_dx11",
    "retail",
    "system",
    "game",
    "release",
];

/// Standard renderer locations relative to a game root, in probe order.
const LAYOUTS: &[&str] = &[
    "",
    "bin",
    "bin\\x64",
    "bin\\win64",
    "bin\\win32",
    "bin\\x64_dx12",
    "binaries",
    "binaries\\win64",
    "binaries\\win32",
    "x64",
    "win64",
    "win32",
    "binary",
    "retail",
    "system",
    "game",
    "game\\bin",
    "game\\bin\\win64",
    "bin\\x86",
];

/// Exe names that are never the game itself.
const BAD_EXE: &[&str] = &[
    "unins",
    "setup",
    "install",
    "update",
    "updater",
    "vcredist",
    "dxsetup",
    "dxwebsetup",
    "crash",
    "report",
    "helper",
    "eaclauncher",
    "easyanticheat",
    "start_protected",
    "unitycrashhandler",
    "benchmark",
    "launcher",
    "server",
    "dedicated",
    "editor",
    "register",
    "config",
    "settings",
    "tool",
    "activation",
    "cleanup",
];

/// Launcher-free games are recognised only by the NVIDIA upscaler DLLs, so the
/// list stays free of ordinary applications.
pub fn discover(on_progress: &mut dyn FnMut(f32, String), cancel: &AtomicBool) -> Vec<FoundGame> {
    on_progress(0.01, "Поиск установленных игр…".into());
    let mut roots = collect_roots();
    dedupe_roots(&mut roots);

    let total = roots.len().max(1) as f32;
    let mut games: Vec<FoundGame> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    for (i, root) in roots.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        on_progress(
            0.02 + 0.86 * (i as f32 / total),
            format!("Сканирую: {}", root.name),
        );
        let Some(hit) = find_game_dir(&root.path, root.trusted, cancel) else {
            continue;
        };
        let key = format!(
            "{}\\{}",
            hit.dir.to_string_lossy().to_ascii_lowercase(),
            hit.exe
        );
        if !seen.insert(key) {
            continue;
        }
        let stem = hit.exe.to_ascii_lowercase();
        let display = if root.name.trim().is_empty() {
            folder_name(&hit.dir)
        } else if is_generic_name(&root.name) {
            prettify_exe(&stem)
        } else {
            root.name.clone()
        };
        let installed = installer::detect_install(&hit.dir).map(|r| r.version);
        games.push(FoundGame {
            name: display,
            dir: hit.dir,
            exe: hit.exe,
            exe_size: hit.size,
            source: root.source.to_string(),
            dlssg: hit.dlssg,
            has_dlss: hit.has_dlss,
            installed_version: installed,
            icon: None,
        });
    }

    let count = games.len().max(1) as f32;
    for (i, game) in games.iter_mut().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        on_progress(
            0.88 + 0.12 * (i as f32 / count),
            format!("Иконка: {}", game.name),
        );
        game.icon = icons::ensure_icon(&game.exe_path());
    }

    games.sort_by(|a, b| {
        b.dlssg
            .cmp(&a.dlssg)
            .then(a.name.to_ascii_lowercase().cmp(&b.name.to_ascii_lowercase()))
    });
    on_progress(1.0, format!("Найдено игр: {}", games.len()));
    games
}

// ---------------------------------------------------------------- sources ---

fn collect_roots() -> Vec<Root> {
    let mut roots = Vec::new();
    roots.extend(steam_roots());
    roots.extend(epic_roots());
    roots.extend(gog_roots());
    roots.extend(xbox_roots());
    roots.extend(ea_roots());
    roots.extend(ubisoft_roots());
    roots.extend(blizzard_roots());
    roots.extend(uninstall_roots());
    roots
}

fn dedupe_roots(roots: &mut Vec<Root>) {
    let mut seen: HashSet<String> = HashSet::new();
    roots.retain(|r| {
        if !r.path.is_dir() {
            return false;
        }
        seen.insert(norm_path(&r.path))
    });
    // An entry that lives inside an already known game folder is a leftover.
    let all: Vec<PathBuf> = roots.iter().map(|r| r.path.clone()).collect();
    roots.retain(|r| {
        !all.iter()
            .any(|p| !same_dir(p, &r.path) && starts_with_dir(&r.path, p))
    });
}

fn steam_roots() -> Vec<Root> {
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ};
    use winreg::RegKey;

    let mut libs: Vec<PathBuf> = Vec::new();
    for (hive, key_path) in [
        (HKEY_CURRENT_USER, r"Software\Valve\Steam"),
        (HKEY_LOCAL_MACHINE, r"SOFTWARE\WOW6432Node\Valve\Steam"),
    ] {
        let Ok(key) = RegKey::predef(hive).open_subkey_with_flags(key_path, KEY_READ) else {
            continue;
        };
        for value in ["SteamPath", "InstallPath"] {
            if let Ok(path) = key.get_value::<String, _>(value) {
                if !path.trim().is_empty() {
                    libs.push(PathBuf::from(path.trim().replace('/', "\\")));
                }
            }
        }
    }

    // Every library lists the others in libraryfolders.vdf.
    let mut i = 0;
    while i < libs.len() && libs.len() < 32 {
        let lib = libs[i].clone();
        i += 1;
        for vdf in [
            lib.join("steamapps").join("libraryfolders.vdf"),
            lib.join("config").join("libraryfolders.vdf"),
        ] {
            for extra in vdf_library_paths(&vdf) {
                if !libs.iter().any(|l| same_dir(l, &extra)) {
                    libs.push(extra);
                }
            }
        }
    }

    let mut out = Vec::new();
    for lib in libs {
        let steamapps = lib.join("steamapps");
        let names = acf_names(&steamapps);
        for dir in read_dirs(&steamapps.join("common")) {
            let folder = folder_name(&dir);
            if is_steam_tool(&folder) {
                continue;
            }
            let name = names
                .get(&folder.to_ascii_lowercase())
                .cloned()
                .unwrap_or_else(|| folder.clone());
            out.push(Root {
                name,
                path: dir,
                source: "Steam",
                trusted: true,
            });
        }
    }
    out
}

/// Steam library folders that are tools, not games.
fn is_steam_tool(folder: &str) -> bool {
    let lower = folder.to_ascii_lowercase();
    [
        "steamworks",
        "steam controller",
        "steamvr",
        "steam linux runtime",
        "proton",
        "steam sdk",
        "steam library",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

fn epic_roots() -> Vec<Root> {
    let base = std::env::var_os("ProgramData")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\ProgramData"));
    let dir = base
        .join("Epic")
        .join("EpicGamesLauncher")
        .join("Data")
        .join("Manifests");
    let mut out = Vec::new();
    for file in read_files_with_ext(&dir, "item") {
        let Ok(text) = fs::read_to_string(&file) else {
            continue;
        };
        let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) else {
            continue;
        };
        let Some(loc) = json.get("InstallLocation").and_then(|v| v.as_str()) else {
            continue;
        };
        if loc.trim().is_empty() {
            continue;
        }
        let path = PathBuf::from(loc.trim().replace('/', "\\"));
        if !path.is_dir() {
            continue;
        }
        let name = json
            .get("DisplayName")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        out.push(Root {
            name: if name.trim().is_empty() {
                folder_name(&path)
            } else {
                name
            },
            path,
            source: "Epic Games",
            trusted: true,
        });
    }
    out
}

fn gog_roots() -> Vec<Root> {
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ};
    use winreg::RegKey;

    let mut out = Vec::new();
    for (hive, key_path) in [
        (
            HKEY_LOCAL_MACHINE,
            r"SOFTWARE\WOW6432Node\GOG.com\Games",
        ),
        (HKEY_CURRENT_USER, r"SOFTWARE\GOG.com\Games"),
    ] {
        let Ok(key) = RegKey::predef(hive).open_subkey_with_flags(key_path, KEY_READ) else {
            continue;
        };
        for id in key.enum_keys().flatten() {
            let Ok(game) = key.open_subkey_with_flags(&id, KEY_READ) else {
                continue;
            };
            let path: String = game.get_value("path").unwrap_or_default();
            if path.trim().is_empty() {
                continue;
            }
            let name: String = game.get_value("gameName").unwrap_or_default();
            let path = PathBuf::from(path.trim());
            out.push(Root {
                name: if name.trim().is_empty() {
                    folder_name(&path)
                } else {
                    name
                },
                path,
                source: "GOG",
                trusted: true,
            });
        }
    }
    out
}

fn xbox_roots() -> Vec<Root> {
    let mut out = Vec::new();
    for letter in b'A'..=b'Z' {
        let dir = PathBuf::from(format!("{}:\\XboxGames", letter as char));
        if !dir.is_dir() {
            continue;
        }
        for game in read_dirs(&dir) {
            let content = game.join("Content");
            if !content.is_dir() {
                continue;
            }
            out.push(Root {
                name: folder_name(&game),
                path: content,
                source: "Xbox",
                trusted: true,
            });
        }
    }
    out
}

fn ea_roots() -> Vec<Root> {
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ};
    use winreg::RegKey;

    let mut out = Vec::new();
    for (hive, key_path) in [
        (
            HKEY_LOCAL_MACHINE,
            r"SOFTWARE\WOW6432Node\Electronic Arts\EA Games",
        ),
        (HKEY_LOCAL_MACHINE, r"SOFTWARE\Electronic Arts\EA Games"),
        (HKEY_CURRENT_USER, r"SOFTWARE\Electronic Arts\EA Games"),
    ] {
        let Ok(key) = RegKey::predef(hive).open_subkey_with_flags(key_path, KEY_READ) else {
            continue;
        };
        for id in key.enum_keys().flatten() {
            let Ok(game) = key.open_subkey_with_flags(&id, KEY_READ) else {
                continue;
            };
            let path = ["Install Dir", "InstallLocation", "Install Dir"]
                .iter()
                .find_map(|v| game.get_value::<String, _>(v).ok())
                .unwrap_or_default();
            if path.trim().is_empty() {
                continue;
            }
            out.push(Root {
                name: id.replace('_', " "),
                path: PathBuf::from(path.trim().replace('/', "\\")),
                source: "EA",
                trusted: true,
            });
        }
    }
    out
}

fn ubisoft_roots() -> Vec<Root> {
    use winreg::enums::{HKEY_LOCAL_MACHINE, KEY_READ};
    use winreg::RegKey;

    let mut out = Vec::new();
    let key_path = r"SOFTWARE\WOW6432Node\Ubisoft\Launcher\Installs";
    let Ok(key) = RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey_with_flags(key_path, KEY_READ)
    else {
        return out;
    };
    for id in key.enum_keys().flatten() {
        let Ok(game) = key.open_subkey_with_flags(&id, KEY_READ) else {
            continue;
        };
        let path: String = game.get_value("InstallDir").unwrap_or_default();
        if path.trim().is_empty() {
            continue;
        }
        out.push(Root {
            name: folder_name(Path::new(path.trim())),
            path: PathBuf::from(path.trim().replace('/', "\\")),
            source: "Ubisoft",
            trusted: true,
        });
    }
    out
}

fn blizzard_roots() -> Vec<Root> {
    use winreg::enums::{HKEY_LOCAL_MACHINE, KEY_READ};
    use winreg::RegKey;

    let mut out = Vec::new();
    for key_path in [
        r"SOFTWARE\WOW6432Node\Blizzard Entertainment",
        r"SOFTWARE\Blizzard Entertainment",
    ] {
        let Ok(key) = RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey_with_flags(key_path, KEY_READ)
        else {
            continue;
        };
        for id in key.enum_keys().flatten() {
            let Ok(game) = key.open_subkey_with_flags(&id, KEY_READ) else {
                continue;
            };
            let path: String = game.get_value("InstallPath").unwrap_or_default();
            if path.trim().is_empty() {
                continue;
            }
            out.push(Root {
                name: id,
                path: PathBuf::from(path.trim().replace('/', "\\")),
                source: "Battle.net",
                trusted: true,
            });
        }
    }
    out
}

fn uninstall_roots() -> Vec<Root> {
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ};
    use winreg::RegKey;

    const BAD_NAMES: &[&str] = &[
        "microsoft",
        "visual c++",
        "redistributable",
        "runtime",
        "driver",
        "nvidia",
        "amd software",
        "intel",
        ".net",
        "sdk",
        "hotfix",
        "update for",
        "python",
        "java",
        "node.js",
        "git",
        "office",
        "adobe",
        "cheat engine",
        "cpu-z",
        "aida64",
        "hwinfo",
        "crystaldisk",
        "easeus",
        "latencymon",
        "7-zip",
        "winrar",
        "obs studio",
        "discord",
        "teams",
        "zoom",
        "anydesk",
        "chrome",
        "firefox",
        "edge",
        "steamworks shared",
    ];

    let mut out: Vec<Root> = Vec::new();
    for (hive, key_path) in [
        (
            HKEY_LOCAL_MACHINE,
            r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall",
        ),
        (
            HKEY_LOCAL_MACHINE,
            r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall",
        ),
        (
            HKEY_CURRENT_USER,
            r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall",
        ),
    ] {
        let Ok(key) = RegKey::predef(hive).open_subkey_with_flags(key_path, KEY_READ) else {
            continue;
        };
        for id in key.enum_keys().flatten() {
            let Ok(app) = key.open_subkey_with_flags(&id, KEY_READ) else {
                continue;
            };
            let system: u32 = app.get_value("SystemComponent").unwrap_or(0);
            if system == 1 {
                continue;
            }
            if app.get_value::<String, _>("ParentKeyName").is_ok() {
                continue; // patch for something else
            }
            let location: String = app.get_value("InstallLocation").unwrap_or_default();
            let path = PathBuf::from(location.trim().trim_matches('"').replace('/', "\\"));
            if location.trim().is_empty() || !path.is_dir() {
                continue;
            }
            let name: String = app.get_value("DisplayName").unwrap_or_default();
            let lower = name.to_ascii_lowercase();
            let loc = norm_path(&path);
            if BAD_NAMES.iter().any(|bad| lower.contains(bad)) {
                continue;
            }
            if loc.contains("\\windows\\") || loc.contains("\\programdata\\") || loc.contains("\\appdata\\") {
                continue;
            }
            if out.len() >= 120 {
                break;
            }
            out.push(Root {
                name: if name.trim().is_empty() {
                    folder_name(&path)
                } else {
                    name
                },
                path,
                source: "Установленные программы",
                trusted: false,
            });
        }
    }
    out
}

// ------------------------------------------------------------- inspection ---

struct DirScan {
    exes: Vec<(String, u64)>,
    dirs: Vec<PathBuf>,
    dlssg: bool,
    dlss: bool,
    steam: bool,
    gog: bool,
    epic: bool,
}

fn scan_dir(dir: &Path) -> Option<DirScan> {
    let entries = fs::read_dir(dir).ok()?;
    let mut scan = DirScan {
        exes: Vec::new(),
        dirs: Vec::new(),
        dlssg: false,
        dlss: false,
        steam: false,
        gog: false,
        epic: false,
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        let lower = name.to_ascii_lowercase();
        let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
        if is_dir {
            if !SKIP_DIRS.contains(&lower.as_str()) {
                scan.dirs.push(entry.path());
            }
            continue;
        }
        match lower.as_str() {
            "nvngx_dlssg.dll" => scan.dlssg = true,
            "nvngx_dlss.dll" => scan.dlss = true,
            "steam_api64.dll" | "steam_api.dll" => scan.steam = true,
            "galaxy64.dll" | "galaxy.dll" | "goggame.info" => scan.gog = true,
            "eossdk-win64-shipping.dll" => scan.epic = true,
            _ => {}
        }
        if lower.ends_with(".exe") && !BAD_EXE.iter().any(|bad| lower.contains(bad)) {
            let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
            // Some games (Unity, Source 2) keep the real exe tiny, so only
            // stub-sized launchers are rejected here.
            if size >= 64 * 1024 {
                scan.exes.push((name, size));
            }
        }
    }
    Some(scan)
}

fn alnum_lower(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase()
}

/// Largest exe, but an exe whose name matches the game folder wins.
fn pick_exe(exes: &[(String, u64)], dir_name: &str, root_name: &str) -> Option<(String, u64)> {
    let dir_key = alnum_lower(dir_name);
    let root_key = alnum_lower(root_name);
    let affinity = |name: &str| -> i32 {
        let stem = alnum_lower(name.trim_end_matches(".exe").trim_end_matches(".EXE"));
        let mut score = 0;
        if !root_key.is_empty() && stem.len() >= 4 && (stem.contains(&root_key) || root_key.contains(&stem)) {
            score += 4;
        }
        if !dir_key.is_empty() && dir_key.len() >= 4 && stem.contains(&dir_key) {
            score += 2;
        }
        score
    };
    exes.iter()
        .max_by(|a, b| {
            affinity(&a.0)
                .cmp(&affinity(&b.0))
                .then(a.1.cmp(&b.1))
        })
        .cloned()
}

fn make_hit(dir: &Path, scan: &DirScan, depth: usize, root_name: &str) -> Option<Hit> {
    let dir_name = folder_name(dir);
    let (exe, size) = pick_exe(&scan.exes, &dir_name, root_name)?;
    let exe_lower = exe.to_ascii_lowercase();
    let dir_key = dir_name.to_ascii_lowercase();
    let mut score = 0;
    if scan.dlssg {
        score += 120;
    }
    if scan.dlss {
        score += 50;
    }
    if scan.steam || scan.gog {
        score += 30;
    }
    if scan.epic {
        score += 20;
    }
    // Unreal Engine names the shipping build of a game `*-Win64-Shipping.exe`.
    if exe_lower.contains("-shipping") {
        score += 40;
    }
    if RENDERER_DIRS.contains(&dir_key.as_str()) {
        score += 30;
    }
    if depth == 0 {
        score += 10;
    }
    if size > 20 * 1024 * 1024 {
        score += 15;
    } else if size > 5 * 1024 * 1024 {
        score += 8;
    } else if size < 256 * 1024 {
        score -= 10;
    }
    Some(Hit {
        dir: dir.to_path_buf(),
        exe,
        size,
        dlssg: scan.dlssg,
        has_dlss: scan.dlss || scan.dlssg,
        score,
    })
}

/// Fast path: the handful of folders where games usually keep the renderer exe.
fn probe_layouts(root: &Path, root_name: &str) -> Option<Hit> {
    let mut candidates: Vec<PathBuf> = LAYOUTS
        .iter()
        .map(|rel| if rel.is_empty() { root.to_path_buf() } else { root.join(rel) })
        .collect();
    for sub in read_dirs(root).into_iter().take(16) {
        let sub_name = folder_name(&sub).to_ascii_lowercase();
        if SKIP_DIRS.contains(&sub_name.as_str()) {
            continue;
        }
        for rel in ["binaries\\win64", "bin\\x64", "bin\\win64", "bin", "win64"] {
            candidates.push(sub.join(rel));
        }
    }

    let mut best: Option<Hit> = None;
    for dir in candidates {
        if !dir.is_dir() {
            continue;
        }
        let Some(scan) = scan_dir(&dir) else {
            continue;
        };
        if scan.exes.is_empty() {
            continue;
        }
        let depth = depth_from_root(root, &dir);
        let Some(hit) = make_hit(&dir, &scan, depth, root_name) else {
            continue;
        };
        let better = best
            .as_ref()
            .map(|current| hit.score > current.score || (hit.score == current.score && hit.size > current.size))
            .unwrap_or(true);
        // The renderer folder with nvngx_dlssg.dll next to the exe is decisive.
        if hit.dlssg {
            return Some(hit);
        }
        if better {
            best = Some(hit);
        }
    }
    best
}

fn depth_from_root(root: &Path, dir: &Path) -> usize {
    dir.strip_prefix(root)
        .map(|rel| rel.components().count())
        .unwrap_or(0)
}

fn find_game_dir(root: &Path, trusted: bool, cancel: &AtomicBool) -> Option<Hit> {
    let root_name = folder_name(root);
    let min_score = if trusted { 0 } else { 50 };
    // 1) Cheap: standard layout, no full tree walk.
    if let Some(hit) = probe_layouts(root, &root_name) {
        if hit.score >= min_score {
            return Some(hit);
        }
    }
    // 2) Bounded search for unusual layouts.
    let max_depth = if trusted { MAX_DEPTH } else { MAX_DEPTH_OTHER };
    let budget = if trusted {
        DIR_BUDGET_TRUSTED
    } else {
        DIR_BUDGET_OTHER
    };
    let mut queue: Vec<(PathBuf, usize)> = vec![(root.to_path_buf(), 0)];
    let mut visited = 0usize;
    let mut best: Option<Hit> = None;

    while let Some((dir, depth)) = queue.pop() {
        if visited >= budget || cancel.load(Ordering::Relaxed) {
            break;
        }
        visited += 1;
        let Some(scan) = scan_dir(&dir) else {
            continue;
        };
        if depth < max_depth {
            queue.extend(scan.dirs.iter().map(|d| (d.clone(), depth + 1)));
        }
        if scan.exes.is_empty() {
            continue;
        }
        let Some(hit) = make_hit(&dir, &scan, depth, &root_name) else {
            continue;
        };
        if best.as_ref().map(|b| hit.score > b.score).unwrap_or(true) {
            best = Some(hit);
        }
        if scan.dlssg {
            break; // nothing beats the FG dll sitting next to an exe
        }
    }

    best.filter(|hit| hit.score >= min_score)
}

// ------------------------------------------------------------------ utils ---

fn read_dirs(path: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(path) else {
        return Vec::new();
    };
    let mut out: Vec<PathBuf> = entries
        .flatten()
        .filter(|e| e.path().is_dir())
        .map(|e| e.path())
        .collect();
    out.sort();
    out
}

fn read_files_with_ext(dir: &Path, ext: &str) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<PathBuf> = entries
        .flatten()
        .filter(|e| {
            e.path()
                .extension()
                .map(|x| x.eq_ignore_ascii_case(ext))
                .unwrap_or(false)
        })
        .map(|e| e.path())
        .collect();
    out.sort();
    out
}

fn folder_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default()
}

pub fn norm_path(path: &Path) -> String {
    path.to_string_lossy()
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_ascii_lowercase()
}

fn same_dir(a: &Path, b: &Path) -> bool {
    norm_path(a) == norm_path(b)
}

fn starts_with_dir(path: &Path, parent: &Path) -> bool {
    let (path, parent) = (norm_path(path), norm_path(parent));
    path.len() > parent.len() && path.starts_with(&parent) && path.as_bytes()[parent.len()] == b'\\'
}

fn is_generic_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    matches!(lower.as_str(), "" | "game" | "bin" | "win64" | "win32" | "x64" | "binaries")
}

/// `oakgame-winshipping` -> `Oakgame Winshipping` for unnamed registry entry points.
fn prettify_exe(stem: &str) -> String {
    let base = stem.trim_end_matches(".exe");
    let words: Vec<String> = base
        .split(['-', '_', '.'])
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut chars = w.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect();
    if words.is_empty() {
        base.to_string()
    } else {
        words.join(" ")
    }
}

/// Pulls `"path" "D:\\SteamLibrary"` pairs out of a Valve libraryfolders.vdf.
fn vdf_library_paths(path: &Path) -> Vec<PathBuf> {
    let Ok(text) = fs::read_to_string(path) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for line in text.lines() {
        let quoted = quoted_values(line);
        if quoted.len() < 2 {
            continue;
        }
        if !quoted[0].eq_ignore_ascii_case("path") {
            continue;
        }
        let value = quoted[1].replace("\\\\", "\\");
        if value.len() > 2 {
            out.push(PathBuf::from(value));
        }
    }
    out
}

/// `installdir` -> game name, from the `appmanifest_*.acf` files of one library.
fn acf_names(steamapps: &Path) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for file in read_files_with_ext(steamapps, "acf") {
        let Ok(text) = fs::read_to_string(&file) else {
            continue;
        };
        let mut name = String::new();
        let mut install = String::new();
        for line in text.lines() {
            let quoted = quoted_values(line);
            if quoted.len() < 2 {
                continue;
            }
            match quoted[0].to_ascii_lowercase().as_str() {
                "name" => name = quoted[1].clone(),
                "installdir" => install = quoted[1].clone(),
                _ => {}
            }
        }
        if !install.is_empty() && !name.is_empty() {
            map.insert(install.to_ascii_lowercase(), name);
        }
    }
    map
}

fn quoted_values(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut chars = line.chars();
    while let Some(ch) = chars.next() {
        if ch == '"' {
            let mut value = String::new();
            for c in chars.by_ref() {
                if c == '"' {
                    break;
                }
                value.push(c);
            }
            out.push(value);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_vdf_lines() {
        let line = "\t\t\t\"path\"\t\t\"B:\\\\SteamLibrary\"";
        let values = quoted_values(line);
        assert_eq!(values, vec!["path".to_string(), "B:\\\\SteamLibrary".into()]);
        assert_eq!(values[1].replace("\\\\", "\\"), "B:\\SteamLibrary");
    }

    #[test]
    fn generic_names_are_replaced() {
        assert!(is_generic_name("Win64"));
        assert!(!is_generic_name("Cyberpunk 2077"));
        assert_eq!(prettify_exe("oakgame-winshipping"), "Oakgame Winshipping");
    }

    #[test]
    fn detects_cyberpunk_renderer_dir() {
        let root = Path::new("D:/SteamLibrary/steamapps/common/Cyberpunk 2077");
        if !root.is_dir() {
            return;
        }
        let cancel = AtomicBool::new(false);
        let hit = find_game_dir(root, true, &cancel).expect("game dir");
        assert!(hit.dlssg);
        assert!(hit.exe.to_ascii_lowercase().starts_with("cyberpunk"));
    }
}
