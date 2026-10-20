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

/// Shared height of passive action buttons and inline text fields, so the
/// general page's new controls line up with the ones above them.
const CONTROL_HEIGHT: f32 = 34.0;
/// Row height for one editable entry in a card list (tool paths).
const LIST_ROW_HEIGHT: f32 = 38.0;

/// Chat font size bounds shared by the stepper, the slider and the live
/// preview, so the three controls cannot drift apart.
const FONT_SIZE_MIN: f32 = 12.0;
const FONT_SIZE_MAX: f32 = 20.0;
/// `settings_font_scale` is a multiplier over this baseline: 1.0 means 14px.
const FONT_SIZE_BASE: f32 = 14.0;

/// Supplement capabilities offered on the general page. The runtime has no
/// real bundle registry yet, so this table is the single source of the labels
/// and their one-line descriptions; `AppState` only stores names plus flags.
const SUPPLEMENT_CATALOGUE: &[(&str, &str)] = &[
    ("扩展语法高亮", "为代码块补充更多语言的着色规则"),
    ("离线文档索引", "在本机缓存常用文档，断网时也能检索"),
    ("实验性工具栏", "在输入区展示额外的快捷操作按钮"),
];

/// Administrator-enabled tool groups mirrored from `settings_page.slint`
/// ("管理员开放工具"). The runtime has no tool registry plumbed into this
/// crate, so the catalogue is the only source of group titles, tool names and
/// their one-line descriptions; `AppState` stores only the flattened flags.
const AGENT_TOOL_GROUPS: &[(&str, &[(&str, &str)])] = &[
    (
        "内置工具",
        &[
            ("文件读写", "在工作区内读取、创建与修改文件"),
            ("终端执行", "在工作目录中运行命令并回传输出"),
            ("网页搜索", "检索公开网页并返回摘要"),
        ],
    ),
    (
        "MCP 工具",
        &[("MCP 服务器", "通过 MCP 协议连接的外部工具服务")],
    ),
    ("A2A 工具", &[("A2A 智能体", "把子任务转交给另一个智能体")]),
    ("技能工具", &[("技能检索", "按关键词检索已安装的技能")]),
    (
        "知识库工具",
        &[("知识库检索", "在内置知识库中检索相关片段")],
    ),
    ("自定义工具", &[("用户脚本", "由用户自建并注册的本地脚本")]),
];

/// Preset-question cap, matching `stc_agent_questions_note`（"最多 12 条"）。
const AGENT_QUESTION_LIMIT: usize = 12;

/// Schedule kinds use exactly the `cr_kind_*` labels. `AppState::cron_kind`
/// stores the label itself, so the selector and the pure helpers can never
/// disagree about which kind is active.
const CRON_KIND_ONCE: &str = "一次性";
const CRON_KIND_INTERVAL: &str = "固定间隔";
const CRON_KIND_CRON: &str = "Cron 表达式";
const CRON_KINDS: [&str; 3] = [CRON_KIND_ONCE, CRON_KIND_INTERVAL, CRON_KIND_CRON];

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
        .backdrop_color(tokens.scrim)
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
                    color: tokens.shadow_modal,
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
                        "agent" => page_agent(ui, state, tokens),
                        "companion" => page_companion(ui, state, tokens),
                        "cron" => page_cron(ui, state, tokens),
                        "memory" => page_memory(ui, tokens),
                        "channels" => page_channels(ui, tokens),
                        "devices" => page_devices(ui, state, tokens),
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
        .stroke(Stroke::new(1.0, tokens.card_border))
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
                tokens.heading,
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
            Stroke::new(1.0, tokens.divider),
        );
    }
}

fn select(ui: &mut Ui, id: &str, value: &mut String, choices: &[&str], tokens: Tokens) {
    ui.scope(|ui| {
        ui.spacing_mut().interact_size.y = 38.0;
        ui.spacing_mut().button_padding = Vec2::new(12.0, 8.0);
        let widgets = &mut ui.visuals_mut().widgets;
        for visual in [&mut widgets.inactive, &mut widgets.noninteractive] {
            visual.bg_fill = tokens.control_fill;
            visual.weak_bg_fill = tokens.control_fill;
            visual.bg_stroke = Stroke::new(1.0, tokens.control_border);
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
    ui.painter().rect_filled(track, 3.0, tokens.track);
    ui.painter().rect_stroke(
        track,
        3.0,
        Stroke::new(1.0, tokens.track_stroke),
        StrokeKind::Inside,
    );
    let center = Pos2::new(
        rect.left() + ((*value - min) / (max - min)).clamp(0.0, 1.0) * rect.width(),
        rect.center().y,
    );
    ui.painter().circle_filled(center, 7.0, tokens.on_accent);
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

/// A tool path is accepted only when it is non-empty after trimming and does
/// not already exist. Comparison ignores ASCII case because Windows paths are
/// case-insensitive, so "D:/Tools" and "d:/tools" are the same directory.
fn valid_tool_path(value: &str, existing: &[String]) -> bool {
    let candidate = value.trim();
    !candidate.is_empty()
        && !existing
            .iter()
            .any(|item| item.trim().eq_ignore_ascii_case(candidate))
}

/// Keep the stepper, slider and preview on one range. `f32::clamp` propagates
/// NaN, which would poison the preview font and the stored scale, so NaN falls
/// back to the documented minimum while infinities clamp like any other value.
fn clamp_font_size(value: f32) -> f32 {
    if value.is_nan() {
        FONT_SIZE_MIN
    } else {
        value.clamp(FONT_SIZE_MIN, FONT_SIZE_MAX)
    }
}

fn supplement_description(name: &str) -> &'static str {
    SUPPLEMENT_CATALOGUE
        .iter()
        .find(|(label, _)| *label == name)
        .map(|(_, description)| *description)
        .unwrap_or("")
}

/// The self check is deliberately local: it only reads fields already in
/// `AppState`, so it can never write user data or reach the network.
fn self_check_summary(state: &AppState) -> String {
    let mut issues = Vec::new();
    if state.runtime_path.trim().is_empty() {
        issues.push("运行时路径未设置");
    }
    if state.workspace_root.trim().is_empty() {
        issues.push("工作目录未设置");
    }
    let enabled = state
        .supplement_bundles
        .iter()
        .filter(|(_, enabled)| *enabled)
        .count();
    let base = format!(
        "工具路径 {} 项，补充包 {enabled}/{} 启用",
        state.tool_paths.len(),
        state.supplement_bundles.len()
    );
    // A missing path is a warning, not a failure: the page still works with
    // the placeholder values, so the wording must not imply broken state.
    if issues.is_empty() {
        format!("自检通过：运行时与工作目录已设置，{base}。")
    } else {
        format!("自检完成（有提醒）：{}，{base}。", issues.join("、"))
    }
}

fn run_self_check(state: &mut AppState, now: f64) {
    let summary = self_check_summary(state);
    state.self_check_state = summary;
    state.self_check_at = Some(now);
}

fn self_check_age_text(now: f64, at: f64) -> String {
    let seconds = (now - at).max(0.0);
    if seconds < 1.0 {
        "刚刚".to_owned()
    } else if seconds < 60.0 {
        format!("{seconds:.0} 秒前")
    } else {
        format!("{:.0} 分钟前", seconds / 60.0)
    }
}

/// Flattened `(start, end)` index ranges of `AGENT_TOOL_GROUPS`, so the group
/// "select all" buttons and `AppState::agent_tool_enabled` address the same
/// slots even when a group is filtered out by the search box.
fn agent_tool_ranges() -> Vec<(usize, usize)> {
    let mut ranges = Vec::with_capacity(AGENT_TOOL_GROUPS.len());
    let mut start = 0;
    for (_, tools) in AGENT_TOOL_GROUPS {
        ranges.push((start, start + tools.len()));
        start += tools.len();
    }
    ranges
}

/// Search runs over the name *and* the description because the Slint picker
/// does the same; an empty keyword keeps every tool so the list is never
/// hidden by a stale query.
fn agent_tool_matches(name: &str, description: &str, keyword: &str) -> bool {
    let needle = keyword.trim().to_lowercase();
    needle.is_empty()
        || name.to_lowercase().contains(&needle)
        || description.to_lowercase().contains(&needle)
}

/// A preset question must carry text (the Slint draft row disables "添加" on an
/// empty buffer) and must stay inside the documented 12-entry cap.
fn agent_question_can_add(draft: &str, count: usize) -> bool {
    !draft.trim().is_empty() && count < AGENT_QUESTION_LIMIT
}

/// Header count on the devices page. `total` is the adapter's device count;
/// `online` is the subset whose tunnel reports a live heartbeat.
fn device_presence_summary(online: usize, total: usize) -> String {
    format!("{online} 台在线 · 共 {total} 台")
}

/// Sharing the workspace catalogue widens what a remote peer may read, so the
/// switch is only offered once interlink itself is on. The page still renders
/// the current value, it just cannot be changed.
fn share_switch_available(allow_remote: bool) -> bool {
    allow_remote
}

fn interlink_state_label(enabled: bool) -> &'static str {
    if enabled {
        "已开启"
    } else {
        "已关闭"
    }
}

/// `YYYY-MM-DD HH:MM`, the format the Slint editor's "执行时间" fields expect.
/// Only digits and separators are accepted, then the numeric ranges are
/// checked; the page has no calendar widget, so a malformed string has to be
/// rejected here rather than silently stored.
fn valid_cron_moment(text: &str) -> bool {
    let value = text.trim();
    if value.len() != 16 {
        return false;
    }
    let shaped = value.bytes().enumerate().all(|(index, byte)| match index {
        4 | 7 => byte == b'-',
        10 => byte == b' ',
        13 => byte == b':',
        _ => byte.is_ascii_digit(),
    });
    if !shaped {
        return false;
    }
    let number =
        |range: std::ops::Range<usize>| value.get(range).and_then(|part| part.parse::<u32>().ok());
    let (Some(month), Some(day), Some(hour), Some(minute)) =
        (number(5..7), number(8..10), number(11..13), number(14..16))
    else {
        return false;
    };
    (1..=12).contains(&month) && (1..=31).contains(&day) && hour < 24 && minute < 60
}

/// Cron fields are numeric with the usual operators; names such as `MON` are
/// deliberately rejected because the runtime's parser is not wired in yet and
/// accepting them here would promise behaviour nothing can verify.
fn valid_cron_field(field: &str) -> bool {
    !field.is_empty()
        && field
            .chars()
            .all(|value| value.is_ascii_digit() || matches!(value, '*' | ',' | '-' | '/' | '?'))
}

/// Returns the first problem with the schedule, or `None` when the draft can be
/// submitted. Errors are user-facing strings so the editor never has to map an
/// enum back to prose.
fn cron_schedule_error(kind: &str, at: &str, every: &str, expression: &str) -> Option<String> {
    match kind {
        CRON_KIND_ONCE => {
            if at.trim().is_empty() {
                Some("请填写执行时间，格式如 2026-10-20 09:00。".to_owned())
            } else if !valid_cron_moment(at) {
                Some("执行时间格式应为 YYYY-MM-DD HH:MM。".to_owned())
            } else {
                None
            }
        }
        CRON_KIND_INTERVAL => {
            let seconds = every.trim().parse::<u32>();
            if !matches!(seconds, Ok(value) if value > 0) {
                return Some("间隔需为正整数秒，例如 300。".to_owned());
            }
            if !at.trim().is_empty() && !valid_cron_moment(at) {
                return Some("首次时间格式应为 YYYY-MM-DD HH:MM。".to_owned());
            }
            None
        }
        CRON_KIND_CRON => {
            let fields: Vec<&str> = expression.split_whitespace().collect();
            if !matches!(fields.len(), 5 | 6) {
                return Some("Cron 表达式需要 5 或 6 个以空格分隔的字段。".to_owned());
            }
            if !fields.iter().all(|field| valid_cron_field(field)) {
                return Some("Cron 字段只能包含数字与 * , - / ? 字符。".to_owned());
            }
            None
        }
        other => Some(format!("未知的调度类型：{other}。")),
    }
}

/// One-line schedule summary for the job list, mirroring
/// `CronCard.schedule + " · 下次 " + next-run` from the Slint list. A missing
/// value is spelled out instead of leaving the row blank.
fn cron_schedule_summary(kind: &str, at: &str, every: &str, expression: &str) -> String {
    match kind {
        CRON_KIND_ONCE => {
            if at.trim().is_empty() {
                "一次性 · 未设置执行时间".to_owned()
            } else {
                format!("一次性 · {}", at.trim())
            }
        }
        CRON_KIND_INTERVAL => match every.trim().parse::<u32>() {
            Ok(seconds) if seconds > 0 => format!("固定间隔 · 每 {seconds} 秒"),
            _ => "固定间隔 · 未设置间隔".to_owned(),
        },
        CRON_KIND_CRON => {
            if expression.trim().is_empty() {
                "Cron 表达式 · 未填写".to_owned()
            } else {
                format!("Cron 表达式 · {}", expression.trim())
            }
        }
        other => other.to_owned(),
    }
}

/// Read-only "label — value" row. The value is right anchored and clipped so
/// a long absolute path cannot widen the card or overlap the label.
fn path_row(ui: &mut Ui, tokens: Tokens, label: &str, value: &str) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 42.0), Sense::hover());
    ui.painter().text(
        rect.left_center(),
        Align2::LEFT_CENTER,
        label,
        FontId::proportional(14.0),
        tokens.text_secondary,
    );
    let empty = value.trim().is_empty();
    let text = if empty { "未设置" } else { value.trim() };
    let bounds = Rect::from_min_max(Pos2::new(rect.left() + 112.0, rect.top()), rect.max);
    ui.painter()
        .with_clip_rect(bounds.intersect(ui.clip_rect()))
        .text(
            bounds.right_center(),
            Align2::RIGHT_CENTER,
            text,
            FontId::proportional(13.0),
            if empty {
                tokens.text_muted
            } else {
                tokens.text
            },
        );
    ui.painter().line_segment(
        [rect.left_bottom(), rect.right_bottom()],
        Stroke::new(1.0, tokens.divider),
    );
}

