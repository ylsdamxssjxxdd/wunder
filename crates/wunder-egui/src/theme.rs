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
    /// Inset background for stat tiles, run logs and chart frames. Sits
    /// inside `surface`, so it must stay one step darker than the card.
    pub surface_sunken: Color32,
    pub sidebar: Color32,
    /// Pointer-over fill for rows, menu entries and passive icon buttons.
    /// Matches the web `--mz-hover`; it must stay darker than `sidebar`,
    /// otherwise a hovered row on the sidebar is invisible.
    pub hover: Color32,
    /// Active thread row and open menu entry (`--mz-selected`).
    pub selected: Color32,
    /// Desktop timeline hover (`--mz-timeline-hover`), one step lighter than
    /// `hover` because it is painted over the canvas rather than the sidebar.
    pub timeline_hover: Color32,
    /// Wash painted behind an assistant block while the pointer rests on it.
    pub reader_hover: Color32,
    pub border: Color32,
    /// Hairline border of raised cards and settings panels.
    pub card_border: Color32,
    /// Separator between rows inside a single card.
    pub divider: Color32,
    /// Neutral chip/pill background for metadata labels.
    pub chip: Color32,
    /// Rest border of passive form controls (selects, pills, steppers).
    pub control_border: Color32,
    /// Rest fill of those controls.
    pub control_fill: Color32,
    /// Border of text editors and text areas.
    pub field_border: Color32,
    pub border_strong: Color32,
    pub text: Color32,
    /// Card and section heading text. Slightly denser than body copy.
    pub heading: Color32,
    pub text_secondary: Color32,
    pub text_muted: Color32,
    pub primary: Color32,
    /// Solid accent hover and pressed fill.
    pub primary_hover: Color32,
    pub primary_soft: Color32,
    pub primary_tint: Color32,
    /// Soft solid accent used by secondary accent buttons.
    pub primary_fill: Color32,
    /// Warm wash behind selected or editable accent surfaces.
    pub primary_wash: Color32,
    pub primary_outline: Color32,
    pub primary_outline_strong: Color32,
    pub primary_outline_soft: Color32,
    /// Foreground drawn on a solid accent fill.
    pub on_accent: Color32,
    /// Avatar placeholder fill for a local account.
    pub avatar: Color32,
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
    /// Slider, toggle and progress tracks.
    pub track: Color32,
    pub track_stroke: Color32,
    /// A quieter track used for thin in-card progress bars.
    pub track_subtle: Color32,
    /// Quota, balance and currency surfaces.
    pub gold_tint: Color32,
    pub gold_border: Color32,
    pub gold_fill: Color32,
    pub gold_text: Color32,
    /// Destructive row surfaces and their labels.
    pub danger_tint: Color32,
    pub danger_border: Color32,
    pub danger_soft: Color32,
    /// Overlay and elevation are premultiplied because `Color32` stores alpha
    /// that way. `scrim` is equivalent to rgba(31, 35, 41, .32).
    pub scrim: Color32,
    /// Elevation steps, all black alpha: a 1px settle under a raised block,
    /// a disclosure button, an anchored popover, a floating card, and the
    /// settings modal.
    pub shadow_rest: Color32,
    pub shadow_lift: Color32,
    pub shadow_popup: Color32,
    pub shadow_panel: Color32,
    pub shadow_modal: Color32,
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
            surface_sunken: Color32::from_rgb(0xfa, 0xfa, 0xfb),
            sidebar: Color32::from_rgb(0xf6, 0xf5, 0xf3),
            // Web `--mz-hover` / `--mz-selected`. The desktop token names are
            // inverted relative to the web ones, so keep the web mapping here
            // and expose the desktop pair as `timeline_hover`.
            hover: Color32::from_rgb(0xf1, 0xef, 0xec),
            selected: Color32::from_rgb(0xed, 0xea, 0xe6),
            timeline_hover: Color32::from_rgb(0xf6, 0xf5, 0xf3),
            reader_hover: Color32::from_rgba_unmultiplied(0xf6, 0xf5, 0xf3, 115),
            border: Color32::from_rgb(0xe8, 0xe6, 0xe3),
            card_border: Color32::from_rgb(0xdf, 0xdf, 0xdf),
            divider: Color32::from_rgb(0xec, 0xec, 0xec),
            chip: Color32::from_rgb(0xee, 0xef, 0xf0),
            control_border: Color32::from_rgb(0xd9, 0xd9, 0xd9),
            control_fill: Color32::from_rgb(0xf8, 0xf8, 0xf8),
            field_border: Color32::from_rgb(0xd9, 0xe1, 0xe8),
            border_strong: Color32::from_rgb(0xd8, 0xd5, 0xd0),
            text: Color32::from_rgb(0x1f, 0x23, 0x29),
            heading: Color32::from_rgb(0x2a, 0x2a, 0x2a),
            text_secondary: Color32::from_rgb(0x3d, 0x3d, 0x3d),
            text_muted: Color32::from_rgb(0x8a, 0x8f, 0x99),
            primary: Color32::from_rgb(0xc9, 0x64, 0x43),
            primary_hover: Color32::from_rgb(0xb5, 0x56, 0x37),
            primary_soft: Color32::from_rgb(0xdf, 0xac, 0x9a),
            primary_tint: Color32::from_rgb(0xf6, 0xe9, 0xe3),
            primary_fill: Color32::from_rgb(0xf2, 0xdc, 0xd2),
            primary_wash: Color32::from_rgb(0xff, 0xf8, 0xf2),
            primary_outline: Color32::from_rgb(0xff, 0xd7, 0xbe),
            primary_outline_strong: Color32::from_rgb(0xf4, 0xc2, 0xae),
            primary_outline_soft: Color32::from_rgb(0xff, 0xdd, 0xcc),
            on_accent: Color32::WHITE,
            avatar: Color32::from_rgb(0x3b, 0x82, 0xf6),
            focus: Color32::from_rgb(0xc9, 0x64, 0x43),
            disabled: Color32::from_rgb(0xb7, 0xb9, 0xbd),
            code_background: Color32::from_rgb(0xf5, 0xf4, 0xf2),
            code_inline: Color32::from_rgb(0xf3, 0xf2, 0xf0),
            process: Color32::from_rgb(0x9b, 0xa0, 0xa8),
            success_tint: Color32::from_rgb(0xf1, 0xfd, 0xf5),
            success_border: Color32::from_rgb(0xbe, 0xe6, 0xcc),
            track: Color32::from_rgb(0xe6, 0xe9, 0xf0),
            track_stroke: Color32::from_rgb(0xd8, 0xdd, 0xe7),
            track_subtle: Color32::from_rgb(0xeb, 0xed, 0xf1),
            gold_tint: Color32::from_rgb(0xff, 0xf8, 0xee),
            gold_border: Color32::from_rgb(0xff, 0xe0, 0xbb),
            gold_fill: Color32::from_rgb(0xf8, 0xe5, 0xaa),
            gold_text: Color32::from_rgb(0xb8, 0x7b, 0x13),
            danger_tint: Color32::from_rgb(0xff, 0xf7, 0xf8),
            danger_border: Color32::from_rgb(0xff, 0xe0, 0xe3),
            danger_soft: Color32::from_rgb(0xeb, 0x9a, 0xa5),
            scrim: Color32::from_rgba_unmultiplied(0x1f, 0x23, 0x29, 82),
            shadow_rest: Color32::from_black_alpha(5),
            shadow_lift: Color32::from_black_alpha(10),
            shadow_popup: Color32::from_black_alpha(20),
            shadow_panel: Color32::from_black_alpha(24),
            shadow_modal: Color32::from_black_alpha(71),
            user_bubble_top: Color32::from_rgb(246, 236, 231),
            user_bubble_bottom: Color32::from_rgb(249, 244, 240),
            timeline_line: Color32::from_rgb(0xe2, 0xdf, 0xda),
            success: Color32::from_rgb(0x1a, 0x8a, 0x5c),
            warning: Color32::from_rgb(0xc9, 0x8a, 0x2b),
            danger: Color32::from_rgb(0xd0, 0x4a, 0x43),
        }
    }
}

