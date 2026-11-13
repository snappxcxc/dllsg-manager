use crate::installer;
use crate::model::{Check, ExeInfo, GameScan, Level, Verdict, DRIVER_FLOOR, PROXIES};
use crate::system::{self, GpuInfo};
use std::fs;
use std::path::{Path, PathBuf};

const SKIP_DIRS: [&str; 6] = [
    "dlssg_sm86",
    ".dlssg-manager",
    "logs",
    "__pycache__",
    "shadercache",
    "cache",
];

const ANTICHEAT_HINTS: [&str; 14] = [
    "easyanticheat",
    "battleye",
    "start_protected_game",
    "eaclauncher",
    "anticheat",
    "mhyprot",
    "vgk.sys",
    "vgc.exe",
    "faceit",
    "equ8",
    "ricochet",
    "nprotect",
    "gameguard",
    "xigncode",
];

/// Games whose anti-cheat is installed outside the game folder (service/driver),
/// so it cannot be found by scanning files. Matched against the selected exe.
const ONLINE_AC_EXES: &[&str] = &[
    "cod.exe",
    "modernwarfare.exe",
    "modernwarfare2.exe",
    "modernwarfare3.exe",
    "sp22-cod.exe",
    "sp23-cod.exe",
    "valorant.exe",
    "valorant-win64-shipping.exe",
    "r5apex.exe",
    "destiny2.exe",
    "fortniteclient-win64-shipping.exe",
    "rainbowsix.exe",
    "rainbowsix_dx11.exe",
    "gta5.exe",
    "pubg.exe",
    "eft.exe",
    "overwatch.exe",
];

/// Width-first search for the first file/folder whose name matches `pred`.
fn find_match(root: &Path, max_depth: usize, pred: &dyn Fn(&str) -> bool) -> Option<PathBuf> {
    let mut queue = vec![(root.to_path_buf(), 0usize)];
    while let Some((dir, depth)) = queue.pop() {
        if depth > max_depth {
            continue;
        }
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            let lower = name.to_ascii_lowercase();
            if pred(&lower) {
                return Some(entry.path());
            }
            if entry.path().is_dir() && !SKIP_DIRS.iter().any(|s| lower == *s) {
                queue.push((entry.path(), depth + 1));
            }
        }
    }
    None
}

pub fn find_dlss_files(dir: &Path) -> (Option<PathBuf>, Option<PathBuf>) {
    let dlssg = find_match(dir, 4, &|n| n == "nvngx_dlssg.dll");
    let sr = find_match(dir, 4, &|n| n == "nvngx_dlss.dll");
    (dlssg, sr)
}

pub fn find_anticheat(dir: &Path) -> Option<PathBuf> {
    find_match(dir, 4, &|n| {
        ANTICHEAT_HINTS
            .iter()
            .any(|hint| n == *hint || n.contains(hint))
    })
}

fn exe_score(name: &str, size: u64, info_d3d12: bool, info_dxgi: bool, has_dlssg: bool) -> i32 {
    let n = name.to_ascii_lowercase();
    let mut score = 0i32;
    if info_d3d12 {
        score += 30;
    }
    if info_dxgi {
        score += 20;
    }
    if has_dlssg {
        score += 15;
    }
    if size > 20 * 1024 * 1024 {
        score += 8;
    } else if size > 5 * 1024 * 1024 {
        score += 4;
    }
    for bad in [
        "unins",
        "setup",
        "vcredist",
        "dxsetup",
        "crashreport",
        "crashpad",
        "eaclauncher",
        "easyanticheat",
        "start_protected",
        "benchmarktool",
        "unitycrashhandler",
    ] {
        if n.contains(bad) {
            score -= 60;
        }
    }
    if n.contains("shipping") || n.contains("win64") || n.contains("game") {
        score += 3;
    }
    score
}

