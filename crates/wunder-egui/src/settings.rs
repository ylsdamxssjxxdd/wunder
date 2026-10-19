//! 蜂巢设置浮窗的 egui 投影。
//!
//! Geometry follows `messenger-shell.css` and `messenger-settings-page.css`:
//! a 960×680 modal, 44px title bar, and a 260px desktop navigation rail.
//! Breakpoints belong to the host viewport, never to the capped modal width.

use crate::icons::{self, Icon};
use crate::model::AppState;
use crate::theme::Tokens;
use egui::{
    Align, Align2, Area, Color32, Context, FontId, Frame, Id, Key, Layout, Margin, Order, Painter,
    Pos2, Rect, RichText, ScrollArea, Sense, Stroke, StrokeKind, TextEdit, Ui, UiBuilder, Vec2,
};

const WINDOW_SIZE: Vec2 = Vec2::new(960.0, 680.0);
const NAV_WIDTH: f32 = 260.0;
const TITLEBAR_HEIGHT: f32 = 44.0;
const COMPACT_VIEWPORT_WIDTH: f32 = 1024.0;
const NAV_REVEALED_ID: &str = "settings-nav-last-revealed";

#[derive(Clone, Copy)]
struct Category {
    id: &'static str,
    title: &'static str,
    desc: &'static str,
    aliases: &'static [&'static str],
}

const CATEGORIES: &[Category] = &[
    Category {
        id: "general",
        title: "常规",
        desc: "外观与行为偏好",
        aliases: &[
            "常规",
            "语言",
            "主题",
            "外观",
            "字号",
            "发送键",
            "general",
            "theme",
        ],
    },
    Category {
        id: "account",
        title: "账号",
        desc: "昵称、头像与密码等个人资料",
        aliases: &["账号", "昵称", "头像", "密码", "account", "profile"],
    },
    Category {
        id: "tools",
        title: "工具管理",
        desc: "内置工具、用户工具与连接状态",
        aliases: &["工具", "技能", "知识库", "mcp", "tool"],
    },
    Category {
        id: "agent",
        title: "智能体设置",
        desc: "智能体的提示词、工具与行为",
        aliases: &["智能体", "提示词", "欢迎语", "agent", "prompt"],
    },
    Category {
        id: "companion",
        title: "桌宠",
        desc: "桌宠形象、缩放与气泡提示",
        aliases: &["桌宠", "形象", "动画", "companion"],
    },
    Category {
        id: "cron",
        title: "定时任务",
        desc: "定时任务的创建与启停",
        aliases: &["定时", "计划", "cron", "schedule"],
    },
    Category {
        id: "memory",
        title: "记忆碎片",
        desc: "记忆碎片的查看与编辑",
        aliases: &["记忆", "memory"],
    },
    Category {
        id: "channels",
        title: "渠道设置",
        desc: "渠道账号绑定与连接状态",
        aliases: &["渠道", "扫码", "channel"],
    },
    Category {
        id: "devices",
        title: "我的设备",
        desc: "设备互联开关与云影子投影",
        aliases: &["设备", "互通", "device"],
    },
    Category {
        id: "prompts",
        title: "提示词包",
        desc: "提示词包的启用与分段编辑",
        aliases: &["提示词包", "分段", "prompts"],
    },
    Category {
        id: "archived",
        title: "归档线程",
        desc: "已归档线程的恢复与删除",
        aliases: &["归档", "线程", "archive"],
    },
];

fn category(id: &str) -> Category {
    CATEGORIES
        .iter()
        .copied()
        .find(|item| item.id == id)
        .unwrap_or(CATEGORIES[0])
}

pub fn draw(ctx: &Context, state: &mut AppState, tokens: Tokens) {
    if !state.settings_open {
        ctx.data_mut(|data| data.remove::<String>(Id::new(NAV_REVEALED_ID)));
        return;
    }
    if ctx.input(|input| input.key_pressed(Key::Escape)) {
        state.settings_open = false;
        ctx.data_mut(|data| data.remove::<String>(Id::new(NAV_REVEALED_ID)));
        return;
    }
    let screen = ctx.input(|input| input.screen_rect());

    let size = Vec2::new(
        WINDOW_SIZE.x.min((screen.width() - 48.0).max(280.0)),
        WINDOW_SIZE.y.min((screen.height() - 48.0).max(280.0)),
    );
    // CSS media queries test the browser viewport. Testing `size.x` here made
    // the 960px modal permanently fall into the <1024px mobile layout.
    let compact = screen.width() < COMPACT_VIEWPORT_WIDTH;
    let pos = screen.center() - size / 2.0;
    egui::Modal::new(Id::new("messenger-settings-window"))
        .area(
            Area::new(Id::new("messenger-settings-window"))
                .order(Order::Foreground)
                .fixed_pos(pos)
                .sense(Sense::hover()),
        )
        .frame(Frame::NONE)
        .backdrop_color(Color32::from_rgba_unmultiplied(31, 35, 41, 82))
        .show(ctx, |ui| {
            ui.spacing_mut().item_spacing = Vec2::ZERO;
            Frame::NONE
                .fill(tokens.canvas)
                .stroke(Stroke::new(1.0, tokens.border))
                .corner_radius(14.0)
                .inner_margin(Margin::ZERO)
                .outer_margin(Margin::ZERO)
                .shadow(egui::epaint::Shadow {
                    offset: [0, 24],
                    blur: 64,
                    spread: 0,
                    color: Color32::from_black_alpha(71),
                })
                .show(ui, |ui| {
                    let (window, _) =
                        ui.allocate_exact_size(size - Vec2::splat(2.0), Sense::hover());
                    let titlebar =
                        Rect::from_min_size(window.min, Vec2::new(window.width(), TITLEBAR_HEIGHT));
                    draw_titlebar(ui, titlebar, state, tokens);
                    let body =
                        Rect::from_min_max(Pos2::new(window.left(), titlebar.bottom()), window.max);
                    let (nav, content) = if compact {
                        let split = body.top() + (body.height() * 0.42).max(160.0);
                        (
                            Rect::from_min_max(body.min, Pos2::new(body.right(), split)),
                            Rect::from_min_max(Pos2::new(body.left(), split + 1.0), body.max),
                        )
                    } else {
                        (
                            Rect::from_min_max(
                                body.min,
                                Pos2::new(body.left() + NAV_WIDTH, body.bottom()),
                            ),
                            Rect::from_min_max(
                                Pos2::new(body.left() + NAV_WIDTH, body.top()),
                                body.max,
                            ),
                        )
                    };
                    ui.painter().rect_filled(nav, 0.0, tokens.sidebar);
                    let divider = if compact {
                        [nav.left_bottom(), nav.right_bottom()]
                    } else {
                        [nav.right_top(), nav.right_bottom()]
                    };
                    ui.painter()
                        .line_segment(divider, Stroke::new(1.0, tokens.border));
                    child(ui, nav, "settings-nav-column", |ui| {
                        draw_nav(ui, state, tokens)
                    });
                    child(ui, content, "settings-content-column", |ui| {
                        draw_content_column(ui, state, tokens, compact);
                    });
                });
        });
}

fn child(ui: &mut Ui, rect: Rect, id: &str, body: impl FnOnce(&mut Ui)) {
    // These are CSS-style positioned regions. `scope_builder` advances the
    // parent's cursor by its child's minimum rect and would let a long
    // settings page enlarge the modal after its size was already allocated.
    let mut region = ui.new_child(
        UiBuilder::new()
            .id_salt(id)
            .max_rect(rect)
            .layout(Layout::top_down(Align::Min)),
    );
    region.set_clip_rect(rect.intersect(ui.clip_rect()));
    body(&mut region);
    #[cfg(test)]
    if matches!(id, "channel-account-list" | "channel-account-detail") {
        // Keep the un-clipped layout bounds: screenshot clipping must not
        // conceal controls or inherited layouts expanding a channel column.
        region.ctx().data_mut(|data| {
            data.insert_temp(
                Id::new(("settings-child-bounds", id)),
                (rect, region.min_rect()),
            );
        });
    }
}

fn draw_titlebar(ui: &mut Ui, rect: Rect, state: &mut AppState, tokens: Tokens) {
    ui.painter().rect_filled(
        rect,
        egui::CornerRadius {
            nw: 14,
            ne: 14,
            sw: 0,
            se: 0,
        },
        tokens.sidebar,
    );
    ui.painter().line_segment(
        [rect.left_bottom(), rect.right_bottom()],
        Stroke::new(1.0, tokens.border),
    );
    draw_icon(
        ui.painter(),
        Pos2::new(rect.left() + 23.0, rect.center().y),
        "general",
        14.0,
        tokens.text_muted,
    );
    ui.painter().text(
        Pos2::new(rect.left() + 38.0, rect.center().y),
        Align2::LEFT_CENTER,
        "设置",
        FontId::proportional(14.0),
        tokens.text,
    );
    let close_rect = Rect::from_center_size(
        Pos2::new(rect.right() - 24.0, rect.center().y),
        Vec2::splat(28.0),
    );
    let close = ui
        .interact(close_rect, Id::new("settings-close"), Sense::click())
        .on_hover_text("关闭");
    if close.hovered() {
        ui.painter().rect_filled(close_rect, 8.0, tokens.hover);
    }
    draw_icon(
        ui.painter(),
        close_rect.center(),
        "close",
        14.0,
        tokens.text_secondary,
    );
    if close.clicked() {
        state.settings_open = false;
        ui.ctx()
            .data_mut(|data| data.remove::<String>(Id::new(NAV_REVEALED_ID)));
    }
}

