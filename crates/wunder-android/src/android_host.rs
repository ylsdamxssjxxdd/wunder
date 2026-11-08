//! Process-scoped Android host.
//!
//! The activity comes and goes (rotation, "don't keep activities", returning
//! from recents after the system destroyed it), but the agent runtime must
//! not. The application context, the foreground service and the wake lock all
//! belong to the process, so the engine and the phone bridge keep working
//! after any single activity is gone.

use anyhow::{Context as _, Result};
use jni::objects::{GlobalRef, JObject, JObjectArray, JString, JValue};
use jni::{JNIEnv, JavaVM};
use std::sync::{Mutex, OnceLock};

const SERVICE_CLASS: &str = "dev.wunder.android.WunderRuntimeService";
const UI_BRIDGE_CLASS: &str = "dev.wunder.android.UiBridge";

struct Host {
    vm: JavaVM,
    app: GlobalRef,
    service: GlobalRef,
    ui_bridge: GlobalRef,
}

static HOST: OnceLock<Host> = OnceLock::new();

/// What the system document picker was opened for. The picker runs in another
/// activity, so the target travels with the selection instead of being
/// re-read after the user returns.
#[derive(Clone, Debug)]
enum PickTarget {
    /// Upload files into this workspace-relative directory.
    Workspace(String),
    /// Upload text files into the tool resource the phone is browsing.
    ToolFiles,
    /// Import one skill archive (first selected file).
    SkillArchive,
}

static PENDING_PICK: Mutex<Option<PickTarget>> = Mutex::new(None);

/// One result of the system document picker, routed by the requester.
pub enum PickedFiles {
    Workspace { dir: String, files: Vec<String> },
    ToolFiles(Vec<String>),
    SkillArchive(String),
}

/// Install the host once per process and ask for notification permission on
/// every activity start (the request is idempotent and only prompts while the
/// user has not answered it).
///
/// `activity` is the current NativeActivity jobject borrowed from
/// `AndroidApp::activity_as_ptr`. It must not be wrapped in a GlobalRef we
/// own, and it is only used synchronously here; the process-scoped host keeps
/// the application context instead.
pub fn install(activity: *mut std::ffi::c_void) {
    if let Err(first) = try_install(activity) {
        // A fresh process can hit a window where ART reports a pending
        // exception with no exception object behind it; one retry has been
        // enough to get past it on first launch.
        if let Err(error) = try_install(activity) {
            log::error!("android runtime host unavailable: {error:#} (first attempt: {first:#})");
        }
    }
    attach_activity(activity);
    // The service may have stopped itself when the task was removed while
    // idle; a fresh activity must bring it back even though the host is
    // already initialized.
    start_service();
    request_notification_permission(activity);
}

/// Forget the previous activity and register the one that just started.
fn attach_activity(activity: *mut std::ffi::c_void) {
    let Some(host) = HOST.get() else {
        return;
    };
    let Ok(mut env) = host.vm.attach_current_thread() else {
        return;
    };
    // SAFETY: borrowed from AndroidApp for this synchronous call.
    let activity = unsafe { JObject::from_raw(activity.cast()) };
    let result = env.call_static_method(
        &host.ui_bridge,
        "attach",
        "(Landroid/app/Activity;)V",
        &[JValue::Object(&activity)],
    );
    if result.is_err() {
        clear_java_exception(&mut env);
    }
}

/// Dismiss the soft keyboard. Safe from any thread; the Java side re-posts.
pub fn hide_keyboard() {
    let Some(host) = HOST.get() else {
        return;
    };
    let Ok(mut env) = host.vm.attach_current_thread() else {
        return;
    };
    let result = env.call_static_method(&host.ui_bridge, "hideKeyboard", "()V", &[]);
    if result.is_err() {
        clear_java_exception(&mut env);
    }
}