/// Row-scoped button. `fixed_button` keys its interaction by label, which
/// would collide across repeated "移除" rows, so the caller supplies an id.
fn row_button(
    ui: &mut Ui,
    rect: Rect,
    id: impl std::hash::Hash,
    label: &str,
    tokens: Tokens,
) -> bool {
    let response = ui.interact(rect, Id::new(id), Sense::click());
    ui.painter().rect_filled(
        rect,
        9.0,
        if response.hovered() {
            tokens.hover
        } else {
            tokens.surface_sunken
        },
    );
    ui.painter().rect_stroke(
        rect,
        9.0,
        Stroke::new(1.0, tokens.border_strong),
        StrokeKind::Inside,
    );
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        label,
        FontId::proportional(12.0),
        tokens.text_secondary,
    );
    response.clicked()
}

/// Bordered single-line editor used by the tool path add row. Returns `true`
/// when Enter submitted the buffer, matching the explicit "添加路径" button.
fn path_input(
    ui: &mut Ui,
    tokens: Tokens,
    id: &str,
    value: &mut String,
    hint: &str,
    width: f32,
) -> bool {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, CONTROL_HEIGHT), Sense::hover());
    ui.painter().rect_filled(rect, 9.0, tokens.surface);
    ui.painter().rect_stroke(
        rect,
        9.0,
        Stroke::new(1.0, tokens.field_border),
        StrokeKind::Inside,
    );
    let mut submitted = false;
    child(ui, rect.shrink2(Vec2::new(10.0, 7.0)), id, |ui| {
        let edit = ui.add_sized(
            ui.available_size(),
            TextEdit::singleline(value)
                .char_limit(260)
                .frame(false)
                .margin(Margin::ZERO)
                .font(FontId::proportional(13.0))
                .hint_text(hint),
        );
        submitted = edit.lost_focus() && ui.input(|input| input.key_pressed(Key::Enter));
    });
    submitted
}

fn general_runtime_card(ui: &mut Ui, state: &mut AppState, tokens: Tokens) {
    card(
        ui,
        tokens,
        "运行时与工作目录",
        "当前运行时的位置与进程工作目录，仅作展示",
        |ui| {
            path_row(ui, tokens, "运行时路径", &state.runtime_path);
            path_row(ui, tokens, "工作目录", &state.workspace_root);
            ui.add_space(10.0);
            let root = state.workspace_root.trim().to_owned();
            let mut note = None;
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                if action_button(ui, tokens, "打开工作目录", Some("external")).clicked() {
                    note = Some(if root.is_empty() {
                        "工作目录未设置，无法打开。请先在桌面端选择工作目录。".to_owned()
                    } else {
                        // The desktop shell owns process launching. Until that
                        // command exists the page must not pretend it ran.
                        "桌面端外壳尚未接入打开目录命令，可先复制路径后手动打开。".to_owned()
                    });
                }
                if action_button(ui, tokens, "复制路径", Some("copy")).clicked() {
                    note = Some(if root.is_empty() {
                        "工作目录未设置，没有可复制的路径。".to_owned()
                    } else {
                        ui.ctx().copy_text(root.clone());
                        "已复制工作目录路径。".to_owned()
                    });
                }
            });
            if let Some(note) = note {
                state.workspace_action_note = note;
            }
            if !state.workspace_action_note.is_empty() {
                ui.add_space(6.0);
                ui.label(
                    RichText::new(state.workspace_action_note.as_str())
                        .size(12.0)
                        .color(tokens.text_muted),
                );
            }
        },
    );
}

