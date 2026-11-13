//! Embeds `assets/app.ico` (and the version block) into the executables.
//!
//! * `*-pc-windows-gnu` targets use MinGW's `windres`, which produces a COFF
//!   object the linker accepts.
//! * `*-pc-windows-msvc` targets use `rc.exe`, which produces a `.res` file that
//!   `link.exe` consumes directly. `rc.exe` lives inside the Visual Studio
//!   installation and is not always on `PATH` (GitHub Actions runners, plain
//!   `cargo build` from a shell), so it is looked up via `vswhere` as well.
//!
//! When neither tool is available the build still succeeds — the exe is simply
//! built without an icon, and cargo prints a warning.

use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=assets/app.rc");
    println!("cargo:rerun-if-changed=assets/app.ico");

    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));

    if env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        let resource = out_dir.join("app-resource.res");
        for rc in rc_candidates() {
            // (a) from the crate root, icons resolved through the include path
            let _ = std::fs::remove_file(&resource);
            let from_root = Command::new(&rc)
                .arg("/nologo")
                .arg("/i")
                .arg("assets")
                .arg("/fo")
                .arg(&resource)
                .arg("assets/app.rc")
                .status();
            if matches!(from_root, Ok(s) if s.success()) && resource.is_file() {
                println!("cargo:rustc-link-arg={}", resource.display());
                return;
            }
            // (b) from inside assets/, where app.ico sits next to the .rc
            let _ = std::fs::remove_file(&resource);
            let from_assets = Command::new(&rc)
                .current_dir("assets")
                .arg("/nologo")
                .arg("/fo")
                .arg(&resource)
                .arg("app.rc")
                .status();
            if matches!(from_assets, Ok(s) if s.success()) && resource.is_file() {
                println!("cargo:rustc-link-arg={}", resource.display());
                return;
            }
        }
        println!(
            "cargo:warning=rc.exe не найден или не смог собрать ресурс — exe без иконки и версии"
        );
        return;
    }

    let object = out_dir.join("app-resource.o");
    for tool in ["windres", "x86_64-w64-mingw32-windres"] {
        let status = Command::new(tool)
            .arg("-i")
            .arg("assets/app.rc")
            .arg("-o")
            .arg(&object)
            .arg("-I")
            .arg("assets")
            .arg("-O")
            .arg("coff")
            .status();
        match status {
            Ok(status) if status.success() && object.is_file() => {
                // `rustc-link-arg` also covers the second binary and tests.
                println!("cargo:rustc-link-arg={}", object.display());
                return;
            }
            _ => continue,
        }
    }
    println!("cargo:warning=windres не найден — exe будет собран без иконки и версии");
}

/// `rc.exe` from `PATH`, then from the newest Visual Studio installation.
fn rc_candidates() -> Vec<PathBuf> {
    let mut out = vec![PathBuf::from("rc")];

    // Build Tools / VS installations, found through vswhere.
    let vswhere = env::var("ProgramFiles(x86)")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(r"C:\Program Files (x86)"))
        .join("Microsoft Visual Studio")
        .join("Installer")
        .join("vswhere.exe");

    if let Ok(output) = Command::new(&vswhere)
        .args(["-latest", "-products", "*", "-property", "installationPath"])
        .output()
    {
        let root = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !root.is_empty() {
            for rc in rc_in_tools(Path::new(&root)) {
                out.push(rc);
            }
        }
    }

    // Direct COM/registry fallback: scan the usual install roots.
    for base in [
        r"C:\Program Files\Microsoft Visual Studio",
        r"C:\Program Files (x86)\Microsoft Visual Studio",
    ] {
        for edition in ["BuildTools", "Community", "Professional", "Enterprise"] {
            for year in ["2022", "2019"] {
                let root = Path::new(base).join(year).join(edition);
                for rc in rc_in_tools(&root) {
                    out.push(rc);
                }
            }
        }
    }
    out
}

fn rc_in_tools(root: &Path) -> Vec<PathBuf> {
    let msvc = root.join("VC").join("Tools").join("MSVC");
    let Ok(entries) = std::fs::read_dir(&msvc) else {
        return Vec::new();
    };
    let mut versions: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
    versions.sort();
    versions.reverse();
    let mut out = Vec::new();
    for version in versions {
        for host in [r"Hostx64\x64", r"Hostx86\x64", r"Hostx64\x86"] {
            let rc = version.join(host).join("rc.exe");
            if rc.is_file() {
                out.push(rc);
            }
        }
    }
    out
}