/// Drain Back presses recorded by the activity. The UI tick turns them into
/// page navigation; a second Back on the chat root closes the activity.
pub fn take_back_requests() -> i32 {
    let Some(host) = HOST.get() else {
        return 0;
    };
    let Ok(mut env) = host.vm.attach_current_thread() else {
        return 0;
    };
    match env.call_static_method(&host.ui_bridge, "takeBackRequests", "()I", &[]) {
        Ok(value) => value.i().unwrap_or(0).max(0),
        Err(error) => {
            clear_java_exception(&mut env);
            log::error!("take back requests: {error}");
            0
        }
    }
}

/// Close the visible activity after the shell accepted a Back-to-exit. The
/// engine and its foreground service are process-scoped and settle on their
/// own; only the window goes away here.
pub fn finish_activity() {
    let Some(host) = HOST.get() else {
        return;
    };
    let Ok(mut env) = host.vm.attach_current_thread() else {
        return;
    };
    if let Err(error) = env.call_static_method(&host.ui_bridge, "finishActivity", "()V", &[]) {
        clear_java_exception(&mut env);
        log::error!("finish activity: {error}");
    }
}

fn open_picker(target: PickTarget) {
    if let Ok(mut pending) = PENDING_PICK.lock() {
        *pending = Some(target);
    }
    let Some(host) = HOST.get() else {
        return;
    };
    let Ok(mut env) = host.vm.attach_current_thread() else {
        return;
    };
    if let Err(error) = env.call_static_method(&host.ui_bridge, "pickFiles", "()V", &[]) {
        clear_java_exception(&mut env);
        log::error!("open the file picker: {error}");
    }
}

/// Open the system document picker for the workspace at `dir`.
pub fn pick_files(dir: &str) {
    open_picker(PickTarget::Workspace(dir.to_string()));
}

/// Open the picker for text files of the tool resource being browsed.
pub fn pick_tool_files() {
    open_picker(PickTarget::ToolFiles);
}

/// Open the picker for one skill archive.
pub fn pick_skill_archive() {
    open_picker(PickTarget::SkillArchive);
}

/// Open the system folder picker for a workspace root.
pub fn pick_directory() {
    let Some(host) = HOST.get() else {
        return;
    };
    let Ok(mut env) = host.vm.attach_current_thread() else {
        return;
    };
    if let Err(error) = env.call_static_method(&host.ui_bridge, "pickDirectory", "()V", &[]) {
        clear_java_exception(&mut env);
        log::error!("open the folder picker: {error}");
    }
}

/// Poll the folder picker result. Java returns null until the system answers;
/// an empty value means the user cancelled.
pub fn take_picked_directory() -> Option<Result<String, String>> {
    let host = HOST.get()?;
    let Ok(mut env) = host.vm.attach_current_thread() else {
        return None;
    };
    let value = match env.call_static_method(
        &host.ui_bridge,
        "takePickedDirectory",
        "()Ljava/lang/String;",
        &[],
    ) {
        Ok(value) => value,
        Err(error) => {
            clear_java_exception(&mut env);
            log::error!("take the picked directory: {error}");
            return None;
        }
    };
    let Ok(object) = value.l() else {
        return None;
    };
    if object.is_null() {
        return None;
    }
    let object = JString::from(object);
    let Ok(path) = env.get_string(&object) else {
        return None;
    };
    let path = path.to_string_lossy().into_owned();
    if path.is_empty() {
        return None;
    }
    match path.strip_prefix('!') {
        Some(message) => Some(Err(message.to_string())),
        None => Some(Ok(path)),
    }
}

