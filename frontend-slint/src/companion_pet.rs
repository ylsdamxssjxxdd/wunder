//! Native floating companions ("desktop pets"), mirroring the web
//! `CompanionFloatingLayer` behaviour on top of layered Win32 windows.
//!
//! The projection is deliberately cheap: `sync()` reads the Slint models once,
//! diffs the result against what the overlay already shows and only then sends
//! commands. Sprite decoding, the frame clock and the drag live on the overlay
//! thread, so a streaming chat costs this module a handful of small messages.

use crate::{
    companion_sprite,
    navigation_ui::agent_key,
    pet_window::{
        self, BubbleTone, PetAnim, PetBubble, PetCommand, PetEvent, PetMenuItem, PetService,
        PetVisual,
    },
    MainWindow,
};
use slint::{ComponentHandle, Model, Weak};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::time::{Duration, Instant};
use wunder_desktop::{CompanionOverlayState, NativeDesktop};
use wunder_server::worker_card_settings::parse_agent_icon_config;

/// Web sprite box and screen inset.
const BASE_WIDTH: f64 = 192.0;
const BASE_HEIGHT: f64 = 208.0;
const SCREEN_MARGIN: f64 = 8.0;
const CLICK_WAVE: Duration = Duration::from_millis(700);
const NAME_BUBBLE: Duration = Duration::from_millis(1800);
const MESSAGE_HINT: Duration = Duration::from_millis(3200);
const DRAG_SETTLE: Duration = Duration::from_millis(900);
const HINT_MAX_CHARS: usize = 96;
/// Web `scalePresets`, offered by the pet context menu.
const SCALE_PRESETS: [f32; 6] = [0.5, 0.8, 1.0, 1.2, 1.4, 1.6];
/// Icon payloads may carry any scale inside the web renderer clamp.
const SCALE_MIN: f32 = 0.5;
const SCALE_MAX: f32 = 1.8;
/// Bubble clock, event drain and re-projection. The web layer polls on a
/// 500 ms timer; 120 ms keeps a click reaction responsive while an idle
/// projection only walks a handful of pets.
const TICK: Duration = Duration::from_millis(120);
/// Bounded drain, so a flood of native events can never starve the UI loop.
const EVENTS_PER_TICK: usize = 16;
/// Web `openMessageAgentChat` debounce.
const CHAT_DEBOUNCE: Duration = Duration::from_millis(1200);
/// A companion whose package has no spritesheet is probed again this slowly,
/// so a dangling binding cannot spawn a decode thread per projection.
const MISSING_RETRY: Duration = Duration::from_millis(30_000);

const MENU_OPEN_CHAT: u16 = 1;
const MENU_TOGGLE_SHOW: u16 = 2;
const MENU_SCALE_FIRST: u16 = 10;

/// Sprite rows, named as the web state table names them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Sprite {
    Idle,
    RunningRight,
    RunningLeft,
    Waving,
    Jumping,
    Failed,
    Waiting,
    Running,
    Review,
}

impl Sprite {
    fn anim(self) -> companion_sprite::Anim {
        match self {
            Sprite::Idle => companion_sprite::IDLE,
            Sprite::RunningRight => companion_sprite::RUNNING_RIGHT,
            Sprite::RunningLeft => companion_sprite::RUNNING_LEFT,
            Sprite::Waving => companion_sprite::WAVING,
            Sprite::Jumping => companion_sprite::JUMPING,
            Sprite::Failed => companion_sprite::FAILED,
            Sprite::Waiting => companion_sprite::WAITING,
            Sprite::Running => companion_sprite::RUNNING,
            Sprite::Review => companion_sprite::REVIEW,
        }
    }
}

/// Normalized runtime state, the same five values the web signal arbitrator
/// produces before it becomes a sprite row. Derived order is the priority the
/// arbitrator uses: running beats pending beats error beats done beats idle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Runtime {
    Idle,
    Done,
    Error,
    Pending,
    Running,
}

