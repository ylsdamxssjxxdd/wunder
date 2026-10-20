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
    /// Application canvas behind the workbench panes.
    pub canvas: Color32,
    /// Raised cards, menus, popovers and text input backgrounds.
    pub surface: Color32,
    pub sidebar: Color32,
    /// Pointer-over fill for rows and passive icon buttons.
    pub hover: Color32,
    /// Persistently selected rows and open menu entries.
    pub selected: Color32,
    pub border: Color32,
    pub border_strong: Color32,
    pub text: Color32,
    pub text_secondary: Color32,
    pub text_muted: Color32,
    pub primary: Color32,
    pub primary_soft: Color32,
    pub primary_tint: Color32,
    /// Keyboard focus ring and the text caret.
    pub focus: Color32,
    /// Muted text and icons for unavailable actions. Disabled controls also
    /// receive `Visuals::disabled_alpha` so they remain distinguishable when
    /// color alone is unavailable.
    pub disabled: Color32,
    /// Fenced Markdown code background.
    pub code_background: Color32,
    pub code_inline: Color32,
    /// Secondary workflow-step text, distinct from general muted copy.
    pub process: Color32,
    /// Success chip colors used by completed user turns.
    pub success_tint: Color32,
    pub success_border: Color32,
    /// Overlay and elevation are premultiplied because `Color32` stores alpha
    /// that way. `scrim` is equivalent to rgba(31, 35, 41, .32).
    pub scrim: Color32,
    pub shadow: Color32,
    /// The two-pixel vertical user-message gradient.
    pub user_bubble_top: Color32,
    pub user_bubble_bottom: Color32,
    pub timeline_line: Color32,
    pub success: Color32,
    pub warning: Color32,
    pub danger: Color32,
}

impl Tokens {
    /// 蜂巢默认的 terracotta 浅色主题（`--mz-*` tokens）。
    pub fn light() -> Self {
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
            focus: Color32::from_rgb(0xc9, 0x64, 0x43),
            disabled: Color32::from_rgb(0xb7, 0xb9, 0xbd),
            code_background: Color32::from_rgb(0xf5, 0xf4, 0xf2),
            code_inline: Color32::from_rgb(0xf3, 0xf2, 0xf0),
            process: Color32::from_rgb(0x9b, 0xa0, 0xa8),
            success_tint: Color32::from_rgb(0xf1, 0xfd, 0xf5),
            success_border: Color32::from_rgb(0xbe, 0xe6, 0xcc),
            scrim: Color32::from_rgba_unmultiplied(0x1f, 0x23, 0x29, 82),
            shadow: Color32::from_black_alpha(24),
            user_bubble_top: Color32::from_rgb(246, 236, 231),
            user_bubble_bottom: Color32::from_rgb(249, 244, 240),
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
    visuals.weak_text_color = Some(tokens.text_muted);
    visuals.hyperlink_color = tokens.primary;
    visuals.panel_fill = tokens.canvas;
    visuals.window_fill = tokens.surface;
    visuals.window_stroke = Stroke::new(1.0, tokens.border);
    visuals.faint_bg_color = tokens.hover;
    visuals.extreme_bg_color = tokens.canvas;
    visuals.text_edit_bg_color = Some(tokens.surface);
    visuals.code_bg_color = tokens.code_background;
    visuals.warn_fg_color = tokens.warning;
    visuals.error_fg_color = tokens.danger;
    visuals.window_shadow.color = tokens.shadow;
    visuals.popup_shadow.color = tokens.shadow;
    visuals.text_cursor.stroke = Stroke::new(1.5, tokens.focus);
    visuals.disabled_alpha = 0.55;
    visuals.window_corner_radius = CornerRadius::same(12);
    visuals.menu_corner_radius = CornerRadius::same(10);
    visuals.widgets.noninteractive.bg_fill = tokens.surface;
    visuals.widgets.noninteractive.weak_bg_fill = tokens.surface;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, tokens.border);
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, tokens.text_secondary);
    visuals.widgets.noninteractive.corner_radius = CornerRadius::same(8);
    visuals.widgets.inactive.bg_fill = tokens.surface;
    visuals.widgets.inactive.weak_bg_fill = tokens.surface;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, tokens.border);
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, tokens.text_secondary);
    visuals.widgets.inactive.corner_radius = CornerRadius::same(8);
    visuals.widgets.hovered.bg_fill = tokens.hover;
    visuals.widgets.hovered.weak_bg_fill = tokens.hover;
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, tokens.primary_soft);
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, tokens.text);
    visuals.widgets.hovered.corner_radius = CornerRadius::same(8);
    // egui maps keyboard focus to `active`; keep its fill quiet so receiving
    // focus never reflows text or makes a focused input look disabled. The
    // accent stroke, cursor, and selection make focus visible without relying
    // on color alone.
    visuals.widgets.active.bg_fill = tokens.primary_tint;
    visuals.widgets.active.weak_bg_fill = tokens.primary_tint;
    visuals.widgets.active.bg_stroke = Stroke::new(1.0, tokens.focus);
    visuals.widgets.active.fg_stroke = Stroke::new(1.0, tokens.text);
    visuals.widgets.active.corner_radius = CornerRadius::same(8);
    visuals.widgets.open.bg_fill = tokens.selected;
    visuals.widgets.open.weak_bg_fill = tokens.selected;
    visuals.widgets.open.bg_stroke = Stroke::new(1.0, tokens.focus);
    visuals.widgets.open.fg_stroke = Stroke::new(1.0, tokens.text);
    visuals.widgets.open.corner_radius = CornerRadius::same(8);
    visuals.selection.bg_fill = tokens.primary_tint;
    visuals.selection.stroke = Stroke::new(1.0, tokens.focus);

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
                vec![tokens().user_bubble_top, tokens().user_bubble_bottom],
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
        egui::epaint::RectShape::filled(rect, 12.0, tokens().surface).with_texture(
            texture.id(),
            egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
        ),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn light_tokens_define_distinct_interaction_and_content_roles() {
        let tokens = Tokens::light();

        assert_eq!(tokens.canvas, Color32::from_rgb(0xfb, 0xfa, 0xf8));
        assert_eq!(tokens.sidebar, Color32::from_rgb(0xf6, 0xf5, 0xf3));
        assert_eq!(tokens.focus, tokens.primary);
        assert_ne!(tokens.hover, tokens.selected);
        assert_ne!(tokens.code_background, tokens.code_inline);
        assert_ne!(tokens.success_tint, tokens.surface);
        assert!(tokens.scrim.a() > 0);
        assert!(tokens.shadow.a() > 0);
    }

    #[test]
    fn apply_maps_open_focus_and_disabled_states_to_tokens() {
        let ctx = Context::default();
        apply(&ctx);
        let visuals = &ctx.style().visuals;
        let tokens = Tokens::light();

        assert_eq!(visuals.widgets.hovered.bg_fill, tokens.hover);
        assert_eq!(visuals.widgets.active.bg_stroke.color, tokens.focus);
        assert_eq!(visuals.widgets.open.bg_fill, tokens.selected);
        assert_eq!(visuals.text_cursor.stroke.color, tokens.focus);
        assert_eq!(visuals.disabled_alpha, 0.55);
        assert_eq!(visuals.code_bg_color, tokens.code_background);
    }
}