fn general_tool_paths_card(ui: &mut Ui, state: &mut AppState, tokens: Tokens) {
    card(
        ui,
        tokens,
        "工具路径",
        "为工具检索追加本地目录，重复或空路径不会添加",
        |ui| {
            if state.tool_paths.is_empty() {
                empty_text(
                    ui,
                    tokens,
                    "暂未添加工具路径，添加后工具会从这些目录检索可执行文件。",
                );
            } else {
                let paths = state.tool_paths.clone();
                let mut remove = None;
                for (index, path) in paths.iter().enumerate() {
                    let (row, _) = ui.allocate_exact_size(
                        Vec2::new(ui.available_width(), LIST_ROW_HEIGHT),
                        Sense::hover(),
                    );
                    ui.painter().rect_filled(row, 9.0, tokens.surface_sunken);
                    ui.painter().rect_stroke(
                        row,
                        9.0,
                        Stroke::new(1.0, tokens.border),
                        StrokeKind::Inside,
                    );
                    draw_icon(
                        ui.painter(),
                        row.left_center() + Vec2::new(16.0, 0.0),
                        "folder",
                        12.0,
                        tokens.text_muted,
                    );
                    let text_bounds = Rect::from_min_max(
                        Pos2::new(row.left() + 32.0, row.top()),
                        Pos2::new(row.right() - 68.0, row.bottom()),
                    );
                    ui.painter()
                        .with_clip_rect(text_bounds.intersect(ui.clip_rect()))
                        .text(
                            text_bounds.left_center(),
                            Align2::LEFT_CENTER,
                            path,
                            FontId::proportional(12.0),
                            tokens.text_secondary,
                        );
                    let button = Rect::from_center_size(
                        Pos2::new(row.right() - 40.0, row.center().y),
                        Vec2::new(56.0, 28.0),
                    );
                    if row_button(ui, button, ("tool-path-remove", index), "移除", tokens) {
                        remove = Some(index);
                    }
                    ui.add_space(6.0);
                }
                if let Some(index) = remove {
                    state.tool_paths.remove(index);
                    state.tool_path_error.clear();
                }
            }
            ui.add_space(8.0);
            let mut submitted = false;
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                // Leave room for the "添加路径" button on the compact layout;
                // a fixed 304px field would otherwise push it out of the card.
                let field_width = 304.0_f32.min((ui.available_width() - 120.0).max(120.0));
                submitted = path_input(
                    ui,
                    tokens,
                    "tool-path-input",
                    &mut state.tool_path_input,
                    "例如 D:/tools/bin",
                    field_width,
                );
                if action_button(ui, tokens, "添加路径", Some("plus")).clicked() {
                    submitted = true;
                }
            });
            if submitted {
                let value = state.tool_path_input.trim().to_owned();
                if valid_tool_path(&value, &state.tool_paths) {
                    state.tool_paths.push(value);
                    state.tool_path_input.clear();
                    state.tool_path_error.clear();
                } else {
                    state.tool_path_error = if value.is_empty() {
                        "路径不能为空。".to_owned()
                    } else {
                        "该路径已在列表中，无需重复添加。".to_owned()
                    };
                }
            }
            if !state.tool_path_error.is_empty() {
                ui.add_space(6.0);
                ui.label(
                    RichText::new(state.tool_path_error.as_str())
                        .size(12.0)
                        .color(tokens.danger_soft),
                );
            }
        },
    );
}

fn general_supplements_card(ui: &mut Ui, state: &mut AppState, tokens: Tokens) {
    card(
        ui,
        tokens,
        "补充包",
        "可选能力开关，关闭的补充包不参与本地能力加载",
        |ui| {
            let total = state.supplement_bundles.len();
            for index in 0..total {
                let (name, mut enabled) = state.supplement_bundles[index].clone();
                let description = supplement_description(&name);
                detail_row(
                    ui,
                    tokens,
                    "prompts",
                    &name,
                    description,
                    index + 1 == total,
                    |ui| {
                        // The text label carries the state; colour alone would
                        // be unreadable for colour-blind users and screenshots.
                        ui.label(
                            RichText::new(if enabled { "已启用" } else { "已关闭" })
                                .size(12.0)
                                .color(if enabled {
                                    tokens.primary
                                } else {
                                    tokens.text_muted
                                }),
                        );
                        toggle(ui, &mut enabled, tokens);
                    },
                );
                state.supplement_bundles[index].1 = enabled;
            }
        },
    );
}

