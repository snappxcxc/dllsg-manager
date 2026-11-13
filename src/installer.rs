use crate::model::{
    InstallRecord, Package, INI_NAME, MARKER_DIR, MARKER_FILE, PAYLOAD_DIR, PROXY_MARKER, PROXIES,
};
use crate::util;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Marker {
    version: String,
    proxy: String,
    exe: String,
    installed_at: String,
    /// Folder (relative to the game dir) with the files we displaced.
    #[serde(default)]
    backup: Option<String>,
    #[serde(default)]
    backed_up: Vec<String>,
}

pub fn is_our_proxy(path: &Path) -> bool {
    path.is_file() && util::file_contains(path, PROXY_MARKER)
}

/// Path of the proxy DLL inside the package. `version.dll` sits at the root,
/// the other five are shipped under `alternatives\`.
pub fn proxy_source(pkg: &Package, proxy: &str) -> Option<PathBuf> {
    let path = if proxy.eq_ignore_ascii_case("version.dll") {
        pkg.dir.join("version.dll")
    } else {
        pkg.dir.join("alternatives").join(proxy)
    };
    path.is_file().then_some(path)
}

pub fn proxy_available(pkg: &Package, proxy: &str) -> bool {
    proxy_source(pkg, proxy).is_some()
}

/// (ours, foreign) proxy DLLs found next to the exe.
pub fn scan_proxies(dir: &Path) -> (Vec<String>, Vec<String>) {
    let mut ours = Vec::new();
    let mut foreign = Vec::new();
    for name in PROXIES {
        let path = dir.join(name);
        if !path.is_file() {
            continue;
        }
        if is_our_proxy(&path) {
            ours.push(name.to_string());
        } else {
            foreign.push(name.to_string());
        }
    }
    (ours, foreign)
}

fn marker_path(dir: &Path) -> PathBuf {
    dir.join(MARKER_DIR).join(MARKER_FILE)
}

fn read_marker(dir: &Path) -> Option<Marker> {
    let text = fs::read_to_string(marker_path(dir)).ok()?;
    serde_json::from_str(&text).ok()
}

/// Finds an installed copy of the mod in `dir`.
pub fn detect_install(dir: &Path) -> Option<InstallRecord> {
    if let Some(marker) = read_marker(dir) {
        if dir.join(&marker.proxy).is_file() && dir.join(PAYLOAD_DIR).is_dir() {
            return Some(InstallRecord {
                path: dir.to_path_buf(),
                exe: marker.exe,
                proxy: marker.proxy,
                version: marker.version,
                installed_at: marker.installed_at,
            });
        }
    }
    // Fallback for copies installed by hand (no marker file).
    let (ours, _) = scan_proxies(dir);
    let proxy = ours.first()?.clone();
    let version = read_payload_version(dir).unwrap_or_else(|| "неизвестна".into());
    Some(InstallRecord {
        path: dir.to_path_buf(),
        exe: String::new(),
        proxy,
        version,
        installed_at: String::new(),
    })
}

fn read_payload_version(dir: &Path) -> Option<String> {
    let ini = fs::read_to_string(dir.join(INI_NAME)).ok()?;
    for line in ini.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            if k.trim().eq_ignore_ascii_case("Version") {
                return Some(v.trim().to_string());
            }
        }
    }
    None
}

fn move_to_backup(src: &Path, backup: &Path, rel: &str) -> Result<(), String> {
    let dst = backup.join(rel);
    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    if fs::rename(src, &dst).is_ok() {
        return Ok(());
    }
    if src.is_dir() {
        util::copy_tree(src, &dst, |_| {}).map_err(|e| format!("Бэкап {rel}: {e}"))?;
    } else {
        fs::copy(src, &dst).map_err(|e| format!("Бэкап {rel}: {e}"))?;
    }
    util::remove_path(src).map_err(|e| format!("Бэкап {rel}: {e}"))?;
    Ok(())
}

