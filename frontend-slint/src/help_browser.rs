//! 帮助文档控制器：内嵌 Markdown 手册，投影成聊天正文同一个 `[MdBlock]` 模型。
//!
//! 手册的正文、目录、索引与搜索都由 `assets/help/manual.{zh,en}.md` 提供：
//! 每个主题由一行 `<!-- topic ... -->` 元数据分隔，其余内容是标准 Markdown。
//! 控制器读一次手册，切出主题；打开主题时用 `src/md/` 投影一次，交给
//! `ui/help_document.slint` 的 `BlockView` 排版。选区、复制与右键菜单复用
//! StyledText 的窗口级实现，不再自己做像素级排版。
use crate::{HelpDocs, HelpEntry, HelpWindow, MdBlock, TextFieldActivation};
use slint::{ComponentHandle, Global, ModelRc, VecModel};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

fn model<T: Clone + 'static>(rows: Vec<T>) -> ModelRc<T> {
    Rc::new(VecModel::from(rows)).into()
}

// ---------------------------------------------------------------------------
// 手册内容模型
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct Topic {
    pub id: String,
    pub title: String,
    /// 侧栏分组名（开始 / 核心 / 工具 / 界面 / 参考）。
    pub group: String,
    pub keywords: Vec<String>,
    /// 主题的 Markdown 正文，不含元数据行。
    pub body: String,
    pub searchable: String,
}

#[derive(Clone)]
pub struct IndexEntry {
    pub keyword: String,
    pub topic: usize,
}

pub struct Manual {
    pub topics: Vec<Topic>,
    pub index: Vec<IndexEntry>,
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

/// The converter escapes only the five XML entities, so decoding is tiny and
/// needs no extra dependency.
fn decode_entities(value: &str) -> String {
    value
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

impl Manual {
    /// 按界面语言加载内嵌 Markdown 手册。
    pub fn load(zh: bool) -> Self {
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
                    topic.searchable = searchable_text(&topic);
                    topics.push(topic);
                    body.clear();
                }
                let keywords = attribute(line, "keywords");
                current = Some(Topic {
                    id: attribute(line, "id"),
                    title: attribute(line, "title"),
                    group: attribute(line, "group"),
                    keywords: keywords
                        .split(',')
                        .map(str::trim)
                        .filter(|word| !word.is_empty())
                        .map(ToOwned::to_owned)
                        .collect(),
                    body: String::new(),
                    searchable: String::new(),
                });
            } else if current.is_some() {
                // Everything else is Markdown. The two header comments precede
                // the first topic and are ignored by the `current.is_some()`
                // guard; a comment inside a topic is harmless to the parser.
                body.push_str(line);
                body.push('\n');
            }
        }
        if let Some(mut topic) = current.take() {
            topic.body = body.trim().to_owned();
            topic.searchable = searchable_text(&topic);
            topics.push(topic);
        }
        assert!(!topics.is_empty(), "内嵌 Markdown 手册没有主题");
        let index = topics
            .iter()
            .enumerate()
            .flat_map(|(index, topic)| {
                topic
                    .keywords
                    .iter()
                    .map(move |keyword| IndexEntry {
                        keyword: keyword.clone(),
                        topic: index,
                    })
                    .collect::<Vec<_>>()
            })
            .collect();
        Self { topics, index }
    }

    pub fn search(&self, query: &str) -> Vec<usize> {
        let query = query.trim().to_lowercase();
        self.topics
            .iter()
            .enumerate()
            .filter(|(_, topic)| topic.searchable.contains(&query))
            .map(|(i, _)| i)
            .collect()
    }

    pub fn topic_index(&self, id: &str) -> Option<usize> {
        self.topics.iter().position(|topic| topic.id == id)
    }
}

fn searchable_text(topic: &Topic) -> String {
    format!(
        "{} {} {}",
        topic.title,
        topic.keywords.join(" "),
        topic.body
    )
    .to_lowercase()
}

// ---------------------------------------------------------------------------
// 美术图册：预览图按需解码
// ---------------------------------------------------------------------------