/// A selectable accent palette entry, rendered by the settings theme tiles.
///
/// These are content values rather than shell tokens: the user picks one and
/// the choice is stored with the profile. Keeping them here stops the settings
/// page from hard-coding colour literals while still shipping the exact web
/// palette.
#[derive(Clone, Copy)]
pub struct AccentChoice {
    pub id: &'static str,
    pub label: &'static str,
    pub accent: Color32,
    pub tint: Color32,
}

pub const ACCENT_CHOICES: &[AccentChoice] = &[
    AccentChoice {
        id: "forest",
        label: "森林",
        accent: Color32::from_rgb(47, 122, 92),
        tint: Color32::from_rgb(243, 248, 245),
    },
    AccentChoice {
        id: "hula-green",
        label: "薄荷",
        accent: Color32::from_rgb(22, 173, 141),
        tint: Color32::from_rgb(239, 251, 247),
    },
    AccentChoice {
        id: "honey",
        label: "蜜蜂",
        accent: Color32::from_rgb(192, 138, 42),
        tint: Color32::from_rgb(251, 246, 234),
    },
    AccentChoice {
        id: "eva-orange",
        label: "橙汁",
        accent: Color32::from_rgb(245, 138, 36),
        tint: Color32::from_rgb(253, 240, 222),
    },
];

