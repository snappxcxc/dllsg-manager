use crate::model::{normalize_version, Package, PackageFile, INI_NAME, PAYLOAD_DIR};
use crate::util;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

pub type Progress<'a> = &'a mut dyn FnMut(f32, String);

/// Reads `manifest.json` of a release package.
pub fn read_manifest(root: &Path) -> Option<Package> {
    let text = fs::read_to_string(root.join("manifest.json")).ok()?;
    let json: Value = serde_json::from_str(&text).ok()?;
    let version = json
        .get("bundle")
        .and_then(|b| b.get("version"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let version = normalize_version(&version);
    if version.is_empty() {
        return None;
    }
    let mut files = Vec::new();
    if let Some(list) = json.get("files").and_then(|f| f.as_array()) {
        for f in list {
            let path = f.get("path").and_then(|p| p.as_str()).unwrap_or("");
            if path.is_empty() {
                continue;
            }
            files.push(PackageFile {
                path: path.replace('\\', "/"),
                size: f.get("size").and_then(|s| s.as_u64()).unwrap_or(0),
                sha256: f
                    .get("sha256")
                    .and_then(|s| s.as_str())
                    .unwrap_or("")
                    .to_ascii_lowercase(),
            });
        }
    }
    Some(Package {
        version,
        dir: root.to_path_buf(),
        files,
        source: root.display().to_string(),
    })
}

/// A release root is the folder that has `manifest.json` next to `version.dll`.
pub fn find_root(start: &Path) -> Option<PathBuf> {
    if start.join("manifest.json").is_file() && start.join("version.dll").is_file() {
        return Some(start.to_path_buf());
    }
    let entries = fs::read_dir(start).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() && path.join("manifest.json").is_file() && path.join("version.dll").is_file()
        {
            return Some(path);
        }
    }
    None
}

/// Copy a package folder (already unpacked) into the local package cache.
pub fn import_from_dir(src: &Path, on_progress: Progress) -> Result<Package, String> {
    let root = find_root(src)
        .ok_or_else(|| format!("В папке {} нет manifest.json + version.dll — это не пакет проекта.", src.display()))?;
    let mut pkg = read_manifest(&root).ok_or("Не удалось прочитать manifest.json")?;
    validate_layout(&root)?;

    let dest = util::packages_dir().join(&pkg.version);
    if read_manifest(&dest).is_some() {
        on_progress(1.0, "Пакет уже в кэше".into());
        return Ok(read_manifest(&dest).unwrap());
    }
    let _ = fs::remove_dir_all(&dest);
    fs::create_dir_all(&dest).map_err(|e| e.to_string())?;

    let total = dir_size(&root).max(1);
    let mut done = 0u64;
    copy_payload(&root, &dest, &mut done, total, on_progress)?;
    pkg.dir = dest;
    on_progress(1.0, "Готово".into());
    Ok(pkg)
}

fn copy_payload(
    src: &Path,
    dst: &Path,
    done: &mut u64,
    total: u64,
    on_progress: Progress,
) -> Result<(), String> {
    fs::create_dir_all(dst).map_err(|e| e.to_string())?;
    let mut entries: Vec<_> = fs::read_dir(src)
        .map_err(|e| e.to_string())?
        .flatten()
        .map(|e| e.path())
        .collect();
    entries.sort();
    for path in entries {
        let name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
        let target = dst.join(&name);
        if path.is_dir() {
            copy_payload(&path, &target, done, total, on_progress)?;
        } else {
            util::copy_with_progress(&path, &target, |n| *done += n).map_err(|e| e.to_string())?;
            on_progress(
                (*done as f32 / total as f32).clamp(0.0, 1.0),
                format!("Копирование: {name}"),
            );
        }
    }
    Ok(())
}

fn dir_size(path: &Path) -> u64 {
    let mut total = 0;
    if let Ok(entries) = fs::read_dir(path) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                total += dir_size(&p);
            } else if let Ok(meta) = entry.metadata() {
                total += meta.len();
            }
        }
    }
    total
}

pub fn validate_layout(root: &Path) -> Result<(), String> {
    let mut missing = Vec::new();
    if !root.join("version.dll").is_file() {
        missing.push("version.dll");
    }
    if !root.join(INI_NAME).is_file() {
        missing.push(INI_NAME);
    }
    if !root.join(PAYLOAD_DIR).is_dir() {
        missing.push(PAYLOAD_DIR);
    }
    if missing.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "В пакете не хватает: {}. Скопируйте файлы из архива целиком.",
            missing.join(", ")
        ))
    }
}

