//! 内嵌使用手册：把 `assets/help/manual.{zh,en}.md` 切成主题，正文投影成
//! 聊天正文同一个 `[MdBlock]` 模型。
//!
//! 手册与桌面版帮助窗口共用同一份源文件（由 `scripts/build_help_manual.py`
//! 从 `docs/使用说明书` 生成），每个主题由一行 `<!-- topic ... -->` 元数据分隔，
//! 其余是标准 Markdown。手机上没有侧栏，目录由设置页按 形态 / 工具 / 能力
//! 三个分区折叠成一列；正文复用 `BlockView`，宽表横向平移。
//!
//! 主题按需投影：打开一篇才解析一次。英文手册同样按需加载，切语言不重启。

use crate::md::MarkdownRenderer;
use crate::{MobileHelpRow, MobileWindow};
use slint::{ComponentHandle, ModelRc, VecModel};
use std::cell::RefCell;
use std::rc::Rc;

/// 目录的三个分区，与说明书、桌面版帮助窗口共用同一套分区键。
const BLOCKS: [(&str, &str, &str); 3] = [
    ("forms", "形态", "Forms"),
    ("tools", "工具", "Tools"),
    ("capabilities", "能力", "Capabilities"),
];

const ROW_BLOCK: i32 = 0;
const ROW_GROUP: i32 = 1;
const ROW_TOPIC: i32 = 2;

#[derive(Clone)]
struct Topic {
    id: String,
    title: String,
    /// 一级分区键：forms / tools / capabilities。
    block: String,
    /// 分区内的二级分组名：概览 / 桌面版 / 核心能力 …
    group: String,
    body: String,
}

struct Manual {
    topics: Vec<Topic>,
}

/// Read one `key="value"` attribute out of a `<!-- topic ... -->` line.
fn attribute(line: &str, key: &str) -> String {
    let needle = format!("{key}=\"");
    let Some(start) = line.find(&needle).map(|at| at + needle.len()) else {
        return String::new();
    };
    let rest = &line[start..];
    let end = rest.find('"').unwrap_or(rest.len());
    decode_entities(&rest[..end])
}

/// The generator escapes only the five XML entities, so decoding stays tiny and
/// pulls in no dependency.
fn decode_entities(value: &str) -> String {
    value
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

impl Manual {
    fn load(zh: bool) -> Self {
        let raw = if zh {
            include_str!("../assets/help/manual.zh.md")
        } else {
            include_str!("../assets/help/manual.en.md")
        };
        let mut topics: Vec<Topic> = Vec::new();
        let mut current: Option<Topic> = None;
        let mut body = String::new();
        for line in raw.lines() {
            if line.starts_with("<!-- topic ") {
                if let Some(mut topic) = current.take() {
                    topic.body = body.trim().to_owned();
                    topics.push(topic);
                    body.clear();
                }
                current = Some(Topic {
                    id: attribute(line, "id"),
                    title: attribute(line, "title"),
                    block: attribute(line, "block"),
                    group: attribute(line, "group"),
                    body: String::new(),
                });
            } else if current.is_some() {
                // 主题之前的两行注释被 `current.is_some()` 挡掉；主题内的注释
                // 对 Markdown 解析无害。
                body.push_str(line);
                body.push('\n');
            }
        }
        if let Some(mut topic) = current.take() {
            topic.body = body.trim().to_owned();
            topics.push(topic);
        }
        debug_assert!(!topics.is_empty(), "内嵌 Markdown 手册没有主题");
        Self { topics }
    }

    fn topic_index(&self, id: &str) -> Option<usize> {
        self.topics.iter().position(|topic| topic.id == id)
    }
}

struct Help {
    /// 下标 0 中文、1 英文；当前语言首次使用时才解析，切语言不重启。
    manuals: [Option<Rc<Manual>>; 2],
    zh: bool,
    /// 各分区的展开状态；默认全部收起，手机上先看到三段目录。
    expanded: [bool; BLOCKS.len()],
    /// 正在阅读的主题下标。
    open: Option<usize>,
    renderer: MarkdownRenderer,
}

fn model<T: Clone + 'static>(rows: Vec<T>) -> ModelRc<T> {
    Rc::new(VecModel::from(rows)).into()
}