fn general_diagnostics_card(ui: &mut Ui, state: &mut AppState, tokens: Tokens) {
    card(
        ui,
        tokens,
        "压测与诊断",
        "自检只读取本地设置，不写入数据、不发起网络请求",
        |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                if action_button(ui, tokens, "运行自检", Some("shield")).clicked() {
                    run_self_check(state, ui.input(|input| input.time));
                }
            });
            ui.add_space(8.0);
            if let Some(at) = state.self_check_at {
                let age = self_check_age_text(ui.input(|input| input.time), at);
                ui.label(
                    RichText::new(format!("上次自检：{age}"))
                        .size(12.0)
                        .color(tokens.text_muted),
                );
                if !state.self_check_state.is_empty() {
                    ui.add_space(4.0);
                    notice(ui, tokens, state.self_check_state.as_str());
                }
            } else {
                ui.label(
                    RichText::new("尚未运行自检")
                        .size(12.0)
                        .color(tokens.text_muted),
                );
            }
        },
    );
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
                    tokens,
                );
            });
            form_row(ui, tokens, "语言", "", false, |ui| {
                select(
                    ui,
                    "settings-language",
                    &mut state.settings_language,
                    &["跟随系统", "简体中文", "English"],
                    tokens,
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
                Stroke::new(1.0, tokens.divider),
            );
            form_row(
                ui,
                tokens,
                "字体大小",
                "聊天内容的字体大小",
                true,
                |ui| {
                    let mut font_size =
                        clamp_font_size((FONT_SIZE_BASE * state.settings_font_scale).round());
                    let (controls, _) =
                        ui.allocate_exact_size(Vec2::new(336.0, 38.0), Sense::hover());
                    let pill = Rect::from_center_size(
                        Pos2::new(controls.left() + 50.0, controls.center().y),
                        Vec2::new(100.0, 34.0),
                    );
                    ui.painter().rect_filled(pill, 17.0, tokens.control_fill);
                    ui.painter().rect_stroke(
                        pill,
                        17.0,
                        Stroke::new(1.0, tokens.control_border),
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
                            font_size = clamp_font_size(font_size + delta);
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
                        FONT_SIZE_MIN..=FONT_SIZE_MAX,
                        1.0,
                        "font-size-range",
                        tokens,
                    );
                    state.settings_font_scale = font_size / FONT_SIZE_BASE;
                },
            );
            // The preview sits inside the same card so the slider and the
            // rendered sample stay visually adjacent at every window size.
            ui.add_space(10.0);
            let preview_size =
                clamp_font_size((FONT_SIZE_BASE * state.settings_font_scale).round());
            Frame::NONE
                .fill(tokens.surface_sunken)
                .stroke(Stroke::new(1.0, tokens.border))
                .corner_radius(10.0)
                .inner_margin(Margin::symmetric(12, 10))
                .show(ui, |ui| {
                    ui.set_width((ui.available_width() - 2.0).max(120.0));
                    ui.spacing_mut().item_spacing.y = 4.0;
                    ui.label(
                        RichText::new(format!("实时预览 · {preview_size:.0}px"))
                            .size(11.0)
                            .color(tokens.text_muted),
                    );
                    ui.label(
                        RichText::new("工作目录与工具路径会以这个字号显示。")
                            .size(preview_size)
                            .color(tokens.text),
                    );
                });
            ui.add_space(4.0);
        },
    );
    general_runtime_card(ui, state, tokens);
    general_tool_paths_card(ui, state, tokens);
    general_supplements_card(ui, state, tokens);
    general_diagnostics_card(ui, state, tokens);
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
    // The palette is content, not shell chrome: it lives in `theme` so this
    // page only describes layout and interaction.
    let choices = crate::theme::accent_choices();
    let width = ui.available_width();
    let columns = (((width + 12.0) / 162.0).floor() as usize).clamp(1, 4);
    let tile_width = (width - 12.0 * (columns - 1) as f32) / columns as f32;
    for (row_index, row) in choices.chunks(columns).enumerate() {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 12.0;
            for choice in row {
                let active = state.settings_theme == choice.id
                    || (state.settings_theme == "浅色" && choice.id == "eva-orange");
                let (rect, response) =
                    ui.allocate_exact_size(Vec2::new(tile_width, 123.0), Sense::click());
                ui.painter().rect_filled(rect, 12.0, tokens.surface);
                ui.painter().rect_stroke(
                    rect,
                    12.0,
                    Stroke::new(
                        1.0,
                        if active {
                            tokens.text
                        } else if response.hovered() {
                            choice.accent
                        } else {
                            tokens.card_border
                        },
                    ),
                    StrokeKind::Inside,
                );
                let mock = Rect::from_min_size(
                    rect.min + Vec2::splat(10.0),
                    Vec2::new(tile_width - 20.0, 74.0),
                );
                let p = ui.painter();
                p.rect_filled(mock, 9.0, choice.tint);
                p.rect_stroke(
                    mock,
                    9.0,
                    Stroke::new(1.0, tokens.divider),
                    StrokeKind::Inside,
                );
                p.rect_filled(
                    Rect::from_min_size(
                        mock.min + Vec2::new(12.0, 12.0),
                        Vec2::new(mock.width() * 0.46, 7.0),
                    ),
                    4.0,
                    choice.accent,
                );
                p.rect_filled(
                    Rect::from_min_size(
                        mock.min + Vec2::new(12.0, 25.0),
                        Vec2::new(mock.width() * 0.30, 5.0),
                    ),
                    3.0,
                    tokens.border_strong,
                );
                let avatar = Pos2::new(mock.right() - 19.5, mock.top() + 16.5);
                p.circle_filled(avatar, 7.5, tokens.on_accent);
                p.circle_stroke(
                    avatar,
                    if active { 5.5 } else { 7.0 },
                    Stroke::new(
                        if active { 4.0 } else { 1.0 },
                        if active { choice.accent } else { tokens.border },
                    ),
                );
                let body = Rect::from_min_max(
                    mock.min + Vec2::new(12.0, 38.0),
                    mock.max - Vec2::new(12.0, 8.0),
                );
                p.rect_filled(body, 7.0, tokens.on_accent);
                p.rect_stroke(
                    body,
                    7.0,
                    Stroke::new(1.0, tokens.divider),
                    StrokeKind::Inside,
                );
                for (dy, factor) in [(6.0, 0.70), (16.0, 0.42)] {
                    p.rect_filled(
                        Rect::from_min_size(
                            body.min + Vec2::new(10.0, dy),
                            Vec2::new((body.width() - 20.0) * factor, 5.0),
                        ),
                        3.0,
                        tokens.border_strong,
                    );
                }
                p.text(
                    Pos2::new(rect.center().x, rect.top() + 104.0),
                    Align2::CENTER_CENTER,
                    choice.label,
                    FontId::proportional(13.0),
                    tokens.heading,
                );
                if response.clicked() {
                    state.settings_theme = choice.id.to_owned();
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
        Vec2::new(
            text_width + if icon.is_some() { 46.0 } else { 24.0 },
            CONTROL_HEIGHT,
        ),
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

fn accent_button(ui: &mut Ui, rect: Rect, label: &str, solid: bool, tokens: Tokens) -> bool {
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
            tokens.primary_fill
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
            tokens.on_accent
        } else {
            tokens.primary
        },
    );
    // Returning the click lets the cron editor share one "primary action"
    // style with the memory page without duplicating the painting code.
    response.clicked()
}

fn panel(ui: &mut Ui, tokens: Tokens, min_height: f32, body: impl FnOnce(&mut Ui)) {
    let width = ui.available_width();
    Frame::NONE
        .fill(tokens.surface)
        .stroke(Stroke::new(1.0, tokens.card_border))
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
        Stroke::new(1.0, tokens.card_border),
        StrokeKind::Inside,
    );
    let origin = profile.min + Vec2::new(17.0, 15.0);
    let avatar = Rect::from_min_size(origin, Vec2::splat(56.0));
    painter.circle_filled(avatar.center(), 28.0, tokens.avatar);
    icons::paint(painter, avatar.shrink(16.0), Icon::User, tokens.on_accent);
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
                tokens.control_fill
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
    painter.rect_filled(progress, 5.0, tokens.track_subtle);
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
        Stroke::new(1.0, tokens.card_border),
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
        painter.rect_filled(rect, 10.0, tokens.surface_sunken);
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
    painter.rect_filled(chart, 12.0, tokens.surface_sunken);
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
    painter.rect_filled(balance, 18.0, tokens.gold_tint);
    painter.rect_stroke(
        balance,
        18.0,
        Stroke::new(1.0, tokens.gold_border),
        StrokeKind::Inside,
    );
    let coin = Rect::from_min_size(balance.min + Vec2::new(17.0, 17.0), Vec2::splat(42.0));
    painter.rect_filled(coin, 14.0, tokens.gold_fill);
    painter.text(
        coin.center(),
        Align2::CENTER_CENTER,
        "\u{f51e}",
        FontId::new(20.0, egui::FontFamily::Name(icons::FONT_NAME.into())),
        tokens.gold_text,
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

/// One administrator-enabled tool. The whole tile is the hit target, matching
/// the Slint `ToolTile`, and the state is also written out as text so the
/// switch position is never the only cue.
fn agent_tool_row(ui: &mut Ui, tokens: Tokens, name: &str, description: &str, enabled: &mut bool) {
    let (row, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 52.0), Sense::click());
    ui.painter().rect_filled(
        row,
        10.0,
        if *enabled {
            tokens.primary_tint
        } else {
            tokens.surface_sunken
        },
    );
    ui.painter().rect_stroke(
        row,
        10.0,
        Stroke::new(
            1.0,
            if *enabled {
                tokens.primary_soft
            } else {
                tokens.border
            },
        ),
        StrokeKind::Inside,
    );
    draw_icon(
        ui.painter(),
        row.left_center() + Vec2::new(18.0, 0.0),
        "tools",
        13.0,
        tokens.text_muted,
    );
    let text_bounds = Rect::from_min_max(
        Pos2::new(row.left() + 34.0, row.top() + 7.0),
        Pos2::new(row.right() - 148.0, row.bottom() - 4.0),
    );
    let painter = ui
        .painter()
        .with_clip_rect(text_bounds.intersect(ui.clip_rect()));
    painter.text(
        text_bounds.left_top(),
        Align2::LEFT_TOP,
        name,
        FontId::proportional(13.0),
        tokens.text,
    );
    painter.text(
        text_bounds.left_top() + Vec2::new(0.0, 19.0),
        Align2::LEFT_TOP,
        if description.is_empty() {
            "无说明"
        } else {
            description
        },
        FontId::proportional(11.0),
        tokens.text_muted,
    );
    ui.painter().text(
        Pos2::new(row.right() - 68.0, row.center().y),
        Align2::RIGHT_CENTER,
        if *enabled { "已启用" } else { "已关闭" },
        FontId::proportional(12.0),
        if *enabled {
            tokens.primary
        } else {
            tokens.text_muted
        },
    );
    let switch = Rect::from_center_size(
        Pos2::new(row.right() - 34.0, row.center().y),
        Vec2::new(44.0, 24.0),
    );
    ui.painter().rect_filled(
        switch,
        12.0,
        if *enabled {
            tokens.primary
        } else {
            tokens.border_strong
        },
    );
    ui.painter().circle_filled(
        Pos2::new(
            if *enabled {
                switch.right() - 12.0
            } else {
                switch.left() + 12.0
            },
            switch.center().y,
        ),
        9.0,
        tokens.surface,
    );
    if response.clicked() {
        *enabled = !*enabled;
    }
}

fn page_agent(ui: &mut Ui, state: &mut AppState, tokens: Tokens) {
    // §9.3: the personal build ships one built-in agent, so this category is a
    // single stacked form. The name is preset-owned and read-only; the extra
    // prompt edits the agent record, while every thread keeps the prompt it
    // froze on first use.
    let name = state.agent_name.clone();
    card(
        ui,
        tokens,
        "智能体",
        "内置智能体只有一条记录；名称由预设决定，其余字段由桌面端保存",
        |ui| {
            detail_row(
                ui,
                tokens,
                "agent",
                "智能体名称",
                "名称由内置预设决定，不能在这里修改",
                false,
                |ui| {
                    ui.label(RichText::new("只读").size(12.0).color(tokens.text_muted));
                    ui.label(RichText::new(name.as_str()).size(13.0).color(tokens.text));
                },
            );
            form_row(
                ui,
                tokens,
                "问候语",
                "用于会话中的首次问候",
                true,
                |ui| {
                    let _ = path_input(
                        ui,
                        tokens,
                        "agent-greeting",
                        &mut state.agent_greeting,
                        "用于会话中的首次问候",
                        336.0,
                    );
                },
            );
            ui.add_space(10.0);
            ui.label(
                RichText::new("额外提示词")
                    .size(14.0)
                    .color(tokens.text_secondary),
            );
            ui.add_space(2.0);
            ui.label(
                RichText::new("追加在基础系统提示词之后，保存后对新线程生效。")
                    .size(12.0)
                    .color(tokens.text_muted),
            );
            ui.add_space(6.0);
            text_area(
                ui,
                tokens,
                "agent-extra-prompt",
                &mut state.agent_extra_prompt,
                "输入需要追加到基础系统提示词后的内容",
                176.0,
            );
        },
    );
    card(
        ui,
        tokens,
        "管理员开放工具",
        "勾选即写入内置智能体的工具启用集合；搜索只过滤当前列表。",
        |ui| {
            let field_width = 270.0_f32.min((ui.available_width() - 8.0).max(120.0));
            let _ = path_input(
                ui,
                tokens,
                "agent-tool-search",
                &mut state.agent_tool_search,
                "搜索工具",
                field_width,
            );
            ui.add_space(10.0);
            let keyword = state.agent_tool_search.trim().to_lowercase();
            let ranges = agent_tool_ranges();
            let mut any_visible = false;
            for (group_index, (title, tools)) in AGENT_TOOL_GROUPS.iter().enumerate() {
                let Some((start, _)) = ranges.get(group_index).copied() else {
                    continue;
                };
                let visible: Vec<(usize, &str, &str)> = tools
                    .iter()
                    .enumerate()
                    .map(|(offset, (name, description))| (start + offset, *name, *description))
                    .filter(|(_, name, description)| {
                        agent_tool_matches(name, description, &keyword)
                    })
                    .collect();
                if visible.is_empty() {
                    continue;
                }
                any_visible = true;
                let all_selected = visible.iter().all(|(index, ..)| {
                    state
                        .agent_tool_enabled
                        .get(*index)
                        .copied()
                        .unwrap_or(false)
                });
                let (head, _) =
                    ui.allocate_exact_size(Vec2::new(ui.available_width(), 28.0), Sense::hover());
                ui.painter().text(
                    head.left_center(),
                    Align2::LEFT_CENTER,
                    format!("{title} · {} 项", visible.len()),
                    FontId::proportional(13.0),
                    tokens.text,
                );
                let toggle_all = Rect::from_min_size(
                    Pos2::new(head.right() - 84.0, head.top() + 2.0),
                    Vec2::new(84.0, 24.0),
                );
                if row_button(
                    ui,
                    toggle_all,
                    ("agent-tool-group", group_index),
                    if all_selected {
                        "取消全选"
                    } else {
                        "全选"
                    },
                    tokens,
                ) {
                    for (index, ..) in &visible {
                        if let Some(flag) = state.agent_tool_enabled.get_mut(*index) {
                            *flag = !all_selected;
                        }
                    }
                }
                ui.add_space(4.0);
                for (index, name, description) in visible {
                    let mut enabled = state
                        .agent_tool_enabled
                        .get(index)
                        .copied()
                        .unwrap_or(false);
                    agent_tool_row(ui, tokens, name, description, &mut enabled);
                    if let Some(flag) = state.agent_tool_enabled.get_mut(index) {
                        *flag = enabled;
                    }
                    ui.add_space(6.0);
                }
                ui.add_space(8.0);
            }
            if !any_visible {
                empty_text(
                    ui,
                    tokens,
                    if keyword.is_empty() {
                        // The runtime has no registry plumbed in yet, so an
                        // empty list must say where tools come from.
                        "工具清单由桌面端提供，当前没有可显示的工具。"
                    } else {
                        "没有匹配的工具，换个关键词试试。"
                    },
                );
            }
        },
    );
    card(
        ui,
        tokens,
        "预设问题",
        "最多 12 条，随配置保存；回车或点「添加预设问题」加入。",
        |ui| {
            if state.agent_preset_questions.is_empty() {
                empty_text(ui, tokens, "暂未配置预设问题，可按需添加多个。");
            } else {
                let questions = state.agent_preset_questions.clone();
                let mut remove = None;
                for (index, question) in questions.iter().enumerate() {
                    let (row, _) = ui.allocate_exact_size(
                        Vec2::new(ui.available_width(), LIST_ROW_HEIGHT),
                        Sense::hover(),
                    );
                    ui.painter().rect_filled(row, 9.0, tokens.surface_sunken);
                    ui.painter().rect_stroke(
                        row,
                        9.0,
                        Stroke::new(1.0, tokens.border),
                        StrokeKind::Inside,
                    );
                    let text_bounds = Rect::from_min_max(
                        Pos2::new(row.left() + 12.0, row.top()),
                        Pos2::new(row.right() - 76.0, row.bottom()),
                    );
                    ui.painter()
                        .with_clip_rect(text_bounds.intersect(ui.clip_rect()))
                        .text(
                            text_bounds.left_center(),
                            Align2::LEFT_CENTER,
                            question,
                            FontId::proportional(12.0),
                            tokens.text_secondary,
                        );
                    let button = Rect::from_center_size(
                        Pos2::new(row.right() - 42.0, row.center().y),
                        Vec2::new(56.0, 28.0),
                    );
                    if row_button(ui, button, ("agent-question-remove", index), "删除", tokens) {
                        remove = Some(index);
                    }
                    ui.add_space(6.0);
                }
                if let Some(index) = remove {
                    if index < state.agent_preset_questions.len() {
                        state.agent_preset_questions.remove(index);
                        state.agent_note =
                            format!("已删除第 {} 条预设问题（仅本地草稿）。", index + 1);
                        state.agent_note_error = false;
                    }
                }
            }
            ui.add_space(8.0);
            let mut submitted = false;
            let mut add = false;
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                // Leave room for the add button; a fixed 320px field would
                // otherwise push it out of the card on the compact layout.
                let field_width = 320.0_f32.min((ui.available_width() - 140.0).max(120.0));
                submitted = path_input(
                    ui,
                    tokens,
                    "agent-question-input",
                    &mut state.agent_question_draft,
                    "输入一条预设问题…",
                    field_width,
                );
                add = action_button(ui, tokens, "添加预设问题", Some("plus")).clicked();
            });
            if submitted || add {
                let draft = state.agent_question_draft.trim().to_owned();
                if agent_question_can_add(&draft, state.agent_preset_questions.len()) {
                    state.agent_preset_questions.push(draft);
                    state.agent_question_draft.clear();
                    state.agent_note = format!(
                        "已添加第 {} 条预设问题（仅本地草稿；桌面端保存命令接入后才会写入配置）。",
                        state.agent_preset_questions.len()
                    );
                    state.agent_note_error = false;
                } else {
                    state.agent_note = if draft.is_empty() {
                        "预设问题不能为空。".to_owned()
                    } else {
                        format!("最多 {AGENT_QUESTION_LIMIT} 条，请先删除一条再添加。")
                    };
                    state.agent_note_error = true;
                }
            }
            if !state.agent_note.is_empty() {
                note_line(
                    ui,
                    tokens,
                    state.agent_note.as_str(),
                    state.agent_note_error,
                );
            }
        },
    );
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
            Stroke::new(1.0, tokens.divider),
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
                .stroke(Stroke::new(1.0, tokens.card_border))
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