/// Map a desktop runtime status string (thread row or message state) onto the
/// five web runtime states.
fn classify_runtime(status: &str) -> Runtime {
    let value = status.trim();
    if value.is_empty() {
        return Runtime::Idle;
    }
    if matches!(
        value,
        "error"
            | "failed"
            | "timeout"
            | "aborted"
            | "terminated"
            | "cancelled"
            | "canceled"
    ) || value.contains("失败")
        || value.contains("错误")
        || value.contains("超时")
        || value.contains("中断")
        || value.contains("已停止")
        || value.contains("取消")
    {
        return Runtime::Error;
    }
    if matches!(
        value,
        "pending"
            | "queued"
            | "waiting"
            | "waiting_input"
            | "waiting_user_input"
            | "pending_question"
            | "pending_confirm"
            | "awaiting_confirmation"
            | "awaiting_approval"
            | "approval_pending"
            | "question"
            | "asking"
    ) || value.starts_with("等待")
        || value.contains("排队")
        || value.contains("确认")
        || value.contains("审批")
        || value.contains("提问")
    {
        return Runtime::Pending;
    }
    if matches!(value, "running" | "streaming" | "executing" | "processing" | "cancelling")
        || value.starts_with("正在")
        || value.starts_with("目标")
    {
        return Runtime::Running;
    }
    if matches!(
        value,
        "done" | "completed" | "complete" | "finish" | "finished" | "success" | "succeeded"
    ) || value.contains("任务完成")
        || value.contains("输出已结束")
    {
        return Runtime::Done;
    }
    Runtime::Idle
}

/// The row a pet plays: web precedence is the transient override, then a
/// settled completion waits, then the runtime mapping.
fn base_sprite(runtime: Runtime, settled: bool) -> Sprite {
    match runtime {
        Runtime::Done if settled => Sprite::Waiting,
        Runtime::Done => Sprite::Jumping,
        Runtime::Pending => Sprite::Review,
        Runtime::Error => Sprite::Failed,
        Runtime::Running => Sprite::Running,
        Runtime::Idle => Sprite::Idle,
    }
}

/// Web `defaultPosition`: four pets per row, 46x34 logical steps.
fn cascade_position(index: usize) -> (f64, f64) {
    (
        28.0 + (index % 4) as f64 * 46.0,
        28.0 + (index / 4) as f64 * 34.0,
    )
}

/// Web `clampPosition`, using the desktop work area instead of the viewport so
/// a pet never hides behind the taskbar.
fn clamp_position(x: f64, y: f64, scale: f32, area: (f64, f64, f64, f64)) -> (f64, f64) {
    let width = BASE_WIDTH * scale as f64;
    let height = BASE_HEIGHT * scale as f64;
    let max_x = (area.2 - width - SCREEN_MARGIN).max(SCREEN_MARGIN);
    let max_y = (area.3 - height - SCREEN_MARGIN).max(SCREEN_MARGIN);
    // Integral logical pixels: the same value is persisted, sent to the overlay
    // and compared for "did this move", so a fraction would re-send every tick.
    (
        x.clamp(SCREEN_MARGIN, max_x).round(),
        y.clamp(SCREEN_MARGIN, max_y).round(),
    )
}

/// Web `truncateBubbleText`: keep 96 characters including the ellipsis.
fn truncate_bubble(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= HINT_MAX_CHARS {
        return chars.iter().collect();
    }
    let mut head: String = chars[..HINT_MAX_CHARS - 1].iter().collect();
    while head.ends_with(char::is_whitespace) {
        head.pop();
    }
    format!("{head}…")
}

/// Web bubble text normalization: drop images, keep link labels, strip inline
/// markdown punctuation, then collapse all whitespace runs.
fn hint_text(raw: &str) -> String {
    let without_images = strip_pairs(raw, "![", true);
    let without_links = strip_pairs(&without_images, "[", false);
    let mut text = String::with_capacity(without_links.len());
    let mut previous_space = false;
    for ch in without_links.chars() {
        let ch = match ch {
            '`' | '#' | '>' | '*' | '_' | '~' | '-' => ' ',
            '\n' | '\r' | '\t' => ' ',
            other => other,
        };
        if ch == ' ' {
            if !previous_space && !text.is_empty() {
                text.push(' ');
            }
            previous_space = true;
            continue;
        }
        text.push(ch);
        previous_space = false;
    }
    text.trim().to_string()
}

/// Remove `![alt](url)` / `[label](url)` constructs. An image carries no text
/// worth reading, so its label goes with it; a link label stays.
fn strip_pairs(input: &str, opener: &str, image: bool) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(start) = rest.find(opener) {
        out.push_str(&rest[..start]);
        let after = &rest[start + opener.len()..];
        let Some(end) = after.find(']') else {
            out.push_str(after);
            return out;
        };
        let label = &after[..end];
        let tail = &after[end + 1..];
        // The URL follows in parentheses; drop it so it never reaches the bubble.
        let tail = match tail.strip_prefix('(') {
            Some(inner) => match inner.find(')') {
                Some(close) => &inner[close + 1..],
                None => "",
            },
            None => tail,
        };
        if !image {
            out.push_str(label);
        }
        rest = tail;
    }
    out.push_str(rest);
    out
}