impl Help {
    fn new(zh: bool) -> Self {
        Self {
            manuals: [None, None],
            zh,
            expanded: [false; BLOCKS.len()],
            open: None,
            renderer: MarkdownRenderer::new(),
        }
    }

    fn lang(&self) -> usize {
        if self.zh {
            0
        } else {
            1
        }
    }

    /// 当前语言的手册；第一次用到时解析整份文件。
    fn manual(&mut self) -> Rc<Manual> {
        let lang = self.lang();
        if self.manuals[lang].is_none() {
            self.manuals[lang] = Some(Rc::new(Manual::load(lang == 0)));
        }
        self.manuals[lang].clone().expect("手册刚刚填充")
    }

    /// 分区、分区内的分组和主题压平成一列目录行。
    fn rows(&mut self) -> Vec<MobileHelpRow> {
        let manual = self.manual();
        let mut rows: Vec<MobileHelpRow> = Vec::new();
        for (block, (key, title_zh, title_en)) in BLOCKS.iter().enumerate() {
            let members: Vec<usize> = manual
                .topics
                .iter()
                .enumerate()
                .filter(|(_, topic)| topic.block == *key)
                .map(|(index, _)| index)
                .collect();
            if members.is_empty() {
                continue;
            }
            rows.push(MobileHelpRow {
                kind: ROW_BLOCK,
                topic: block as i32,
                title: (if self.zh { *title_zh } else { *title_en }).into(),
                expanded: self.expanded[block],
                count: members.len() as i32,
            });
            if !self.expanded[block] {
                continue;
            }
            let mut group = String::new();
            for index in members {
                let topic = &manual.topics[index];
                if topic.group != group {
                    group = topic.group.clone();
                    rows.push(MobileHelpRow {
                        kind: ROW_GROUP,
                        topic: -1,
                        title: group.as_str().into(),
                        expanded: true,
                        count: 0,
                    });
                }
                rows.push(MobileHelpRow {
                    kind: ROW_TOPIC,
                    topic: index as i32,
                    title: topic.title.as_str().into(),
                    expanded: true,
                    count: 0,
                });
            }
        }
        rows
    }

    fn toggle(&mut self, ui: &MobileWindow, block: i32) {
        let Some(slot) = self.expanded.get_mut(block.max(0) as usize) else {
            return;
        };
        *slot = !*slot;
        let rows = self.rows();
        ui.set_settings_help_rows(model(rows));
    }

    /// 打开一篇主题：投影一次 Markdown，正文交给 `BlockView` 排版。
    fn open(&mut self, ui: &MobileWindow, index: usize) {
        let manual = self.manual();
        let Some(topic) = manual.topics.get(index).cloned() else {
            return;
        };
        // 当前主题所在分区保持展开，回到目录时能看见自己读到哪。
        if let Some(slot) = BLOCKS
            .iter()
            .position(|entry| entry.0 == topic.block)
            .and_then(|block| self.expanded.get_mut(block))
        {
            *slot = true;
        }
        let blocks = self.renderer.render(&topic.body);
        self.open = Some(index);
        ui.set_settings_help_topic_title(topic.title.as_str().into());
        ui.set_settings_help_blocks(model(blocks));
        ui.set_settings_help_topic_open(true);
        ui.set_settings_help_open_request(ui.get_settings_help_open_request() + 1);
        let rows = self.rows();
        ui.set_settings_help_rows(model(rows));
    }

    /// 回到目录；正文随即释放，手机内存只留一篇。
    fn back(&mut self, ui: &MobileWindow) {
        self.open = None;
        ui.set_settings_help_topic_open(false);
        ui.set_settings_help_blocks(model(Vec::<crate::MdBlock>::new()));
    }

    fn link(&mut self, ui: &MobileWindow, url: &str) {
        let Some(id) = url.strip_prefix("topic:") else {
            return;
        };
        let manual = self.manual();
        if let Some(index) = manual.topic_index(id) {
            self.open(ui, index);
        }
    }

