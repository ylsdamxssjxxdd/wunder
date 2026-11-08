//! System-level floating companion (desktop pet) for the Android form factor.
//!
//! The controller runs on its own process-scoped thread, not on the Slint UI
//! tick: the activity and its event loop come and go, but the pet must keep
//! reflecting the engine while the app is backgrounded. State comes from the
//! shared engine projection; settings and positions persist through the same
//! `NativeDesktop` companion store the desktop form uses. The Java side
//! (`CompanionBridge`) owns the overlay window, the frame clock and touch.

use crate::engine::Shared;
use anyhow::{anyhow, Context as _, Result};
use jni::objects::{GlobalRef, JObject, JString, JValue};
use jni::{JNIEnv, JavaVM};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use wunder_desktop::NativeDesktop;

const BRIDGE_CLASS: &str = "dev.wunder.android.CompanionBridge";
const TICK: Duration = Duration::from_millis(120);
const CLICK_WAVE: Duration = Duration::from_millis(700);
const NAME_BUBBLE: Duration = Duration::from_millis(1800);
const MESSAGE_HINT: Duration = Duration::from_millis(3200);
const DRAG_SETTLE: Duration = Duration::from_millis(900);
const FAILED_HOLD: Duration = Duration::from_secs(6);
/// Overlay permission is polled at roughly 1 Hz so returning from the system
/// settings page presents the pet without another settings round-trip.
const PERMISSION_POLL_TICKS: u32 = 8;

/// One animation cycle: sheet row, frame count, total cycle milliseconds.
/// Mirrors the shared sprite table in the desktop `companion_sprite.rs`.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Anim {
    row: i32,
    frames: i32,
    duration_ms: i32,
}

impl Anim {
    fn frame_ms(self) -> i32 {
        (self.duration_ms / self.frames).max(1)
    }
}

const IDLE: Anim = Anim { row: 0, frames: 6, duration_ms: 1100 };
const WAVING: Anim = Anim { row: 3, frames: 4, duration_ms: 700 };
const JUMPING: Anim = Anim { row: 4, frames: 5, duration_ms: 840 };
const FAILED: Anim = Anim { row: 5, frames: 8, duration_ms: 1220 };
const WAITING: Anim = Anim { row: 6, frames: 6, duration_ms: 1010 };
const RUNNING: Anim = Anim { row: 7, frames: 6, duration_ms: 820 };
const REVIEW: Anim = Anim { row: 8, frames: 6, duration_ms: 1030 };

/// Normalized engine state, ordered by the desktop arbitrator's priority.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Runtime {
    Idle,
    Stopped,
    Done,
    Error,
    Pending,
    Running,
}

impl Runtime {
    fn of(state: &Shared) -> Self {
        if state.loading {
            return Runtime::Pending;
        }
        if state.generating {
            return match state.turn_phase.as_str() {
                "requesting" | "queued" | "waiting" => Runtime::Pending,
                _ => Runtime::Running,
            };
        }
        match state.turn_phase.as_str() {
            "done" => Runtime::Done,
            "error" => Runtime::Error,
            "stopped" => Runtime::Stopped,
            _ => Runtime::Idle,
        }
    }
}

/// The row a pet plays: transient override, then a settled completion waits,
/// then the runtime mapping (same precedence as the desktop pet).
fn base_sprite(runtime: Runtime, settled: bool) -> Anim {
    match runtime {
        Runtime::Done if settled => WAITING,
        Runtime::Done => JUMPING,
        Runtime::Pending => REVIEW,
        Runtime::Stopped => WAITING,
        Runtime::Error => FAILED,
        Runtime::Running => RUNNING,
        Runtime::Idle => IDLE,
    }
}

struct Bridge {
    vm: JavaVM,
    app: GlobalRef,
    class: GlobalRef,
}

static BRIDGE: OnceLock<Bridge> = OnceLock::new();
static LAST_ERROR: Mutex<String> = Mutex::new(String::new());