fn draw_nav(ui: &mut Ui, state: &mut AppState, tokens: Tokens) {
    ui.spacing_mut().item_spacing = Vec2::new(0.0, 2.0);
    Frame::NONE
        .inner_margin(Margin::symmetric(12, 16))
        .show(ui, |ui| {
            let width = ui.available_width() - 1.0;
            let (search, _) = ui.allocate_exact_size(Vec2::new(width, 34.0), Sense::hover());
            ui.painter().rect_filled(search, 10.0, tokens.surface);
            draw_icon(
                ui.painter(),
                search.left_center() + Vec2::new(17.0, 0.0),
                "search",
                14.0,
                tokens.text_muted,
            );
            let input = Rect::from_min_max(
                search.min + Vec2::new(32.0, 7.0),
                search.max - Vec2::new(10.0, 5.0),
            );
            let mut focused = false;
            child(ui, input, "settings-search-input", |ui| {
                let edit = ui.add_sized(
                    input.size(),
                    TextEdit::singleline(&mut state.settings_search)
                        .char_limit(64)
                        .frame(false)
                        .margin(Margin::ZERO)
                        .font(FontId::proportional(13.0))
                        .hint_text("搜索设置"),
                );
                focused = edit.has_focus();
            });
            ui.painter().rect_stroke(
                search,
                8.0,
                Stroke::new(
                    1.0,
                    if focused {
                        tokens.primary_soft
                    } else {
                        tokens.border
                    },
                ),
                StrokeKind::Inside,
            );
            ui.add_space(10.0);
            let needle = state.settings_search.trim().to_lowercase();
            let reveal_active = ui.ctx().data_mut(|data| {
                data.get_temp::<String>(Id::new(NAV_REVEALED_ID))
                    .is_none_or(|last| last != state.settings_category)
            });
            let matches = |item: &&Category| {
                needle.is_empty()
                    || item.title.to_lowercase().contains(&needle)
                    || item
                        .aliases
                        .iter()
                        .any(|alias| alias.to_lowercase().contains(&needle))
            };
            ScrollArea::vertical()
                .id_salt("settings-nav")
                .animated(false)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.set_width(width);
                    let mut has_match = false;
                    for item in CATEGORIES.iter().filter(matches) {
                        has_match = true;
                        let active = state.settings_category == item.id;
                        let (rect, response) =
                            ui.allocate_exact_size(Vec2::new(width - 2.0, 51.8), Sense::click());
                        if active && reveal_active {
                            response.scroll_to_me(None);
                            ui.ctx().data_mut(|data| {
                                data.insert_temp(
                                    Id::new(NAV_REVEALED_ID),
                                    state.settings_category.clone(),
                                );
                            });
                        }
                        let fill = if active {
                            tokens.primary_tint
                        } else if response.hovered() {
                            tokens.surface
                        } else {
                            Color32::TRANSPARENT
                        };
                        ui.painter().rect_filled(rect, 10.0, fill);
                        if active {
                            ui.painter().rect_filled(
                                Rect::from_min_max(
                                    rect.min + Vec2::new(0.0, 8.0),
                                    Pos2::new(rect.left() + 3.0, rect.bottom() - 8.0),
                                ),
                                2.0,
                                tokens.primary,
                            );
                        }
                        draw_icon(
                            ui.painter(),
                            rect.min + Vec2::new(20.0, 18.0),
                            item.id,
                            16.0,
                            if active {
                                tokens.primary
                            } else {
                                tokens.text_muted
                            },
                        );
                        let painter = ui.painter().with_clip_rect(Rect::from_min_max(
                            rect.min + Vec2::new(38.0, 0.0),
                            rect.max - Vec2::new(10.0, 0.0),
                        ));
                        painter.text(
                            rect.min + Vec2::new(38.0, 8.0),
                            Align2::LEFT_TOP,
                            item.title,
                            FontId::proportional(14.0),
                            if active {
                                tokens.text
                            } else {
                                tokens.text_secondary
                            },
                        );
                        painter.text(
                            rect.min + Vec2::new(38.0, 28.0),
                            Align2::LEFT_TOP,
                            item.desc,
                            FontId::proportional(12.0),
                            tokens.text_muted,
                        );
                        if response.clicked() {
                            state.settings_category = item.id.to_owned();
                        }
                    }
                    if !has_match {
                        ui.add_space(16.0);
                        ui.label(
                            RichText::new("没有匹配的设置分类")
                                .size(13.0)
                                .color(tokens.text_muted),
                        );
                    }
                });
        });
}

fn draw_content_column(ui: &mut Ui, state: &mut AppState, tokens: Tokens, compact: bool) {
    let horizontal = if compact { 16 } else { 24 };
    Frame::NONE
        .inner_margin(Margin {
            left: horizontal,
            right: horizontal,
            top: if compact { 12 } else { 16 },
            bottom: 0,
        })
        .show(ui, |ui| {
            let item = category(&state.settings_category);
            ScrollArea::vertical()
                .id_salt(("settings-content", item.id))
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing = Vec2::new(8.0, 0.0);
                    ui.set_width(ui.available_width().min(880.0));
                    match item.id {
                        "general" => page_general(ui, state, tokens),
                        "account" => page_account(ui, state, tokens),
                        "tools" => page_tools(ui, state, tokens),
                        "agent" => page_agent(ui, tokens),
                        "companion" => page_companion(ui, state, tokens),
                        "cron" => page_cron(ui, tokens),
                        "memory" => page_memory(ui, tokens),
                        "channels" => page_channels(ui, tokens),
                        "devices" => page_devices(ui, tokens),
                        "prompts" => page_prompts(ui, tokens),
                        "archived" => page_archived(ui, state, tokens),
                        _ => page_general(ui, state, tokens),
                    }
                    ui.add_space(if compact { 24.0 } else { 32.0 });
                });
        });
}

fn card(ui: &mut Ui, tokens: Tokens, title: &str, subtitle: &str, body: impl FnOnce(&mut Ui)) {
    let width = ui.available_width();
    Frame::NONE
        .fill(tokens.surface)
        .stroke(Stroke::new(1.0, Color32::from_rgb(223, 223, 223)))
        .corner_radius(12.0)
        .inner_margin(Margin::symmetric(16, 14))
        .show(ui, |ui| {
            ui.set_width((width - 34.0).max(160.0));
            ui.spacing_mut().item_spacing = Vec2::new(8.0, 0.0);
            let (head, _) =
                ui.allocate_exact_size(Vec2::new(ui.available_width(), 51.0), Sense::hover());
            paint_heading(
                ui.painter(),
                head.min + Vec2::new(0.0, 1.0),
                title,
                18.0,
                Color32::from_rgb(42, 42, 42),
            );
            ui.painter().text(
                head.min + Vec2::new(0.0, 25.0),
                Align2::LEFT_TOP,
                subtitle,
                FontId::proportional(12.0),
                tokens.text_muted,
            );
            paint_dashed_rule(
                ui.painter(),
                head.left_bottom(),
                head.right_bottom(),
                tokens.border,
            );
            ui.add_space(8.0);
            body(ui);
        });
    ui.add_space(16.0);
}

fn dashed_rule(ui: &mut Ui, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 1.0), Sense::hover());
    paint_dashed_rule(ui.painter(), rect.left_center(), rect.right_center(), color);
}

fn paint_heading(painter: &Painter, pos: Pos2, text: &str, size: f32, color: Color32) {
    let font = FontId::proportional(size);
    painter.text(pos, Align2::LEFT_TOP, text, font.clone(), color);
    painter.text(
        pos + Vec2::new(0.45, 0.0),
        Align2::LEFT_TOP,
        text,
        font,
        color,
    );
}

fn paint_dashed_rule(painter: &Painter, start: Pos2, end: Pos2, color: Color32) {
    let mut x = start.x;
    while x < end.x {
        painter.line_segment(
            [
                Pos2::new(x, start.y),
                Pos2::new((x + 3.0).min(end.x), start.y),
            ],
            Stroke::new(1.0, color),
        );
        x += 6.0;
    }
}

fn form_row(
    ui: &mut Ui,
    tokens: Tokens,
    label: &str,
    hint: &str,
    last: bool,
    body: impl FnOnce(&mut Ui),
) {
    let height = if hint.is_empty() { 63.0 } else { 50.0 };
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), height), Sense::hover());
    let label_y = if hint.is_empty() {
        rect.center().y
    } else {
        rect.center().y - 9.0
    };
    ui.painter().text(
        Pos2::new(rect.left(), label_y),
        Align2::LEFT_CENTER,
        label,
        FontId::proportional(14.0),
        tokens.text_secondary,
    );
    if !hint.is_empty() {
        ui.painter().text(
            Pos2::new(rect.left(), label_y + 22.0),
            Align2::LEFT_CENTER,
            hint,
            FontId::proportional(12.0),
            tokens.text_muted,
        );
    }
    let controls_width = if !hint.is_empty() { 336.0 } else { 214.0 };
    let controls = Rect::from_center_size(
        Pos2::new(rect.right() - controls_width / 2.0, rect.center().y),
        Vec2::new(controls_width, 38.0),
    );
    child(ui, controls, label, |ui| {
        ui.with_layout(Layout::right_to_left(Align::Center), body);
    });
    if !last {
        ui.painter().line_segment(
            [rect.left_bottom(), rect.right_bottom()],
            Stroke::new(1.0, Color32::from_rgb(236, 236, 236)),
        );
    }
}

fn select(ui: &mut Ui, id: &str, value: &mut String, choices: &[&str]) {
    ui.scope(|ui| {
        ui.spacing_mut().interact_size.y = 38.0;
        ui.spacing_mut().button_padding = Vec2::new(12.0, 8.0);
        let widgets = &mut ui.visuals_mut().widgets;
        for visual in [&mut widgets.inactive, &mut widgets.noninteractive] {
            visual.bg_fill = Color32::from_rgb(248, 248, 248);
            visual.weak_bg_fill = Color32::from_rgb(248, 248, 248);
            visual.bg_stroke = Stroke::new(1.0, Color32::from_rgb(217, 217, 217));
            visual.corner_radius = egui::CornerRadius::same(10);
        }
        egui::ComboBox::from_id_salt(id)
            .width(if id == "settings-send-key" {
                212.0
            } else if id == "settings-language" {
                132.0
            } else if id == "memory-tag" {
                184.0
            } else {
                160.0
            })
            .selected_text(RichText::new(select_label(value)).size(13.0))
            .show_ui(ui, |ui| {
                for option in choices {
                    let _ = ui.selectable_value(value, (*option).to_owned(), select_label(option));
                }
            });
    });
}