    fn apply_language(&mut self, ui: &MobileWindow, zh: bool) {
        if self.zh == zh {
            return;
        }
        self.zh = zh;
        self.expanded = [false; BLOCKS.len()];
        self.back(ui);
        let rows = self.rows();
        ui.set_settings_help_rows(model(rows));
    }
}

thread_local! {
    /// 当前窗口的手册状态；`build()` 每次创建窗口时替换。语言回调通过它
    /// 切换手册语言，不必把状态复制进每个闭包。
    static STATE: RefCell<Option<Rc<RefCell<Help>>>> = const { RefCell::new(None) };
}

/// 绑定手册的回调并铺好目录。窗口重建时重新调用一次。
pub fn attach(ui: &MobileWindow) {
    let state = Rc::new(RefCell::new(Help::new(ui.get_ui_zh())));
    {
        let mut help = state.borrow_mut();
        let rows = help.rows();
        ui.set_settings_help_rows(model(rows));
    }
    STATE.with(|slot| *slot.borrow_mut() = Some(Rc::clone(&state)));

    let weak = ui.as_weak();
    let shared = Rc::clone(&state);
    ui.on_settings_help_toggle(move |block| {
        if let Some(ui) = weak.upgrade() {
            shared.borrow_mut().toggle(&ui, block);
        }
    });

    let weak = ui.as_weak();
    let shared = Rc::clone(&state);
    ui.on_settings_help_open(move |topic| {
        if let Some(ui) = weak.upgrade() {
            shared.borrow_mut().open(&ui, topic.max(0) as usize);
        }
    });

    let weak = ui.as_weak();
    let shared = Rc::clone(&state);
    ui.on_settings_help_back(move || {
        if let Some(ui) = weak.upgrade() {
            shared.borrow_mut().back(&ui);
        }
    });

    let weak = ui.as_weak();
    let shared = Rc::clone(&state);
    ui.on_settings_help_link(move |url| {
        if let Some(ui) = weak.upgrade() {
            shared.borrow_mut().link(&ui, url.as_str());
        }
    });
}

/// 界面语言变了就换一份手册；主题下标按当前语言重建。
pub fn set_language(ui: &MobileWindow, zh: bool) {
    STATE.with(|slot| {
        let Some(state) = slot.borrow().clone() else {
            return;
        };
        state.borrow_mut().apply_language(ui, zh);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manual_splits_into_topics() {
        let manual = Manual::load(true);
        assert!(manual.topics.len() > 40, "主题数 {}", manual.topics.len());
        let first = &manual.topics[0];
        assert_eq!(first.block, "forms");
        assert!(!first.title.is_empty() && !first.body.is_empty());
        // 元数据行不能混进正文。
        assert!(!first.body.contains("<!-- topic "));
        assert!(manual.topic_index(&first.id).is_some());
        assert!(manual.topics.iter().all(|topic| !topic.body.is_empty()));
    }

    #[test]
    fn english_manual_has_the_same_shape() {
        let zh = Manual::load(true);
        let en = Manual::load(false);
        assert_eq!(zh.topics.len(), en.topics.len());
        assert!(en.topics.iter().any(|topic| topic.block == "capabilities"));
    }

    #[test]
    fn nav_rows_fold_into_three_sections() {
        let mut help = Help::new(true);
        let total = help.manual().topics.len();
        // 默认三个分区都收起：整列只有分区标题，点开才铺主题。
        let rows = help.rows();
        assert_eq!(rows.len(), BLOCKS.len());
        assert!(rows.iter().all(|row| row.kind == ROW_BLOCK && !row.expanded));
        assert_eq!(rows.iter().map(|row| row.count as usize).sum::<usize>(), total);
        // 展开形态：分组行先于该组的主题行。
        help.expanded[0] = true;
        let rows = help.rows();
        let group = rows.iter().position(|row| row.kind == ROW_GROUP).unwrap();
        let topic = rows.iter().position(|row| row.kind == ROW_TOPIC).unwrap();
        assert!(group < topic);
        assert_eq!(
            rows.iter().filter(|row| row.kind == ROW_TOPIC).count(),
            rows[0].count as usize
        );
    }
}
