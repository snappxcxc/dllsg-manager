use eframe::egui::{self, Color32, FontData, FontDefinitions, FontFamily, FontId, Rounding, Stroke, TextStyle};

pub const BG: Color32 = Color32::from_rgb(11, 13, 18);
pub const PANEL: Color32 = Color32::from_rgb(19, 23, 31);
pub const PANEL_2: Color32 = Color32::from_rgb(26, 31, 42);
pub const BORDER: Color32 = Color32::from_rgb(42, 50, 66);
pub const TEXT: Color32 = Color32::from_rgb(226, 232, 240);
pub const MUTED: Color32 = Color32::from_rgb(139, 150, 168);
pub const ACCENT: Color32 = Color32::from_rgb(118, 223, 74);
pub const ACCENT_DARK: Color32 = Color32::from_rgb(38, 78, 32);
pub const BLUE: Color32 = Color32::from_rgb(96, 165, 250);
pub const YELLOW: Color32 = Color32::from_rgb(250, 204, 21);
pub const RED: Color32 = Color32::from_rgb(248, 113, 113);

pub fn setup_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    // Cyrillic coverage: try a few fonts that ship with Windows 10/11.
    let candidates = [
        r"C:\Windows\Fonts\segoeui.ttf",
        r"C:\Windows\Fonts\arial.ttf",
        r"C:\Windows\Fonts\tahoma.ttf",
    ];
    for path in candidates {
        if let Ok(bytes) = std::fs::read(path) {
            fonts
                .font_data
                .insert("ui".to_owned(), FontData::from_owned(bytes).into());
            fonts
                .families
                .entry(FontFamily::Proportional)
                .or_default()
                .insert(0, "ui".to_owned());
            break;
        }
    }
    if let Ok(bytes) = std::fs::read(r"C:\Windows\Fonts\consola.ttf") {
        fonts
            .font_data
            .insert("mono".to_owned(), FontData::from_owned(bytes).into());
        fonts
            .families
            .entry(FontFamily::Monospace)
            .or_default()
            .insert(0, "mono".to_owned());
    }
    ctx.set_fonts(fonts);
}

pub fn apply(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    style.visuals = egui::Visuals::dark();
    let v = &mut style.visuals;
    v.panel_fill = BG;
    v.window_fill = PANEL;
    v.extreme_bg_color = Color32::from_rgb(8, 10, 14);
    v.faint_bg_color = PANEL_2;
    v.override_text_color = Some(TEXT);
    v.window_stroke = Stroke::new(1.0, BORDER);
    v.widgets.noninteractive.bg_fill = PANEL;
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, BORDER);
    v.widgets.inactive.bg_fill = PANEL_2;
    v.widgets.inactive.weak_bg_fill = PANEL_2;
    v.widgets.inactive.bg_stroke = Stroke::new(1.0, BORDER);
    v.widgets.hovered.bg_fill = Color32::from_rgb(34, 41, 55);
    v.widgets.hovered.weak_bg_fill = Color32::from_rgb(34, 41, 55);
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, ACCENT);
    v.widgets.active.bg_fill = ACCENT_DARK;
    v.widgets.active.weak_bg_fill = ACCENT_DARK;
    v.selection.bg_fill = ACCENT_DARK;
    v.hyperlink_color = BLUE;
    for w in [
        &mut v.widgets.noninteractive,
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
        &mut v.widgets.open,
    ] {
        w.rounding = Rounding::same(8.0);
    }
    style.spacing.item_spacing = egui::vec2(10.0, 10.0);
    style.spacing.button_padding = egui::vec2(14.0, 8.0);
    style.text_styles = [
        (TextStyle::Heading, FontId::proportional(24.0)),
        (TextStyle::Body, FontId::proportional(14.5)),
        (TextStyle::Button, FontId::proportional(14.5)),
        (TextStyle::Small, FontId::proportional(12.0)),
        (TextStyle::Monospace, FontId::monospace(12.5)),
    ]
    .into();
    ctx.set_style(style);
}

pub fn card(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::none()
        .fill(PANEL)
        .rounding(Rounding::same(12.0))
        .stroke(Stroke::new(1.0, BORDER))
        .inner_margin(egui::Margin::same(16.0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui);
        });
}

pub fn pill(ui: &mut egui::Ui, text: &str, color: Color32) {
    let text = dlssgm::i18n::tr(text).into_owned();
    let galley = ui.painter().layout_no_wrap(
        text,
        FontId::proportional(12.5),
        color,
    );
    let pad = egui::vec2(9.0, 4.0);
    let (rect, _) = ui.allocate_exact_size(galley.size() + pad * 2.0, egui::Sense::hover());
    let bg = Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 28);
    ui.painter().rect_filled(rect, Rounding::same(999.0), bg);
    ui.painter().rect_stroke(rect, Rounding::same(999.0), Stroke::new(1.0, color.gamma_multiply(0.6)));
    ui.painter()
        .galley(rect.min + pad, galley, color);
}

pub fn big_button(ui: &mut egui::Ui, text: &str, enabled: bool, accent: bool) -> egui::Response {
    let text = dlssgm::i18n::tr(text).into_owned();
    let font = FontId::proportional(15.5);
    let galley = ui
        .painter()
        .layout_no_wrap(text.clone(), font.clone(), if accent { BG } else { TEXT });
    let size = egui::vec2(galley.size().x + 42.0, 40.0);
    let sense = if enabled {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    };
    let (rect, resp) = ui.allocate_exact_size(size, sense);
    let hovered = resp.hovered() && enabled;
    let fill = if !enabled {
        Color32::from_rgb(30, 35, 45)
    } else if accent {
        if hovered { Color32::from_rgb(140, 240, 100) } else { ACCENT }
    } else if hovered {
        Color32::from_rgb(34, 41, 55)
    } else {
        PANEL_2
    };
    ui.painter().rect_filled(rect, Rounding::same(10.0), fill);
    if !accent {
        ui.painter()
            .rect_stroke(rect, Rounding::same(10.0), Stroke::new(1.0, BORDER));
    }
    let color = if !enabled {
        MUTED
    } else if accent {
        BG
    } else {
        TEXT
    };
    let galley = ui.painter().layout_no_wrap(text, font, color);
    let pos = rect.center() - galley.size() / 2.0;
    ui.painter().galley(pos, galley, color);
    if hovered {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

pub fn section_title(ui: &mut egui::Ui, text: &str) {
    ui.label(
        egui::RichText::new(dlssgm::i18n::tr(text).to_uppercase())
            .size(11.5)
            .color(MUTED)
            .strong(),
    );
    ui.add_space(2.0);
}

pub fn kv(ui: &mut egui::Ui, key: &str, value: &str) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(dlssgm::i18n::tr(key)).color(MUTED).size(13.0));
        ui.label(
            egui::RichText::new(dlssgm::i18n::tr(value))
                .color(TEXT)
                .size(13.0),
        );
    });
}
