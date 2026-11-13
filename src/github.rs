use crate::model::{normalize_version, Release, DEFAULT_REPO};
use serde_json::Value;
use std::fs;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

const UA: &str = "DLSSG-Manager/1.0 (Windows; +https://github.com/sdli1995/dlssg_for_sm86)";

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .user_agent(UA)
        .timeout_connect(Duration::from_secs(20))
        .timeout_read(Duration::from_secs(60))
        .build()
}

/// Asks the GitHub API for the latest release of the mod and picks the x64 zip
/// asset. Falls back to the `/releases/latest` redirect when the API is rate
/// limited. The repository is fixed — there is nothing to configure.
pub fn latest_release() -> Result<Release, String> {
    let repo = DEFAULT_REPO;
    let agent = agent();
    let api = format!("https://api.github.com/repos/{repo}/releases/latest");
    match agent
        .get(&api)
        .set("Accept", "application/vnd.github+json")
        .call()
    {
        Ok(resp) => {
            let body = resp
                .into_string()
                .map_err(|e| format!("Ответ GitHub не прочитан: {e}"))?;
            parse_release(&body)
        }
        Err(ureq::Error::Status(code, resp)) => {
            let body = resp.into_string().unwrap_or_default();
            // 404 -> no releases at all, 403 -> rate limit: try the redirect route.
            if code == 404 {
                return Err(format!("У {repo} нет опубликованных релизов"));
            }
            match latest_by_redirect(&agent) {
                Ok(release) => Ok(release),
                Err(_) => Err(format!(
                    "GitHub API ответил {code}. {}",
                    body.chars().take(160).collect::<String>()
                )),
            }
        }
        Err(e) => match latest_by_redirect(&agent) {
            Ok(release) => Ok(release),
            Err(_) => Err(format!("Нет связи с GitHub: {e}")),
        },
    }
}

fn parse_release(body: &str) -> Result<Release, String> {
    let json: Value = serde_json::from_str(body).map_err(|e| format!("JSON GitHub: {e}"))?;
    let tag = json
        .get("tag_name")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    if tag.is_empty() {
        return Err("В ответе GitHub нет tag_name".into());
    }
    let name = json
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or(&tag)
        .to_string();
    let notes = json
        .get("body")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let published = json
        .get("published_at")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();

    let assets = json
        .get("assets")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let mut best: Option<(String, String, u64)> = None;
    for asset in &assets {
        let asset_name = asset.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let url = asset
            .get("browser_download_url")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if url.is_empty() {
            continue;
        }
        let lower = asset_name.to_ascii_lowercase();
        if !lower.ends_with(".zip") {
            continue;
        }
        let score = if lower.contains("x64") && lower.contains("release") {
            3
        } else if lower.contains("x64") {
            2
        } else {
            1
        };
        if best.as_ref().map(|b| score > b.2).unwrap_or(true) {
            best = Some((asset_name.to_string(), url.to_string(), score as u64));
        }
    }

    let (asset_name, asset_url, _) = best.ok_or_else(|| {
        format!(
            "В релизе {tag} нет zip-архива с пакетом (проверьте {DEFAULT_REPO}/releases)"
        )
    })?;
    let size = assets
        .iter()
        .find(|a| a.get("browser_download_url").and_then(|v| v.as_str()) == Some(asset_url.as_str()))
        .and_then(|a| a.get("size"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    Ok(Release {
        tag,
        name,
        asset_name,
        asset_url,
        size,
        published,
        notes,
    })
}

/// GitHub redirects `/releases/latest` to `/releases/tag/<tag>`; the final URL
/// gives us the tag even without API access. Asset names are predictable.
fn latest_by_redirect(agent: &ureq::Agent) -> Result<Release, String> {
    let repo = DEFAULT_REPO;
    let url = format!("https://github.com/{repo}/releases/latest");
    let resp = agent.get(&url).call().map_err(|e| e.to_string())?;
    let final_url = resp.get_url().to_string();
    let tag = final_url
        .rsplit('/')
        .next()
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty() && s != "latest" && s != "releases")
        .ok_or("Не удалось определить тег последнего релиза")?;
    let version = normalize_version(&tag);

    let candidates = [
        format!("dlssg-release-{version}-x64.zip"),
        format!("dlssg-release-{tag}-x64.zip"),
        format!("dlssg-release-{version}.zip"),
    ];
    for asset_name in candidates {
        let asset_url = format!("https://github.com/{repo}/releases/download/{tag}/{asset_name}");
        match agent.head(&asset_url).call() {
            Ok(r) if r.status() == 200 => {
                return Ok(Release {
                    tag: tag.clone(),
                    name: tag.clone(),
                    asset_name,
                    asset_url,
                    size: 0,
                    published: String::new(),
                    notes: "Проверено через страницу релизов GitHub (API недоступен).".into(),
                });
            }
            _ => continue,
        }
    }
    Err("Не удалось найти zip-архив в последнем релизе".into())
}

/// Streams `url` to `dest` with progress (0..1). Returns the number of bytes.
pub fn download(
    url: &str,
    dest: &Path,
    size_hint: u64,
    progress: &mut dyn FnMut(f32, String),
    cancel: &AtomicBool,
) -> Result<u64, String> {
    let agent = agent();
    let resp = agent
        .get(url)
        .call()
        .map_err(|e| format!("Загрузка не началась: {e}"))?;
    let total = resp
        .header("content-length")
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(size_hint);
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let part = dest.with_extension("part");
    let mut file = fs::File::create(&part).map_err(|e| format!("Не создать файл: {e}"))?;
    let mut reader = resp.into_reader();
    let mut buf = vec![0u8; 256 * 1024];
    let mut done = 0u64;
    loop {
        if cancel.load(Ordering::Relaxed) {
            let _ = fs::remove_file(&part);
            return Err("Загрузка отменена".into());
        }
        let n = reader.read(&mut buf).map_err(|e| format!("Обрыв загрузки: {e}"))?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        done += n as u64;
        let frac = if total > 0 {
            (done as f32 / total as f32).clamp(0.0, 1.0)
        } else {
            0.0
        };
        progress(
            frac,
            format!(
                "Скачано {} из {}",
                crate::util::human_size(done),
                if total > 0 {
                    crate::util::human_size(total)
                } else {
                    "?".into()
                }
            ),
        );
    }
    file.flush().ok();
    drop(file);
    fs::rename(&part, dest).map_err(|e| format!("Не переименовать файл: {e}"))?;
    if total > 0 && done != total {
        return Err(format!(
            "Размер не совпал: ожидалось {}, получено {}",
            total, done
        ));
    }
    Ok(done)
}

pub fn release_page() -> String {
    format!("https://github.com/{DEFAULT_REPO}/releases")
}

pub fn release_tag_page(tag: &str) -> String {
    format!("{}/tag/{}", release_page(), tag.trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_release_json() {
        let body = r#"{
            "tag_name": "0.4.1",
            "name": "0.4.1",
            "published_at": "2026-10-27T06:27:00Z",
            "body": "notes",
            "assets": [
                {"name": "sha256sums.txt", "browser_download_url": "https://x/a.txt", "size": 10},
                {"name": "dlssg-release-0.4.1-x64.zip", "browser_download_url": "https://x/package.zip", "size": 88819934}
            ]
        }"#;
        let r = parse_release(body).unwrap();
        assert_eq!(r.version(), "0.4.1");
        assert_eq!(r.asset_name, "dlssg-release-0.4.1-x64.zip");
        assert_eq!(r.size, 88819934);
    }
}