fn range_control(
    ui: &mut Ui,
    rect: Rect,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    step: f32,
    id: &str,
    tokens: Tokens,
) {
    let response = ui.interact(rect, Id::new(id), Sense::click_and_drag());
    let min = *range.start();
    let max = *range.end();
    if response.dragged() || response.clicked() {
        if let Some(pointer) = response.interact_pointer_pos() {
            *value = ((min
                + ((pointer.x - rect.left()) / rect.width()).clamp(0.0, 1.0) * (max - min))
                / step)
                .round()
                * step;
        }
    }
    let track = Rect::from_center_size(rect.center(), Vec2::new(rect.width(), 5.0));
    ui.painter()
        .rect_filled(track, 3.0, Color32::from_rgb(230, 233, 240));
    ui.painter().rect_stroke(
        track,
        3.0,
        Stroke::new(1.0, Color32::from_rgb(216, 221, 231)),
        StrokeKind::Inside,
    );
    let center = Pos2::new(
        rect.left() + ((*value - min) / (max - min)).clamp(0.0, 1.0) * rect.width(),
        rect.center().y,
    );
    ui.painter().circle_filled(center, 7.0, Color32::WHITE);
    ui.painter()
        .circle_stroke(center, 7.0, Stroke::new(1.0, tokens.primary_soft));
    ui.painter().circle_filled(center, 4.5, tokens.primary);
}

fn select_label(value: &str) -> &str {
    match value {
        "Enter 发送" => "Enter",
        "Ctrl+Enter 发送" => "Ctrl + Enter",
        value => value,
    }
}

fn page_general(ui: &mut Ui, state: &mut AppState, tokens: Tokens) {
    card(
        ui,
        tokens,
        "外观与行为",
        "语言、主题、字号与发送键（本地即时生效）",
        |ui| {
            form_row(ui, tokens, "发送键", "", false, |ui| {
                select(
                    ui,
                    "settings-send-key",
                    &mut state.settings_send_key,
                    &["Enter 发送", "Ctrl+Enter 发送", "不使用快捷键"],
                );
            });
            form_row(ui, tokens, "语言", "", false, |ui| {
                select(
                    ui,
                    "settings-language",
                    &mut state.settings_language,
                    &["跟随系统", "简体中文", "English"],
                );
            });
            ui.add_space(12.0);
            ui.label(
                RichText::new("主题模式")
                    .size(14.0)
                    .strong()
                    .color(tokens.text_secondary),
            );
            ui.add_space(4.0);
            ui.label(
                RichText::new("选择界面强调色与聊天区底纹，即时生效")
                    .size(12.0)
                    .color(tokens.text_muted),
            );
            ui.add_space(12.0);
            draw_theme_tiles(ui, state, tokens);
            ui.add_space(12.0);
            let (rule, _) =
                ui.allocate_exact_size(Vec2::new(ui.available_width(), 1.0), Sense::hover());
            ui.painter().line_segment(
                [rule.left_center(), rule.right_center()],
                Stroke::new(1.0, Color32::from_rgb(236, 236, 236)),
            );
            form_row(
                ui,
                tokens,
                "字体大小",
                "聊天内容的字体大小",
                true,
                |ui| {
                    let mut font_size =
                        (14.0 * state.settings_font_scale).round().clamp(12.0, 20.0);
                    let (controls, _) =
                        ui.allocate_exact_size(Vec2::new(336.0, 38.0), Sense::hover());
                    let pill = Rect::from_center_size(
                        Pos2::new(controls.left() + 50.0, controls.center().y),
                        Vec2::new(100.0, 34.0),
                    );
                    ui.painter()
                        .rect_filled(pill, 17.0, Color32::from_rgb(248, 248, 248));
                    ui.painter().rect_stroke(
                        pill,
                        17.0,
                        Stroke::new(1.0, Color32::from_rgb(217, 217, 217)),
                        StrokeKind::Inside,
                    );
                    for (offset, label, delta) in [(16.0, "−", -1.0), (84.0, "+", 1.0)] {
                        let button = Rect::from_center_size(
                            Pos2::new(pill.left() + offset, pill.center().y),
                            Vec2::new(30.0, 32.0),
                        );
                        if ui
                            .interact(button, Id::new(("font-step", label)), Sense::click())
                            .clicked()
                        {
                            font_size = (font_size + delta).clamp(12.0, 20.0);
                        }
                        ui.painter().text(
                            button.center(),
                            Align2::CENTER_CENTER,
                            label,
                            FontId::proportional(13.0),
                            tokens.text_secondary,
                        );
                    }
                    ui.painter().text(
                        pill.center(),
                        Align2::CENTER_CENTER,
                        format!("{font_size:.0}"),
                        FontId::proportional(13.0),
                        tokens.text_secondary,
                    );
                    let slider = Rect::from_center_size(
                        Pos2::new(controls.right() - 110.0, controls.center().y),
                        Vec2::new(220.0, 24.0),
                    );
                    range_control(
                        ui,
                        slider,
                        &mut font_size,
                        12.0..=20.0,
                        1.0,
                        "font-size-range",
                        tokens,
                    );
                    state.settings_font_scale = font_size / 14.0;
                },
            );
        },
    );
    card(
        ui,
        tokens,
        "帮助",
        "在新窗口浏览文档站，快速检索功能与接入说明",
        |ui| {
            form_row(ui, tokens, "打开帮助文档", "", true, |ui| {
                let _ = action_button(ui, tokens, "打开帮助文档", Some("external"));
            });
        },
    );
}

fn draw_theme_tiles(ui: &mut Ui, state: &mut AppState, tokens: Tokens) {
    let choices = [
        (
            "forest",
            "森林",
            Color32::from_rgb(47, 122, 92),
            Color32::from_rgb(243, 248, 245),
        ),
        (
            "hula-green",
            "薄荷",
            Color32::from_rgb(22, 173, 141),
            Color32::from_rgb(239, 251, 247),
        ),
        (
            "honey",
            "蜜蜂",
            Color32::from_rgb(192, 138, 42),
            Color32::from_rgb(251, 246, 234),
        ),
        (
            "eva-orange",
            "羊皮纸",
            Color32::from_rgb(201, 100, 67),
            Color32::from_rgb(251, 244, 241),
        ),
    ];
    let width = ui.available_width();
    let columns = (((width + 12.0) / 162.0).floor() as usize).clamp(1, 4);
    let tile_width = (width - 12.0 * (columns - 1) as f32) / columns as f32;
    for (row_index, row) in choices.chunks(columns).enumerate() {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 12.0;
            for &(id, name, accent, tint) in row {
                let active = state.settings_theme == id
                    || (state.settings_theme == "浅色" && id == "eva-orange");
                let (rect, response) =
                    ui.allocate_exact_size(Vec2::new(tile_width, 123.0), Sense::click());
                ui.painter().rect_filled(rect, 12.0, tokens.surface);
                ui.painter().rect_stroke(
                    rect,
                    12.0,
                    Stroke::new(
                        1.0,
                        if active {
                            Color32::from_rgb(31, 35, 41)
                        } else if response.hovered() {
                            accent
                        } else {
                            Color32::from_rgb(226, 224, 220)
                        },
                    ),
                    StrokeKind::Inside,
                );
                let mock = Rect::from_min_size(
                    rect.min + Vec2::splat(10.0),
                    Vec2::new(tile_width - 20.0, 74.0),
                );
                let p = ui.painter();
                p.rect_filled(mock, 9.0, tint);
                p.rect_stroke(
                    mock,
                    9.0,
                    Stroke::new(1.0, Color32::from_rgb(236, 234, 230)),
                    StrokeKind::Inside,
                );
                p.rect_filled(
                    Rect::from_min_size(
                        mock.min + Vec2::new(12.0, 12.0),
                        Vec2::new(mock.width() * 0.46, 7.0),
                    ),
                    4.0,
                    accent,
                );
                p.rect_filled(
                    Rect::from_min_size(
                        mock.min + Vec2::new(12.0, 25.0),
                        Vec2::new(mock.width() * 0.30, 5.0),
                    ),
                    3.0,
                    Color32::from_rgb(220, 216, 210),
                );
                let avatar = Pos2::new(mock.right() - 19.5, mock.top() + 16.5);
                p.circle_filled(avatar, 7.5, Color32::WHITE);
                p.circle_stroke(
                    avatar,
                    if active { 5.5 } else { 7.0 },
                    Stroke::new(
                        if active { 4.0 } else { 1.0 },
                        if active { accent } else { tokens.border },
                    ),
                );
                let body = Rect::from_min_max(
                    mock.min + Vec2::new(12.0, 38.0),
                    mock.max - Vec2::new(12.0, 8.0),
                );
                p.rect_filled(body, 7.0, Color32::WHITE);
                p.rect_stroke(
                    body,
                    7.0,
                    Stroke::new(1.0, Color32::from_rgb(236, 234, 230)),
                    StrokeKind::Inside,
                );
                for (dy, factor) in [(6.0, 0.70), (16.0, 0.42)] {
                    p.rect_filled(
                        Rect::from_min_size(
                            body.min + Vec2::new(10.0, dy),
                            Vec2::new((body.width() - 20.0) * factor, 5.0),
                        ),
                        3.0,
                        Color32::from_rgb(216, 212, 206),
                    );
                }
                p.text(
                    Pos2::new(rect.center().x, rect.top() + 104.0),
                    Align2::CENTER_CENTER,
                    name,
                    FontId::proportional(13.0),
                    Color32::from_rgb(47, 47, 47),
                );
                if response.clicked() {
                    state.settings_theme = id.to_owned();
                }
            }
        });
        if (row_index + 1) * columns < choices.len() {
            ui.add_space(12.0);
        }
    }
}

