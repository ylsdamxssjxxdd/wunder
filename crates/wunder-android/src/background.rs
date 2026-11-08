//! Headless schedule tick for 安卓版's `:background` process.
//!
//! The engine's cron scheduler only runs while its process lives. Once the
//! system reclaims the app, a due scheduled job would wait for the next manual
//! launch, so `WunderScheduleReceiver` wakes a dedicated `:background` process
//! every fifteen minutes. That process has no window, no Slint and no
//! `android_host`: it starts a headless `NativeDesktop`, drains whatever is due,
//! and exits. Running it in its own process keeps the UI engine's global state,
//! monitor cache and thread runtime untouched; SQLite WAL plus the cron lease
//! already make claims safe across processes.

#![cfg(target_os = "android")]

use anyhow::{anyhow, Context, Result};
use jni::objects::{JClass, JObject, JString};
use jni::sys::jstring;
use jni::JNIEnv;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// How long one wake may spend on due jobs before the JNI call reports what it
/// finished. A JobScheduler execution has a bounded window, but a wake must not
/// outlive it: unfinished work keeps its lease and is retried by a later wake.
const TICK_TIMEOUT: Duration = Duration::from_secs(300);

/// Longest error text handed back to Java. The reply is logged, not rendered,
/// and a stack of SQLite/IO details only bloats logcat.
const ERROR_MAX_CHARS: usize = 240;

/// `BackgroundRuntime.tick(Context)` in Java. Returns a small JSON object:
/// `{"ok":true,"ran":N}` or `{"ok":false,"error":"..."}`.
#[no_mangle]
pub extern "system" fn Java_dev_wunder_android_BackgroundRuntime_tick(
    mut env: JNIEnv<'_>,
    _class: JClass<'_>,
    context: JObject<'_>,
) -> jstring {
    install_logger();
    // A panic must not escape into ART: an unwinding panic across `extern
    // "system"` aborts the process, and a dead `:background` process would look
    // like a crash loop in the system's wake statistics.
    let payload = match catch_unwind(AssertUnwindSafe(|| tick(&mut env, &context))) {
        Ok(Ok(ran)) => serde_json::json!({ "ok": true, "ran": ran }).to_string(),
        Ok(Err(error)) => error_payload(&describe(&error)),
        Err(panic) => error_payload(&format!("panic: {}", panic_message(panic.as_ref()))),
    };
    // A pending Java exception would poison every later JNI call on this
    // thread, including the `new_string` below.
    clear_pending_exception(&mut env);
    match env.new_string(payload) {
        Ok(text) => text.into_raw(),
        Err(error) => {
            log::error!("background tick could not build the reply: {error}");
            std::ptr::null_mut()
        }
    }
}

/// Start a headless runtime, drain due scheduled jobs, and release it again.
fn tick(env: &mut JNIEnv<'_>, context: &JObject<'_>) -> Result<usize> {
    configure_sandbox(env, context)?;
    let desktop = wunder_desktop::NativeDesktop::start_background()
        .context("start the headless native desktop")?;
    let ran = desktop
        .tick_cron_now(TICK_TIMEOUT)
        .context("run due scheduled jobs")?;
    log::info!("background tick finished: ran={ran}");
    // Dropping the façade releases the thread leases it opened and stops its
    // heartbeat thread before the process exits.
    drop(desktop);
    Ok(ran)
}

/// Point every engine path at the app's private directory, exactly like
/// `android_main` does for the UI process.
fn configure_sandbox(env: &mut JNIEnv<'_>, context: &JObject<'_>) -> Result<PathBuf> {
    let files = context_dir(env, context, "getFilesDir")?;
    let cache = context_dir(env, context, "getCacheDir")?;
    let home = files.join("wunder");
    // The UI process uses `<files>/cache` as TMPDIR; sharing the same directory
    // keeps partially written runtime files reachable across processes. The
    // app's cache directory is only a fallback when that path is unusable.
    let tmp = files.join("cache");
    std::fs::create_dir_all(&home).context("create the wunder home directory")?;
    let tmp = match std::fs::create_dir_all(&tmp) {
        Ok(()) => tmp,
        Err(error) => {
            log::warn!("files/cache unusable ({error}), falling back to the cache dir");
            std::fs::create_dir_all(&cache).context("create the cache directory")?;
            cache
        }
    };
    set_sandbox_env(&home, &tmp);
    Ok(home)
}

fn set_sandbox_env(home: &Path, tmp: &Path) {
    // Android has no `$HOME` and no `/tmp`; the engine resolves config, data and
    // logs from `WUNDER_HOME` only.
    std::env::set_var("WUNDER_HOME", home);
    std::env::set_var("HOME", home);
    std::env::set_var("TMPDIR", tmp);
}

fn context_dir(env: &mut JNIEnv<'_>, context: &JObject<'_>, getter: &str) -> Result<PathBuf> {
    let file = env
        .call_method(context, getter, "()Ljava/io/File;", &[])
        .with_context(|| format!("call Context.{getter}"))?
        .l()
        .with_context(|| format!("read Context.{getter}"))?;
    if file.is_null() {
        return Err(anyhow!("Context.{getter} returned null"));
    }
    let path = env
        .call_method(&file, "getAbsolutePath", "()Ljava/lang/String;", &[])
        .with_context(|| format!("read the path of Context.{getter}"))?
        .l()
        .with_context(|| format!("read the path of Context.{getter}"))?;
    if path.is_null() {
        return Err(anyhow!("Context.{getter} has no absolute path"));
    }
    let text: String = env
        .get_string(&JString::from(path))
        .with_context(|| format!("decode the path of Context.{getter}"))?
        .to_string_lossy()
        .into_owned();
    if text.trim().is_empty() {
        return Err(anyhow!("Context.{getter} returned an empty path"));
    }
    Ok(PathBuf::from(text))
}

/// Outer context plus the root cause: "start the headless native desktop:
/// database is locked" tells an operator what to look at, a full path dump does
/// not.
fn describe(error: &anyhow::Error) -> String {
    let outer = error.to_string();
    match error.chain().last() {
        Some(root) if root.to_string() != outer => format!("{outer}: {root}"),
        _ => outer,
    }
}

fn error_payload(message: &str) -> String {
    let cleaned = message.replace(['\n', '\r', '"', '\\'], " ");
    let mut text = cleaned.trim().to_string();
    if text.chars().count() > ERROR_MAX_CHARS {
        text = text.chars().take(ERROR_MAX_CHARS).collect::<String>() + "…";
    }
    serde_json::json!({ "ok": false, "error": text }).to_string()
}

fn clear_pending_exception(env: &mut JNIEnv<'_>) {
    match env.exception_check() {
        Ok(true) => {
            let _ = env.exception_describe();
            let _ = env.exception_clear();
        }
        // `exception_check` itself failed: there is nothing safe to clear.
        Ok(false) | Err(_) => {}
    }
}

fn panic_message(panic: &(dyn std::any::Any + Send)) -> String {
    if let Some(message) = panic.downcast_ref::<&str>() {
        return (*message).to_string();
    }
    if let Some(message) = panic.downcast_ref::<String>() {
        return message.clone();
    }
    "unknown panic".to_string()
}

/// The `:background` process never runs `android_main`, so nothing initializes
/// logcat for it. Route `log` output to logcat once, under its own tag.
fn install_logger() {
    use std::sync::Once;
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        android_logger::init_once(
            android_logger::Config::default()
                .with_max_level(log::LevelFilter::Info)
                .with_tag("wunder-bg"),
        );
    });
}