fn page_cron(ui: &mut Ui, state: &mut AppState, tokens: Tokens) {
    let height = page_height(ui);
    panel(ui, tokens, height, |ui| {
        panel_header(
            ui,
            tokens,
            "定时任务",
            "定时任务按智能体隔离，支持创建、编辑与启停",
            |_| {},
        );
        // Toolbar geometry follows `cron_page.slint`: a muted caption on the
        // left, "刷新" and the primary "新建定时任务" anchored to the right.
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
        if accent_button(ui, create, "新建定时任务", false, tokens) {
            state.cron_editor_open = !state.cron_editor_open;
            state.cron_note.clear();
            state.cron_note_error = false;
        }
        let refresh = Rect::from_center_size(
            Pos2::new(create.left() - 32.0, toolbar.center().y),
            Vec2::new(50.0, 30.0),
        );
        if fixed_button(ui, refresh, "刷新", true, false, tokens) {
            // The desktop adapter owns the job list. Refresh must not leave the
            // page pretending a request is in flight, so it reports why the
            // list did not change instead of clearing the empty state.
            state.cron_note = if state.cron_loading {
                "正在刷新定时任务…".to_owned()
            } else {
                "桌面端尚未接入定时任务列表命令，列表没有变化。".to_owned()
            };
            state.cron_note_error = false;
        }
        let runs = Rect::from_center_size(
            Pos2::new(refresh.left() - 33.0, toolbar.center().y),
            Vec2::new(58.0, 30.0),
        );
        if fixed_button(ui, runs, "运行记录", true, false, tokens) {
            state.cron_runs_open = !state.cron_runs_open;
        }

        let width = ui.available_width();
        let body = (height - 132.0).max(240.0);
        let (columns, _) = ui.allocate_exact_size(Vec2::new(width, body), Sense::hover());
        let left_width = (width * 0.25).clamp(150.0, (width * 0.45).max(150.0));
        let left = Rect::from_min_max(
            columns.min,
            Pos2::new(
                (columns.left() + left_width).min(columns.right()),
                columns.bottom(),
            ),
        );
        let right = Rect::from_min_max(Pos2::new(left.right() + 10.0, columns.top()), columns.max);
        for rect in [left, right] {
            ui.painter().rect_filled(rect, 12.0, tokens.surface_sunken);
            ui.painter().rect_stroke(
                rect,
                12.0,
                Stroke::new(1.0, tokens.border),
                StrokeKind::Inside,
            );
        }
        child(ui, left.shrink(12.0), "cron-job-list", |ui| {
            ui.spacing_mut().item_spacing = Vec2::new(0.0, 6.0);
            ui.label(RichText::new("任务列表").size(13.0).color(tokens.text));
            ui.add_space(6.0);
            if state.cron_loading {
                ui.label(
                    RichText::new("正在加载…")
                        .size(12.0)
                        .color(tokens.text_muted),
                );
            } else {
                empty_text(ui, tokens, "暂无定时任务");
                ui.label(
                    RichText::new("列表由桌面端提供，在桌面端创建后会显示在这里。")
                        .size(11.0)
                        .color(tokens.text_muted),
                );
            }
        });
        child(ui, right.shrink(14.0), "cron-job-detail", |ui| {
            // The pane scrolls on its own so the detail rows and the adapter
            // notice stay reachable when the modal shrinks to its minimum.
            ScrollArea::vertical()
                .id_salt("cron-detail")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.spacing_mut().item_spacing = Vec2::new(0.0, 4.0);
                    ui.label(
                        RichText::new("定时任务")
                            .size(18.0)
                            .strong()
                            .color(tokens.text),
                    );
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new("在左侧选择任务查看详情")
                            .size(12.0)
                            .color(tokens.text_muted),
                    );
                    ui.add_space(10.0);
                    dashed_rule(ui, tokens.divider);
                    ui.add_space(6.0);
                    // The read-only rows mirror `cron_page.slint`'s detail
                    // block. With no adapter data they stay visible and spelled
                    // out rather than collapsing into a blank pane.
                    for (label, value) in [
                        ("调度", "—"),
                        ("下次执行", "—"),
                        ("上次状态", "尚未运行"),
                        ("上次错误", "无"),
                    ] {
                        ui.horizontal(|ui| {
                            let _ = ui.add_sized(
                                Vec2::new(96.0, 28.0),
                                egui::Label::new(
                                    RichText::new(label).size(12.0).color(tokens.text_muted),
                                ),
                            );
                            ui.label(
                                RichText::new(value)
                                    .size(13.0)
                                    .color(tokens.text_secondary),
                            );
                        });
                    }
                    ui.add_space(10.0);
                    notice(
                        ui,
                        tokens,
                        "任务详情、启停、立即运行与运行记录都由桌面端提供；当前尚未接入，因此这些操作在此页不可用。",
                    );
                });
        });

        if state.cron_editor_open {
            cron_editor_card(ui, state, tokens);
        }
        if state.cron_runs_open {
            card(
                ui,
                tokens,
                "运行记录",
                "最近 0 次（最多 200 条），由桌面端按任务返回",
                |ui| {
                    if state.cron_loading {
                        ui.label(RichText::new("加载中…").size(12.0).color(tokens.text_muted));
                    } else {
                        empty_text(ui, tokens, "暂无运行记录");
                    }
                    if action_button(ui, tokens, "关闭", Some("close")).clicked() {
                        state.cron_runs_open = false;
                    }
                },
            );
        }
        if !state.cron_note.is_empty() && !state.cron_editor_open {
            note_line(ui, tokens, state.cron_note.as_str(), state.cron_note_error);
        }
    });
}

