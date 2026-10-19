//! 蜂巢用户工作台的共享视觉令牌。
//!
//! 这些值对应 `frontend/src/styles/pages/messenger-shell.css` 的最终浅色
//! 主题。egui 端不从 CSS 读取运行时样式，集中保留一份令牌可以让 native
//! 与 WASM 的绘制保持相同的尺寸和颜色，也避免在每个组件里散落颜色常量。

use egui::{
    Color32, Context, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin, Stroke,
    TextStyle, Vec2, Visuals,
};
use std::sync::Arc;

// Keep the native and WASM shells independent from the fonts installed on the
// host.  The Slint shell already carries this exact Microsoft YaHei TTC; use
// the same bytes and face index here so Chinese text has identical glyphs in
// both front ends.  A TTC is accepted by egui/ab_glyph through `FontData::index`.
const BUNDLED_YAHEI_TTC: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../frontend-slint/assets/fonts/msyh.ttc"
));

fn bundled_fonts() -> FontDefinitions {
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert(
        crate::icons::FONT_NAME.to_owned(),
        Arc::new(FontData::from_static(crate::icons::FONT_BYTES)),
    );
    fonts.families.insert(
        FontFamily::Name(crate::icons::FONT_NAME.into()),
        vec![crate::icons::FONT_NAME.to_owned()],
    );
    fonts.font_data.insert(
        "MicrosoftYaHei".to_owned(),
        Arc::new(FontData {
            font: std::borrow::Cow::Borrowed(BUNDLED_YAHEI_TTC),
            index: 0,
            tweak: Default::default(),
        }),
    );
    // Use YaHei as the UI face.  For monospace text keep Hack first so code
    // remains aligned; YaHei is still selected for CJK glyphs missing there.
    if let Some(entries) = fonts.families.get_mut(&FontFamily::Proportional) {
        entries.insert(0, "MicrosoftYaHei".to_owned());
    }
    if let Some(entries) = fonts.families.get_mut(&FontFamily::Monospace) {
        entries.push("MicrosoftYaHei".to_owned());
    }
    fonts
}

#[derive(Clone, Copy)]
pub struct Tokens {
    pub canvas: Color32,
    pub surface: Color32,
    pub sidebar: Color32,
    pub hover: Color32,
    pub selected: Color32,
    pub border: Color32,
    pub border_strong: Color32,
    pub text: Color32,
    pub text_secondary: Color32,
    pub text_muted: Color32,
    pub primary: Color32,
    pub primary_soft: Color32,
    pub primary_tint: Color32,
    pub timeline_line: Color32,
    pub success: Color32,
    pub warning: Color32,
    pub danger: Color32,
}

impl Tokens {
    /// 蜂巢默认的 terracotta 浅色主题（`--mz-*` tokens）。
    pub const fn light() -> Self {
        Self {
            canvas: Color32::from_rgb(0xfb, 0xfa, 0xf8),
            surface: Color32::from_rgb(0xff, 0xff, 0xff),
            sidebar: Color32::from_rgb(0xf6, 0xf5, 0xf3),
            // Slint and the final messenger shell use the softer #F6F5F3
            // hover, while #F1EFEC is reserved for selected rows.
            hover: Color32::from_rgb(0xf6, 0xf5, 0xf3),
            selected: Color32::from_rgb(0xf1, 0xef, 0xec),
            border: Color32::from_rgb(0xe8, 0xe6, 0xe3),
            border_strong: Color32::from_rgb(0xd8, 0xd5, 0xd0),
            text: Color32::from_rgb(0x1f, 0x23, 0x29),
            text_secondary: Color32::from_rgb(0x3d, 0x3d, 0x3d),
            text_muted: Color32::from_rgb(0x8a, 0x8f, 0x99),
            primary: Color32::from_rgb(0xc9, 0x64, 0x43),
            primary_soft: Color32::from_rgb(0xdf, 0xac, 0x9a),
            primary_tint: Color32::from_rgb(0xf6, 0xe9, 0xe3),
            timeline_line: Color32::from_rgb(0xe2, 0xdf, 0xda),
            success: Color32::from_rgb(0x1a, 0x8a, 0x5c),
            warning: Color32::from_rgb(0xc9, 0x8a, 0x2b),
            danger: Color32::from_rgb(0xd0, 0x4a, 0x43),
        }
    }
}