/// One pet, as projected from the agent list plus the persisted overlay state.
#[derive(Clone)]
struct PetSpec {
    key: String,
    agent_id: String,
    companion_id: String,
    name: String,
    scale: f32,
    message_hints: bool,
    x: f64,
    y: f64,
}

struct Pet {
    spec: PetSpec,
    runtime: Runtime,
    /// Web `doneSettledByKey`: a finished task jumps until the user acknowledges it.
    settled: bool,
    transient: Option<(Sprite, Instant)>,
    bubble: Option<(String, BubbleTone, Instant)>,
    /// First observed bubble per pet is history, not news.
    hint_signature: Option<String>,
    /// Last visual identity sent to the overlay, so an unchanged tick is silent.
    sent_visual: Option<String>,
    /// Last position sent to the overlay, in whole logical pixels.
    sent_position: Option<(i32, i32)>,
}

impl Pet {
    fn new(spec: PetSpec) -> Self {
        Pet {
            spec,
            runtime: Runtime::Idle,
            settled: true,
            transient: None,
            bubble: None,
            hint_signature: None,
            sent_visual: None,
            sent_position: None,
        }
    }

    fn apply_spec(&mut self, spec: PetSpec, runtime: Runtime, hint: Option<&Hint>) {
        let was_done = self.runtime == Runtime::Done;
        self.spec = spec;
        self.runtime = runtime;
        if runtime == Runtime::Done {
            if !was_done {
                self.settled = false;
            }
        } else {
            self.settled = true;
        }
        let Some(hint) = hint.filter(|hint| hint.agent_id == self.spec.agent_id) else {
            return;
        };
        match self.hint_signature.as_deref() {
            None => self.hint_signature = Some(hint.signature.clone()),
            Some(seen) if seen != hint.signature.as_str() => {
                self.hint_signature = Some(hint.signature.clone());
                if self.spec.message_hints && self.bubble.is_none() {
                    self.bubble = Some((
                        hint.text.clone(),
                        hint.tone,
                        Instant::now() + MESSAGE_HINT,
                    ));
                }
            }
            _ => {}
        }
    }

    /// Drop an expired one-shot and report the row to play.
    fn sprite(&mut self, now: Instant) -> Sprite {
        match self.transient {
            Some((sprite, until)) if until > now => sprite,
            Some(_) => {
                self.transient = None;
                base_sprite(self.runtime, self.settled)
            }
            None => base_sprite(self.runtime, self.settled),
        }
    }

    fn bubble(&mut self, now: Instant) -> Option<(String, BubbleTone)> {
        match &self.bubble {
            Some((text, tone, until)) if *until > now => Some((text.clone(), *tone)),
            Some(_) => {
                self.bubble = None;
                None
            }
            None => None,
        }
    }
}

struct Hint {
    agent_id: String,
    text: String,
    tone: BubbleTone,
    signature: String,
}

struct Overlay {
    api: Arc<NativeDesktop>,
    weak: Weak<MainWindow>,
    service: PetService,
    state: CompanionOverlayState,
    pets: HashMap<String, Pet>,
    /// Companions with no spritesheet, and when they were last probed.
    missing: HashMap<String, Instant>,
    last_chat_open: Option<Instant>,
    timer: slint::Timer,
}

thread_local! {
    static OVERLAY: RefCell<Option<Rc<RefCell<Overlay>>>> = const { RefCell::new(None) };
    // Drained without holding the overlay borrow, so an event handler can
    // re-enter the projection.
    static RECEIVER: RefCell<Option<Receiver<PetEvent>>> = const { RefCell::new(None) };
}

/// Start the overlay thread and keep the projection context alive.
pub fn install(app: &MainWindow, api: Arc<NativeDesktop>) {
    if !pet_window::available() {
        return;
    }
    let already = OVERLAY.with(|slot| slot.borrow().is_some());
    if already {
        return;
    }
    let Some((service, receiver)) = pet_window::PetService::start() else {
        return;
    };
    let state = api.companion_overlay_state();
    let overlay = Rc::new(RefCell::new(Overlay {
        api,
        weak: app.as_weak(),
        service,
        state,
        pets: HashMap::new(),
        missing: HashMap::new(),
        last_chat_open: None,
        timer: slint::Timer::default(),
    }));
    OVERLAY.with(|slot| *slot.borrow_mut() = Some(Rc::clone(&overlay)));
    RECEIVER.with(|slot| *slot.borrow_mut() = Some(receiver));
    reproject(&overlay, app);
}

