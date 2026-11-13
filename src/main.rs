//! GUI entry point.
//!
//! The Windows GUI subsystem is used in every profile, so no console window
//! ever appears — even when the program is started from a .bat file or from
//! `cargo run`. Panics are appended to `%LOCALAPPDATA%\DLSSG Manager\manager.log`
//! instead of being printed to a (non-existent) console.
#![windows_subsystem = "windows"]

mod app;
mod theme;

use app::App;
use std::io::Write;
use std::sync::Arc;

fn install_panic_hook() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let line = format!("[{}] PANIC: {info}\n", dlssgm::util::now_human());
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(dlssgm::util::log_path())
        {
            let _ = file.write_all(line.as_bytes());
        }
        default_hook(info);
    }));
}

fn main() -> eframe::Result<()> {
    install_panic_hook();

    // Resolve the UI language before anything is drawn.
    let cfg = dlssgm::config::Config::load();
    dlssgm::i18n::set_lang(cfg.lang());

    let (width, height, rgba) = app::app_icon_data();
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title(dlssgm::i18n::t("DLSSG Manager — Frame Generation для RTX 20/30"))
            .with_inner_size([1180.0, 800.0])
            .with_min_inner_size([940.0, 640.0])
            .with_app_id("dlssg-manager")
            .with_icon(Arc::new(eframe::egui::IconData {
                rgba,
                width,
                height,
            })),
        ..Default::default()
    };
    eframe::run_native(
        "DLSSG Manager",
        options,
        Box::new(|cc| {
            theme::setup_fonts(&cc.egui_ctx);
            theme::apply(&cc.egui_ctx);
            Ok(Box::new(App::new(cc.egui_ctx.clone())))
        }),
    )
}