fn is_running(exe_names: &[String]) -> Option<String> {
    if exe_names.is_empty() {
        return None;
    }
    let procs = crate::system::running_processes();
    let wanted: Vec<String> = exe_names.iter().map(|n| n.to_ascii_lowercase()).collect();
    procs
        .into_iter()
        .find(|p| wanted.iter().any(|w| w == p))
}

pub struct InstallOpts<'a> {
    pub dir: &'a Path,
    pub exe: &'a str,
    pub pkg: &'a Package,
    pub proxy: &'a str,
    pub remove_extra_proxies: bool,
}

/// Installs (or re-installs / upgrades) the mod into the game folder.
pub fn install(
    opts: InstallOpts<'_>,
    on_progress: &mut dyn FnMut(f32, String),
    on_log: &mut dyn FnMut(String),
) -> Result<InstallRecord, String> {
    let dir = opts.dir;
    if !dir.is_dir() {
        return Err("Папка игры не найдена".into());
    }
    let src = proxy_source(opts.pkg, opts.proxy)
        .ok_or_else(|| format!("В пакете нет прокси {} (alternatives)", opts.proxy))?;
    let ini = opts.pkg.dir.join(INI_NAME);
    if !ini.is_file() {
        return Err(format!("В пакете нет {INI_NAME}"));
    }
    let payload = opts.pkg.dir.join(PAYLOAD_DIR);
    if !payload.is_dir() {
        return Err("В пакете нет папки dlssg_sm86".into());
    }

    // Refuse to touch files of a running game: the DLLs would be locked.
    let exe_list: Vec<String> = if opts.exe.is_empty() {
        top_level_exes(dir)
    } else {
        vec![opts.exe.to_string()]
    };
    if let Some(name) = is_running(&exe_list) {
        return Err(format!(
            "Игра запущена ({name}). Полностью закройте её и повторите установку."
        ));
    }
    if !crate::system::dir_writable(dir) {
        return Err(
            "Нет прав на запись в папку игры. Запустите программу от имени администратора.".into(),
        );
    }

    let backup_rel = format!("backups/{}-install", util::timestamp_slug());
    let backup = dir.join(MARKER_DIR).join(&backup_rel);
    let mut backed_up: Vec<String> = Vec::new();

    // 1) Clear out this project's other proxies: several of them in one folder
    //    is a known source of "old version runs" bugs.
    let (ours, foreign) = scan_proxies(dir);

    // 2) Back up everything that will be replaced or removed.
    for name in &ours {
        if opts.remove_extra_proxies && name != opts.proxy {
            move_to_backup(&dir.join(name), &backup, name)?;
            backed_up.push(name.clone());
            on_log(format!("Убран лишний прокси прошлой установки: {name}"));
        }
    }
    if dir.join(opts.proxy).exists() {
        move_to_backup(&dir.join(opts.proxy), &backup, opts.proxy)?;
        backed_up.push(opts.proxy.to_string());
        on_log(format!("Старый {} сохранён в бэкап", opts.proxy));
    }
    for name in &foreign {
        on_log(format!(
            "В папке уже был посторонний {name} — он не тронут (это может быть другой мод)"
        ));
    }
    if dir.join(INI_NAME).exists() {
        move_to_backup(&dir.join(INI_NAME), &backup, INI_NAME)?;
        backed_up.push(INI_NAME.to_string());
    }
    if dir.join(PAYLOAD_DIR).exists() {
        on_log("Старая папка dlssg_sm86 перемещена в бэкап".into());
        move_to_backup(&dir.join(PAYLOAD_DIR), &backup, PAYLOAD_DIR)?;
        backed_up.push(PAYLOAD_DIR.to_string());
    }

    // 3) Copy the new payload.
    let payload_bytes = crate::packages::payload_size(opts.pkg);
    let total = payload_bytes
        + fs::metadata(&src).map(|m| m.len()).unwrap_or(0)
        + fs::metadata(&ini).map(|m| m.len()).unwrap_or(0);
    let total = total.max(1);
    let mut done = 0u64;
    on_log(format!(
        "Копирование {} в {}",
        opts.pkg.version,
        dir.display()
    ));
    copy_file_progress(&src, &dir.join(opts.proxy), &mut done, total, on_progress)?;
    copy_file_progress(&ini, &dir.join(INI_NAME), &mut done, total, on_progress)?;
    copy_dir_progress(&payload, &dir.join(PAYLOAD_DIR), &mut done, total, on_progress)?;

    // 4) Marker used by the manager for upgrades / uninstalls.
    let record = InstallRecord {
        path: dir.to_path_buf(),
        exe: opts.exe.to_string(),
        proxy: opts.proxy.to_string(),
        version: opts.pkg.version.clone(),
        installed_at: util::now_human(),
    };
    let marker = Marker {
        version: record.version.clone(),
        proxy: record.proxy.clone(),
        exe: record.exe.clone(),
        installed_at: record.installed_at.clone(),
        backup: (!backed_up.is_empty()).then(|| backup_rel.clone()),
        backed_up: backed_up.clone(),
    };
    fs::create_dir_all(dir.join(MARKER_DIR)).map_err(|e| e.to_string())?;
    let text = serde_json::to_string_pretty(&marker).map_err(|e| e.to_string())?;
    fs::write(marker_path(dir), text).map_err(|e| format!("Не записать маркер: {e}"))?;

    on_progress(1.0, "Установлено".into());
    on_log(format!(
        "Готово: {} / прокси {} / {:.1} МБ",
        record.version,
        record.proxy,
        payload_bytes as f64 / 1_048_576.0
    ));
    Ok(record)
}