fn action_button(ui: &mut Ui, tokens: Tokens, label: &str, icon: Option<&str>) -> egui::Response {
    let text_width = ui
        .painter()
        .layout_no_wrap(
            label.to_owned(),
            FontId::proportional(13.0),
            tokens.text_secondary,
        )
        .size()
        .x;
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(text_width + if icon.is_some() { 46.0 } else { 24.0 }, 34.0),
        Sense::click(),
    );
    ui.painter().rect_filled(
        rect,
        9.0,
        if response.hovered() {
            tokens.hover
        } else {
            tokens.surface
        },
    );
    ui.painter().rect_stroke(
        rect,
        9.0,
        Stroke::new(1.0, tokens.border_strong),
        StrokeKind::Inside,
    );
    if let Some(icon) = icon {
        draw_icon(
            ui.painter(),
            rect.left_center() + Vec2::new(19.0, 0.0),
            icon,
            13.0,
            tokens.text_secondary,
        );
    }
    ui.painter().text(
        rect.left_center() + Vec2::new(if icon.is_some() { 34.0 } else { 12.0 }, 0.0),
        Align2::LEFT_CENTER,
        label,
        FontId::proportional(13.0),
        tokens.text_secondary,
    );
    response
}

fn accent_button(ui: &mut Ui, rect: Rect, label: &str, solid: bool, tokens: Tokens) {
    let response = ui.interact(rect, Id::new(("settings-accent", label)), Sense::click());
    let radius = if solid { 10.0 } else { 16.0 };
    ui.painter().rect_filled(
        rect,
        radius,
        if solid {
            tokens.primary
        } else if response.hovered() {
            tokens.primary_tint
        } else {
            Color32::from_rgb(242, 220, 210)
        },
    );
    if !solid {
        ui.painter().rect_stroke(
            rect,
            radius,
            Stroke::new(1.0, tokens.primary_soft),
            StrokeKind::Inside,
        );
    }
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        label,
        FontId::proportional(13.0),
        if solid {
            Color32::WHITE
        } else {
            tokens.primary
        },
    );
}

fn panel(ui: &mut Ui, tokens: Tokens, min_height: f32, body: impl FnOnce(&mut Ui)) {
    let width = ui.available_width();
    Frame::NONE
        .fill(tokens.surface)
        .stroke(Stroke::new(1.0, Color32::from_rgb(223, 223, 223)))
        .corner_radius(12.0)
        .inner_margin(Margin::symmetric(16, 14))
        .show(ui, |ui| {
            ui.set_width((width - 34.0).max(160.0));
            ui.set_min_height((min_height - 30.0).max(0.0));
            ui.spacing_mut().item_spacing = Vec2::new(8.0, 8.0);
            body(ui);
        });
    ui.add_space(16.0);
}

fn panel_header(
    ui: &mut Ui,
    tokens: Tokens,
    title: &str,
    subtitle: &str,
    actions: impl FnOnce(&mut Ui),
) {
    let (head, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 51.0), Sense::hover());
    paint_heading(
        ui.painter(),
        head.min + Vec2::new(0.0, 1.0),
        title,
        18.0,
        tokens.text,
    );
    if !subtitle.is_empty() {
        ui.painter().text(
            head.min + Vec2::new(0.0, 25.0),
            Align2::LEFT_TOP,
            subtitle,
            FontId::proportional(12.0),
            tokens.text_muted,
        );
    }
    let actions_rect = Rect::from_min_size(head.min, Vec2::new(head.width(), 34.0));
    child(ui, actions_rect, title, |ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        ui.with_layout(Layout::right_to_left(Align::Min), actions);
    });
    paint_dashed_rule(
        ui.painter(),
        head.left_bottom(),
        head.right_bottom(),
        tokens.border,
    );
}

fn empty_text(ui: &mut Ui, tokens: Tokens, text: &str) {
    ui.add_space(10.0);
    Frame::NONE
        .inner_margin(Margin::symmetric(10, 0))
        .show(ui, |ui| {
            ui.label(RichText::new(text).size(12.0).color(tokens.primary));
        });
    ui.add_space(10.0);
}

fn page_height(ui: &Ui) -> f32 {
    (ui.clip_rect().bottom() - ui.cursor().top() - 32.0).clamp(280.0, 586.0)
}