/// Full compatibility report for one game folder.
pub fn scan_game(dir: &Path, gpu: &GpuInfo, pkg_version: Option<&str>) -> GameScan {
    let mut scan = GameScan {
        dir: dir.to_path_buf(),
        ..Default::default()
    };
    if !dir.is_dir() {
        scan.checks.push(Check::new(
            Level::Bad,
            "Папка не найдена",
            format!("{} не существует или недоступна", dir.display()),
        ));
        scan.verdict = Verdict::Unsupported;
        return scan;
    }

    let (dlssg_dll, sr_dll) = find_dlss_files(dir);
    let has_dlssg_near = dlssg_dll.is_some();

    // 1) Game exes.
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path
                .extension()
                .map(|e| e.eq_ignore_ascii_case("exe"))
                .unwrap_or(false)
            {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
            let imports = crate::pe::imports(&path);
            let d3d12 = imports
                .iter()
                .any(|i| i.starts_with("d3d12") || i.contains("d3d12"));
            let dxgi = imports.iter().any(|i| i.starts_with("dxgi"));
            scan.exes.push(ExeInfo {
                path: path.clone(),
                name: name.clone(),
                size,
                imports_d3d12: d3d12,
                imports_dxgi: dxgi,
                has_dlssg: has_dlssg_near,
                score: exe_score(&name, size, d3d12, dxgi, has_dlssg_near),
            });
        }
    }
    scan.exes.sort_by(|a, b| b.score.cmp(&a.score).then(b.size.cmp(&a.size)));
    scan.selected = 0;

    let best = scan.exes.first().cloned();
    scan.d3d12 = best.as_ref().map(|e| e.imports_d3d12 || e.imports_dxgi).unwrap_or(false);
    scan.dlssg_dll = dlssg_dll.clone();
    scan.sr_dll = sr_dll;
    scan.writable = system::dir_writable(dir);
    scan.installed = installer::detect_install(dir);

    let (ours, foreign) = installer::scan_proxies(dir);
    scan.our_proxies = ours;
    scan.conflicts = foreign;

    // 2) Is the game running right now?
    let procs = system::running_processes();
    scan.running = scan
        .exes
        .iter()
        .filter(|e| procs.iter().any(|p| p == &e.name.to_ascii_lowercase()))
        .map(|e| e.name.clone())
        .collect();

    // 3) Checks, most important first.
    if !gpu.is_nvidia {
        scan.checks.push(Check::new(
            Level::Bad,
            "Видеокарта не NVIDIA",
            format!(
                "Найдено: {}. Мод работает только на NVIDIA RTX 20/30.",
                gpu.name
            ),
        ));
    } else if !gpu.arch.is_supported() {
        scan.checks.push(Check::new(
            Level::Warn,
            "Архитектура GPU не распознана",
            format!(
                "{} — мод рассчитан на RTX 20 (SM75) и RTX 30 (SM86).",
                gpu.name
            ),
        ));
    } else if matches!(
        gpu.arch,
        crate::system::Arch::Sm89 | crate::system::Arch::Sm120
    ) {
        scan.checks.push(Check::new(
            Level::Warn,
            "Frame Generation уже есть нативно",
            format!(
                "{} — на RTX 40/50 DLSS Frame Generation работает из коробки, мод не нужен и ничего не добавит.",
                gpu.name
            ),
        ));
    } else {
        let extra = if gpu.arch == crate::system::Arch::Sm86 {
            " — это ровно та архитектура, под которую собран пакет."
        } else {
            " — поддерживается, но пакет оптимизирован под SM86."
        };
        scan.checks.push(Check::new(
            Level::Ok,
            format!("GPU совместим: {}", gpu.arch.label()),
            format!("{}{}", gpu.name, extra),
        ));
    }

    if gpu.driver_ok() {
        scan.checks.push(Check::new(
            Level::Ok,
            format!("Драйвер NVIDIA {}", gpu.driver_text()),
            format!("Требование мода — R{DRIVER_FLOOR} или новее."),
        ));
    } else {
        scan.checks.push(Check::new(
            Level::Bad,
            "Драйвер старше R580",
            format!(
                "Установлен {}. Мод не включит генерацию кадров (в логе будет driver_below_floor) — обновите драйвер NVIDIA.",
                if gpu.driver.is_none() {
                    crate::i18n::t("неизвестный драйвер").to_string()
                } else {
                    gpu.driver_text()
                }
            ),
        ));
    }

    match best.as_ref() {
        Some(exe) if scan.d3d12 => scan.checks.push(Check::new(
            Level::Ok,
            "Игра рендерит через D3D12",
            format!(
                "{} импортирует {}. Прокси-мод работает с D3D12.",
                exe.name,
                if exe.imports_d3d12 { "d3d12.dll" } else { "dxgi.dll" }
            ),
        )),
        Some(exe) => scan.checks.push(Check::new(
            Level::Warn,
            "D3D12 в импортах не найден",
            format!(
                "{} не ссылается на d3d12.dll/dxgi.dll (возможна отложенная загрузка). Если игра на Vulkan/D3D11 — мод не заработает.",
                exe.name
            ),
        )),
        None => scan.checks.push(Check::new(
            Level::Bad,
            "EXE игры не найден",
            "В папке нет ни одного .exe — выберите каталог с исполняемым файлом игры."
                .to_string(),
        )),
    }

    if let Some(path) = &dlssg_dll {
        let rel = path
            .strip_prefix(dir)
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| path.display().to_string());
        scan.checks.push(Check::new(
            Level::Ok,
            "DLSS Frame Generation есть в игре",
            format!(
                "Найден nvngx_dlssg.dll ({rel}) — игре есть что подменять, генерация кадров должна появиться в настройках."
            ),
        ));
    } else {
        scan.checks.push(Check::new(
            Level::Warn,
            "nvngx_dlssg.dll в папке игры не найден",
            "Мод подменяет загрузку DLSS-G. Без этого плагина игра, скорее всего, не умеет Frame Generation — установка ничего не изменит. Проверьте, что выбрана папка с EXE рендера (например, ...\\Binaries\\Win64).".to_string(),
        ));
    }

    let anticheat = find_anticheat(dir);
    if let Some(ac) = &anticheat {
        scan.checks.push(Check::new(
            Level::Bad,
            "Обнаружен античит",
            format!(
                "{} — прокси-DLL рядом с античитом может привести к бану. Устанавливайте только если уверены, что античит не проверяет целостность файлов.",
                ac.file_name().unwrap_or_default().to_string_lossy()
            ),
        ));
    } else if best
        .as_ref()
        .map(|e| ONLINE_AC_EXES.contains(&e.name.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
    {
        // The anti-cheat of these games does not live inside the game folder.
        scan.checks.push(Check::new(
            Level::Warn,
            "Игра с онлайновым античитом",
            format!(
                "{} — античит этой игры (Ricochet / VAC / BattlEye) не лежит в папке игры, поэтому проверить его нельзя. Подмена DLL в онлайне может закончиться баном; в офлайне риск ниже.",
                best.as_ref().map(|e| e.name.as_str()).unwrap_or_default()
            ),
        ));
    } else {
        scan.checks.push(Check::new(
            Level::Ok,
            "Античит не обнаружен",
            "Файлы EasyAntiCheat / BattlEye / Vanguard рядом с игрой не найдены — подмена DLL не блокируется.".to_string(),
        ));
    }

    if scan.running.is_empty() {
        scan.checks.push(Check::new(
            Level::Info,
            "Игра не запущена",
            "Файлы не заблокированы, установка возможна.".to_string(),
        ));
    } else {
        scan.checks.push(Check::new(
            Level::Bad,
            "Игра запущена",
            format!("Закройте процесс: {}", scan.running.join(", ")),
        ));
    }

    if scan.writable {
        scan.checks.push(Check::new(
            Level::Ok,
            "Папка доступна для записи",
            "Права администратора не нужны.".to_string(),
        ));
    } else {
        scan.checks.push(Check::new(
            Level::Bad,
            "Нет прав на запись",
            "Папка защищена (Program Files?). Запустите программу от имени администратора."
                .to_string(),
        ));
    }

    if let Some(installed) = &scan.installed {
        let unknown = installed.version.trim().is_empty()
            || installed.version.eq_ignore_ascii_case("неизвестна")
            || installed.version.eq_ignore_ascii_case("unknown");
        let fresh = pkg_version
            .map(|v| crate::model::version_gt(v, &installed.version))
            .unwrap_or(false);
        if unknown {
            scan.checks.push(Check::new(
                Level::Warn,
                "Мод уже установлен (версия неизвестна)",
                format!(
                    "Прокси: {}. Похоже, его ставили без метки .dlssg-manager\\install.json — поставьте текущую версию пакета, чтобы версия определялась.",
                    installed.proxy
                ),
            ));
        } else if fresh {
            scan.checks.push(Check::new(
                Level::Warn,
                format!("Установлена версия {}", installed.version),
                "Доступна более новая версия пакета — обновите через вкладку «Обновления»."
                    .to_string(),
            ));
        } else {
            scan.checks.push(Check::new(
                Level::Ok,
                format!("Уже установлено: {}", installed.version),
                format!("Прокси: {}. Можно переустановить или удалить.", installed.proxy),
            ));
        }
    } else {
        scan.checks.push(Check::new(
            Level::Info,
            "Мод не установлен",
            "Файлов dlssg_sm86 в папке нет.".to_string(),
        ));
    }

    if !scan.conflicts.is_empty() {
        scan.checks.push(Check::new(
            Level::Warn,
            "В папке есть чужие DLL с теми же именами",
            format!(
                "{}. При установке они будут сохранены в .dlssg-manager\\backups и восстановлены при удалении.",
                scan.conflicts.join(", ")
            ),
        ));
    }
    if scan.our_proxies.len() > 1 {
        scan.checks.push(Check::new(
            Level::Warn,
            "Несколько прокси мода в одной папке",
            format!(
                "{} — работает только первый загруженный, остальные лучше убрать (это делает установщик).",
                scan.our_proxies.join(", ")
            ),
        ));
    }

    scan.verdict = if scan.bad_count() > 0 {
        Verdict::Unsupported
    } else if scan.warn_count() > 0 {
        Verdict::Attention
    } else {
        Verdict::Ready
    };
    scan
}

/// Suggested proxy order for the "try each one" workflow.
pub fn proxy_hint(dir: &Path, proxy: &str) -> String {
    let _ = dir;
    match proxy {
        "version.dll" => crate::i18n::t("Лучший выбор: его загружает почти любая игра.").into(),
        "winmm.dll" => crate::i18n::t("Второй по надёжности (мультимедиа-API).").into(),
        "dbghelp.dll" => crate::i18n::t("Третий: его часто грузят игры и оверлеи.").into(),
        "dinput8.dll" => crate::i18n::t("Четвёртый: старый DirectInput8.").into(),
        "dxgi.dll" => crate::i18n::t("Рискованно: стоит в рендер-пути, но встречается часто.").into(),
        "d3d12.dll" => crate::i18n::t("Последний вариант: только если dxgi.dll не сработал.").into(),
        _ => String::new(),
    }
}

pub fn proxy_priority(proxy: &str) -> usize {
    PROXIES
        .iter()
        .position(|p| p.eq_ignore_ascii_case(proxy))
        .map(|i| i + 1)
        .unwrap_or(99)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_folder_reports_error() {
        let scan = scan_game(Path::new("Z:/definitely/not/here"), &GpuInfo::default(), None);
        assert_eq!(scan.verdict, Verdict::Unsupported);
        assert!(!scan.checks.is_empty());
    }

    #[test]
    fn hand_made_install_and_online_anticheat_are_reported() {
        let dir = std::env::temp_dir().join(format!("dlssgm-scan-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(crate::model::PAYLOAD_DIR)).unwrap();
        std::fs::write(dir.join("cod.exe"), b"MZ").unwrap();
        std::fs::write(dir.join("version.dll"), b"....dlssg_sm86....").unwrap();
        std::fs::write(dir.join("nvngx_dlssg.dll"), b"x").unwrap();

        let scan = scan_game(&dir, &GpuInfo::default(), Some("0.4.1"));
        let titles: Vec<&str> = scan.checks.iter().map(|c| c.title.as_str()).collect();
        assert!(scan.installed.is_some());
        assert!(
            titles.contains(&"Мод уже установлен (версия неизвестна)"),
            "{titles:?}"
        );
        assert!(titles.contains(&"Игра с онлайновым античитом"), "{titles:?}");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
