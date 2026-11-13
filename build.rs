//! Embeds `assets/app.ico` (and the version block) into the executables.
//!
//! * `*-pc-windows-gnu` targets use MinGW's `windres`, which produces a COFF
//!   object the linker accepts.
//! * `*-pc-windows-msvc` targets use `rc.exe` (shipped with Visual Studio),
//!   which produces a `.res` file that `link.exe` consumes directly.
//!
//! When neither tool is available the build still succeeds — the exe is simply
//! built without an icon, and cargo prints a warning.

use std::env;
use std::path::PathBuf;
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
        let status = Command::new("rc")
            .arg("/nologo")
            .arg("/fo")
            .arg(&resource)
            .arg("assets/app.rc")
            .status();
        if let Ok(status) = status {
            if status.success() && resource.is_file() {
                println!("cargo:rustc-link-arg={}", resource.display());
                return;
            }
        }
        println!("cargo:warning=rc.exe не найден — exe будет собран без иконки и версии");
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