/// Poll the filenames staged by the picker. Java must not call native code
/// (NativeActivity loads the library through the framework class loader), so
/// the UI tick pulls the result instead. An empty result still consumes the
/// pending target, which covers the user cancelling the picker.
pub fn take_picked() -> Option<PickedFiles> {
    // Peek first: Java answers null while its copy thread is still staging the
    // selection, and consuming the target then would drop the eventual result.
    let target = PENDING_PICK.lock().ok().and_then(|pending| pending.clone())?;
    let Some(host) = HOST.get() else {
        return None;
    };
    let Ok(mut env) = host.vm.attach_current_thread() else {
        return None;
    };
    let value = match env.call_static_method(
        &host.ui_bridge,
        "takePickedFiles",
        "()[Ljava/lang/String;",
        &[],
    ) {
        Ok(value) => value,
        Err(error) => {
            clear_java_exception(&mut env);
            log::error!("take picked files: {error}");
            return None;
        }
    };
    let Ok(object) = value.l() else {
        return None;
    };
    if object.is_null() {
        return None;
    }
    let array = JObjectArray::from(object);
    let mut files = Vec::new();
    let count = env.get_array_length(&array).unwrap_or(0);
    for index in 0..count {
        let Ok(element) = env.get_object_array_element(&array, index) else {
            continue;
        };
        if element.is_null() {
            continue;
        }
        if let Ok(path) = env.get_string(&JString::from(element)) {
            files.push(path.to_string_lossy().into_owned());
        }
    }
    // The picker answered (including "cancelled" as an empty list); the target
    // must not leak into the next pick.
    PENDING_PICK.lock().ok().and_then(|mut pending| pending.take());
    if files.is_empty() {
        return None;
    }
    match target {
        PickTarget::Workspace(dir) => Some(PickedFiles::Workspace { dir, files }),
        PickTarget::ToolFiles => Some(PickedFiles::ToolFiles(files)),
        PickTarget::SkillArchive => Some(PickedFiles::SkillArchive(files.remove(0))),
    }
}

/// Poll the legacy save dialog result. Android 10+ saves synchronously and
/// never lands here.
pub fn take_save_results() -> Vec<(bool, String)> {
    let Some(host) = HOST.get() else {
        return Vec::new();
    };
    let Ok(mut env) = host.vm.attach_current_thread() else {
        return Vec::new();
    };
    let value = match env.call_static_method(
        &host.ui_bridge,
        "takeSaveResult",
        "()Ljava/lang/String;",
        &[],
    ) {
        Ok(value) => value,
        Err(error) => {
            clear_java_exception(&mut env);
            log::error!("take the save result: {error}");
            return Vec::new();
        }
    };
    let Ok(object) = value.l() else {
        return Vec::new();
    };
    if object.is_null() {
        return Vec::new();
    }
    let text = env
        .get_string(&JString::from(object))
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_default();
    let (flag, name) = text.split_once('|').unwrap_or(("0", ""));
    vec![(flag == "1", name.to_string())]
}

/// Start a voice clip. Returns `recording`, `permission` (system prompt
/// opened; the granted callback starts capture) or `failed:<reason>`.
pub fn start_voice() -> String {
    call_string("startVoice")
}

/// Ask the capture thread to stop. The WAV path arrives through
/// [`take_voice_events`].
pub fn stop_voice() -> String {
    call_string("stopVoice")
}

/// Drain capture events: `recording`, `denied`, `done|<path>` or
/// `error|<message>`.
pub fn take_voice_events() -> Vec<String> {
    call_string_array("takeVoiceEvents")
}

/// Capture the current screen. Returns `pending` (the result arrives through
/// [`take_screenshot_events`]) or `failed:<reason>`.
pub fn capture_screenshot() -> String {
    call_string("captureScreenshot")
}

/// Drain screenshot results: `ok|<path>`, `no_service` or `error|<message>`.
pub fn take_screenshot_events() -> Vec<String> {
    call_string_array("takeScreenshotEvents")
}

/// Raw permission states as one JSON object. The settings page maps them to
/// user-facing labels; an unavailable activity yields an empty object.
pub fn permission_snapshot() -> String {
    call_string("permissionSnapshot")
}

/// Open the system settings surface for one managed permission.
pub fn open_permission(id: &str) {
    let Some(host) = HOST.get() else {
        return;
    };
    let Ok(mut env) = host.vm.attach_current_thread() else {
        return;
    };
    let Ok(id) = env.new_string(id) else {
        return;
    };
    let result = env.call_static_method(
        &host.ui_bridge,
        "openPermission",
        "(Ljava/lang/String;)V",
        &[JValue::Object(&id)],
    );
    if result.is_err() {
        clear_java_exception(&mut env);
    }
}

