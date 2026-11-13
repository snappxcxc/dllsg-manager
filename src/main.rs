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

/// Position that puts a `w` x `h` window in the middle of the primary work area.
#[cfg(windows)]
fn centered_position(w: f32, h: f32) -> Option<eframe::egui::Pos2> {
    use windows_sys::Win32::Foundation::RECT;
    use windows_sys::Win32::Graphics::Gdi::{GetDC, GetDeviceCaps, ReleaseDC, LOGPIXELSX};
    use windows_sys::Win32::UI::WindowsAndMessaging::{SystemParametersInfoW, SPI_GETWORKAREA};

    unsafe {
        let mut area = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        let ok = SystemParametersInfoW(
            SPI_GETWORKAREA,
            0,
            &mut area as *mut RECT as *mut core::ffi::c_void,
            0,
        );
        if ok == 0 {
            return None;
        }
        let hdc = GetDC(0);
        let dpi = if hdc != 0 {
            GetDeviceCaps(hdc, LOGPIXELSX as i32) as f32
        } else {
            96.0
        };
        if hdc != 0 {
            ReleaseDC(0, hdc);
        }
        let scale = if dpi > 0.0 { dpi / 96.0 } else { 1.0 };
        let left = area.left as f32 / scale;
        let top = area.top as f32 / scale;
        let area_w = (area.right - area.left) as f32 / scale;
        let area_h = (area.bottom - area.top) as f32 / scale;
        let size = if w >= 940.0 && h >= 640.0 {
            (w, h)
        } else {
            (1180.0, 800.0)
        };
        let x = left + ((area_w - size.0) / 2.0).max(0.0);
        let y = top + ((area_h - size.1) / 3.0).max(0.0);
        Some(eframe::egui::pos2(x, y))
    }
}

#[cfg(not(windows))]
fn centered_position(_w: f32, _h: f32) -> Option<eframe::egui::Pos2> {
    None
}

fn main() -> eframe::Result<()> {    install_panic_hook();

    // Resolve the UI language before anything is drawn.
    let cfg = dlssgm::config::Config::load();
    dlssgm::i18n::set_lang(cfg.lang());

    let (width, height, rgba) = app::app_icon_data();
    let win = cfg.window;
    let mut viewport = eframe::egui::ViewportBuilder::default()
        .with_title(dlssgm::i18n::t("DLSSG Manager — Frame Generation для RTX 20/30"))
        .with_min_inner_size([940.0, 640.0])
        .with_app_id("dlssg-manager")
        .with_icon(Arc::new(eframe::egui::IconData {
            rgba,
            width,
            height,
        }));
    viewport = viewport.with_inner_size(if win.has_size() {
        [win.w, win.h]
    } else {
        [1180.0, 800.0]
    });
    if win.has_position() {
        viewport = viewport.with_position(eframe::egui::pos2(win.x, win.y));
    } else if let Some(pos) = centered_position(win.w, win.h) {
        viewport = viewport.with_position(pos);
    }
    if win.maximized {
        viewport = viewport.with_maximized(true);
    }
    let options = eframe::NativeOptions {
        viewport,
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