fn top_level_exes(dir: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter(|e| {
            e.path()
                .extension()
                .map(|x| x.eq_ignore_ascii_case("exe"))
                .unwrap_or(false)
        })
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect()
}

fn copy_file_progress(
    src: &Path,
    dst: &Path,
    done: &mut u64,
    total: u64,
    on_progress: &mut dyn FnMut(f32, String),
) -> Result<(), String> {
    let name = src
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    util::copy_with_progress(src, dst, |n| *done += n)
        .map_err(|e| format!("Не скопировать {name}: {e}"))?;
    on_progress(
        (*done as f32 / total as f32).clamp(0.0, 1.0),
        format!("Копирование: {name}"),
    );
    Ok(())
}

fn copy_dir_progress(
    src: &Path,
    dst: &Path,
    done: &mut u64,
    total: u64,
    on_progress: &mut dyn FnMut(f32, String),
) -> Result<(), String> {
    fs::create_dir_all(dst).map_err(|e| e.to_string())?;
    let mut entries: Vec<PathBuf> = fs::read_dir(src)
        .map_err(|e| e.to_string())?
        .flatten()
        .map(|e| e.path())
        .collect();
    entries.sort();
    for path in entries {
        let name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
        if path.is_dir() {
            if name.eq_ignore_ascii_case("logs") {
                continue;
            }
            copy_dir_progress(&path, &dst.join(&name), done, total, on_progress)?;
        } else {
            copy_file_progress(&path, &dst.join(&name), done, total, on_progress)?;
        }
    }
    Ok(())
}