/// Unpacks a release zip into `dest`, reporting progress, then flattens a
/// possible single top-level folder.
pub fn extract_zip(zip_path: &Path, dest: &Path, on_progress: Progress) -> Result<(), String> {
    let file = fs::File::open(zip_path).map_err(|e| format!("Не открыть архив: {e}"))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| format!("Повреждённый архив: {e}"))?;
    fs::create_dir_all(dest).map_err(|e| e.to_string())?;
    let total = archive.len().max(1);
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
        let Some(rel) = entry.enclosed_name() else {
            continue;
        };
        let out_path = dest.join(rel);
        if entry.is_dir() {
            fs::create_dir_all(&out_path).map_err(|e| e.to_string())?;
        } else {
            if let Some(parent) = out_path.parent() {
                fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            let mut out = fs::File::create(&out_path).map_err(|e| e.to_string())?;
            std::io::copy(&mut entry, &mut out).map_err(|e| e.to_string())?;
            on_progress(
                ((i + 1) as f32 / total as f32).clamp(0.0, 1.0),
                format!("Распаковка: {}", entry.name()),
            );
        }
    }
    flatten_root(dest)?;
    Ok(())
}

fn flatten_root(dest: &Path) -> Result<(), String> {
    if find_root(dest).is_some() {
        return Ok(());
    }
    let entries: Vec<PathBuf> = fs::read_dir(dest)
        .map_err(|e| e.to_string())?
        .flatten()
        .map(|e| e.path())
        .collect();
    if entries.len() == 1 && entries[0].is_dir() {
        let inner = &entries[0];
        for entry in fs::read_dir(inner).map_err(|e| e.to_string())?.flatten() {
            let target = dest.join(entry.file_name());
            fs::rename(entry.path(), &target)
                .or_else(|_| {
                    util::copy_tree(&entry.path(), &target, |_| {}).and_then(|_| {
                        util::remove_path(&entry.path())
                    })
                })
                .map_err(|e| e.to_string())?;
        }
        let _ = fs::remove_dir(inner);
    }
    Ok(())
}

/// Install a downloaded release zip into the cache and return the package.
pub fn install_zip_into_cache(zip_path: &Path, on_progress: Progress) -> Result<Package, String> {
    on_progress(0.0, "Распаковка архива".into());
    let tmp = util::data_dir().join(format!("unpack-{}", util::timestamp_slug()));
    let _ = fs::remove_dir_all(&tmp);
    extract_zip(zip_path, &tmp, on_progress)?;
    let result = import_from_dir(&tmp, on_progress);
    let _ = fs::remove_dir_all(&tmp);
    result
}

/// All packages known in the local cache, newest first.
pub fn list_packages() -> Vec<Package> {
    let mut out = Vec::new();
    let dir = util::packages_dir();
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            if let Some(pkg) = read_manifest(&path) {
                if pkg.dir.join("dlssg_sm86").is_dir() && pkg.dir.join("version.dll").is_file() {
                    out.push(pkg);
                }
            }
        }
    }
    out.sort_by(|a, b| {
        if crate::model::version_gt(&a.version, &b.version) {
            std::cmp::Ordering::Less
        } else if a.version == b.version {
            std::cmp::Ordering::Equal
        } else {
            std::cmp::Ordering::Greater
        }
    });
    out
}

pub fn best_cached() -> Option<Package> {
    list_packages().into_iter().next()
}

/// Checks the essential files against the hashes from manifest.json.
/// Returns a list of problems (empty = everything matches).
pub fn verify_package(pkg: &Package) -> Vec<String> {
    let mut problems = Vec::new();
    let required = ["version.dll", INI_NAME];
    for rel in required {
        let path = pkg.dir.join(rel);
        if !path.is_file() {
            problems.push(format!("нет файла {rel}"));
            continue;
        }
        if let Some(expected) = pkg.sha_of(rel) {
            match util::sha256_file(&path) {
                Ok(actual) if actual.eq_ignore_ascii_case(expected) => {}
                Ok(actual) => problems.push(format!(
                    "{rel}: SHA-256 не совпадает (ожидался {}, получен {})",
                    &expected[..12.min(expected.len())],
                    &actual[..12.min(actual.len())]
                )),
                Err(e) => problems.push(format!("{rel}: не прочитать ({e})")),
            }
        }
    }
    if !pkg.dir.join(PAYLOAD_DIR).join("runtime").join("nvngx_dlssg.dll").is_file() {
        problems.push("нет dlssg_sm86\\runtime\\nvngx_dlssg.dll".into());
    }
    if !pkg.dir.join(PAYLOAD_DIR).join("kernels").join("fg_kernels.dll").is_file() {
        problems.push("нет dlssg_sm86\\kernels\\fg_kernels.dll".into());
    }
    problems
}

/// Size of `dlssg_sm86\` so the UI can warn about disk usage.
pub fn payload_size(pkg: &Package) -> u64 {
    dir_size(&pkg.dir.join(PAYLOAD_DIR))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_layout_is_reported() {
        let tmp = std::env::temp_dir().join("dlssgm-layout-test");
        let _ = fs::create_dir_all(&tmp);
        let err = validate_layout(&tmp).unwrap_err();
        assert!(err.contains("version.dll"));
        let _ = fs::remove_dir_all(&tmp);
    }
}
