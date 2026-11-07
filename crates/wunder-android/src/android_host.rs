//! Process-scoped Android host.
//!
//! The activity comes and goes (rotation, "don't keep activities", returning
//! from recents after the system destroyed it), but the agent runtime must
//! not. The application context, the foreground service and the wake lock all
//! belong to the process, so the engine and the phone bridge keep working
//! after any single activity is gone.

use anyhow::{Context as _, Result};
use jni::objects::{GlobalRef, JObject, JValue};
use jni::{JNIEnv, JavaVM};
use std::sync::OnceLock;

const SERVICE_CLASS: &str = "dev.wunder.android.WunderRuntimeService";

struct Host {
    vm: JavaVM,
    app: GlobalRef,
    service: GlobalRef,
}

static HOST: OnceLock<Host> = OnceLock::new();

/// Install the host once per process and ask for notification permission on
/// every activity start (the request is idempotent and only prompts while the
/// user has not answered it).
///
/// `activity` is the current NativeActivity jobject borrowed from
/// `AndroidApp::activity_as_ptr`. It must not be wrapped in a GlobalRef we
/// own, and it is only used synchronously here; the process-scoped host keeps
/// the application context instead.
pub fn install(activity: *mut std::ffi::c_void) {
    if let Err(error) = try_install(activity) {
        log::error!("android runtime host unavailable: {error:#}");
    }
    // The service may have stopped itself when the task was removed while
    // idle; a fresh activity must bring it back even though the host is
    // already initialized.
    start_service();
    request_notification_permission(activity);
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

fn try_install(activity: *mut std::ffi::c_void) -> Result<()> {
    if HOST.get().is_some() {
        return Ok(());
    }
    let context = ndk_context::android_context();
    // SAFETY: android-activity initialized the process JVM before android_main.
    let vm = unsafe { JavaVM::from_raw(context.vm().cast()) }
        .context("wrap the Android JavaVM")?;
    let mut env = vm.attach_current_thread()?;
    let outcome = install_with_env(&mut env, activity);
    if outcome.is_err() {
        clear_java_exception(&mut env);
    }
    drop(env);
    let (app, service) = outcome?;
    let _ = HOST.set(Host { vm, app, service });
    log::info!("android runtime host installed");
    Ok(())
}

fn install_with_env(
    env: &mut JNIEnv<'_>,
    activity: *mut std::ffi::c_void,
) -> Result<(GlobalRef, GlobalRef)> {
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
    let name = env.new_string(SERVICE_CLASS)?;
    let class = env
        .call_method(
            &loader,
            "loadClass",
            "(Ljava/lang/String;)Ljava/lang/Class;",
            &[JValue::Object(&name)],
        )
        .and_then(|value| value.l())
        .context("load WunderRuntimeService")?;
    let service = env.new_global_ref(class)?;

    Ok((app, service))
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