/// Re-project every pet. Called from the agent/session hooks; it is a no-op
/// when no agent binds a companion, so the desktop stays free of extra timers.
pub fn sync(app: &MainWindow) {
    if let Some(overlay) = current() {
        reproject(&overlay, app);
    }
}

fn current() -> Option<Rc<RefCell<Overlay>>> {
    OVERLAY.with(|slot| slot.borrow().clone())
}

/// Drop cached pixels for a companion package that was just deleted, and the
/// pets that used them.
pub fn forget(companion_id: &str) {
    companion_sprite::forget(companion_id);
    let Some(overlay) = current() else {
        return;
    };
    let mut guard = overlay.borrow_mut();
    guard.missing.remove(companion_id);
    let keys: Vec<String> = guard
        .pets
        .iter()
        .filter(|(_, pet)| pet.spec.companion_id == companion_id)
        .map(|(key, _)| key.clone())
        .collect();
    for key in keys {
        guard.pets.remove(&key);
        guard.service.send(PetCommand::Dismiss { id: key });
    }
}

/// Agents that bind a companion, in list order, with the persisted overrides
/// the web layer keeps beside the agent record.
fn project_specs(app: &MainWindow, state: &CompanionOverlayState) -> Vec<PetSpec> {
    let rows = app.get_agents();
    let mut seen = HashSet::new();
    let mut specs = Vec::new();
    for (index, agent) in rows.iter().enumerate() {
        let config = parse_agent_icon_config(Some(agent.icon_config.as_str()));
        if !config.is_companion() {
            continue;
        }
        let id = agent_key(Some(agent.id.as_str())).to_string();
        if !seen.insert(id.clone()) {
            continue;
        }
        let entry = state.overrides.get(&id).copied();
        if !entry.and_then(|entry| entry.show).unwrap_or(config.show) {
            continue;
        }
        let scale = entry
            .and_then(|entry| entry.scale)
            .unwrap_or(config.scale)
            .clamp(SCALE_MIN, SCALE_MAX);
        let scope = if config.scope.is_empty() {
            "global"
        } else {
            config.scope.as_str()
        };
        let companion_id = if config.id.is_empty() {
            config.name.as_str()
        } else {
            config.id.as_str()
        };
        let key = format!("{id}:{scope}:{companion_id}");
        let position = state
            .positions
            .get(&key)
            .map(|placement| (placement.x.max(0) as f64, placement.y.max(0) as f64))
            .unwrap_or_else(|| cascade_position(index));
        specs.push(PetSpec {
            key,
            agent_id: id,
            companion_id: companion_id.to_string(),
            name: if agent.name.trim().is_empty() {
                config.name.clone()
            } else {
                agent.name.to_string()
            },
            scale,
            message_hints: config.message_hints && state.message_hints,
            x: position.0,
            y: position.1,
        });
    }
    specs
}

/// The active session's last answer, projected the way the web does it: an
/// in-flight turn never becomes a hint. The answer text comes from the
/// timeline's bounded tail lookup, so a long history costs one small copy.
fn active_hint(app: &MainWindow) -> Option<Hint> {
    if app.get_busy() {
        return None;
    }
    let text = app.invoke_timeline_last_answer().to_string();
    let runtime = classify_runtime(app.get_active_thread_status().as_str());
    if matches!(runtime, Runtime::Running | Runtime::Pending) {
        return None;
    }
    let text = hint_text(&text);
    if text.is_empty() {
        return None;
    }
    let agent_id = agent_key(Some(app.get_active_agent_id().as_str())).to_string();
    let tone = match runtime {
        Runtime::Error => BubbleTone::Warning,
        Runtime::Done => BubbleTone::Success,
        _ => BubbleTone::Info,
    };
    let signature = format!(
        "{agent_id}::{}::{:?}",
        text.chars().count(),
        tone
    );
    Some(Hint {
        agent_id,
        text: truncate_bubble(&text),
        tone,
        signature,
    })
}