/// The cron editor mirrors the Slint modal field-for-field. It is rendered as a
/// card inside the page instead of an overlay so the settings modal keeps its
/// single foreground layer; the field set and validation are unchanged.
fn cron_editor_card(ui: &mut Ui, state: &mut AppState, tokens: Tokens) {
    card(
        ui,
        tokens,
        "新建定时任务",
        "保存前会检查调度格式；桌面端保存命令接入前不会写入配置",
        |ui| {
            field_label(ui, tokens, "名称", "");
            let _ = path_input(
                ui,
                tokens,
                "cron-name",
                &mut state.cron_name,
                "例如 每日晨报",
                ui.available_width(),
            );
            ui.add_space(6.0);
            form_row(
                ui,
                tokens,
                "调度",
                "一次性、固定间隔或 Cron 表达式",
                false,
                |ui| {
                    select(ui, "cron-kind", &mut state.cron_kind, &CRON_KINDS, tokens);
                },
            );
            if state.cron_kind == CRON_KIND_ONCE {
                field_label(ui, tokens, "执行时间", "格式 YYYY-MM-DD HH:MM");
                let _ = path_input(
                    ui,
                    tokens,
                    "cron-at",
                    &mut state.cron_at,
                    "2026-10-20 09:00",
                    ui.available_width(),
                );
            } else if state.cron_kind == CRON_KIND_INTERVAL {
                field_label(ui, tokens, "间隔（秒）", "");
                let _ = path_input(
                    ui,
                    tokens,
                    "cron-every",
                    &mut state.cron_every,
                    "300",
                    ui.available_width(),
                );
                ui.add_space(6.0);
                field_label(ui, tokens, "首次时间", "留空表示创建后立即开始");
                let _ = path_input(
                    ui,
                    tokens,
                    "cron-first-at",
                    &mut state.cron_at,
                    "2026-10-20 09:00",
                    ui.available_width(),
                );
            } else {
                field_label(ui, tokens, "表达式", "5 或 6 个以空格分隔的字段");
                let _ = path_input(
                    ui,
                    tokens,
                    "cron-expression",
                    &mut state.cron_expression,
                    "0 9 * * *",
                    ui.available_width(),
                );
                ui.add_space(6.0);
                field_label(ui, tokens, "时区", "留空使用系统时区");
                let _ = path_input(
                    ui,
                    tokens,
                    "cron-timezone",
                    &mut state.cron_timezone,
                    "Asia/Shanghai",
                    ui.available_width(),
                );
            }
            ui.add_space(8.0);
            field_label(ui, tokens, "执行消息", "");
            text_area(
                ui,
                tokens,
                "cron-message",
                &mut state.cron_message,
                "任务触发时发送给智能体的消息",
                52.0,
            );
            ui.add_space(6.0);
            form_row(
                ui,
                tokens,
                "目标智能体",
                "个人版只有一个内置智能体",
                false,
                |ui| {
                    select(
                        ui,
                        "cron-agent",
                        &mut state.cron_target_agent,
                        &["默认智能体"],
                        tokens,
                    );
                },
            );
            for (label, hint, value) in [
                (
                    "运行一次后删除",
                    "任务成功执行一次后自动移除",
                    &mut state.cron_delete_after,
                ),
                ("启用任务", "保存后立即参与调度", &mut state.cron_enabled),
            ] {
                form_row(ui, tokens, label, hint, false, |ui| {
                    ui.label(
                        RichText::new(if *value { "已开启" } else { "已关闭" })
                            .size(12.0)
                            .color(if *value {
                                tokens.primary
                            } else {
                                tokens.text_muted
                            }),
                    );
                    toggle(ui, value, tokens);
                });
            }
            let (footer, _) =
                ui.allocate_exact_size(Vec2::new(ui.available_width(), 34.0), Sense::hover());
            let cancel_rect = Rect::from_min_size(
                Pos2::new(footer.right() - 84.0, footer.top()),
                Vec2::new(84.0, 34.0),
            );
            let save_rect = Rect::from_min_size(
                Pos2::new(cancel_rect.left() - 92.0, footer.top()),
                Vec2::new(84.0, 34.0),
            );
            let save = accent_button(ui, save_rect, "保存", true, tokens);
            let cancel = fixed_button(ui, cancel_rect, "取消", true, false, tokens);
            if cancel {
                state.cron_editor_open = false;
                state.cron_note.clear();
                state.cron_note_error = false;
            }
            if save {
                let kind = state.cron_kind.clone();
                match cron_schedule_error(
                    &kind,
                    &state.cron_at,
                    &state.cron_every,
                    &state.cron_expression,
                ) {
                    Some(error) => {
                        state.cron_note = format!("无法保存：{error}");
                        state.cron_note_error = true;
                    }
                    None if state.cron_name.trim().is_empty() => {
                        state.cron_note = "无法保存：请填写任务名称。".to_owned();
                        state.cron_note_error = true;
                    }
                    None => {
                        // No adapter command exists, so report the validation
                        // result and say the draft was not submitted. Claiming
                        // success here would be a lie the user could not undo.
                        state.cron_note = format!(
                            "{} 校验通过；桌面端尚未接入定时任务保存命令，本次草稿未提交。",
                            cron_schedule_summary(
                                &kind,
                                &state.cron_at,
                                &state.cron_every,
                                &state.cron_expression
                            )
                        );
                        state.cron_note_error = false;
                    }
                }
            }
            if !state.cron_note.is_empty() {
                note_line(ui, tokens, state.cron_note.as_str(), state.cron_note_error);
            }
        },
    );
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

/// One-line feedback under a control group. Errors use `danger_soft` *and* the
/// caller's wording, so the reason survives a colour-blind reader or a
/// greyscale screenshot instead of hiding behind a red border.
fn note_line(ui: &mut Ui, tokens: Tokens, message: &str, error: bool) {
    ui.add_space(6.0);
    ui.label(RichText::new(message).size(12.0).color(if error {
        tokens.danger_soft
    } else {
        tokens.text_muted
    }));
}

/// Label (and optional hint) above an editor field, matching the Slint
/// `FormField` metrics (18px label, 7px gap). `path_input` only paints the box,
/// so the two helpers are always used together.
fn field_label(ui: &mut Ui, tokens: Tokens, label: &str, hint: &str) {
    let height = if hint.is_empty() { 20.0 } else { 36.0 };
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), height), Sense::hover());
    ui.painter().text(
        rect.left_top() + Vec2::new(0.0, 2.0),
        Align2::LEFT_TOP,
        label,
        FontId::proportional(12.0),
        tokens.text_secondary,
    );
    if !hint.is_empty() {
        ui.painter().text(
            rect.left_top() + Vec2::new(0.0, 20.0),
            Align2::LEFT_TOP,
            hint,
            FontId::proportional(11.0),
            tokens.text_muted,
        );
    }
}