/// 预览图随应用内嵌；Markdown 里写成 `![alt](art/<slug>.png)`。
fn art_preview(slug: &str) -> Option<&'static [u8]> {
    Some(match slug {
        "00-overview" => include_bytes!("../assets/help/art/00-overview.png"),
        "01-foundations" => include_bytes!("../assets/help/art/01-foundations.png"),
        "02-icons" => include_bytes!("../assets/help/art/02-icons.png"),
        "03-shell-sidebar" => include_bytes!("../assets/help/art/03-shell-sidebar.png"),
        "04-timeline" => include_bytes!("../assets/help/art/04-timeline.png"),
        "05-process-tools" => include_bytes!("../assets/help/art/05-process-tools.png"),
        "06-plan-inquiry" => include_bytes!("../assets/help/art/06-plan-inquiry.png"),
        "07-composer" => include_bytes!("../assets/help/art/07-composer.png"),
        "08-settings-dialogs" => include_bytes!("../assets/help/art/08-settings-dialogs.png"),
        "09-states-motion" => include_bytes!("../assets/help/art/09-states-motion.png"),
        _ => return None,
    })
}

/// 手册一次只显示一页，缓存最近一张解码图就够，避免整套图册常驻内存。
#[derive(Default)]
struct ArtCache {
    slug: String,
    image: Option<slint::Image>,
    width: f32,
    height: f32,
}

impl ArtCache {
    fn get(&mut self, slug: &str) -> Option<(slint::Image, f32, f32)> {
        if let Some(image) = self.image.as_ref().filter(|_| self.slug == slug) {
            return Some((image.clone(), self.width, self.height));
        }
        let decoded = image::load_from_memory(art_preview(slug)?).ok()?.to_rgba8();
        let (width, height) = decoded.dimensions();
        let buffer = slint::SharedPixelBuffer::clone_from_slice(decoded.as_raw(), width, height);
        let image = slint::Image::from_rgba8(buffer);
        self.slug = slug.to_owned();
        self.image = Some(image.clone());
        self.width = width as f32;
        self.height = height as f32;
        Some((image, self.width, self.height))
    }
}

/// 把一个主题的 Markdown 投影成块，并把内嵌图册的像素回填到图片行。
fn project_topic(topic: &Topic, art: &mut ArtCache) -> Vec<MdBlock> {
    let mut rows = crate::message_blocks::render_full(&topic.body);
    for row in &mut rows {
        if row.kind != crate::md::block::IMAGE {
            continue;
        }
        let source = row.image_source.to_string();
        let slug = source
            .strip_prefix("art/")
            .and_then(|rest| rest.strip_suffix(".png"));
        if let Some((image, width, height)) = slug.and_then(|slug| art.get(slug)) {
            row.image = image;
            row.image_width = width;
            row.image_height = height;
        } else {
            row.image_error = true;
        }
    }
    rows
}

// ---------------------------------------------------------------------------
// 阅读历史与控制器
// ---------------------------------------------------------------------------

#[derive(Default)]
struct History {
    entries: Vec<usize>,
    cursor: usize,
}

impl History {
    fn new() -> Self {
        Self {
            entries: vec![0],
            cursor: 0,
        }
    }
    fn current(&self) -> usize {
        self.entries[self.cursor]
    }
    fn can_back(&self) -> bool {
        self.cursor > 0
    }
    fn can_forward(&self) -> bool {
        self.cursor + 1 < self.entries.len()
    }
    fn open(&mut self, topic: usize) {
        self.entries.truncate(self.cursor + 1);
        self.entries.push(topic);
        self.cursor += 1;
    }
    fn go(&mut self, delta: i32) -> bool {
        let next = self.cursor as i64 + i64::from(delta);
        if next < 0 || next >= self.entries.len() as i64 {
            return false;
        }
        self.cursor = next as usize;
        true
    }
}

struct Controller {
    /// 下标 0 为中文手册、1 为英文手册；切语言不重新解析。
    manuals: [Manual; 2],
    zh: bool,
    history: History,
    art: ArtCache,
    blocks: Vec<MdBlock>,
    layout_key: Option<(bool, usize)>,
}

impl Controller {
    fn new(zh: bool) -> Self {
        Self {
            manuals: [Manual::load(true), Manual::load(false)],
            zh,
            history: History::new(),
            art: ArtCache::default(),
            blocks: Vec::new(),
            layout_key: None,
        }
    }

    fn lang(&self) -> usize {
        if self.zh { 0 } else { 1 }
    }

    fn current(&self) -> &Topic {
        let lang = self.lang();
        &self.manuals[lang].topics[self.history.current()]
    }