/// Highest runtime state per agent, plus the live state of the active session.
/// One walk of the thread rows, shared by every pet.
fn agent_runtimes(app: &MainWindow) -> HashMap<String, Runtime> {
    let session = app.get_active_session_id().to_string();
    let mut runtimes = HashMap::<String, Runtime>::new();
    let mut active_agent = String::new();
    let mut active_runtime = Runtime::Idle;
    for row in app.get_conversations().iter() {
        let agent = agent_key(Some(row.agent_id.as_str())).to_string();
        let runtime = classify_runtime(row.runtime_status.as_str());
        if row.id == session {
            active_agent = agent.clone();
            active_runtime = runtime;
        }
        let slot = runtimes.entry(agent).or_insert(Runtime::Idle);
        *slot = (*slot).max(runtime);
    }
    // `active-thread-status` is the live stream state and the row can lag it.
    if active_runtime == Runtime::Idle {
        active_runtime = classify_runtime(app.get_active_thread_status().as_str());
    }
    if matches!(active_runtime, Runtime::Running | Runtime::Error) {
        if active_agent.is_empty() {
            active_agent = agent_key(Some(app.get_active_agent_id().as_str())).to_string();
        }
        let slot = runtimes.entry(active_agent).or_insert(Runtime::Idle);
        *slot = (*slot).max(active_runtime);
    }
    runtimes
}

/// Read the models, diff against the overlay and send only the changes.
fn reproject(overlay: &Rc<RefCell<Overlay>>, app: &MainWindow) {
    // A native menu action can re-enter here through a Slint callback; the
    // projection already reflects the persisted state, so skip the re-entry.
    if overlay.try_borrow().is_err() {
        return;
    }
    let specs = {
        let guard = overlay.borrow();
        project_specs(app, &guard.state)
    };
    let runtimes = agent_runtimes(app);
    let hint = active_hint(app);
    let mut guard = overlay.borrow_mut();
    apply(&mut guard, specs, &runtimes, hint.as_ref());
    let running = !guard.pets.is_empty();
    set_timer(&mut guard, running, app);
}

fn apply(
    overlay: &mut Overlay,
    specs: Vec<PetSpec>,
    runtimes: &HashMap<String, Runtime>,
    hint: Option<&Hint>,
) {
    let mut keep = HashSet::new();
    for spec in specs {
        let runtime = runtimes
            .get(spec.agent_id.as_str())
            .copied()
            .unwrap_or(Runtime::Idle);
        let key = spec.key.clone();
        keep.insert(key.clone());
        let companion = spec.companion_id.clone();
        // Pixels come first: without a decoded sheet there is nothing to draw,
        // and the projection retries once the background decode lands.
        let sheet = match companion_sprite::sheet(&companion) {
            Some(sheet) => Some(sheet),
            None => {
                if is_probing(&mut overlay.missing, &companion) {
                    schedule_sheet_load(&overlay.api, &overlay.weak, &companion);
                }
                None
            }
        };
        let pet = overlay
            .pets
            .entry(key)
            .or_insert_with(|| Pet::new(spec.clone()));
        pet.apply_spec(spec, runtime, hint);
        if let Some(sheet) = sheet.as_ref() {
            present(&overlay.service, pet, sheet);
        }
    }
    let stale: Vec<String> = overlay
        .pets
        .keys()
        .filter(|key| !keep.contains(*key))
        .cloned()
        .collect();
    for key in stale {
        overlay.pets.remove(&key);
        overlay.service.send(PetCommand::Dismiss { id: key });
    }
}

/// Send the window's visual, creating it on first use.
fn present(service: &PetService, pet: &mut Pet, sheet: &Arc<companion_sprite::Sheet>) {
    if let Some(area) = pet_window::work_area() {
        let (x, y) = clamp_position(pet.spec.x, pet.spec.y, pet.spec.scale, area);
        pet.spec.x = x;
        pet.spec.y = y;
    }
    let now = Instant::now();
    let sprite = pet.sprite(now);
    let bubble = pet.bubble(now);
    let position = (pet.spec.x.round() as i32, pet.spec.y.round() as i32);
    let identity = format!(
        "{sprite:?}@{:.3}{}",
        pet.spec.scale,
        bubble
            .as_ref()
            .map(|(text, tone)| format!("|{tone:?}|{text}"))
            .unwrap_or_default()
    );
    if pet.sent_visual.as_deref() == Some(identity.as_str()) {
        if pet.sent_position != Some(position) {
            pet.sent_position = Some(position);
            service.send(PetCommand::Move {
                id: pet.spec.key.clone(),
                x: position.0 as f64,
                y: position.1 as f64,
            });
        }
        return;
    }
    let visual = Box::new(PetVisual {
        width: BASE_WIDTH * pet.spec.scale as f64,
        height: BASE_HEIGHT * pet.spec.scale as f64,
        anim: pet_anim(sheet, sprite),
        drag: [
            pet_anim(sheet, Sprite::RunningLeft),
            pet_anim(sheet, Sprite::RunningRight),
        ],
        bubble: bubble.map(|(text, tone)| PetBubble { text, tone }),
    });
    let moved = pet.sent_position != Some(position);
    pet.sent_visual = Some(identity);
    pet.sent_position = Some(position);
    if moved {
        service.send(PetCommand::Present {
            id: pet.spec.key.clone(),
            x: position.0 as f64,
            y: position.1 as f64,
            visual,
        });
    } else {
        service.send(PetCommand::Update {
            id: pet.spec.key.clone(),
            visual,
        });
    }
}

