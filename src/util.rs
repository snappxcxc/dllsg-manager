use std::fs;
use std::path::{Path, PathBuf};

/// `%LOCALAPPDATA%\DLSSG Manager` or a fallback next to the exe.
pub fn data_dir() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir());
    let dir = base.join("DLSSG Manager");
    let _ = fs::create_dir_all(&dir);
    dir
}

pub fn packages_dir() -> PathBuf {
    let dir = data_dir().join("packages");
    let _ = fs::create_dir_all(&dir);
    dir
}

pub fn downloads_dir() -> PathBuf {
    let dir = data_dir().join("downloads");
    let _ = fs::create_dir_all(&dir);
    dir
}

pub fn icons_dir() -> PathBuf {
    let dir = data_dir().join("icons");
    let _ = fs::create_dir_all(&dir);
    dir
}

pub fn config_path() -> PathBuf {
    data_dir().join("config.json")
}

pub fn log_path() -> PathBuf {
    data_dir().join("manager.log")
}

pub fn timestamp_slug() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // YYYYMMDD-HHMMSS in UTC, computed without external crates.
    let days = secs / 86_400;
    let rem = secs % 86_400;
    let (h, m, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    let (y, mo, d) = civil_from_days(days as i64);
    format!("{y:04}{mo:02}{d:02}-{h:02}{m:02}{s:02}")
}

pub fn now_human() -> String {
    timestamp_slug()
}

/// Howard Hinnant's civil_from_days.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["Б", "КБ", "МБ", "ГБ", "ТБ"];
    let mut v = bytes as f64;
    let mut i = 0;
    while v >= 1024.0 && i < UNITS.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    let unit = crate::i18n::t(UNITS[i]);
    if i == 0 {
        format!("{bytes} {unit}")
    } else {
        format!("{v:.1} {unit}")
    }
}

pub fn copy_with_progress(
    src: &Path,
    dst: &Path,
    mut on_bytes: impl FnMut(u64),
) -> std::io::Result<()> {
    use std::io::{Read, Write};
    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut input = fs::File::open(src)?;
    let mut output = fs::File::create(dst)?;
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = input.read(&mut buf)?;
        if n == 0 {
            break;
        }
        output.write_all(&buf[..n])?;
        on_bytes(n as u64);
    }
    output.flush()?;
    Ok(())
}

/// Copy an entire directory tree, reporting the number of copied bytes.
/// Existing files are overwritten; logs are skipped.
pub fn copy_tree(src: &Path, dst: &Path, mut on_bytes: impl FnMut(u64)) -> std::io::Result<()> {
    copy_tree_inner(src, dst, &mut on_bytes)
}

fn copy_tree_inner(
    src: &Path,
    dst: &Path,
    on_bytes: &mut dyn FnMut(u64),
) -> std::io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let path = entry.path();
        let target = dst.join(entry.file_name());
        if path.is_dir() {
            if path
                .file_name()
                .map(|n| n.to_string_lossy().eq_ignore_ascii_case("logs"))
                .unwrap_or(false)
            {
                continue;
            }
            copy_tree_inner(&path, &target, on_bytes)?;
        } else {
            copy_with_progress(&path, &target, &mut *on_bytes)?;
        }
    }
    Ok(())
}

pub fn remove_path(path: &Path) -> std::io::Result<()> {
    if path.is_dir() {
        fs::remove_dir_all(path)
    } else if path.exists() {
        fs::remove_file(path)
    } else {
        Ok(())
    }
}

pub fn open_in_explorer(path: &Path) {
    let mut cmd = std::process::Command::new("explorer.exe");
    if path.is_dir() {
        cmd.arg(path);
    } else {
        cmd.arg("/select,").arg(path);
    }
    let _ = cmd.spawn();
}

pub fn open_url(url: &str) {
    let _ = std::process::Command::new("cmd")
        .args(["/c", "start", "", url])
        .spawn();
}

/// Case-insensitive check that `needle` occurs in the first 4 MB of a file.
pub fn file_contains(path: &Path, needle: &[u8]) -> bool {
    use std::io::Read;
    let Ok(mut f) = fs::File::open(path) else {
        return false;
    };
    let mut buf = vec![0u8; 4 << 20];
    let mut filled = 0usize;
    while filled < buf.len() {
        match f.read(&mut buf[filled..]) {
            Ok(0) => break,
            Ok(n) => filled += n,
            Err(_) => return false,
        }
    }
    buf[..filled]
        .windows(needle.len())
        .any(|w| w.eq_ignore_ascii_case(needle))
}

pub fn sha256_file(path: &Path) -> std::io::Result<String> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn date_is_plausible() {
        let s = timestamp_slug();
        assert_eq!(s.len(), 15);
        assert!(human_size(1536).starts_with("1.5"));
    }
}