fn page_account(ui: &mut Ui, state: &AppState, tokens: Tokens) {
    // Fixed local geometry mirrors the Hive profile card. Default widget
    // heights and nested item spacing otherwise grow this 167px card on every
    // platform and push the quota below the modal's first viewport.
    let width = ui.available_width();
    let (profile, _) = ui.allocate_exact_size(Vec2::new(width, 167.0), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(profile, 12.0, tokens.surface);
    painter.rect_stroke(
        profile,
        12.0,
        Stroke::new(1.0, Color32::from_rgb(223, 223, 223)),
        StrokeKind::Inside,
    );
    let origin = profile.min + Vec2::new(17.0, 15.0);
    let avatar = Rect::from_min_size(origin, Vec2::splat(56.0));
    painter.circle_filled(avatar.center(), 28.0, Color32::from_rgb(59, 130, 246));
    icons::paint(painter, avatar.shrink(16.0), Icon::User, Color32::WHITE);
    paint_heading(
        painter,
        origin + Vec2::new(68.0, 0.0),
        "本地用户",
        18.0,
        tokens.text,
    );
    painter.text(
        origin + Vec2::new(68.0, 29.0),
        Align2::LEFT_TOP,
        "本地工作台",
        FontId::proportional(12.0),
        tokens.text_muted,
    );
    for (x, text, badge_width) in [(68.0, "本地模式", 60.0), (136.0, "Lv.1", 38.0)] {
        let rect = Rect::from_min_size(origin + Vec2::new(x, 52.0), Vec2::new(badge_width, 22.0));
        painter.rect_filled(rect, 11.0, tokens.primary_tint);
        painter.rect_stroke(
            rect,
            11.0,
            Stroke::new(1.0, tokens.primary_soft),
            StrokeKind::Inside,
        );
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            text,
            FontId::proportional(11.0),
            tokens.primary,
        );
    }
    for (offset, icon) in [(93.0, Icon::User), (51.0, Icon::Pen)] {
        let rect = Rect::from_min_size(
            Pos2::new(profile.right() - offset, origin.y),
            Vec2::splat(34.0),
        );
        let response = ui.interact(
            rect,
            Id::new(("profile-action", offset as u32)),
            Sense::click(),
        );
        painter.rect_filled(
            rect,
            10.0,
            if response.hovered() {
                tokens.hover
            } else {
                Color32::from_rgb(248, 248, 248)
            },
        );
        painter.rect_stroke(
            rect,
            10.0,
            Stroke::new(1.0, tokens.border_strong),
            StrokeKind::Inside,
        );
        icons::paint(painter, rect.shrink(10.0), icon, tokens.text_secondary);
    }
    painter.line_segment(
        [
            Pos2::new(origin.x, profile.top() + 100.0),
            Pos2::new(profile.right() - 17.0, profile.top() + 100.0),
        ],
        Stroke::new(1.0, tokens.border),
    );
    let progress = Rect::from_min_size(
        Pos2::new(origin.x, profile.top() + 119.0),
        Vec2::new(width - 34.0, 10.0),
    );
    painter.rect_filled(progress, 5.0, Color32::from_rgb(235, 237, 241));
    for (x, align, text) in [
        (origin.x, Align2::LEFT_CENTER, "0 / 0"),
        (
            profile.right() - 17.0,
            Align2::RIGHT_CENTER,
            "距离下一级还需 0 经验",
        ),
    ] {
        painter.text(
            Pos2::new(x, profile.top() + 145.0),
            align,
            text,
            FontId::proportional(11.0),
            tokens.text_muted,
        );
    }
    ui.add_space(12.0);
    let (stats, _) = ui.allocate_exact_size(Vec2::new(width, 486.0), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(stats, 12.0, tokens.surface);
    painter.rect_stroke(
        stats,
        12.0,
        Stroke::new(1.0, Color32::from_rgb(223, 223, 223)),
        StrokeKind::Inside,
    );
    let origin = stats.min + Vec2::new(17.0, 0.0);
    let inner_width = width - 34.0;
    paint_heading(
        painter,
        origin + Vec2::new(0.0, 16.0),
        "统计信息",
        18.0,
        tokens.text,
    );
    painter.text(
        origin + Vec2::new(0.0, 49.0),
        Align2::LEFT_TOP,
        "会话、工具与额度使用概况",
        FontId::proportional(12.0),
        tokens.text_muted,
    );
    let values = [
        ("会话总数", state.sessions.len().to_string()),
        ("近7天会话", "0".to_owned()),
        ("工具调用", "0".to_owned()),
        ("总额度消耗", "—".to_owned()),
        ("总token消耗", "—".to_owned()),
        ("最近活跃", "—".to_owned()),
    ];
    let tile_width = (inner_width - 16.0) / 3.0;
    for (index, (label, value)) in values.iter().enumerate() {
        let rect = Rect::from_min_size(
            origin
                + Vec2::new(
                    (index % 3) as f32 * (tile_width + 8.0),
                    71.0 + (index / 3) as f32 * 62.0,
                ),
            Vec2::new(tile_width, 54.0),
        );
        painter.rect_filled(rect, 10.0, Color32::from_rgb(250, 250, 251));
        painter.rect_stroke(
            rect,
            10.0,
            Stroke::new(1.0, tokens.border),
            StrokeKind::Inside,
        );
        painter.text(
            rect.min + Vec2::new(10.0, 10.0),
            Align2::LEFT_TOP,
            *label,
            FontId::proportional(12.0),
            tokens.text_muted,
        );
        paint_heading(
            painter,
            rect.min + Vec2::new(10.0, 28.0),
            value,
            14.0,
            tokens.text,
        );
    }
    let chart = Rect::from_min_size(
        origin + Vec2::new(0.0, 193.0),
        Vec2::new(inner_width, 146.0),
    );
    painter.rect_filled(chart, 12.0, Color32::from_rgb(250, 250, 251));
    painter.rect_stroke(
        chart,
        12.0,
        Stroke::new(1.0, tokens.border),
        StrokeKind::Inside,
    );
    painter.text(
        chart.min + Vec2::new(10.0, 12.0),
        Align2::LEFT_TOP,
        "近7天会话",
        FontId::proportional(12.0),
        tokens.text_secondary,
    );
    paint_heading(
        painter,
        chart.right_top() + Vec2::new(-17.0, 12.0),
        "0",
        12.0,
        tokens.text,
    );
    let bar_width = (inner_width - 22.0 - 42.0) / 7.0;
    for (index, day) in ["一", "二", "三", "四", "五", "六", "日"]
        .iter()
        .enumerate()
    {
        let bar = Rect::from_min_size(
            chart.min + Vec2::new(11.0 + index as f32 * (bar_width + 7.0), 49.0),
            Vec2::new(bar_width, 72.0),
        );
        painter.text(
            Pos2::new(bar.center().x, chart.top() + 36.0),
            Align2::CENTER_TOP,
            "0",
            FontId::proportional(10.0),
            tokens.text_muted,
        );
        painter.rect_filled(bar, 8.0, tokens.primary_tint);
        painter.rect_stroke(
            bar,
            8.0,
            Stroke::new(1.0, tokens.border),
            StrokeKind::Inside,
        );
        painter.rect_filled(
            Rect::from_min_max(
                bar.left_bottom() + Vec2::new(4.0, -8.0),
                bar.right_bottom() + Vec2::new(-4.0, -4.0),
            ),
            2.0,
            tokens.primary,
        );
        painter.text(
            Pos2::new(bar.center().x, chart.top() + 125.0),
            Align2::CENTER_TOP,
            *day,
            FontId::proportional(10.0),
            tokens.text_muted,
        );
    }
    let quota = Rect::from_min_size(
        origin + Vec2::new(0.0, 347.0),
        Vec2::new(inner_width, 122.0),
    );
    painter.rect_filled(quota, 12.0, tokens.primary_tint);
    painter.rect_stroke(
        quota,
        12.0,
        Stroke::new(1.0, tokens.primary_soft),
        StrokeKind::Inside,
    );
    painter.text(
        quota.min + Vec2::new(10.0, 11.0),
        Align2::LEFT_TOP,
        "剩余额度",
        FontId::proportional(12.0),
        tokens.primary,
    );
    let balance = Rect::from_min_max(
        quota.min + Vec2::new(11.0, 35.0),
        quota.max - Vec2::new(11.0, 11.0),
    );
    painter.rect_filled(balance, 18.0, Color32::from_rgb(255, 248, 238));
    painter.rect_stroke(
        balance,
        18.0,
        Stroke::new(1.0, Color32::from_rgb(255, 224, 187)),
        StrokeKind::Inside,
    );
    let coin = Rect::from_min_size(balance.min + Vec2::new(17.0, 17.0), Vec2::splat(42.0));
    painter.rect_filled(coin, 14.0, Color32::from_rgb(248, 229, 170));
    painter.text(
        coin.center(),
        Align2::CENTER_CENTER,
        "\u{f51e}",
        FontId::new(20.0, egui::FontFamily::Name(icons::FONT_NAME.into())),
        Color32::from_rgb(184, 123, 19),
    );
    paint_heading(
        painter,
        balance.min + Vec2::new(74.0, 20.0),
        "—",
        28.0,
        tokens.text,
    );
}

fn page_tools(ui: &mut Ui, state: &mut AppState, tokens: Tokens) {
    Frame::NONE
        .fill(tokens.canvas)
        .stroke(Stroke::new(1.0, tokens.border))
        .corner_radius(8.0)
        .inner_margin(Margin::same(3))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                for (id, title, icon) in [
                    ("builtin", "全局工具", Icon::Tool),
                    ("mcp", "MCP 工具", Icon::Plug),
                    ("skill", "技能工具", Icon::File),
                    ("knowledge", "知识库工具", Icon::Database),
                ] {
                    let active = state.settings_tools_tab == id;
                    let text_width = ui
                        .painter()
                        .layout_no_wrap(title.to_owned(), FontId::proportional(13.0), tokens.text)
                        .size()
                        .x;
                    let (rect, response) =
                        ui.allocate_exact_size(Vec2::new(text_width + 36.0, 30.0), Sense::click());
                    if active || response.hovered() {
                        ui.painter().rect_filled(rect, 8.0, tokens.surface);
                    }
                    if active {
                        ui.painter().rect_stroke(
                            rect,
                            8.0,
                            Stroke::new(1.0, tokens.border),
                            StrokeKind::Inside,
                        );
                    }
                    icons::paint(
                        ui.painter(),
                        Rect::from_center_size(
                            rect.left_center() + Vec2::new(17.0, 0.0),
                            Vec2::splat(12.0),
                        ),
                        icon,
                        tokens.text_secondary,
                    );
                    ui.painter().text(
                        rect.left_center() + Vec2::new(28.0, 0.0),
                        Align2::LEFT_CENTER,
                        title,
                        FontId::proportional(13.0),
                        tokens.text_secondary,
                    );
                    if response.clicked() {
                        state.settings_tools_tab = id.to_owned();
                    }
                }
            });
        });
    ui.add_space(16.0);
    if state.settings_tools_tab == "builtin" {
        panel(ui, tokens, 128.0, |ui| {
            panel_header(
                ui,
                tokens,
                "全局工具",
                "管理员开放的内置工具、MCP 工具、技能工具与知识库工具，共 0 项",
                |ui| {
                    let _ = action_button(ui, tokens, "刷新", Some("refresh"));
                },
            );
            empty_text(ui, tokens, "暂无可用的工具");
        });
    } else {
        let title = match state.settings_tools_tab.as_str() {
            "mcp" => "MCP 工具",
            "skill" => "技能工具",
            _ => "知识库工具",
        };
        panel(ui, tokens, 480.0, |ui| {
            panel_header(
                ui,
                tokens,
                title,
                "管理工具连接与可用资源",
                |ui| {
                    let _ = action_button(ui, tokens, "新增", Some("plus"));
                    let _ = action_button(ui, tokens, "刷新", Some("refresh"));
                },
            );
            split_empty(
                ui,
                tokens,
                370.0,
                0.35,
                "暂无工具连接",
                "请选择左侧条目查看详情",
            );
        });
    }
}

fn page_agent(ui: &mut Ui, tokens: Tokens) {
    panel(ui, tokens, 484.0, |ui| {
        input_field(
            ui,
            tokens,
            "智能体名称",
            "默认智能体",
            "",
            36.0,
            "agent-name",
        );
        input_field(
            ui,
            tokens,
            "问候语",
            "",
            "用于会话中的首次问候",
            36.0,
            "agent-greeting",
        );
        input_field(
            ui,
            tokens,
            "额外提示词",
            "",
            "输入需要追加到基础系统提示词后的内容",
            136.0,
            "agent-prompt",
        );
        ui.add_space(18.0);
        empty_text(ui, tokens, "工具列表尚未连接");
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("预设问题")
                    .size(12.0)
                    .strong()
                    .color(tokens.text),
            );
            let _ = icons::button(ui, Icon::Plus, 25.0, tokens.text_muted);
        });
        notice(ui, tokens, "暂未配置预设问题，可按需添加多个。");
    });
}

fn input_field(
    ui: &mut Ui,
    tokens: Tokens,
    label: &str,
    initial: &str,
    hint: &str,
    height: f32,
    id: &str,
) {
    // CSS form labels use 16px line height, a 6px field gap, and 12px
    // between fields. Reserve them together so egui's default spacing does
    // not accumulate after each label and editor.
    let (field, _) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), height + 26.0),
        Sense::hover(),
    );
    paint_heading(ui.painter(), field.min, label, 12.0, tokens.text);
    let editor = Rect::from_min_size(
        field.min + Vec2::new(0.0, 22.0),
        Vec2::new(field.width(), height),
    );
    ui.painter().rect_filled(editor, 10.0, tokens.surface);
    ui.painter().rect_stroke(
        editor,
        10.0,
        Stroke::new(1.0, Color32::from_rgb(217, 225, 232)),
        StrokeKind::Inside,
    );
    let key = Id::new(("settings-field", id));
    let mut value = ui.ctx().data_mut(|data| {
        data.get_temp::<String>(key)
            .unwrap_or_else(|| initial.to_owned())
    });
    let edit = if height > 50.0 {
        TextEdit::multiline(&mut value)
    } else {
        TextEdit::singleline(&mut value)
    };
    child(ui, editor.shrink2(Vec2::new(10.0, 8.0)), id, |ui| {
        let _ = ui.add_sized(
            ui.available_size(),
            edit.char_limit(if height > 50.0 { 8192 } else { 256 })
                .font(FontId::proportional(13.0))
                .hint_text(hint)
                .frame(false)
                .margin(Margin::ZERO),
        );
    });
    ui.ctx().data_mut(|data| data.insert_temp(key, value));
}