/// Multiline editor used by the agent prompt and the cron message. Both are a
/// fixed height in the Slint form, so the buttons below them do not jump when
/// the text grows.
fn text_area(ui: &mut Ui, tokens: Tokens, id: &str, value: &mut String, hint: &str, height: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), height), Sense::hover());
    ui.painter().rect_filled(rect, 10.0, tokens.surface);
    ui.painter().rect_stroke(
        rect,
        10.0,
        Stroke::new(1.0, tokens.field_border),
        StrokeKind::Inside,
    );
    child(ui, rect.shrink2(Vec2::new(10.0, 8.0)), id, |ui| {
        let _ = ui.add_sized(
            ui.available_size(),
            TextEdit::multiline(value)
                .char_limit(2048)
                .frame(false)
                .margin(Margin::ZERO)
                .font(FontId::proportional(13.0))
                .hint_text(hint),
        );
    });
}

/// A switch the page cannot operate. It keeps `toggle`'s footprint so the row
/// stays comparable, but the accompanying text explains what unlocks it; a
/// dimmed colour alone would not say why.
fn disabled_toggle(ui: &mut Ui, value: bool, tokens: Tokens) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(50.0, 28.0), Sense::hover());
    ui.painter().rect_filled(rect, 14.0, tokens.control_fill);
    ui.painter().rect_stroke(
        rect,
        14.0,
        Stroke::new(1.0, tokens.control_border),
        StrokeKind::Inside,
    );
    ui.painter().circle_filled(
        Pos2::new(
            if value {
                rect.right() - 14.0
            } else {
                rect.left() + 14.0
            },
            rect.center().y,
        ),
        10.0,
        tokens.border_strong,
    );
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
            select(ui, "memory-tag", &mut tag, &["全部标签"], tokens);
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
            ui.painter().rect_filled(log, 10.0, tokens.surface_sunken);
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
fn fixed_button(
    ui: &mut Ui,
    rect: Rect,
    label: &str,
    enabled: bool,
    danger: bool,
    tokens: Tokens,
) -> bool {
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
        tokens.danger_tint
    } else if enabled && response.hovered() {
        tokens.hover
    } else {
        tokens.surface_sunken
    };
    let border = if danger {
        tokens.danger_border
    } else if enabled {
        tokens.border_strong
    } else {
        tokens.border
    };
    let color = if danger {
        tokens.danger_soft
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
    // Callers that need the click (the cron toolbar) get it; the pages that
    // only project a disabled control keep calling this as a statement.
    response.clicked()
}