/// Install time of the current APK, in Unix milliseconds. The Android update
/// check compares a same-version nightly release against this value.
pub fn installed_at_millis() -> i64 {
    let Some(host) = HOST.get() else {
        return 0;
    };
    let Ok(mut env) = host.vm.attach_current_thread() else {
        return 0;
    };
    match env.call_static_method(&host.ui_bridge, "installedAtMillis", "()J", &[]) {
        Ok(value) => value.j().unwrap_or(0),
        Err(error) => {
            clear_java_exception(&mut env);
            log::error!("read installed-at time: {error}");
            0
        }
    }
}

/// Drain the terminal status of the last package-install session.
pub fn take_install_result() -> Option<String> {
    let result = call_string("takeInstallResult");
    (!result.is_empty()).then_some(result)
}

/// Hand a verified update APK to the system package installer. Returns
/// `started`, `permission` or `failed:<reason>`.
pub fn install_apk(path: &str) -> String {
    let Some(host) = HOST.get() else {
        return "failed:no host".into();
    };
    let Ok(mut env) = host.vm.attach_current_thread() else {
        return "failed:jvm".into();
    };
    let result = (|| -> anyhow::Result<String> {
        let path = env.new_string(path)?;
        let value = env.call_static_method(
            &host.ui_bridge,
            "installApk",
            "(Ljava/lang/String;)Ljava/lang/String;",
            &[JValue::Object(&path)],
        )?;
        let object = value.l()?;
        if object.is_null() {
            return Ok("failed:null".into());
        }
        Ok(env
            .get_string(&JString::from(object))?
            .to_string_lossy()
            .into_owned())
    })();
    match result {
        Ok(value) => value,
        Err(error) => {
            clear_java_exception(&mut env);
            log::error!("install Android update: {error:#}");
            "failed:jni".into()
        }
    }
}

fn call_string(method: &str) -> String {
    let Some(host) = HOST.get() else {
        return "failed:no host".into();
    };
    let Ok(mut env) = host.vm.attach_current_thread() else {
        return "failed:jvm".into();
    };
    let result = (|| -> Result<String> {
        let value =
            env.call_static_method(&host.ui_bridge, method, "()Ljava/lang/String;", &[])?;
        let object = value.l()?;
        if object.is_null() {
            return Ok(String::new());
        }
        Ok(env
            .get_string(&JString::from(object))?
            .to_string_lossy()
            .into_owned())
    })();
    match result {
        Ok(value) => value,
        Err(error) => {
            clear_java_exception(&mut env);
            log::error!("call UiBridge.{method}: {error:#}");
            "failed:jni".into()
        }
    }
}

fn call_string_array(method: &str) -> Vec<String> {
    let Some(host) = HOST.get() else {
        return Vec::new();
    };
    let Ok(mut env) = host.vm.attach_current_thread() else {
        return Vec::new();
    };
    let value = match env.call_static_method(
        &host.ui_bridge,
        method,
        "()[Ljava/lang/String;",
        &[],
    ) {
        Ok(value) => value,
        Err(error) => {
            clear_java_exception(&mut env);
            log::error!("call UiBridge.{method}: {error}");
            return Vec::new();
        }
    };
    let Ok(object) = value.l() else {
        return Vec::new();
    };
    if object.is_null() {
        return Vec::new();
    }
    let array = JObjectArray::from(object);
    let mut events = Vec::new();
    let count = env.get_array_length(&array).unwrap_or(0);
    for index in 0..count {
        let Ok(element) = env.get_object_array_element(&array, index) else {
            continue;
        };
        if element.is_null() {
            continue;
        }
        if let Ok(text) = env.get_string(&JString::from(element)) {
            events.push(text.to_string_lossy().into_owned());
        }
    }
    events
}