fn page_companion(ui: &mut Ui, state: &mut AppState, tokens: Tokens) {
    card(
        ui,
        tokens,
        "桌宠行为",
        "开关即时保存，动画随线程运行状态联动",
        |ui| {
            detail_row(
                ui,
                tokens,
                "companion",
                "动画形象",
                "在浏览器中显示已选形象，蛹支持时可显示在桌面",
                false,
                |ui| toggle(ui, &mut state.companion_enabled, tokens),
            );
            detail_row(
                ui,
                tokens,
                "message",
                "形象消息提示",
                "让形象显示必要的简短状态信息",
                false,
                |ui| toggle(ui, &mut state.companion_hints, tokens),
            );
            detail_row(
                ui,
                tokens,
                "expand",
                "大小",
                "同步到已绑定桌宠的智能体",
                true,
                |ui| {
                    let (controls, _) =
                        ui.allocate_exact_size(Vec2::new(220.0, 28.0), Sense::hover());
                    ui.painter().text(
                        controls.left_center(),
                        Align2::LEFT_CENTER,
                        format!("{:.1}x", state.companion_scale),
                        FontId::proportional(13.0),
                        tokens.text_secondary,
                    );
                    let range = Rect::from_center_size(
                        Pos2::new(controls.right() - 80.0, controls.center().y),
                        Vec2::new(160.0, 24.0),
                    );
                    range_control(
                        ui,
                        range,
                        &mut state.companion_scale,
                        0.5..=1.6,
                        0.1,
                        "companion-scale",
                        tokens,
                    );
                },
            );
        },
    );
    panel(ui, tokens, 164.0, |ui| {
        panel_header(
            ui,
            tokens,
            "形象库",
            "本机与全局形象库共 0 个形象",
            |ui| {
                let _ = action_button(ui, tokens, "导入", Some("upload"));
                let _ = action_button(ui, tokens, "刷新", Some("refresh"));
            },
        );
        empty_text(ui, tokens, "导入标准形象包后可进行预览和选用");
        dashed_rule(ui, tokens.border);
        ui.label(RichText::new("桌宠是否显示由智能体的头像绑定决定：在「智能体设置 → 头像」中选择动态形象后，这里才会出现桌宠。")
            .size(12.0).color(tokens.text_muted));
    });
}

fn detail_row(
    ui: &mut Ui,
    tokens: Tokens,
    icon: &str,
    label: &str,
    hint: &str,
    last: bool,
    body: impl FnOnce(&mut Ui),
) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 62.0), Sense::hover());
    draw_icon(
        ui.painter(),
        rect.min + Vec2::new(8.0, 23.0),
        icon,
        14.0,
        tokens.text_muted,
    );
    ui.painter().text(
        rect.min + Vec2::new(26.0, 23.0),
        Align2::LEFT_CENTER,
        label,
        FontId::proportional(14.0),
        tokens.text_secondary,
    );
    ui.painter().text(
        rect.min + Vec2::new(26.0, 44.0),
        Align2::LEFT_CENTER,
        hint,
        FontId::proportional(12.0),
        tokens.text_muted,
    );
    let controls = Rect::from_min_max(Pos2::new(rect.right() - 220.0, rect.top()), rect.max);
    child(ui, controls, label, |ui| {
        ui.with_layout(Layout::right_to_left(Align::Center), body);
    });
    if !last {
        ui.painter().line_segment(
            [rect.left_bottom(), rect.right_bottom()],
            Stroke::new(1.0, Color32::from_rgb(236, 236, 236)),
        );
    }
}

fn toggle(ui: &mut Ui, value: &mut bool, tokens: Tokens) {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(50.0, 28.0), Sense::click());
    if response.clicked() {
        *value = !*value;
    }
    ui.painter().rect_filled(
        rect,
        14.0,
        if *value {
            tokens.primary
        } else {
            tokens.border_strong
        },
    );
    ui.painter().circle_filled(
        Pos2::new(
            if *value {
                rect.right() - 14.0
            } else {
                rect.left() + 14.0
            },
            rect.center().y,
        ),
        11.0,
        tokens.surface,
    );
}

fn split_empty(ui: &mut Ui, tokens: Tokens, height: f32, ratio: f32, left: &str, right: &str) {
    let width = ui.available_width();
    let left_width = (width - 10.0) * ratio;
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        for (width, text) in [(left_width, left), (width - left_width - 10.0, right)] {
            Frame::NONE
                .fill(tokens.surface)
                .stroke(Stroke::new(1.0, Color32::from_rgb(223, 223, 223)))
                .corner_radius(12.0)
                .inner_margin(Margin::symmetric(20, 20))
                .show(ui, |ui| {
                    ui.set_min_size(Vec2::new((width - 42.0).max(20.0), height - 42.0));
                    ui.set_max_width((width - 42.0).max(20.0));
                    ui.label(RichText::new(text).size(12.0).color(tokens.primary));
                });
        }
    });
}

fn page_cron(ui: &mut Ui, tokens: Tokens) {
    let height = page_height(ui);
    panel(ui, tokens, height, |ui| {
        panel_header(
            ui,
            tokens,
            "定时任务",
            "定时任务按智能体隔离，支持创建、编辑与启停",
            |_| {},
        );
        let (toolbar, _) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), 34.0), Sense::hover());
        ui.painter().text(
            toolbar.left_center(),
            Align2::LEFT_CENTER,
            "查看与管理智能体定时任务",
            FontId::proportional(12.0),
            tokens.text_muted,
        );
        let create = Rect::from_center_size(
            Pos2::new(toolbar.right() - 49.0, toolbar.center().y),
            Vec2::new(98.0, 30.0),
        );
        accent_button(ui, create, "新建定时任务", false, tokens);
        let refresh = Rect::from_center_size(
            Pos2::new(create.left() - 33.0, toolbar.center().y),
            Vec2::new(50.0, 30.0),
        );
        fixed_button(ui, refresh, "刷新", true, false, tokens);
        split_empty(
            ui,
            tokens,
            (height - 132.0).max(200.0),
            0.46,
            "暂无定时任务",
            "请选择任务查看详情",
        );
    });
}

fn notice(ui: &mut Ui, tokens: Tokens, message: &str) {
    Frame::NONE
        .fill(tokens.canvas)
        .stroke(Stroke::new(1.0, tokens.border))
        .corner_radius(10.0)
        .inner_margin(Margin::symmetric(12, 12))
        .show(ui, |ui| {
            ui.set_width((ui.available_width() - 2.0).max(30.0));
            ui.label(RichText::new(message).size(13.0).color(tokens.text_muted));
        });
}

fn page_memory(ui: &mut Ui, tokens: Tokens) {
    let height = page_height(ui);
    panel(ui, tokens, height, |ui| {
        panel_header(
            ui,
            tokens,
            "记忆碎片",
            "记忆碎片支持搜索、标签筛选、编辑与删除",
            |_| {},
        );
        let (toolbar, _) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), 45.0), Sense::hover());
        paint_heading(
            ui.painter(),
            toolbar.min + Vec2::new(0.0, 1.0),
            "记忆碎片",
            18.0,
            tokens.text,
        );
        ui.painter().text(
            toolbar.min + Vec2::new(0.0, 25.0),
            Align2::LEFT_TOP,
            "将可复用的长期事实整理为模型友好的记忆索引与详情。",
            FontId::proportional(12.0),
            tokens.text_muted,
        );
        let create = Rect::from_min_size(
            toolbar.right_top() + Vec2::new(-79.0, 1.0),
            Vec2::new(79.0, 35.0),
        );
        accent_button(ui, create, "新建碎片", true, tokens);
        let clone = Rect::from_min_size(create.min - Vec2::new(90.0, 0.0), Vec2::new(80.0, 35.0));
        fixed_button(ui, clone, "记忆复刻", false, false, tokens);
        let refresh = Rect::from_min_size(clone.min - Vec2::new(62.0, 0.0), Vec2::new(52.0, 35.0));
        fixed_button(ui, refresh, "刷新", true, false, tokens);
        let (filters, _) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), 39.0), Sense::hover());
        let search =
            Rect::from_min_max(filters.min, filters.right_bottom() - Vec2::new(196.0, 0.0));
        ui.painter().rect_filled(search, 10.0, tokens.surface);
        ui.painter().rect_stroke(
            search,
            10.0,
            Stroke::new(1.0, tokens.border_strong),
            StrokeKind::Inside,
        );
        let key = Id::new("memory-query");
        let mut query = ui
            .ctx()
            .data_mut(|data| data.get_temp::<String>(key).unwrap_or_default());
        child(
            ui,
            search.shrink2(Vec2::new(11.0, 9.0)),
            "memory-query-input",
            |ui| {
                ui.add_sized(
                    ui.available_size(),
                    TextEdit::singleline(&mut query)
                        .char_limit(128)
                        .frame(false)
                        .margin(Margin::ZERO)
                        .font(FontId::proportional(13.0))
                        .hint_text("搜索记忆 ID、索引标题或内容详情"),
                );
            },
        );
        ui.ctx().data_mut(|data| data.insert_temp(key, query));
        let tags = Rect::from_min_max(filters.right_top() - Vec2::new(184.0, 0.0), filters.max);
        child(ui, tags, "memory-tag-filter", |ui| {
            let mut tag = "全部标签".to_owned();
            select(ui, "memory-tag", &mut tag, &["全部标签"]);
        });
        ui.add_space(8.0);
        notice(ui, tokens, "当前智能体还没有记忆碎片。");
    });
}