/// Removes the mod from the game folder and restores whatever was displaced
/// during the first install (for example a foreign `version.dll`).
pub fn uninstall(
    dir: &Path,
    on_progress: &mut dyn FnMut(f32, String),
    on_log: &mut dyn FnMut(String),
) -> Result<(), String> {
    if !dir.is_dir() {
        return Err("Папка игры не найдена".into());
    }
    let exe_list = top_level_exes(dir);
    if let Some(name) = is_running(&exe_list) {
        return Err(format!("Игра запущена ({name}). Закройте её перед удалением."));
    }
    let marker = read_marker(dir);
    let record = detect_install(dir);
    let proxy = marker
        .as_ref()
        .map(|m| m.proxy.clone())
        .or_else(|| record.map(|r| r.proxy))
        .unwrap_or_else(|| "version.dll".into());

    let trash = dir
        .join(MARKER_DIR)
        .join(format!("backups/{}-uninstall", util::timestamp_slug()));
    let mut step = 0.0f32;
    for (rel, is_dir) in [
        (proxy.clone(), false),
        (INI_NAME.to_string(), false),
        (PAYLOAD_DIR.to_string(), true),
    ] {
        let path = dir.join(&rel);
        if !path.exists() {
            continue;
        }
        on_progress(step, format!("Удаление: {rel}"));
        if !is_dir && !is_our_proxy(&path) && rel != INI_NAME {
            on_log(format!("{rel} не принадлежит моду — оставлен на месте"));
            continue;
        }
        move_to_backup(&path, &trash, &rel)?;
        step += 0.4;
    }
    on_log("Файлы мода перемещены в .dlssg-manager\\backups (удаление обратимо)".into());

    // Put back the files that were displaced by the install.
    if let Some(marker) = marker {
        if let Some(backup_rel) = marker.backup {
            let backup = dir.join(MARKER_DIR).join(backup_rel);
            for rel in marker.backed_up {
                let src = backup.join(&rel);
                if !src.exists() {
                    continue;
                }
                let dst = dir.join(&rel);
                if dst.exists() {
                    continue;
                }
                if src.is_dir() {
                    if util::copy_tree(&src, &dst, |_| {}).is_ok() {
                        on_log(format!("Восстановлен {rel} из бэкапа"));
                    }
                } else if fs::copy(&src, &dst).is_ok() {
                    on_log(format!("Восстановлен {rel} из бэкапа"));
                }
            }
        }
    }
    let _ = fs::remove_file(marker_path(dir));
    on_progress(1.0, "Удалено".into());
    Ok(())
}

/// Deletes the backup folders of the given game (used by "очистить бэкапы").
pub fn clear_backups(dir: &Path) -> Result<u64, String> {
    let path = dir.join(MARKER_DIR).join("backups");
    if !path.exists() {
        return Ok(0);
    }
    let size = dir_size(&path);
    fs::remove_dir_all(&path).map_err(|e| e.to_string())?;
    Ok(size)
}

fn dir_size(path: &Path) -> u64 {
    let mut total = 0;
    if let Ok(entries) = fs::read_dir(path) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                total += dir_size(&p);
            } else if let Ok(m) = entry.metadata() {
                total += m.len();
            }
        }
    }
    total
}

#[derive(Debug, Clone, Default)]
pub struct LogInfo {
    pub path: PathBuf,
    pub exists: bool,
    pub hits: usize,
    pub tail: Vec<String>,
}

/// Reads `dlssg_sm86\logs\dlssg_sm86.log` and counts lines mentioning the exe.
pub fn read_log(dir: &Path, exe: &str) -> LogInfo {
    let path = dir.join(PAYLOAD_DIR).join("logs").join("dlssg_sm86.log");
    let mut info = LogInfo {
        path: path.clone(),
        ..Default::default()
    };
    let Ok(text) = fs::read_to_string(&path) else {
        return info;
    };
    info.exists = true;
    let needle = exe.to_ascii_lowercase();
    let lines: Vec<&str> = text.lines().collect();
    if !needle.is_empty() {
        info.hits = lines
            .iter()
            .filter(|l| l.to_ascii_lowercase().contains(&needle))
            .count();
    }
    info.tail = lines
        .iter()
        .rev()
        .take(60)
        .rev()
        .map(|s| s.to_string())
        .collect();
    info
}

pub fn set_ini_value(dir: &Path, key: &str, value: &str) -> Result<(), String> {
    let path = dir.join(INI_NAME);
    let text = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let mut out = String::new();
    let mut replaced = false;
    for line in text.lines() {
        let t = line.trim();
        if !replaced && !t.starts_with(';') && !t.starts_with('[') {
            if let Some((k, _)) = t.split_once('=') {
                if k.trim().eq_ignore_ascii_case(key) {
                    out.push_str(&format!("{key}={value}\r\n"));
                    replaced = true;
                    continue;
                }
            }
        }
        out.push_str(line);
        out.push_str("\r\n");
    }
    fs::write(&path, out).map_err(|e| e.to_string())
}