pub fn apply(ctx: &Context) {
    let tokens = Tokens::light();
    // Install the bundled face before the first frame lays out any text.  This
    // avoids the tofu fallback seen when only egui's Latin defaults are used.
    ctx.set_fonts(bundled_fonts());
    let mut visuals = Visuals::light();

    visuals.override_text_color = Some(tokens.text);
    visuals.panel_fill = tokens.canvas;
    visuals.window_fill = tokens.surface;
    visuals.faint_bg_color = tokens.hover;
    visuals.extreme_bg_color = tokens.canvas;
    visuals.window_corner_radius = CornerRadius::same(12);
    visuals.menu_corner_radius = CornerRadius::same(10);
    visuals.widgets.noninteractive.bg_fill = tokens.surface;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, tokens.border);
    visuals.widgets.noninteractive.corner_radius = CornerRadius::same(8);
    visuals.widgets.inactive.bg_fill = tokens.surface;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, tokens.border);
    visuals.widgets.inactive.corner_radius = CornerRadius::same(8);
    visuals.widgets.hovered.bg_fill = tokens.hover;
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, tokens.primary_soft);
    visuals.widgets.hovered.corner_radius = CornerRadius::same(8);
    visuals.widgets.active.bg_fill = tokens.primary;
    visuals.widgets.active.bg_stroke = Stroke::new(1.0, tokens.primary);
    visuals.widgets.active.fg_stroke = Stroke::new(1.0, Color32::WHITE);
    visuals.widgets.active.corner_radius = CornerRadius::same(8);
    visuals.selection.bg_fill = tokens.primary_tint;
    visuals.selection.stroke = Stroke::new(1.0, tokens.primary);

    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = Vec2::new(8.0, 6.0);
    style.spacing.button_padding = Vec2::new(10.0, 6.0);
    style.spacing.interact_size = Vec2::new(24.0, 28.0);
    style.spacing.window_margin = Margin::symmetric(16, 16);
    style
        .text_styles
        .insert(TextStyle::Small, FontId::proportional(11.0));
    style
        .text_styles
        .insert(TextStyle::Body, FontId::proportional(14.0));
    style
        .text_styles
        .insert(TextStyle::Button, FontId::proportional(13.0));
    style
        .text_styles
        .insert(TextStyle::Heading, FontId::proportional(15.0));
    ctx.set_style(style);
    ctx.set_visuals(visuals);
    // Let egui/eframe honor the platform scale. Browser CSS pixels and native
    // DPI then remain readable on HiDPI displays while the logical layout
    // metrics still match the 240/1000px 蜂巢 measurements.
}

pub fn tokens() -> Tokens {
    Tokens::light()
}

/// The web polish layer adds a faint vertical tint to user messages. Share a
/// two-pixel texture across bubbles, so rounded clipping remains native and
/// the gradient costs one cached texture rather than a per-frame image.
pub fn paint_user_bubble(ui: &egui::Ui, rect: egui::Rect) {
    let id = egui::Id::new("hive-user-bubble-gradient");
    let cached = ui
        .ctx()
        .data_mut(|data| data.get_temp::<egui::TextureHandle>(id));
    let texture = match cached {
        Some(texture) => texture,
        None => {
            let image = egui::ColorImage::new(
                [1, 2],
                vec![
                    Color32::from_rgb(246, 236, 231),
                    Color32::from_rgb(249, 244, 240),
                ],
            );
            let texture = ui
                .ctx()
                .load_texture("user-bubble", image, egui::TextureOptions::LINEAR);
            ui.ctx()
                .data_mut(|data| data.insert_temp(id, texture.clone()));
            texture
        }
    };
    ui.painter().add(
        egui::epaint::RectShape::filled(rect, 12.0, Color32::WHITE).with_texture(
            texture.id(),
            egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
        ),
    );
}