    /// 按当前语言重建目录与索引，并重新投影当前主题。
    fn apply_language(&mut self, docs: &HelpDocs, zh: bool) {
        self.zh = zh;
        self.history = History::new();
        self.layout_key = None;
        let lang = self.lang();
        docs.set_contents(model(
            self.manuals[lang]
                .topics
                .iter()
                .enumerate()
                .map(|(i, topic)| HelpEntry {
                    topic: i as i32,
                    title: topic.title.clone().into(),
                    detail: topic.keywords.join(" / ").into(),
                    group: topic.group.clone().into(),
                })
                .collect(),
        ));
        docs.set_keyword_index(model(
            self.manuals[lang]
                .index
                .iter()
                .map(|entry| HelpEntry {
                    topic: entry.topic as i32,
                    title: entry.keyword.clone().into(),
                    detail: format!(
                        "{} / {}",
                        self.manuals[lang].topics[entry.topic].group,
                        self.manuals[lang].topics[entry.topic].title
                    )
                    .into(),
                    group: String::new().into(),
                })
                .collect(),
        ));
        self.search(docs, docs.get_search_text().as_str());
        self.navigate(docs);
    }

    fn search(&self, docs: &HelpDocs, query: &str) {
        let lang = self.lang();
        let rows = self.manuals[lang]
            .search(query)
            .into_iter()
            .map(|i| {
                let topic = &self.manuals[lang].topics[i];
                HelpEntry {
                    topic: i as i32,
                    title: topic.title.clone().into(),
                    detail: format!("{} / {}", topic.group, topic.keywords.join(" / ")).into(),
                    group: String::new().into(),
                }
            })
            .collect();
        docs.set_search_results(model(rows));
    }

    /// 一个主题只投影一次；缩放只改字号，不需要重新解析 Markdown。
    fn render(&mut self, docs: &HelpDocs) {
        let key = (self.zh, self.history.current());
        if self.layout_key == Some(key) {
            return;
        }
        let topic = self.current().clone();
        self.blocks = project_topic(&topic, &mut self.art);
        self.layout_key = Some(key);
        docs.set_selected_topic(self.history.current() as i32);
        docs.set_topic_title(topic.title.clone().into());
        docs.set_blocks(model(self.blocks.clone()));
        self.find(docs, false);
    }

    fn navigate(&mut self, docs: &HelpDocs) {
        self.layout_key = None;
        self.render(docs);
        docs.set_can_back(self.history.can_back());
        docs.set_can_forward(self.history.can_forward());
        docs.set_scroll_request(docs.get_scroll_request() + 1);
    }

    /// 页内查找：Markdown 原文上的大小写不敏感计数。首个命中按字符位置估算
    /// 滚动目标，让工具栏的「查找」仍能跳到大致位置。
    fn find(&self, docs: &HelpDocs, reveal: bool) {
        let query = docs.get_find_text().trim().to_lowercase();
        if query.is_empty() {
            docs.set_match_count(0);
            return;
        }
        let haystack = self.current().body.to_lowercase();
        let mut hits = haystack.match_indices(&query);
        let first = hits.next();
        let count = first.is_some() as usize + hits.count();
        docs.set_match_count(count as i32);
        if reveal {
            if let Some((offset, _)) = first {
                // ~80 characters per wrapped line at 100% zoom, ~22px per line.
                let estimated = (offset / 80) as f32 * 22.0;
                docs.set_reveal_y(estimated);
                docs.set_reveal_request(docs.get_reveal_request() + 1);
            }
        }
    }

    fn open_link(&mut self, docs: &HelpDocs, url: &str) {
        let Some(id) = url.strip_prefix("topic:") else {
            return;
        };
        if let Some(index) = self.manuals[self.lang()].topic_index(id) {
            self.history.open(index);
            self.navigate(docs);
        }
    }

    fn current_body(&self) -> String {
        self.current().body.clone()
    }
}

// ---------------------------------------------------------------------------
// 独立帮助窗口
// ---------------------------------------------------------------------------

thread_local! {
    /// 已创建的帮助窗口；帮助只开一份，重复点击仅聚焦。
    static WINDOW: RefCell<Option<HelpWindow>> = const { RefCell::new(None) };
    /// 帮助窗口当前的手册语言；语言切换时用于比对，避免重复重建目录。
    static ZH: Cell<bool> = const { Cell::new(true) };
}