fn page_channels(ui: &mut Ui, tokens: Tokens) {
    let height = page_height(ui);
    panel(ui, tokens, height, |ui| {
        panel_header(
            ui,
            tokens,
            "渠道设置",
            "渠道账号绑定、扫码登录、启停与连接日志",
            |_| {},
        );
        // Reserve one row before positioning either column. Frames inside a
        // horizontal layout inherit that layout and cannot constrain their
        // children with set_min_size/set_max_width; independent vertical
        // regions keep the 340 + 12 + 264 desktop geometry bounded.
        let width = ui.available_width();
        let (columns, _) =
            ui.allocate_exact_size(Vec2::new(width, (height - 89.0).max(280.0)), Sense::hover());
        let left_width = (width - 12.0) * (340.0 / 604.0);
        let left = Rect::from_min_max(
            columns.min,
            Pos2::new(columns.left() + left_width, columns.bottom()),
        );
        let right = Rect::from_min_max(Pos2::new(left.right() + 12.0, columns.top()), columns.max);
        #[cfg(test)]
        ui.ctx().data_mut(|data| {
            data.insert_temp(Id::new("settings-channel-columns"), (columns, left, right));
        });
        for rect in [left, right] {
            ui.painter().rect_filled(rect, 12.0, tokens.surface);
            ui.painter().rect_stroke(
                rect,
                12.0,
                Stroke::new(1.0, tokens.border),
                StrokeKind::Inside,
            );
        }
        child(ui, left.shrink(10.0), "channel-account-list", |ui| {
            let bounds = ui.max_rect();
            paint_heading(
                ui.painter(),
                bounds.min + Vec2::new(0.0, 3.0),
                "我的渠道账号",
                13.0,
                tokens.text,
            );
            for (offset, label) in [(50.0, "刷新"), (108.0, "新增")] {
                let rect = Rect::from_min_size(
                    Pos2::new(bounds.right() - offset, bounds.top() + 1.0),
                    Vec2::new(50.0, 30.0),
                );
                fixed_button(ui, rect, label, true, false, tokens);
            }
            ui.painter().text(
                Pos2::new(bounds.center().x, bounds.top() + 65.0),
                Align2::CENTER_CENTER,
                "暂无渠道账号，请先新增",
                FontId::proportional(12.0),
                tokens.text_muted,
            );
        });
        child(ui, right.shrink(12.0), "channel-account-detail", |ui| {
            let bounds = ui.max_rect();
            paint_heading(
                ui.painter(),
                bounds.min + Vec2::new(0.0, 16.0),
                "请选择左侧渠道",
                13.0,
                tokens.text,
            );
            paint_heading(
                ui.painter(),
                bounds.min + Vec2::new(0.0, 36.0),
                "账号",
                13.0,
                tokens.text,
            );
            let controls_left = bounds.right() - 130.0;
            for (x, y, label, danger) in [
                (0.0, 0.0, "保存", false),
                (58.0, 0.0, "删除", true),
                (0.0, 38.0, "重置", false),
            ] {
                fixed_button(
                    ui,
                    Rect::from_min_size(
                        Pos2::new(controls_left + x, bounds.top() + y),
                        Vec2::new(50.0, 30.0),
                    ),
                    label,
                    false,
                    danger,
                    tokens,
                );
            }
            ui.painter().text(
                Pos2::new(bounds.center().x, bounds.top() + 103.0),
                Align2::CENTER_CENTER,
                "请选择左侧渠道账号",
                FontId::proportional(12.0),
                tokens.text_muted,
            );
            let log = Rect::from_min_size(
                bounds.min + Vec2::new(0.0, 137.0),
                Vec2::new(bounds.width(), 114.0),
            );
            ui.painter()
                .rect_filled(log, 10.0, Color32::from_rgb(250, 250, 251));
            ui.painter().rect_stroke(
                log,
                10.0,
                Stroke::new(1.0, tokens.border),
                StrokeKind::Inside,
            );
            child(ui, log.shrink(10.0), "channel-run-log", |ui| {
                let bounds = ui.max_rect();
                paint_heading(ui.painter(), bounds.min, "渠道运行", 13.0, tokens.text);
                paint_heading(
                    ui.painter(),
                    bounds.min + Vec2::new(0.0, 18.0),
                    "日志",
                    13.0,
                    tokens.text,
                );
                let available = (bounds.width() - 64.0).max(60.0);
                let first = (available * 0.44).min(58.0);
                let small = ((available - first - 16.0) / 2.0).max(24.0);
                let start = bounds.right() - (first + small * 2.0 + 16.0);
                for (x, width, label) in [
                    (start, first, "写入测试日志"),
                    (start + first + 8.0, small, "刷新"),
                    (start + first + small + 16.0, small, "清空"),
                ] {
                    fixed_button(
                        ui,
                        Rect::from_min_size(Pos2::new(x, bounds.top()), Vec2::new(width, 32.0)),
                        label,
                        true,
                        false,
                        tokens,
                    );
                }
                ui.painter().text(
                    Pos2::new(bounds.center().x, bounds.top() + 69.0),
                    Align2::CENTER_CENTER,
                    "暂无运行日志",
                    FontId::proportional(12.0),
                    tokens.text_muted,
                );
            });
        });
    });
}

/// A bounded native projection of a compact web settings action. The caller
/// owns its rectangle, so labels cannot enlarge a surrounding column.
fn fixed_button(ui: &mut Ui, rect: Rect, label: &str, enabled: bool, danger: bool, tokens: Tokens) {
    // This affects min_rect rather than clipping, allowing the layout smoke
    // test to see actual controls escaping their assigned column.
    ui.expand_to_include_rect(rect);
    let response = ui.interact(
        rect,
        ui.id().with(label),
        if enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    let fill = if danger {
        Color32::from_rgb(255, 247, 248)
    } else if enabled && response.hovered() {
        tokens.hover
    } else {
        Color32::from_rgb(250, 250, 251)
    };
    let border = if danger {
        Color32::from_rgb(255, 224, 227)
    } else if enabled {
        tokens.border_strong
    } else {
        tokens.border
    };
    let color = if danger {
        Color32::from_rgb(235, 154, 165)
    } else if enabled {
        tokens.text_secondary
    } else {
        tokens.text_muted
    };
    ui.painter().rect_filled(rect, 9.0, fill);
    ui.painter()
        .rect_stroke(rect, 9.0, Stroke::new(1.0, border), StrokeKind::Inside);
    let galley = ui.painter().layout(
        label.to_owned(),
        FontId::proportional(12.0),
        color,
        (rect.width() - 10.0).max(12.0),
    );
    ui.painter()
        .with_clip_rect(rect.intersect(ui.clip_rect()))
        .galley(rect.center() - galley.size() / 2.0, galley, color);
}

fn page_devices(ui: &mut Ui, tokens: Tokens) {
    let height = page_height(ui);
    panel(ui, tokens, height, |ui| {
        panel_header(
            ui,
            tokens,
            "我的设备",
            "管理各设备的云端互联开关与云影子投影",
            |ui| {
                let _ = action_button(ui, tokens, "刷新", Some("refresh"));
                ui.label(
                    RichText::new("0 台设备在线 · 共 0 台")
                        .size(12.0)
                        .color(tokens.text_secondary),
                );
            },
        );
        empty_text(ui, tokens, "暂无设备，登录蛹或舵机后会自动出现在这里");
    });
}

fn page_prompts(ui: &mut Ui, tokens: Tokens) {
    panel(ui, tokens, 0.0, |ui| {
        panel_header(
            ui,
            tokens,
            "提示词包",
            "提示词包的启用、分段编辑与最终提示词预览",
            |_| {},
        );
        // The complete template selector is a fixed-width card; its content
        // must not choose the width or add widget-spacing to its 121px height.
        let (template, _) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), 121.0), Sense::hover());
        ui.painter().rect_filled(template, 12.0, tokens.surface);
        ui.painter().rect_stroke(
            template,
            12.0,
            Stroke::new(1.0, tokens.border),
            StrokeKind::Inside,
        );
        child(
            ui,
            Rect::from_min_max(
                template.min + Vec2::splat(16.0),
                template.max - Vec2::new(16.0, 15.0),
            ),
            "prompt-template-card",
            |ui| {
                let bounds = ui.max_rect();
                ui.painter().text(
                    bounds.min + Vec2::new(0.0, 2.0),
                    Align2::LEFT_TOP,
                    "模板包",
                    FontId::proportional(12.0),
                    tokens.text_secondary,
                );
                let selector =
                    Rect::from_min_size(bounds.min + Vec2::new(0.0, 21.0), Vec2::new(180.0, 34.0));
                ui.painter()
                    .rect_filled(selector, 10.0, Color32::from_rgb(255, 248, 242));
                ui.painter().rect_stroke(
                    selector,
                    10.0,
                    Stroke::new(1.0, Color32::from_rgb(255, 215, 190)),
                    StrokeKind::Inside,
                );
                ui.painter().text(
                    selector.left_center() + Vec2::new(14.0, 0.0),
                    Align2::LEFT_CENTER,
                    "内置中文模板包",
                    FontId::proportional(13.0),
                    tokens.text,
                );
                icons::paint(
                    ui.painter(),
                    Rect::from_center_size(
                        selector.right_center() - Vec2::new(13.0, 0.0),
                        Vec2::splat(10.0),
                    ),
                    Icon::ChevronDown,
                    tokens.text_secondary,
                );
                for (offset, width, label, enabled, danger) in [
                    (188.0, 80.0, "设为启用", false, false),
                    (276.0, 92.0, "新建模板包", true, false),
                    (376.0, 92.0, "删除模板包", false, true),
                ] {
                    fixed_button(
                        ui,
                        Rect::from_min_size(
                            bounds.min + Vec2::new(offset, 21.0),
                            Vec2::new(width, 34.0),
                        ),
                        label,
                        enabled,
                        danger,
                        tokens,
                    );
                }
                let mut x = bounds.left();
                for label in [
                    "当前启用：内置中文模板包",
                    "系统语言默认",
                    "同步系统模板包：default",
                ] {
                    let text = ui.painter().layout_no_wrap(
                        label.to_owned(),
                        FontId::proportional(11.0),
                        tokens.text_muted,
                    );
                    let tag = Rect::from_min_size(
                        Pos2::new(x, bounds.top() + 73.0),
                        Vec2::new(text.size().x + 12.0, 17.0),
                    );
                    ui.painter()
                        .rect_filled(tag, 99.0, Color32::from_rgb(238, 239, 240));
                    ui.painter()
                        .galley(tag.center() - text.size() / 2.0, text, tokens.text_muted);
                    x = tag.right() + 6.0;
                }
            },
        );
        ui.add_space(2.0);
        let key = Id::new("prompt-section");
        let mut selected = ui.ctx().data_mut(|d| d.get_temp::<usize>(key).unwrap_or(0));
        let sections = [
            ("角色定位", "role.txt"),
            ("工程指导", "engineering.txt"),
            ("工具协议", "tools_protocol.txt"),
            ("技能协议", "skills_protocol.txt"),
            ("历史记忆", "memory.txt"),
            ("额外提示", "extra.txt"),
        ];
        // The outer editor starts at y378 in the desktop reference. Its 16px
        // insets contain a 220px section list and the 352px detail pane.
        let (editor, _) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), 420.0), Sense::hover());
        ui.painter().rect_filled(editor, 12.0, tokens.surface);
        ui.painter().rect_stroke(
            editor,
            12.0,
            Stroke::new(1.0, tokens.border),
            StrokeKind::Inside,
        );
        let editor_inner = Rect::from_min_max(
            editor.min + Vec2::new(17.0, 15.0),
            editor.max - Vec2::new(17.0, 16.0),
        );
        let nav_width = 220.0_f32.min((editor_inner.width() - 10.0) * 0.5);
        let nav = Rect::from_min_size(editor_inner.min, Vec2::new(nav_width, 389.0));
        let detail = Rect::from_min_max(
            Pos2::new(nav.right() + 10.0, editor_inner.top()),
            editor_inner.max,
        );
        for rect in [nav, detail] {
            ui.painter().rect_stroke(
                rect,
                12.0,
                Stroke::new(1.0, Color32::from_rgb(255, 221, 204)),
                StrokeKind::Inside,
            );
        }
        child(ui, nav.shrink(8.0), "prompt-sections", |ui| {
            ui.spacing_mut().item_spacing = Vec2::ZERO;
            for (index, (name, file)) in sections.iter().enumerate() {
                let (rect, response) =
                    ui.allocate_exact_size(Vec2::new(ui.available_width(), 57.0), Sense::click());
                if index == selected {
                    let selected_rect = rect.shrink2(Vec2::new(0.0, 3.0));
                    ui.painter()
                        .rect_filled(selected_rect, 10.0, tokens.primary_tint);
                    ui.painter().rect_stroke(
                        selected_rect,
                        10.0,
                        Stroke::new(1.0, Color32::from_rgb(244, 194, 174)),
                        StrokeKind::Inside,
                    );
                }
                paint_heading(
                    ui.painter(),
                    rect.min + Vec2::new(10.0, 9.0),
                    name,
                    14.0,
                    tokens.text,
                );
                ui.painter().text(
                    rect.min + Vec2::new(10.0, 28.0),
                    Align2::LEFT_TOP,
                    *file,
                    FontId::proportional(12.0),
                    tokens.text_muted,
                );
                if response.clicked() {
                    selected = index;
                }
            }
        });
        child(ui, detail.shrink(10.0), "prompt-detail", |ui| {
            // Position each field in the pane. Inherited item spacing used to
            // push the footer below the clipping boundary despite a 389px pane.
            let bounds = ui.max_rect();
            paint_heading(
                ui.painter(),
                bounds.min + Vec2::new(0.0, 5.0),
                sections[selected].0,
                18.0,
                tokens.text,
            );
            fixed_button(
                ui,
                Rect::from_min_size(
                    Pos2::new(bounds.right() - 54.0, bounds.top()),
                    Vec2::new(54.0, 34.0),
                ),
                "保存",
                false,
                false,
                tokens,
            );
            let body = Rect::from_min_size(
                bounds.min + Vec2::new(0.0, 42.0),
                Vec2::new(bounds.width(), 297.0),
            );
            ui.painter()
                .rect_filled(body, 10.0, Color32::from_rgb(255, 248, 242));
            ui.painter().rect_stroke(
                body,
                10.0,
                Stroke::new(1.0, Color32::from_rgb(255, 221, 204)),
                StrokeKind::Inside,
            );
            ui.painter().text(
                bounds.min + Vec2::new(0.0, 349.0),
                Align2::LEFT_TOP,
                "内置模板包只读（同步系统模板包：default）。",
                FontId::proportional(12.0),
                tokens.primary,
            );
        });
        ui.ctx().data_mut(|d| d.insert_temp(key, selected));
    });
}