fn bridge() -> Option<&'static Bridge> {
    if let Some(bridge) = BRIDGE.get() {
        return Some(bridge);
    }
    let context = ndk_context::android_context();
    // SAFETY: android-activity initialized the process JVM before android_main.
    let vm = unsafe { JavaVM::from_raw(context.vm().cast()) }.ok()?;
    let app = crate::android_host::app_context()?;
    let class = {
        let mut env = vm.attach_current_thread().ok()?;
        load_bridge_class(&mut env, app.as_obj()).ok()?
    };
    Some(BRIDGE.get_or_init(|| Bridge { vm, app, class }))
}

/// `FindClass` on a native worker cannot see app classes; resolve through the
/// application context's own loader (same rule as the WebView bridge).
fn load_bridge_class(env: &mut JNIEnv<'_>, context: &JObject<'_>) -> Result<GlobalRef> {
    let loader = env
        .call_method(context, "getClassLoader", "()Ljava/lang/ClassLoader;", &[])
        .and_then(|value| value.l())
        .context("read the application class loader")?;
    let name = env.new_string(BRIDGE_CLASS)?;
    let class = env
        .call_method(
            &loader,
            "loadClass",
            "(Ljava/lang/String;)Ljava/lang/Class;",
            &[JValue::Object(&name)],
        )
        .and_then(|value| value.l())
        .context("load CompanionBridge")?;
    Ok(env.new_global_ref(class)?)
}

fn call(action: &str, args: &Value) -> Option<Value> {
    let bridge = bridge()?;
    let mut env = bridge.vm.attach_current_thread().ok()?;
    let result = (|| -> Result<Value> {
        let action_string = env.new_string(action)?;
        let args_string = env.new_string(args.to_string())?;
        let value = env
            .call_static_method(
                &bridge.class,
                "execute",
                "(Landroid/content/Context;Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;",
                &[
                    JValue::Object(bridge.app.as_obj()),
                    JValue::Object(&action_string),
                    JValue::Object(&args_string),
                ],
            )
            .map_err(|error| {
                clear_exception(&mut env);
                anyhow!("CompanionBridge.execute: {error}")
            })?
            .l()?;
        if value.is_null() {
            return Err(anyhow!("CompanionBridge.execute returned null"));
        }
        let text: String = env.get_string(&JString::from(value))?.into();
        Ok(serde_json::from_str(&text)?)
    })();
    match result {
        Ok(value) => Some(value),
        Err(error) => {
            clear_exception(&mut env);
            // A broken bridge fails every tick; only log when the story changes.
            let message = format!("{error:#}");
            if let Ok(mut last) = LAST_ERROR.lock() {
                if *last != message {
                    *last = message.clone();
                    log::error!("companion bridge: {message}");
                }
            }
            None
        }
    }
}

fn clear_exception(env: &mut JNIEnv<'_>) {
    if env.exception_check().unwrap_or(false) {
        let _ = env.exception_describe();
        let _ = env.exception_clear();
    }
}

/// Overlay permission as seen by the system: `granted`, `denied` or `unknown`
/// when the bridge is unavailable.
pub fn permission_status() -> String {
    call("status", &json!({}))
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| "unknown".into())
}

/// Open the system overlay-permission page for this app.
pub fn request_permission() {
    call("request_permission", &json!({}));
}

/// Spawn the process-scoped companion controller.
pub fn install(desktop: Arc<NativeDesktop>, shared: Arc<Mutex<Shared>>) {
    let spawned = std::thread::Builder::new()
        .name("wunder-android-companion".into())
        .spawn(move || {
            let mut controller = Controller::default();
            loop {
                std::thread::sleep(TICK);
                controller.tick(&desktop, &shared);
            }
        });
    if let Err(error) = spawned {
        log::error!("spawn the companion controller: {error}");
    }
}

#[derive(Default)]
struct Controller {
    presented: bool,
    sent_key: String,
    sent_anim: Option<(i32, i32, i32)>,
    sent_bubble: (String, i32),
    position: (i32, i32),
    position_loaded_for: String,
    runtime: Option<Runtime>,
    settled: bool,
    failed_until: Option<Instant>,
    transient: Option<(Anim, Instant)>,
    bubble: Option<(String, i32, Instant)>,
    hint_seen: String,
    missing_sheet: String,
    ticks: u32,
    permission: String,
}