/// 把 HelpDocs 回调绑到任意承载 HelpBrowser 的窗口。顶层窗口不共享
/// globals，每个窗口实例都要绑定自己的 HelpDocs 副本，因此参数化窗口类型。
fn wire<T>(window: &T, zh: bool)
where
    T: ComponentHandle + 'static,
    for<'a> HelpDocs<'a>: Global<'a, T>,
    for<'a> TextFieldActivation<'a>: Global<'a, T>,
{
    use slint::winit_030::WinitWindowAccessor;
    window.window().on_winit_window_event(|_, event| {
        crate::touch_keyboard::note_winit_event(event);
        slint::winit_030::EventResult::Propagate
    });
    let weak = window.as_weak();
    window
        .global::<TextFieldActivation>()
        .on_activated(move || {
            if let Some(app) = weak.upgrade() {
                crate::touch_keyboard::on_text_field_activated(app.window());
            }
        });
    let docs = window.global::<HelpDocs>();
    let controller = Rc::new(RefCell::new(Controller::new(zh)));
    {
        let mut controller = controller.borrow_mut();
        controller.apply_language(&docs, zh);
    }
    docs.set_font_scale(docs.get_zoom_percent().clamp(80, 160) as f32 / 100.0);

    let weak = window.as_weak();
    let state = Rc::clone(&controller);
    docs.on_language_changed(move |zh| {
        let Some(app) = weak.upgrade() else { return };
        let docs = app.global::<HelpDocs>();
        ZH.set(zh);
        state.borrow_mut().apply_language(&docs, zh);
    });
    let weak = window.as_weak();
    let state = Rc::clone(&controller);
    docs.on_topic_opened(move |index| {
        let Some(app) = weak.upgrade() else { return };
        let docs = app.global::<HelpDocs>();
        let mut state = state.borrow_mut();
        if index >= 0 && (index as usize) < state.manuals[state.lang()].topics.len() {
            state.history.open(index as usize);
            state.navigate(&docs);
        }
    });
    let weak = window.as_weak();
    let state = Rc::clone(&controller);
    docs.on_history_requested(move |delta| {
        let Some(app) = weak.upgrade() else { return };
        let docs = app.global::<HelpDocs>();
        let mut state = state.borrow_mut();
        if state.history.go(delta) {
            state.navigate(&docs);
        }
    });
    let weak = window.as_weak();
    let state = Rc::clone(&controller);
    docs.on_search_changed(move |query: slint::SharedString| {
        let Some(app) = weak.upgrade() else { return };
        let docs = app.global::<HelpDocs>();
        state.borrow().search(&docs, &query);
    });
    let weak = window.as_weak();
    let state = Rc::clone(&controller);
    docs.on_find_changed(move |_: slint::SharedString| {
        let Some(app) = weak.upgrade() else { return };
        let docs = app.global::<HelpDocs>();
        state.borrow().find(&docs, true);
    });
    let weak = window.as_weak();
    docs.on_layout_changed(move || {
        // Zoom is a pure type scale now: no re-projection is needed.
        let Some(app) = weak.upgrade() else { return };
        let docs = app.global::<HelpDocs>();
        docs.set_font_scale(docs.get_zoom_percent().clamp(80, 160) as f32 / 100.0);
    });
    let weak = window.as_weak();
    let state = Rc::clone(&controller);
    docs.on_link_opened(move |url: slint::SharedString| {
        let Some(app) = weak.upgrade() else { return };
        let docs = app.global::<HelpDocs>();
        state.borrow_mut().open_link(&docs, &url);
    });
    let weak = window.as_weak();
    docs.on_select_all(move || {
        if weak.upgrade().is_none() {
            return;
        }
        i_slint_core::text_selection::select_all();
    });
    let weak = window.as_weak();
    let state = Rc::clone(&controller);
    docs.on_copy_requested(move || {
        let Some(app) = weak.upgrade() else { return };
        // A selection wins; otherwise the toolbar copies the whole topic,
        // which is what the classic help "Copy" button did.
        if i_slint_core::text_selection::copy_context(&app.window()) {
            return;
        }
        let docs = app.global::<HelpDocs>();
        docs.set_copy_text(state.borrow().current_body().into());
        docs.set_copy_request(docs.get_copy_request() + 1);
    });
}

/// 打开（或聚焦）独立帮助窗口。首次调用创建窗口并绑定控制器，已在显示
/// 时仅置前；手册语言在窗口存活期间由 sync_language 跟随主窗口。
pub fn open(app: &crate::MainWindow) {
    let zh = !app.get_runtime_language().starts_with("en");
    let window = WINDOW.with(|slot| {
        {
            let mut slot = slot.borrow_mut();
            if slot.is_none() {
                match HelpWindow::new() {
                    Ok(window) => {
                        ZH.set(zh);
                        wire(&window, zh);
                        *slot = Some(window);
                    }
                    Err(error) => {
                        app.set_status(format!("无法创建帮助窗口：{error}").into());
                        return None;
                    }
                }
            }
        }
        slot.borrow().as_ref().map(|window| window.clone_strong())
    });
    let Some(window) = window else { return };
    sync_language(app);
    if window.show().is_ok() {
        use slint::winit_030::WinitWindowAccessor;
        window
            .window()
            .with_winit_window(|native| native.focus_window());
    }
}