/// A companion without pixels is only probed once per retry window.
fn is_probing(missing: &mut HashMap<String, Instant>, companion_id: &str) -> bool {
    let now = Instant::now();
    match missing.get(companion_id) {
        Some(seen) if now.duration_since(*seen) < MISSING_RETRY => false,
        _ => {
            missing.insert(companion_id.to_string(), now);
            true
        }
    }
}

fn pet_anim(sheet: &Arc<companion_sprite::Sheet>, sprite: Sprite) -> Option<PetAnim> {
    let anim = sprite.anim();
    let frames = sheet.frames_for(anim);
    (frames > 0).then_some(PetAnim {
        sheet: Arc::clone(sheet),
        row: anim.row,
        frames,
        frame_millis: anim.frame_millis(),
    })
}

/// Decode a companion sheet once, off the UI thread, then re-project.
fn schedule_sheet_load(api: &Arc<NativeDesktop>, weak: &Weak<MainWindow>, id: &str) {
    if !companion_sprite::mark_sheet_load_scheduled(id) {
        return;
    }
    let api = Arc::clone(api);
    let weak = weak.clone();
    let id = id.to_string();
    let pending = id.clone();
    std::thread::spawn(move || {
        let sheet = api
            .companion_spritesheet(&id)
            .ok()
            .flatten()
            .and_then(|(_mime, bytes)| companion_sprite::decode_sheet(&bytes));
        let _ = weak.upgrade_in_event_loop(move |app| {
            companion_sprite::clear_sheet_load_mark(&pending);
            if let Some(sheet) = sheet {
                companion_sprite::store_sheet(&pending, sheet);
            }
            if let Some(overlay) = current() {
                overlay.borrow_mut().missing.remove(&pending);
            }
            sync(&app);
        });
    });
}

fn set_timer(overlay: &mut Overlay, running: bool, app: &MainWindow) {
    if !running {
        overlay.timer.stop();
        return;
    }
    if overlay.timer.running() {
        return;
    }
    let weak = app.as_weak();
    overlay.timer.start(slint::TimerMode::Repeated, TICK, move || {
        let Some(app) = weak.upgrade() else {
            if let Some(overlay) = current() {
                overlay.borrow_mut().timer.stop();
            }
            return;
        };
        if let Some(overlay) = current() {
            tick(&overlay, &app);
        }
    });
}

/// Drain native input events, then re-project: expiries, drags and menu picks
/// all land through the same diff.
fn tick(overlay: &Rc<RefCell<Overlay>>, app: &MainWindow) {
    let events = drain();
    let mut open_chat: Option<String> = None;
    if !events.is_empty() {
        let mut guard = overlay.borrow_mut();
        for event in events {
            if let Some(agent) = handle_event(&mut guard, event) {
                open_chat = Some(agent);
            }
        }
    }
    reproject(overlay, app);
    let _ = open_chat;
}

fn drain() -> Vec<PetEvent> {
    let mut events = Vec::new();
    RECEIVER.with(|slot| {
        let receiver = slot.borrow_mut().take();
        let Some(receiver) = receiver else { return };
        while events.len() < EVENTS_PER_TICK {
            if let Ok(event) = receiver.try_recv() {
                events.push(event);
                continue;
            }
            break;
        }
        *slot.borrow_mut() = Some(receiver);
    });
    events
}