fn page_devices(ui: &mut Ui, state: &mut AppState, tokens: Tokens) {
    let height = page_height(ui);
    panel(ui, tokens, height, |ui| {
        panel_header(
            ui,
            tokens,
            "我的设备",
            "管理各设备的云端互联开关与云影子投影",
            |ui| {
                if action_button(ui, tokens, "刷新", Some("refresh")).clicked() {
                    // The tunnel list belongs to the desktop adapter. Saying so
                    // keeps an unchanged list from reading as "no devices".
                    state.devices_note =
                        "桌面端尚未接入设备列表命令，刷新没有取到新的隧道信息。".to_owned();
                }
                ui.label(
                    RichText::new(device_presence_summary(0, 0))
                        .size(12.0)
                        .color(tokens.text_secondary),
                );
            },
        );
        card(
            ui,
            tokens,
            "本机设备",
            "设备标识与系统版本由桌面端读取，这里只读展示",
            |ui| {
                path_row(ui, tokens, "设备名称", "由桌面端提供");
                path_row(ui, tokens, "系统与版本", "由桌面端提供");
                ui.add_space(8.0);
                ui.label(
                    RichText::new("本机名称与系统版本尚未从桌面端读取；接入后会在上面两行显示。")
                        .size(12.0)
                        .color(tokens.text_muted),
                );
            },
        );
        card(
            ui,
            tokens,
            "设备互通",
            "开关保存在本机配置；共享目录还需要舰桥授权",
            |ui| {
                detail_row(
                    ui,
                    tokens,
                    "devices",
                    "允许远程访问本地",
                    "启用设备互通；写入和线程操作仍需本地审批",
                    false,
                    |ui| {
                        // Text, not just the switch position: the same wording
                        // is what a screen reader or a greyscale screenshot
                        // can still resolve.
                        ui.label(
                            RichText::new(interlink_state_label(state.devices_allow_remote))
                                .size(12.0)
                                .color(if state.devices_allow_remote {
                                    tokens.primary
                                } else {
                                    tokens.text_muted
                                }),
                        );
                        toggle(ui, &mut state.devices_allow_remote, tokens);
                    },
                );
                detail_row(
                    ui,
                    tokens,
                    "folder",
                    "共享工作区与线程目录",
                    "需同时获得舰桥授权；关闭时只共享设备摘要",
                    false,
                    |ui| {
                        if share_switch_available(state.devices_allow_remote) {
                            ui.label(
                                RichText::new(interlink_state_label(state.devices_share_catalog))
                                    .size(12.0)
                                    .color(if state.devices_share_catalog {
                                        tokens.primary
                                    } else {
                                        tokens.text_muted
                                    }),
                            );
                            toggle(ui, &mut state.devices_share_catalog, tokens);
                        } else {
                            ui.label(
                                RichText::new("需先开启互通")
                                    .size(12.0)
                                    .color(tokens.text_muted),
                            );
                            disabled_toggle(ui, state.devices_share_catalog, tokens);
                        }
                    },
                );
                detail_row(
                    ui,
                    tokens,
                    "expand",
                    "云影子投影",
                    "把本机设备摘要投影到云端，供蛹与舵机查看",
                    true,
                    |ui| {
                        ui.label(
                            RichText::new(interlink_state_label(state.devices_cloud_shadow))
                                .size(12.0)
                                .color(if state.devices_cloud_shadow {
                                    tokens.primary
                                } else {
                                    tokens.text_muted
                                }),
                        );
                        toggle(ui, &mut state.devices_cloud_shadow, tokens);
                    },
                );
                if !state.devices_allow_remote && state.devices_share_catalog {
                    note_line(
                        ui,
                        tokens,
                        "互通已关闭，共享工作区的设置已保留但当前不生效。",
                        false,
                    );
                }
            },
        );
        card(
            ui,
            tokens,
            "互通隧道",
            "隧道与连接状态由桌面端心跳上报；在线表示最近一次心跳成功",
            |ui| {
                // Column labels come from the Slint interlink strings
                // (`il_col_*`) so the table headers match the desktop shell.
                let (head, _) =
                    ui.allocate_exact_size(Vec2::new(ui.available_width(), 24.0), Sense::hover());
                let columns = [
                    ("名称", Align2::LEFT_CENTER, head.left()),
                    (
                        "类型",
                        Align2::LEFT_CENTER,
                        head.left() + head.width() * 0.42,
                    ),
                    (
                        "状态",
                        Align2::LEFT_CENTER,
                        head.left() + head.width() * 0.62,
                    ),
                    ("连接", Align2::RIGHT_CENTER, head.right()),
                ];
                for (label, align, x) in columns {
                    ui.painter().text(
                        Pos2::new(x, head.center().y),
                        align,
                        label,
                        FontId::proportional(11.0),
                        tokens.text_muted,
                    );
                }
                dashed_rule(ui, tokens.divider);
                if state.devices_loading {
                    ui.add_space(10.0);
                    ui.label(RichText::new("加载中…").size(12.0).color(tokens.text_muted));
                    ui.add_space(10.0);
                } else if !state.devices_error.is_empty() {
                    note_line(ui, tokens, state.devices_error.as_str(), true);
                } else {
                    empty_text(
                        ui,
                        tokens,
                        "暂无互通隧道，开启互通并登录蛹或舵机后会自动出现在这里。",
                    );
                }
            },
        );
        if !state.devices_note.is_empty() {
            note_line(ui, tokens, state.devices_note.as_str(), false);
        }
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
                    .rect_filled(selector, 10.0, tokens.primary_wash);
                ui.painter().rect_stroke(
                    selector,
                    10.0,
                    Stroke::new(1.0, tokens.primary_outline),
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
                    ui.painter().rect_filled(tag, 99.0, tokens.chip);
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
                Stroke::new(1.0, tokens.primary_outline_soft),
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
                        Stroke::new(1.0, tokens.primary_outline_strong),
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
            ui.painter().rect_filled(body, 10.0, tokens.primary_wash);
            ui.painter().rect_stroke(
                body,
                10.0,
                Stroke::new(1.0, tokens.primary_outline_soft),
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
        "folder" => Icon::Folder,
        "copy" => Icon::Copy,
        "shield" => Icon::Shield,
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
    use super::{
        agent_question_can_add, agent_tool_matches, agent_tool_ranges, clamp_font_size,
        cron_schedule_error, cron_schedule_summary, device_presence_summary, draw,
        interlink_state_label, self_check_summary, share_switch_available, valid_tool_path,
        AGENT_QUESTION_LIMIT, AGENT_TOOL_GROUPS, CATEGORIES, CRON_KIND_CRON, CRON_KIND_INTERVAL,
        CRON_KIND_ONCE, FONT_SIZE_MAX, FONT_SIZE_MIN, SUPPLEMENT_CATALOGUE,
    };
    use crate::model::AppState;
    use crate::theme;
    use egui::{Context, Id, Pos2, RawInput, Rect, Vec2};

    #[test]
    fn tool_path_validation_rejects_empty_and_duplicate_entries() {
        let existing = vec!["D:/tools".to_owned(), "  E:/bin  ".to_owned()];
        assert!(!valid_tool_path("", &existing));
        assert!(!valid_tool_path("   ", &existing));
        assert!(!valid_tool_path("D:/tools", &existing));
        // Windows paths are case-insensitive, so casing must not bypass the
        // duplicate check; the stored entry is also trimmed before compare.
        assert!(!valid_tool_path(" d:/TOOLS ", &existing));
        assert!(!valid_tool_path("E:/bin", &existing));
        assert!(valid_tool_path(" D:/tools/bin ", &existing));
        assert!(valid_tool_path("D:/tools", &[]));
    }

    #[test]
    fn font_size_clamps_to_the_settings_range() {
        assert_eq!(clamp_font_size(8.0), FONT_SIZE_MIN);
        assert_eq!(clamp_font_size(30.0), FONT_SIZE_MAX);
        assert_eq!(clamp_font_size(FONT_SIZE_MIN), FONT_SIZE_MIN);
        assert_eq!(clamp_font_size(FONT_SIZE_MAX), FONT_SIZE_MAX);
        assert_eq!(clamp_font_size(16.0), 16.0);
        // A non-finite scale must not leak NaN into the preview or the stored
        // multiplier; it degrades to the documented minimum instead.
        assert_eq!(clamp_font_size(f32::NAN), FONT_SIZE_MIN);
        assert_eq!(clamp_font_size(f32::INFINITY), FONT_SIZE_MAX);
    }

    #[test]
    fn cron_schedule_validation_covers_every_kind() {
        // 一次性 needs a real moment, not just any non-empty string.
        assert!(cron_schedule_error(CRON_KIND_ONCE, "", "", "").is_some());
        assert!(cron_schedule_error(CRON_KIND_ONCE, "2026-10-20 09:00", "", "").is_none());
        assert!(cron_schedule_error(CRON_KIND_ONCE, "2026-13-20 09:00", "", "").is_some());
        assert!(cron_schedule_error(CRON_KIND_ONCE, "2026-10-20 24:00", "", "").is_some());
        assert!(cron_schedule_error(CRON_KIND_ONCE, "明天九点", "", "").is_some());
        // 固定间隔 parses positive seconds; the first run stays optional.
        assert!(cron_schedule_error(CRON_KIND_INTERVAL, "", "0", "").is_some());
        assert!(cron_schedule_error(CRON_KIND_INTERVAL, "", "abc", "").is_some());
        assert!(cron_schedule_error(CRON_KIND_INTERVAL, "", "300", "").is_none());
        assert!(cron_schedule_error(CRON_KIND_INTERVAL, "2026-10-20 09:00", "300", "").is_none());
        assert!(cron_schedule_error(CRON_KIND_INTERVAL, "明天", "300", "").is_some());
        // Cron needs 5 or 6 fields drawn from the numeric operator set.
        assert!(cron_schedule_error(CRON_KIND_CRON, "", "", "0 9 * * *").is_none());
        assert!(cron_schedule_error(CRON_KIND_CRON, "", "", "0 9 * * * *").is_none());
        assert!(cron_schedule_error(CRON_KIND_CRON, "", "", "0 9 * *").is_some());
        assert!(cron_schedule_error(CRON_KIND_CRON, "", "", "0 9 * * MON").is_some());
        assert!(cron_schedule_error("未知调度", "", "", "").is_some());
    }

    #[test]
    fn cron_schedule_summary_spells_out_missing_fields() {
        assert_eq!(
            cron_schedule_summary(CRON_KIND_ONCE, "", "", ""),
            "一次性 · 未设置执行时间"
        );
        assert_eq!(
            cron_schedule_summary(CRON_KIND_ONCE, " 2026-10-20 09:00 ", "", ""),
            "一次性 · 2026-10-20 09:00"
        );
        assert_eq!(
            cron_schedule_summary(CRON_KIND_INTERVAL, "", "300", ""),
            "固定间隔 · 每 300 秒"
        );
        assert_eq!(
            cron_schedule_summary(CRON_KIND_INTERVAL, "", "0", ""),
            "固定间隔 · 未设置间隔"
        );
        assert_eq!(
            cron_schedule_summary(CRON_KIND_CRON, "", "", " 0 9 * * * "),
            "Cron 表达式 · 0 9 * * *"
        );
        assert_eq!(
            cron_schedule_summary(CRON_KIND_CRON, "", "", ""),
            "Cron 表达式 · 未填写"
        );
    }

    #[test]
    fn agent_tool_search_matches_name_and_description() {
        assert!(agent_tool_matches(
            "文件读写",
            "在工作区内读取与修改文件",
            ""
        ));
        assert!(agent_tool_matches(
            "文件读写",
            "在工作区内读取与修改文件",
            "读写"
        ));
        assert!(agent_tool_matches("MCP 服务器", "通过 MCP 协议连接", "mcp"));
        assert!(agent_tool_matches(
            "文件读写",
            "在工作区内读取与修改文件",
            "工作区"
        ));
        assert!(!agent_tool_matches(
            "文件读写",
            "在工作区内读取与修改文件",
            "知识库"
        ));
        // A whitespace-only query must behave like an empty one, otherwise the
        // list would vanish while the search box looks untouched.
        assert!(agent_tool_matches("文件读写", "", "   "));
    }

    #[test]
    fn agent_tool_ranges_cover_the_whole_catalogue() {
        let ranges = agent_tool_ranges();
        assert_eq!(ranges.len(), AGENT_TOOL_GROUPS.len());
        let mut expected = 0;
        for (group, (start, end)) in ranges.iter().enumerate() {
            assert_eq!(*start, expected);
            assert!(end > start, "empty group at {group}");
            assert_eq!(*end - *start, AGENT_TOOL_GROUPS[group].1.len());
            expected = *end;
        }
        let total = AGENT_TOOL_GROUPS
            .iter()
            .map(|(_, tools)| tools.len())
            .sum::<usize>();
        assert_eq!(expected, total);
        // The model cannot import the catalogue, so the default flags must
        // still line up with it or the rows would show the wrong state.
        assert_eq!(AppState::default().agent_tool_enabled.len(), total);
    }

    #[test]
    fn agent_preset_questions_respect_the_cap() {
        assert!(!agent_question_can_add("", 0));
        assert!(!agent_question_can_add("   ", 0));
        assert!(agent_question_can_add("帮我总结今天的改动", 0));
        assert!(agent_question_can_add("第 12 条", AGENT_QUESTION_LIMIT - 1));
        assert!(!agent_question_can_add("第 13 条", AGENT_QUESTION_LIMIT));
        assert_eq!(AGENT_QUESTION_LIMIT, 12);
    }

    #[test]
    fn device_page_gates_catalogue_sharing_on_interlink() {
        assert_eq!(device_presence_summary(0, 0), "0 台在线 · 共 0 台");
        assert_eq!(device_presence_summary(2, 3), "2 台在线 · 共 3 台");
        assert!(!share_switch_available(false));
        assert!(share_switch_available(true));
        assert_eq!(interlink_state_label(true), "已开启");
        assert_eq!(interlink_state_label(false), "已关闭");
        let state = AppState::default();
        assert!(!state.devices_allow_remote && !state.devices_share_catalog);
        assert!(!state.devices_loading && state.devices_error.is_empty());
        assert!(!state.cron_editor_open && state.cron_note.is_empty());
    }

    #[test]
    fn general_page_defaults_are_renderable() {
        let state = AppState::default();
        assert!(state.runtime_path.is_empty() && state.workspace_root.is_empty());
        assert!(state.workspace_action_note.is_empty());
        assert!(state.tool_paths.is_empty());
        assert!(state.tool_path_input.is_empty() && state.tool_path_error.is_empty());
        assert!(state.self_check_state.is_empty());
        assert!(state.self_check_at.is_none());
        // The model cannot import the catalogue, so both lists must stay in
        // sync by name and order; drift would leave rows without a hint.
        assert_eq!(state.supplement_bundles.len(), SUPPLEMENT_CATALOGUE.len());
        for (bundle, (name, _)) in state.supplement_bundles.iter().zip(SUPPLEMENT_CATALOGUE) {
            assert_eq!(bundle.0, *name);
        }
        assert!(
            state
                .supplement_bundles
                .iter()
                .any(|(_, enabled)| !*enabled),
            "at least one sample bundle should show the disabled state"
        );
        let summary = self_check_summary(&state);
        assert!(summary.contains("运行时路径未设置"));
        assert!(summary.contains("工具路径 0 项"));
    }

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