impl Controller {
    fn tick(&mut self, desktop: &Arc<NativeDesktop>, shared: &Arc<Mutex<Shared>>) {
        let now = Instant::now();
        self.ticks = self.ticks.wrapping_add(1);
        let snapshot = {
            let Ok(state) = shared.lock() else { return };
            Snapshot {
                companion: state.companion.clone(),
                runtime: Runtime::of(&state),
                hint: state.turn_hint.clone(),
            }
        };
        let name = snapshot
            .companion
            .companions
            .iter()
            .find(|choice| choice.id == snapshot.companion.selected_id)
            .map(|choice| choice.name.clone())
            .unwrap_or_else(|| snapshot.companion.selected_id.clone());
        self.drain_events(desktop, shared, &snapshot, &name, now);
        if self.ticks == 1 || self.ticks % PERMISSION_POLL_TICKS == 0 {
            self.permission = permission_status();
        }
        self.track_runtime(&snapshot, now);

        let show = snapshot.companion.enabled
            && !snapshot.companion.selected_id.is_empty()
            && self.permission == "granted";
        if !show {
            self.dismiss();
            return;
        }
        let Some(sheet) = sheet_path(&snapshot.companion.selected_id) else {
            if self.missing_sheet != snapshot.companion.selected_id {
                self.missing_sheet = snapshot.companion.selected_id.clone();
                log::warn!("companion spritesheet missing: {}", snapshot.companion.selected_id);
            }
            self.dismiss();
            return;
        };
        self.missing_sheet.clear();
        if self.position_loaded_for != snapshot.companion.selected_id {
            self.position_loaded_for = snapshot.companion.selected_id.clone();
            self.position = desktop
                .companion_overlay_state()
                .positions
                .get(&position_key(&snapshot.companion.selected_id))
                .map(|placement| (placement.x, placement.y))
                .unwrap_or((-1, -1));
        }

        let anim = self.sprite(now);
        let bubble = self.current_bubble(now);
        let key = format!(
            "{}|{}|{}",
            snapshot.companion.selected_id, snapshot.companion.scale, sheet
        );
        let anim_key = (anim.row, anim.frames, anim.frame_ms());
        if !self.presented || self.sent_key != key {
            let result = call(
                "present",
                &json!({
                    "sheet": sheet,
                    "scale": snapshot.companion.scale,
                    "x": self.position.0,
                    "y": self.position.1,
                    "row": anim.row,
                    "frames": anim.frames,
                    "frameMs": anim.frame_ms(),
                    "bubble": bubble.0,
                    "tone": bubble.1,
                }),
            );
            self.presented = result.as_ref().and_then(Value::as_str) == Some("ok");
            if self.presented {
                self.sent_key = key;
                self.sent_anim = Some(anim_key);
                self.sent_bubble = bubble;
            } else if let Some(Value::String(message)) = result {
                if message != "error|no_permission" {
                    log::warn!("present companion failed: {message}");
                }
            }
        } else if self.sent_anim != Some(anim_key) || self.sent_bubble != bubble {
            let result = call(
                "update",
                &json!({
                    "row": anim.row,
                    "frames": anim.frames,
                    "frameMs": anim.frame_ms(),
                    "bubble": bubble.0,
                    "tone": bubble.1,
                }),
            );
            if result.as_ref().and_then(Value::as_str) == Some("ok") {
                self.sent_anim = Some(anim_key);
                self.sent_bubble = bubble;
            }
        }
    }

