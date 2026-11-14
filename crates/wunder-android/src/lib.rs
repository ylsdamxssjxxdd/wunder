//! 安卓版 — the Android shell for wunder.
//!
//! The crate is a `cdylib` because Android loads it through NativeActivity.
//! `android_main` is the process entry; everything below the shell is the same
//! native engine 桌面版 uses (`wunder-desktop` → `wunder-runtime` → `wunder-core`).

slint::include_modules!();

#[cfg(target_os = "android")]
mod android_host;
mod app;
#[cfg(target_os = "android")]
mod background;
#[cfg(target_os = "android")]
mod backup;
mod channels;
mod cloud;
#[cfg(target_os = "android")]
mod companion;
#[cfg(target_os = "android")]
mod crash;
mod engine;
mod fonts;
mod help;
#[cfg(target_os = "android")]
mod intent;
mod md;
#[cfg(target_os = "android")]
mod notifications;
mod permissions;
#[cfg(target_os = "android")]
mod phone;
mod project;
#[cfg(target_os = "android")]
mod screen;
mod ubuntu;
mod update;
#[cfg(target_os = "android")]
mod webview;
#[cfg(target_os = "android")]
mod workspace_docs;

pub use app::{build, build_preview, open_preview_page};

#[cfg(target_os = "android")]
#[no_mangle]
// `AndroidApp` is Slint's entry payload; the backend resolves this exact
// signature from NativeActivity, so the lint is not actionable here.
#[allow(improper_ctypes_definitions)]
pub extern "C" fn android_main(app: slint::android::AndroidApp) {
    android_logger::init_once(
        android_logger::Config::default()
            .with_max_level(log::LevelFilter::Info)
            .with_tag("wunder"),
    );

    // Android has neither `$HOME` nor `/tmp` in the app sandbox. The engine
    // resolves every config/data/log path from `WUNDER_HOME`, so point it at
    // the app's private directory before anything touches the filesystem.
    if let Some(data) = app.internal_data_path() {
        let home = data.join("wunder");
        let _ = std::fs::create_dir_all(&home);
        std::env::set_var("WUNDER_HOME", &home);
        std::env::set_var("HOME", &home);
        let cache = data.join("cache");
        let _ = std::fs::create_dir_all(&cache);
        std::env::set_var("TMPDIR", &cache);
    }

    // Rust panics on engine and stream workers must leave a bounded report
    // under WUNDER_HOME/logs; there is no console on a release device.
    #[cfg(target_os = "android")]
    crash::install();

    // `ndk_context` publishes the Application, not the Activity; JNI methods
    // such as requestPermissions need the Activity, which only the AndroidApp
    // borrows. Capture it before init consumes the app.
    let activity = app.activity_as_ptr();
    if let Err(error) = slint::android::init(app) {
        eprintln!("slint android init failed: {error}");
        return;
    }
    init_tls_verifier();
    // The runtime host is process-scoped: it survives this activity, so a
    // background turn keeps its foreground service and wake lock when the
    // window goes away. Install it before the engine starts so the first turn
    // already has a device host and a notification.
    android_host::install(activity);
    let ui = app::build();
    if let Err(error) = ui.run() {
        eprintln!("wunder android event loop failed: {error}");
    }
}

/// rustls verifies HTTPS through Android's trust store, but its JNI bridge is
/// inert until the shell hands it the activity context. The first model call
/// otherwise panics on a Tokio worker, and the turn ends with no reply and no
/// error. The verifier's 0.7 line uses jni 0.22; the phone bridge stays on
/// 0.21, so the two never share types.
#[cfg(target_os = "android")]
fn init_tls_verifier() {
    let context = ndk_context::android_context();
    // SAFETY: android-activity initializes the process JVM before android_main.
    let vm = unsafe { jni022::JavaVM::from_raw(context.vm().cast()) };
    let result = vm.attach_current_thread_for_scope(|env| {
        // SAFETY: the activity's global reference lives as long as the process.
        let activity = unsafe { jni022::objects::JObject::from_raw(env, context.context().cast()) };
        rustls_platform_verifier::android::init_with_env(env, activity)
    });
    if let Err(error) = result {
        log::error!("rustls platform verifier unavailable: {error:#}");
    }
}