/// Copy one workspace file out of the sandbox. Returns `saved`, `pending`
/// (legacy save dialog opened) or `failed:<reason>`.
pub fn save_file_to_phone(source_path: &str, display_name: &str) -> String {
    let Some(host) = HOST.get() else {
        return "failed:no host".into();
    };
    let Ok(mut env) = host.vm.attach_current_thread() else {
        return "failed:jvm".into();
    };
    let result = (|| -> Result<String> {
        let source = env.new_string(source_path)?;
        let name = env.new_string(display_name)?;
        let value = env.call_static_method(
            &host.ui_bridge,
            "saveFile",
            "(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;",
            &[JValue::Object(&source), JValue::Object(&name)],
        )?;
        let object = value.l()?;
        if object.is_null() {
            return Ok("failed:null".into());
        }
        Ok(env
            .get_string(&JString::from(object))?
            .to_string_lossy()
            .into_owned())
    })();
    match result {
        Ok(status) => status,
        Err(error) => {
            clear_java_exception(&mut env);
            log::error!("save a workspace file: {error:#}");
            "failed:jni".into()
        }
    }
}

/// Human-readable phone model used as the cloud device label.
pub fn device_name() -> Option<String> {
    let host = HOST.get()?;
    let mut env = host.vm.attach_current_thread().ok()?;
    let result = (|| -> Result<String> {
        let value = env.call_static_method(
            &host.ui_bridge,
            "deviceName",
            "()Ljava/lang/String;",
            &[],
        )?;
        let object = value.l()?;
        if object.is_null() {
            return Ok(String::new());
        }
        Ok(env
            .get_string(&JString::from(object))?
            .to_string_lossy()
            .into_owned())
    })();
    match result {
        Ok(name) if !name.trim().is_empty() => Some(name.trim().to_string()),
        Ok(_) => None,
        Err(error) => {
            clear_java_exception(&mut env);
            log::warn!("read the Android device name: {error:#}");
            None
        }
    }
}

/// The application context. Stable for the whole process, unlike an Activity.
pub fn app_context() -> Option<GlobalRef> {
    HOST.get().map(|host| host.app.clone())
}

/// Run `f` on the current thread with the JNI env and application context.
/// Used by the process-scoped helpers (phone bridge, Ubuntu runtime) that need
/// the app class loader; a Java exception left behind by `f` is cleared so it
/// cannot leak into the next JNI call.
pub fn with_app<R>(f: impl FnOnce(&mut JNIEnv<'_>, &JObject<'_>) -> R) -> Option<R> {
    let host = HOST.get()?;
    let mut env = host.vm.attach_current_thread().ok()?;
    let result = f(&mut env, host.app.as_obj());
    if env.exception_check().unwrap_or(false) {
        clear_java_exception(&mut env);
    }
    Some(result)
}

/// Reflect the engine's busy state into the foreground notification and hold a
/// wake lock while a turn runs. Called from engine threads; a failure only
/// costs the notification text, never the turn.
pub fn set_busy(busy: bool, title: &str) {
    let Some(host) = HOST.get() else {
        return;
    };
    let Ok(mut env) = host.vm.attach_current_thread() else {
        return;
    };
    let Ok(text) = env.new_string(title) else {
        return;
    };
    let result = env.call_static_method(
        &host.service,
        "setBusy",
        "(Landroid/content/Context;ZLjava/lang/String;)V",
        &[
            JValue::Object(host.app.as_obj()),
            JValue::Bool(busy as u8),
            JValue::Object(&text),
        ],
    );
    if result.is_err() {
        clear_java_exception(&mut env);
    }
}