fn page_archived(ui: &mut Ui, state: &AppState, tokens: Tokens) {
    let height = page_height(ui);
    panel(ui, tokens, height, |ui| {
        panel_header(
            ui,
            tokens,
            "归档线程",
            "已归档线程支持搜索、分页查看、恢复与彻底删除",
            |_| {},
        );
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("已归档线程")
                    .size(13.0)
                    .strong()
                    .color(tokens.text),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let _ = action_button(ui, tokens, "刷新", Some("refresh"));
                let key = Id::new("archived-query");
                let mut query = ui
                    .ctx()
                    .data_mut(|d| d.get_temp::<String>(key).unwrap_or_default());
                ui.add_sized(
                    Vec2::new(200.0, 30.0),
                    TextEdit::singleline(&mut query)
                        .char_limit(128)
                        .hint_text("搜索归档线程标题或内容")
                        .font(FontId::proportional(12.0))
                        .margin(Margin::symmetric(10, 6)),
                );
                ui.ctx().data_mut(|d| d.insert_temp(key, query));
            });
        });
        if state.connection == crate::model::ConnectionState::Ready {
            empty_text(ui, tokens, "暂无归档线程");
        } else {
            empty_text(ui, tokens, "连接后可查看已归档线程");
        }
    });
}

/// Shared Font Awesome glyphs match the web client and avoid emoji fallback.
fn draw_icon(p: &Painter, center: Pos2, name: &str, size: f32, color: Color32) {
    let icon = match name {
        "close" => Icon::Close,
        "search" => Icon::Search,
        "general" => Icon::Settings,
        "account" => Icon::IdCard,
        "tools" => Icon::ScrewdriverWrench,
        "agent" => Icon::Agent,
        "companion" => Icon::Paw,
        "cron" => Icon::Clock,
        "memory" => Icon::Brain,
        "channels" => Icon::Plug,
        "devices" => Icon::SatelliteDish,
        "prompts" => Icon::File,
        "archived" => Icon::Archive,
        "external" => Icon::ExternalLink,
        "refresh" => Icon::Refresh,
        "plus" => Icon::Plus,
        "upload" => Icon::Upload,
        "message" => Icon::More,
        "expand" => Icon::Desktop,
        _ => Icon::Settings,
    };
    icons::paint(
        p,
        Rect::from_center_size(center, Vec2::splat(size)),
        icon,
        color,
    );
}

#[cfg(test)]
mod tests {
    use super::{draw, CATEGORIES};
    use crate::model::AppState;
    use crate::theme;
    use egui::{Context, Id, Pos2, RawInput, Rect, Vec2};

    #[test]
    fn all_settings_categories_render_inside_the_desktop_viewport() {
        // Run the real layout and font pipeline without an OS window. The
        // second frame resolves Area sizing, as it does in the native shell.
        // Reusing the context also exercises transitions between categories.
        let ctx = Context::default();
        theme::apply(&ctx);
        let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(1440.0, 900.0));
        let mut state = AppState {
            settings_open: true,
            ..Default::default()
        };
        let mut frame = 0_u32;
        for category in CATEGORIES {
            state.settings_category = category.id.to_owned();
            let mut output = egui::FullOutput::default();
            for _ in 0..2 {
                output = ctx.run(
                    RawInput {
                        screen_rect: Some(screen),
                        time: Some(f64::from(frame) / 30.0),
                        ..Default::default()
                    },
                    |ctx| draw(ctx, &mut state, theme::tokens()),
                );
                frame += 1;
            }

            let panel = ctx.memory(|memory| memory.area_rect(Id::new("messenger-settings-window")));
            assert!(
                panel.is_some(),
                "{} did not create its settings panel",
                category.id
            );
            if let Some(panel) = panel {
                assert!(
                    panel.is_finite(),
                    "{} produced non-finite geometry",
                    category.id
                );
                assert!(
                    screen.contains_rect(panel),
                    "{} panel escaped the viewport: {panel:?}",
                    category.id
                );
                assert!(
                    panel.width() > screen.width() / 2.0 && panel.height() > screen.height() / 2.0,
                    "{} panel collapsed: {panel:?}",
                    category.id
                );
            }
            if category.id == "channels" {
                let columns = ctx.data(|data| {
                    data.get_temp::<(Rect, Rect, Rect)>(Id::new("settings-channel-columns"))
                });
                assert!(columns.is_some(), "channel columns were not laid out");
                if let Some((parent, left, right)) = columns {
                    assert!(parent.contains_rect(left) && parent.contains_rect(right),
                        "channel columns escaped their allocated row: {parent:?}, {left:?}, {right:?}");
                    assert!(left.right() < right.left(), "channel columns overlap");
                }
                for name in ["channel-account-list", "channel-account-detail"] {
                    let bounds = ctx.data(|data| {
                        data.get_temp::<(Rect, Rect)>(Id::new(("settings-child-bounds", name)))
                    });
                    assert!(bounds.is_some(), "{name} did not produce layout bounds");
                    if let Some((assigned, used)) = bounds {
                        assert!(assigned.expand(0.5).contains_rect(used),
                            "{name} content escaped its column before clipping: {assigned:?}, {used:?}");
                    }
                }
            }
            // Tessellation catches invalid text/font or shape geometry after
            // layout, rather than merely checking that the draw call returned.
            let primitives = ctx.tessellate(output.shapes, output.pixels_per_point);
            assert!(
                !primitives.is_empty(),
                "{} produced no rendered content",
                category.id
            );
            assert!(
                state.settings_open,
                "{} unexpectedly closed its panel",
                category.id
            );
        }
    }
}