/// Apply one native interaction. Returns the agent whose chat to open, which
/// the caller invokes after releasing the overlay borrow.
fn handle_event(overlay: &mut Overlay, event: PetEvent) -> Option<String> {
    match event {
        PetEvent::Clicked { id } => {
            let Some(pet) = overlay.pets.get_mut(&id) else {
                return None;
            };
            pet.transient = Some((Sprite::Waving, Instant::now() + CLICK_WAVE));
            pet.settled = true;
            if pet.bubble.is_none() {
                pet.bubble = Some((
                    pet.spec.name.clone(),
                    BubbleTone::Info,
                    Instant::now() + NAME_BUBBLE,
                ));
            }
            None
        }
        PetEvent::Dragged { id, x, y } => {
            let Some(pet) = overlay.pets.get_mut(&id) else {
                return None;
            };
            pet.spec.x = x;
            pet.spec.y = y;
            pet.transient = Some((Sprite::Waiting, Instant::now() + DRAG_SETTLE));
            pet.settled = true;
            // One write per drag release; the overlay owns the live position.
            if let Ok(state) = overlay.api.save_companion_position(&id, x as i32, y as i32) {
                overlay.state = state;
            }
            None
        }
        PetEvent::RightClicked { id, x, y } => {
            let Some(pet) = overlay.pets.get(&id) else {
                return None;
            };
            let items = menu_items(pet);
            overlay.service.send(PetCommand::ShowMenu {
                id: id.clone(),
                x,
                y,
                items,
            });
            None
        }
        PetEvent::MenuSelected { id, code } => {
            let Some(agent_id) = overlay.pets.get(&id).map(|pet| pet.spec.agent_id.clone()) else {
                return None;
            };
            if code == MENU_OPEN_CHAT {
                let now = Instant::now();
                if overlay
                    .last_chat_open
                    .is_some_and(|seen| now.duration_since(seen) < CHAT_DEBOUNCE)
                {
                    return None;
                }
                overlay.last_chat_open = Some(now);
                return Some(agent_id);
            }
            if code == MENU_TOGGLE_SHOW {
                toggle(overlay, &id, &agent_id);
            } else if code >= MENU_SCALE_FIRST {
                let index = (code - MENU_SCALE_FIRST) as usize;
                if let Some(scale) = SCALE_PRESETS.get(index).copied() {
                    set_scale(overlay, &agent_id, scale);
                }
            }
            None
        }
    }
}

fn menu_items(pet: &Pet) -> Vec<PetMenuItem> {
    let mut items = vec![
        PetMenuItem {
            code: MENU_OPEN_CHAT,
            label: "打开会话".to_string(),
            checked: false,
            separator: false,
        },
        PetMenuItem {
            code: MENU_TOGGLE_SHOW,
            label: "隐藏".to_string(),
            checked: false,
            separator: false,
        },
        PetMenuItem {
            code: 0,
            label: String::new(),
            checked: false,
            separator: true,
        },
    ];
    for (index, scale) in SCALE_PRESETS.iter().enumerate() {
        items.push(PetMenuItem {
            code: MENU_SCALE_FIRST + index as u16,
            label: format!("{scale:.1}x"),
            checked: (pet.spec.scale - *scale).abs() < 0.001,
            separator: false,
        });
    }
    items
}

/// Hide this pet through the local override only, never the agent record.
fn toggle(overlay: &mut Overlay, key: &str, agent_id: &str) {
    let show = overlay
        .state
        .overrides
        .get(agent_id)
        .and_then(|entry| entry.show)
        .unwrap_or(true);
    let scale = overlay.pets.get(key).map(|pet| pet.spec.scale).unwrap_or(1.0);
    if let Ok(state) = overlay.api.save_companion_override(agent_id, !show, scale) {
        overlay.state = state;
    }
    if !show {
        return;
    }
    overlay.pets.remove(key);
    overlay.service.send(PetCommand::Dismiss { id: key.to_string() });
}

