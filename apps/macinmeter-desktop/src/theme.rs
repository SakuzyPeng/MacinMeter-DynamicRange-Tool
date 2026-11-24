//! The paper palette the Tauri stylesheet defined, and the embedded UI font.
//!
//! The colours are the `:root` tokens of `tauri-app/src/styles.css`; the
//! MacinDecode Spatial Player's theme already uses the same values, so the two
//! desktop apps read as one family.

use eframe::egui::{
    self, Color32, CornerRadius, Stroke,
    epaint::text::{FontInsert, FontPriority, InsertFontFamily},
};

const NOTO_SANS_CJK_SC: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/NotoSansCJKsc-Regular.otf"));

pub const BACKGROUND: Color32 = Color32::from_rgb(0xfb, 0xf7, 0xf0);
pub const SURFACE: Color32 = Color32::from_rgb(0xff, 0xfe, 0xfa);
pub const SURFACE_SOFT: Color32 = Color32::from_rgb(0xf6, 0xf0, 0xe6);
pub const HOVER: Color32 = Color32::from_rgb(0xf1, 0xe9, 0xdc);
pub const TEXT: Color32 = Color32::from_rgb(0x37, 0x2c, 0x20);
pub const TEXT_SECONDARY: Color32 = Color32::from_rgb(0x75, 0x68, 0x55);
pub const TEXT_TERTIARY: Color32 = Color32::from_rgb(0x8a, 0x79, 0x63);
pub const ACCENT: Color32 = Color32::from_rgb(0xce, 0x7a, 0x3b);
pub const ACCENT_STRONG: Color32 = Color32::from_rgb(0xad, 0x5c, 0x25);
pub const ACCENT_SOFT: Color32 = Color32::from_rgb(0xf8, 0xea, 0xdc);
pub const BORDER: Color32 = Color32::from_rgb(0xe6, 0xda, 0xc8);
pub const BORDER_STRONG: Color32 = Color32::from_rgb(0xd5, 0xc5, 0xaf);
pub const DANGER: Color32 = Color32::from_rgb(0xb0, 0x48, 0x3a);
pub const DANGER_SOFT: Color32 = Color32::from_rgb(0xfb, 0xeb, 0xe8);
pub const WARNING: Color32 = Color32::from_rgb(0x91, 0x64, 0x1f);
pub const WARNING_SOFT: Color32 = Color32::from_rgb(0xff, 0xf4, 0xdc);

pub fn install(context: &egui::Context) {
    install_ui_font(context);

    let mut visuals = egui::Visuals::light();
    visuals.override_text_color = Some(TEXT);
    visuals.panel_fill = BACKGROUND;
    visuals.window_fill = SURFACE;
    visuals.faint_bg_color = SURFACE_SOFT;
    visuals.extreme_bg_color = SURFACE;
    visuals.selection.bg_fill = ACCENT_SOFT;
    visuals.selection.stroke = Stroke::new(1.0, ACCENT_STRONG);
    visuals.hyperlink_color = ACCENT_STRONG;
    let radius = CornerRadius::same(6);
    visuals.window_corner_radius = radius;
    visuals.menu_corner_radius = radius;
    for (widgets, fill, stroke) in [
        (&mut visuals.widgets.noninteractive, SURFACE, BORDER),
        (&mut visuals.widgets.inactive, SURFACE, BORDER_STRONG),
        (&mut visuals.widgets.hovered, HOVER, ACCENT),
        (&mut visuals.widgets.active, ACCENT_SOFT, ACCENT_STRONG),
        (&mut visuals.widgets.open, ACCENT_SOFT, ACCENT),
    ] {
        widgets.weak_bg_fill = fill;
        widgets.bg_stroke = Stroke::new(1.0, stroke);
        widgets.corner_radius = radius;
    }
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, TEXT_SECONDARY);
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT);
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, TEXT);
    visuals.widgets.active.fg_stroke = Stroke::new(1.0, TEXT);
    context.set_visuals(visuals);

    context.all_styles_mut(|style| {
        style.spacing.item_spacing = egui::vec2(8.0, 8.0);
        style.spacing.button_padding = egui::vec2(12.0, 6.0);
        style.spacing.interact_size.y = 30.0;
    });
}

fn install_ui_font(context: &egui::Context) {
    context.add_font(FontInsert::new(
        "noto-sans-cjk-sc",
        egui::FontData::from_static(NOTO_SANS_CJK_SC),
        vec![
            InsertFontFamily {
                family: egui::FontFamily::Proportional,
                priority: FontPriority::Highest,
            },
            // Monospace keeps its Latin face and falls back to CJK, so the raw
            // JSON view lines up while a Chinese path inside it still renders.
            InsertFontFamily {
                family: egui::FontFamily::Monospace,
                priority: FontPriority::Lowest,
            },
        ],
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_font_covers_latin_chinese_japanese_and_korean() {
        let context = egui::Context::default();
        install_ui_font(&context);
        context.begin_pass(egui::RawInput::default());
        let (primary, proportional, monospace) = context.fonts_mut(|fonts| {
            let primary = fonts
                .definitions()
                .families
                .get(&egui::FontFamily::Proportional)
                .and_then(|family| family.first())
                .is_some_and(|name| name == "noto-sans-cjk-sc");
            let proportional = fonts.has_glyphs(
                &egui::FontId::proportional(12.0),
                "MacinMeter 原顺序 日本語テスト 한국어",
            );
            let monospace = fonts.has_glyphs(&egui::FontId::monospace(12.0), "中文日本語");
            (primary, proportional, monospace)
        });
        let mut output = context.end_pass();
        output.textures_delta.clear();
        assert!(primary);
        assert!(proportional);
        assert!(monospace);
    }
}