/// 主窗口语言变化后同步已打开的帮助窗口；窗口未创建时无事可做。
pub fn sync_language(app: &crate::MainWindow) {
    let zh = !app.get_runtime_language().starts_with("en");
    if ZH.with(Cell::get) == zh {
        return;
    }
    let window = WINDOW.with(|slot| slot.borrow().as_ref().map(|window| window.clone_strong()));
    if let Some(window) = window {
        window.global::<HelpDocs>().invoke_language_changed(zh);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use slint::Model;

    #[test]
    fn manual_parses_and_links_resolve() {
        // 中英两份 Markdown 手册都要满足同样的约束：主题齐全、索引不越界、
        // 站内链接与图册引用的目标都存在。
        for zh in [true, false] {
            let manual = Manual::load(zh);
            assert!(manual.topics.len() >= 10);
            for entry in &manual.index {
                assert!(entry.topic < manual.topics.len(), "index 引用越界");
            }
            for topic in &manual.topics {
                assert!(!topic.group.is_empty(), "主题 {} 缺少分组", topic.id);
                assert!(!topic.body.is_empty(), "主题 {} 没有正文", topic.id);
            }
            for topic in &manual.topics {
                for (target, _) in topic.body.match_indices("topic:") {
                    let rest = &topic.body[target + "topic:".len()..];
                    let id: String = rest
                        .chars()
                        .take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '-')
                        .collect();
                    assert!(
                        manual.topic_index(&id).is_some(),
                        "站内链接目标不存在: {id}"
                    );
                }
                for (at, _) in topic.body.match_indices("](art/") {
                    let rest = &topic.body[at + "](art/".len()..];
                    let slug: String = rest
                        .chars()
                        .take_while(|ch| *ch != ')')
                        .collect::<String>()
                        .trim_end_matches(".png")
                        .to_string();
                    assert!(art_preview(&slug).is_some(), "预览图资源不存在: {slug}");
                }
            }
        }
    }

    #[test]
    fn markdown_projects_the_document() {
        let _ui = crate::test_support::lock();
        crate::register_bundled_font().expect("注册内嵌字体");
        let manual = Manual::load(true);
        let mut art = ArtCache::default();
        let rows = project_topic(&manual.topics[0], &mut art);
        assert!(rows.iter().any(|row| row.kind == crate::md::block::HEADING));
        assert!(rows.iter().any(|row| row.kind == crate::md::block::LIST_ITEM));
        assert!(
            rows.iter().any(|row| row.plain.contains("本地 AI 工作台")),
            "正文文本应完整进入块的纯文本投影"
        );
    }

    #[test]
    fn art_sheet_images_are_hydrated() {
        let _ui = crate::test_support::lock();
        crate::register_bundled_font().expect("注册内嵌字体");
        let manual = Manual::load(true);
        let topic = manual
            .topics
            .iter()
            .find(|topic| topic.id == "art-overview")
            .expect("缺少美术图册主题");
        let mut art = ArtCache::default();
        let rows = project_topic(topic, &mut art);
        let images: Vec<_> = rows
            .iter()
            .filter(|row| row.kind == crate::md::block::IMAGE)
            .collect();
        assert_eq!(images.len(), 1, "美术主题应带一张预览图");
        assert!(images[0].image.size().width > 700);
        assert!(images[0].image_height > 500.0);
        assert!(
            rows.iter().any(|row| row.plain.contains("1462×866")),
            "图注应进入正文"
        );
    }

    /// The help window publishes the very same block model the chat uses.
    #[test]
    #[ignore = "需要窗口与事件循环，单独运行"]
    fn help_window_projects_blocks() {
        crate::register_bundled_font().expect("注册内嵌字体");
        let window = HelpWindow::new().expect("创建帮助窗口");
        wire(&window, true);
        window.show().expect("显示帮助窗口");
        let done = Rc::new(Cell::new(false));
        let probe = done.clone();
        slint::Timer::single_shot(std::time::Duration::from_millis(800), move || {
            probe.set(true);
            let _ = slint::quit_event_loop();
        });
        let _ = slint::run_event_loop();
        assert!(done.get(), "事件循环未按时退出");
        let docs = window.global::<HelpDocs>();
        assert!(
            docs.get_blocks().row_count() > 3,
            "帮助窗口应发布 Markdown 块模型"
        );
    }
}