/// Resize through the same local override, keeping the current visibility.
fn set_scale(overlay: &mut Overlay, agent_id: &str, scale: f32) {
    if let Ok(state) = overlay.api.save_companion_override(agent_id, true, scale) {
        overlay.state = state;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_strings_map_to_the_web_states() {
        assert_eq!(classify_runtime("running"), Runtime::Running);
        assert_eq!(classify_runtime("streaming"), Runtime::Running);
        assert_eq!(classify_runtime("正在生成…"), Runtime::Running);
        assert_eq!(classify_runtime("queued"), Runtime::Pending);
        assert_eq!(classify_runtime("等待用户输入"), Runtime::Pending);
        assert_eq!(classify_runtime("任务完成"), Runtime::Done);
        assert_eq!(classify_runtime("输出已结束"), Runtime::Done);
        assert_eq!(classify_runtime("failed"), Runtime::Error);
        assert_eq!(classify_runtime("执行失败"), Runtime::Error);
        assert_eq!(classify_runtime("已停止"), Runtime::Error);
        assert_eq!(classify_runtime(""), Runtime::Idle);
        assert_eq!(classify_runtime("idle"), Runtime::Idle);
    }

    #[test]
    fn runtime_priority_matches_the_arbitrator() {
        assert!(Runtime::Running > Runtime::Pending);
        assert!(Runtime::Pending > Runtime::Error);
        assert!(Runtime::Error > Runtime::Done);
        assert!(Runtime::Done > Runtime::Idle);
    }

    #[test]
    fn done_jumps_until_acknowledged_then_waits() {
        assert_eq!(base_sprite(Runtime::Done, false), Sprite::Jumping);
        assert_eq!(base_sprite(Runtime::Done, true), Sprite::Waiting);
        assert_eq!(base_sprite(Runtime::Pending, true), Sprite::Review);
        assert_eq!(base_sprite(Runtime::Error, true), Sprite::Failed);
        assert_eq!(base_sprite(Runtime::Running, true), Sprite::Running);
        assert_eq!(base_sprite(Runtime::Idle, false), Sprite::Idle);
    }

    #[test]
    fn cascade_matches_the_web_grid() {
        assert_eq!(cascade_position(0), (28.0, 28.0));
        assert_eq!(cascade_position(3), (166.0, 28.0));
        assert_eq!(cascade_position(4), (28.0, 62.0));
    }

    #[test]
    fn positions_stay_inside_the_work_area() {
        let area = (0.0, 0.0, 1920.0, 1080.0);
        assert_eq!(clamp_position(-50.0, -10.0, 1.0, area), (8.0, 8.0));
        assert_eq!(clamp_position(9999.0, 9999.0, 1.0, area), (1720.0, 864.0));
        // A 1.6x pet is 307x333, so the bottom edge moves up accordingly.
        assert_eq!(clamp_position(1900.0, 1000.0, 1.6, area), (1605.0, 739.0));
    }

    #[test]
    fn hint_text_drops_markdown_and_keeps_labels() {
        let text = hint_text("看 ![图](a.webp) 这份 [报告](https://example.com) 吧\n\n  OK");
        assert_eq!(text, "看 这份 报告 吧 OK");
    }

    #[test]
    fn hint_text_is_truncated_to_96_characters() {
        let long = "字".repeat(200);
        let value = truncate_bubble(&hint_text(&long));
        assert_eq!(value.chars().count(), HINT_MAX_CHARS);
        assert!(value.ends_with('…'));
        assert_eq!(truncate_bubble("短文本"), "短文本");
    }

    #[test]
    fn scale_stays_inside_the_renderer_range() {
        let clamp = |value: f32| value.clamp(SCALE_MIN, SCALE_MAX);
        assert_eq!(clamp(0.2), 0.5);
        assert_eq!(clamp(2.4), 1.8);
        assert_eq!(clamp(1.3), 1.3);
    }

    #[test]
    fn a_new_bubble_pops_once_and_a_settled_done_stops_jumping() {
        let spec = PetSpec {
            key: "agent-a:global:pet-a".to_string(),
            agent_id: "agent-a".to_string(),
            companion_id: "pet-a".to_string(),
            name: "小助手".to_string(),
            scale: 1.0,
            message_hints: true,
            x: 28.0,
            y: 28.0,
        };
        let mut pet = Pet::new(spec.clone());
        let hint = Hint {
            agent_id: "agent-a".to_string(),
            text: "已经处理完了".to_string(),
            tone: BubbleTone::Success,
            signature: "sig-1".to_string(),
        };
        // The first observed bubble is history, so nothing pops.
        pet.apply_spec(spec.clone(), Runtime::Done, Some(&hint));
        assert_eq!(pet.sprite(Instant::now()), Sprite::Jumping);
        assert!(pet.bubble(Instant::now()).is_none());
        // A newer hint pops, and an acknowledged completion settles to waiting.
        let next = Hint {
            signature: "sig-2".to_string(),
            ..hint
        };
        pet.apply_spec(spec.clone(), Runtime::Done, Some(&next));
        assert_eq!(
            pet.bubble(Instant::now()).map(|(text, _)| text),
            Some("已经处理完了".to_string())
        );
        pet.transient = Some((Sprite::Waiting, Instant::now() + CLICK_WAVE));
        assert_eq!(pet.sprite(Instant::now()), Sprite::Waiting);
        // The one-shot is the controller's: once it ages out the pet returns to
        // its runtime row, and a completion keeps jumping until acknowledged.
        let late = Instant::now() + CLICK_WAVE * 2;
        assert_eq!(pet.sprite(late), Sprite::Jumping);
        pet.settled = true;
        assert_eq!(pet.sprite(Instant::now()), Sprite::Waiting);
        // The hint runs on its own clock.
        assert!(pet.bubble(Instant::now() + MESSAGE_HINT * 2).is_none());
    }
}