    /// Java queues touch results; the controller owns what they mean.
    fn drain_events(
        &mut self,
        desktop: &Arc<NativeDesktop>,
        shared: &Arc<Mutex<Shared>>,
        snapshot: &Snapshot,
        name: &str,
        now: Instant,
    ) {
        let Some(Value::Array(events)) = call("take_events", &json!({})) else {
            return;
        };
        for event in events.iter().filter_map(Value::as_str) {
            if event == "click" {
                self.transient = Some((WAVING, now + CLICK_WAVE));
                self.bubble = Some((name.to_owned(), 0, now + NAME_BUBBLE));
                self.settled = true;
            } else if let Some(coords) = event.strip_prefix("drag|") {
                let mut parts = coords.split('|');
                let x = parts.next().and_then(|v| v.parse::<i32>().ok());
                let y = parts.next().and_then(|v| v.parse::<i32>().ok());
                if let (Some(x), Some(y)) = (x, y) {
                    self.position = (x, y);
                    let key = position_key(&snapshot.companion.selected_id);
                    if let Err(error) = desktop.save_companion_position(&key, x, y) {
                        log::warn!("save companion position: {error:#}");
                    }
                    self.transient = Some((WAITING, now + DRAG_SETTLE));
                }
            } else if event == "hide" {
                let companion = &snapshot.companion;
                match desktop.save_companion_settings(
                    false,
                    &companion.selected_id,
                    companion.scale,
                    companion.message_hints,
                ) {
                    Ok(_) => {
                        if let Ok(mut state) = shared.lock() {
                            state.companion.enabled = false;
                            state.companion_rev = state.companion_rev.wrapping_add(1);
                            state.notify("伙伴已隐藏，可在设置中重新开启");
                            state.touch();
                        }
                    }
                    Err(error) => log::warn!("hide companion: {error:#}"),
                }
            }
        }
    }

    /// Mirror the desktop pet's transient bookkeeping: a fresh completion jumps
    /// until acknowledged, a failure face holds for six seconds, and a new
    /// answer hint bubbles once when message hints are on.
    fn track_runtime(&mut self, snapshot: &Snapshot, now: Instant) {
        let previous = self.runtime;
        self.runtime = Some(snapshot.runtime);
        if snapshot.runtime == Runtime::Done {
            if previous != Some(Runtime::Done) {
                self.settled = false;
            }
        } else {
            self.settled = true;
        }
        if snapshot.runtime == Runtime::Error {
            if previous != Some(Runtime::Error) {
                self.failed_until = Some(now + FAILED_HOLD);
            }
        } else {
            self.failed_until = None;
        }
        if self.hint_seen != snapshot.hint {
            let first = previous.is_none();
            self.hint_seen = snapshot.hint.clone();
            if !first
                && snapshot.companion.message_hints
                && self.bubble.is_none()
                && !snapshot.hint.is_empty()
                && matches!(snapshot.runtime, Runtime::Done | Runtime::Error)
            {
                let tone = if snapshot.runtime == Runtime::Error { 2 } else { 1 };
                self.bubble = Some((snapshot.hint.clone(), tone, now + MESSAGE_HINT));
            }
        }
    }

    fn sprite(&mut self, now: Instant) -> Anim {
        if let Some((anim, until)) = self.transient {
            if until > now {
                return anim;
            }
            self.transient = None;
        }
        let resting = match self.runtime.unwrap_or(Runtime::Idle) {
            Runtime::Error if self.failed_until.is_some_and(|until| until <= now) => Runtime::Idle,
            other => other,
        };
        base_sprite(resting, self.settled)
    }

    fn current_bubble(&mut self, now: Instant) -> (String, i32) {
        match &self.bubble {
            Some((text, tone, until)) if *until > now => (text.clone(), *tone),
            Some(_) => {
                self.bubble = None;
                (String::new(), 0)
            }
            None => (String::new(), 0),
        }
    }

    fn dismiss(&mut self) {
        if self.presented {
            call("dismiss", &json!({}));
        }
        self.presented = false;
        self.sent_key.clear();
        self.sent_anim = None;
        self.sent_bubble = (String::new(), 0);
    }
}

struct Snapshot {
    companion: crate::engine::CompanionView,
    runtime: Runtime,
    hint: String,
}

fn position_key(id: &str) -> String {
    format!("desktop:{id}")
}

/// The Java side reads the sheet straight from the shared companion library;
/// both sides live in the same app sandbox, so no bytes cross JNI.
fn sheet_path(id: &str) -> Option<String> {
    let summary = wunder_server::companions::load_global_companion_summary(id)
        .ok()
        .flatten()?;
    let path = wunder_server::companions::global_companion_root()
        .join(id)
        .join(&summary.spritesheet_path);
    path.is_file().then(|| path.to_string_lossy().into_owned())
}