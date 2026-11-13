use dlssgm::config::Config;
use dlssgm::model::{GameScan, Level, Release};
use dlssgm::system::{self};
use dlssgm::{config, gamedetect, github, installer, packages, scan};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

fn print_checks(scan: &GameScan) {
    println!("\n== {} ==", scan.dir.display());
    println!("Вердикт: {}", scan.verdict.title());
    for c in &scan.checks {
        let mark = match c.level {
            Level::Ok => "[ OK ]",
            Level::Warn => "[ !! ]",
            Level::Bad => "[FAIL]",
            Level::Info => "[ -- ]",
        };
        println!("{mark} {}: {}", c.title, c.detail);
    }
    if let Some(exe) = scan.selected_exe() {
        println!("\nРекомендуемый EXE: {} ({})", exe.name, dlssgm::util::human_size(exe.size));
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let gpu = system::gpu_info();
    println!("GPU: {} | драйвер {} | {}", gpu.name, gpu.driver_text(), gpu.arch.label());
    println!("ОС: {}", system::windows_version());

    match args.first().map(|s| s.as_str()) {
        Some("scan") => {
            let dir = args.get(1).map(PathBuf::from).unwrap_or_default();
            let s = scan::scan_game(&dev_null(&dir), &gpu, None);
            print_checks(&s);
        }
        Some("latest") => match github::latest_release() {
            Ok(r) => println!("Последний релиз: {} ({}) {}", r.version(), r.asset_name, r.asset_url),
            Err(e) => println!("Ошибка: {e}"),
        },
        Some("games") => {
            let cancel = AtomicBool::new(false);
            println!("Поиск игр…");
            let list = gamedetect::discover(
                &mut |p, m| {
                    print!("\r{:>5.1}% {m:<70}", p * 100.0);
                    use std::io::Write;
                    let _ = std::io::stdout().flush();
                },
                &cancel,
            );
            println!("\n");
            if list.is_empty() {
                println!("Игры не найдены.");
            }
            for (i, g) in list.iter().enumerate() {
                println!(
                    "{:>2}. {}{}\n    {}  [{}]\n    {} ({})",
                    i + 1,
                    g.name,
                    if g.dlssg { "  · DLSS-G" } else if g.has_dlss { "  · DLSS" } else { "" },
                    g.dir.display(),
                    g.source,
                    g.exe,
                    dlssgm::util::human_size(g.exe_size)
                );
            }
        }
        Some("check") => {
            let cfg = config::Config::load();
            match github::latest_release() {
                Ok(r) => {
                    let installed = cfg.latest_installed_version();
                    println!("Последний релиз: {}", r.version());
                    println!("Установлено: {}", installed.clone().unwrap_or_else(|| "нет".into()));
                    if let Some(v) = installed {
                        println!("{}", if dlssgm::model::version_gt(&r.version(), &v) { "Доступно обновление" } else { "Актуально" });
                    }
                }
                Err(e) => println!("Ошибка: {e}"),
            }
        }
        Some("download") => {
            let rel: Release = match github::latest_release() {
                Ok(r) => r,
                Err(e) => {
                    println!("Ошибка: {e}");
                    return;
                }
            };
            let dest = dlssgm::util::downloads_dir().join(&rel.asset_name);
            let cancel = AtomicBool::new(false);
            match github::download(&rel.asset_url, &dest, rel.size, &mut |p, msg| {
                println!("{:>5.1}% {msg}", p * 100.0);
            }, &cancel) {
                Ok(_) => match packages::install_zip_into_cache(&dest, &mut |p, m| println!("{:.0}% {m}", p * 100.0)) {
                    Ok(pkg) => println!("Пакет {} готов: {}", pkg.version, pkg.dir.display()),
                    Err(e) => println!("Ошибка: {e}"),
                },
                Err(e) => println!("Ошибка: {e}"),
            }
        }
        Some("install") => {
            let dir = PathBuf::from(args.get(1).cloned().unwrap_or_default());
            let proxy = args.get(2).cloned().unwrap_or_else(|| "version.dll".into());
            let pkg = packages::best_cached().or_else(|| {
                let _ = install_local(&args);
                packages::best_cached()
            });
            let Some(pkg) = pkg else {
                println!("Нет пакета в кэше. Скачайте: dlssgm-cli download");
                return;
            };
            let s = scan::scan_game(&dir, &gpu, Some(&pkg.version));
            let exe = s.selected_exe().map(|e| e.name.clone()).unwrap_or_default();
            let opts = installer::InstallOpts { dir: &dir, exe: &exe, pkg: &pkg, proxy: &proxy, remove_extra_proxies: true };
            match installer::install(opts, &mut |_, _| {}, &mut |l| println!("  {l}")) {
                Ok(rec) => {
                    let mut cfg = Config::load();
                    cfg.upsert_install(rec.clone());
                    let _ = cfg.save();
                    println!("Установлено {} в {}", rec.version, rec.path.display());
                }
                Err(e) => println!("Ошибка: {e}"),
            }
        }
        Some("uninstall") => {
            let dir = PathBuf::from(args.get(1).cloned().unwrap_or_default());
            match installer::uninstall(&dir, &mut |_, _| {}, &mut |l| println!("  {l}")) {
                Ok(_) => {
                    let mut cfg = Config::load();
                    cfg.remove_install(&dir);
                    let _ = cfg.save();
                    println!("Удалено");
                }
                Err(e) => println!("Ошибка: {e}"),
            }
        }
        Some("import") => {
            let _ = install_local(&args);
        }
        _ => {
            println!("\nDLSSG Manager CLI");
            println!("  dlssgm-cli scan <папка игры>");
            println!("  dlssgm-cli games");
            println!("  dlssgm-cli latest");
            println!("  dlssgm-cli check");
            println!("  dlssgm-cli download");
            println!("  dlssgm-cli import <папка пакета>");
            println!("  dlssgm-cli install <папка игры> [прокси]");
            println!("  dlssgm-cli uninstall <папка игры>");
        }
    }
}

fn dev_null(p: &PathBuf) -> PathBuf {
    p.clone()
}

fn install_local(args: &[String]) -> Result<(), String> {
    let src = PathBuf::from(args.get(1).cloned().unwrap_or_default());
    let pkg = packages::import_from_dir(&src, &mut |p, m| println!("{:.0}% {m}", p * 100.0))?;
    println!("Импортирован пакет {}: {}", pkg.version, pkg.dir.display());
    Ok(())
}