/// Replace the notification's live phase/detail while the turn keeps running.
/// The service keeps its foreground notification and wake lock; only the text
/// and chronometer projection changes.
pub fn set_turn_status(phase: &str, detail: &str) {
    let Some(host) = HOST.get() else {
        return;
    };
    let Ok(mut env) = host.vm.attach_current_thread() else {
        return;
    };
    let Ok(phase) = env.new_string(phase) else {
        return;
    };
    let Ok(detail) = env.new_string(detail) else {
        return;
    };
    let result = env.call_static_method(
        &host.service,
        "setTurnStatus",
        "(Landroid/content/Context;Ljava/lang/String;Ljava/lang/String;)V",
        &[
            JValue::Object(host.app.as_obj()),
            JValue::Object(&phase),
            JValue::Object(&detail),
        ],
    );
    if result.is_err() {
        clear_java_exception(&mut env);
    }
}

fn try_install(activity: *mut std::ffi::c_void) -> Result<()> {
    if HOST.get().is_some() {
        return Ok(());
    }
    let context = ndk_context::android_context();
    // SAFETY: android-activity initialized the process JVM before android_main.
    let vm = unsafe { JavaVM::from_raw(context.vm().cast()) }.context("wrap the Android JavaVM")?;
    let mut env = vm.attach_current_thread()?;
    let outcome = install_with_env(&mut env, activity);
    if outcome.is_err() {
        clear_java_exception(&mut env);
    }
    drop(env);
    let (app, service, ui_bridge) = outcome?;
    let _ = HOST.set(Host {
        vm,
        app,
        service,
        ui_bridge,
    });
    log::info!("android runtime host installed");
    Ok(())
}

fn install_with_env(
    env: &mut JNIEnv<'_>,
    activity: *mut std::ffi::c_void,
) -> Result<(GlobalRef, GlobalRef, GlobalRef)> {
    // SAFETY: borrowed from AndroidApp; valid for this synchronous call.
    let activity = unsafe { JObject::from_raw(activity.cast()) };
    let app = env
        .call_method(
            &activity,
            "getApplicationContext",
            "()Landroid/content/Context;",
            &[],
        )
        .and_then(|value| value.l())
        .context("read the application context")?;
    let app = env.new_global_ref(app)?;

    let loader = env
        .call_method(
            app.as_obj(),
            "getClassLoader",
            "()Ljava/lang/ClassLoader;",
            &[],
        )
        .and_then(|value| value.l())
        .context("read the application class loader")?;
    let load = |env: &mut JNIEnv<'_>, class_name: &str| -> Result<GlobalRef> {
        let name = env.new_string(class_name)?;
        let class = env
            .call_method(
                &loader,
                "loadClass",
                "(Ljava/lang/String;)Ljava/lang/Class;",
                &[JValue::Object(&name)],
            )
            .and_then(|value| value.l())
            .with_context(|| format!("load {class_name}"))?;
        Ok(env.new_global_ref(class)?)
    };
    let service = load(env, SERVICE_CLASS)?;
    let ui_bridge = load(env, UI_BRIDGE_CLASS)?;

    Ok((app, service, ui_bridge))
}

fn start_service() {
    let Some(host) = HOST.get() else {
        return;
    };
    let Ok(mut env) = host.vm.attach_current_thread() else {
        return;
    };
    let result = env.call_static_method(
        &host.service,
        "start",
        "(Landroid/content/Context;)V",
        &[JValue::Object(host.app.as_obj())],
    );
    if result.is_err() {
        clear_java_exception(&mut env);
    }
}

fn request_notification_permission(activity: *mut std::ffi::c_void) {
    let Some(host) = HOST.get() else {
        return;
    };
    let Ok(mut env) = host.vm.attach_current_thread() else {
        return;
    };
    // SAFETY: borrowed from AndroidApp; valid for this synchronous call.
    let activity = unsafe { JObject::from_raw(activity.cast()) };
    let result = env.call_static_method(
        &host.service,
        "requestNotificationPermission",
        "(Landroid/app/Activity;)V",
        &[JValue::Object(&activity)],
    );
    if result.is_err() {
        clear_java_exception(&mut env);
    }
}

fn clear_java_exception(env: &mut JNIEnv<'_>) {
    if env.exception_check().unwrap_or(false) {
        let _ = env.exception_describe();
        let _ = env.exception_clear();
    }
}