pub fn accent_choices() -> &'static [AccentChoice] {
    ACCENT_CHOICES
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
    visuals.window_shadow.color = tokens.shadow_panel;
    visuals.popup_shadow.color = tokens.shadow_popup;
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
        // A hovered row must not blend into the sidebar it sits on.
        assert_ne!(tokens.hover, tokens.sidebar);
        assert_ne!(tokens.code_background, tokens.code_inline);
        assert_ne!(tokens.success_tint, tokens.surface);
        assert_ne!(tokens.surface_sunken, tokens.surface);
        assert_ne!(tokens.heading, tokens.text);
        assert!(tokens.scrim.a() > 0);
    }

    #[test]
    fn elevation_and_surface_tokens_stay_ordered() {
        let tokens = Tokens::light();
        // Darker surface means "further inside"; a card must never blend into
        // the canvas, and its inset content must not match the card fill.
        assert!(tokens.surface_sunken.r() <= tokens.surface.r());
        assert!(tokens.surface_sunken.r() >= tokens.canvas.r());
        // Elevation grows monotonically so popovers stack above light lifts.
        let steps = [
            tokens.shadow_rest,
            tokens.shadow_lift,
            tokens.shadow_popup,
            tokens.shadow_panel,
            tokens.shadow_modal,
        ];
        for pair in steps.windows(2) {
            assert!(
                pair[0].a() < pair[1].a(),
                "elevation steps must increase: {:?} >= {:?}",
                pair[0],
                pair[1]
            );
        }
    }

    #[test]
    fn accent_palette_ids_are_unique_and_cover_the_web_palette() {
        let choices = accent_choices();
        assert_eq!(choices.len(), 4);
        for (index, choice) in choices.iter().enumerate() {
            assert!(!choice.id.is_empty() && !choice.label.is_empty());
            assert!(
                choices
                    .iter()
                    .skip(index + 1)
                    .all(|other| other.id != choice.id),
                "duplicate accent id: {}",
                choice.id
            );
            assert_ne!(choice.accent, choice.tint);
        }
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
        assert_eq!(visuals.window_shadow.color, tokens.shadow_panel);
        assert_eq!(visuals.popup_shadow.color, tokens.shadow_popup);
    }
}
