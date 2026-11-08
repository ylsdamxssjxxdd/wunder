//! Local backup and restore.
//!
//! Java's `BackupBridge` owns the archive (config plus the top-level SQLite
//! databases); this module only forwards the call, resolves the app home, and
//! applies a staged restore **before** the engine opens SQLite. Restoring a
//! live database in place would race the running engine, so `restore` stages
//! the archive and the next process start performs the swap.

use anyhow::{anyhow, Context as _, Result};
use jni::objects::{GlobalRef, JString, JValue};
use jni::{JNIEnv, JavaVM};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

const BRIDGE_CLASS: &str = "dev.wunder.android.BackupBridge";
const PENDING_MARKER: &str = "pending";

struct Bridge {
    vm: JavaVM,
    class: GlobalRef,
    context: GlobalRef,
}

static BRIDGE: OnceLock<Option<Bridge>> = OnceLock::new();

/// Result of staging an archive for the next start.
pub enum RestoreOutcome {
    Restart,
}

/// Absolute `WUNDER_HOME`. The shell sets it to `filesDir/wunder` before the
/// engine starts; the fallback derives it from the shared logs directory.
pub(crate) fn home_dir() -> PathBuf {
    if let Some(home) = std::env::var_os("WUNDER_HOME") {
        let path = PathBuf::from(home);
        if !path.as_os_str().is_empty() {
            return path;
        }
    }
    wunder_desktop::local_logs_dir()
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Create a new archive with the Java bridge.
pub fn backup() -> Result<PathBuf> {
    let reply = call_with_context(
        "backup",
        "(Landroid/content/Context;)Ljava/lang/String;",
        "backup",
    )?;
    let reply = strip_failure(reply)?;
    if reply.is_empty() {
        return Err(anyhow!("备份没有返回文件路径"));
    }
    Ok(PathBuf::from(reply))
}

/// Validate and stage an archive. The caller must restart the app.
pub fn restore(zip_path: &str) -> Result<RestoreOutcome> {
    let bridge = bridge().context("the backup bridge is unavailable")?;
    let mut env = bridge.vm.attach_current_thread()?;
    let path = env.new_string(zip_path)?;
    let value = env
        .call_static_method(
            &bridge.class,
            "restore",
            "(Landroid/content/Context;Ljava/lang/String;)Ljava/lang/String;",
            &[
                JValue::Object(bridge.context.as_obj()),
                JValue::Object(&path),
            ],
        )
        .map_err(|error| {
            clear_java_exception(&mut env);
            anyhow!("BackupBridge.restore: {error}")
        })?;
    let reply = strip_failure(string_value(&mut env, value, "restore")?)?;
    match reply.as_str() {
        "restart" => Ok(RestoreOutcome::Restart),
        "" => Err(anyhow!("恢复没有返回结果")),
        other => Err(anyhow!("{other}")),
    }
}

/// Newest archive created by the settings page, if any.
pub fn latest_backup() -> Option<PathBuf> {
    let reply = call_with_context(
        "latestBackup",
        "(Landroid/content/Context;)Ljava/lang/String;",
        "latestBackup",
    )
    .ok()?;
    (!reply.is_empty()).then(|| PathBuf::from(reply))
}

/// Apply a staged restore from a previous session. Must run before SQLite is
/// opened; a partial failure leaves the marker in place for the next start.
pub fn apply_pending() {
    let home = home_dir();
    let staging = home.join(".restore");
    if !staging.join(PENDING_MARKER).is_file() {
        return;
    }
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
    let mut applied = 0usize;
    let mut failed = false;

    let incoming_config = staging.join("config");
    if incoming_config.is_dir() {
        let config = home.join("config");
        // A previous start that died mid-swap may have left the old config
        // aside; keep every such copy instead of overwriting it.
        let mut aside = home.join(format!("config.restore-{stamp}"));
        let mut index = 2;
        while aside.exists() {
            aside = home.join(format!("config.restore-{stamp}-{index}"));
            index += 1;
        }
        let had_config = config.exists();
        let mut moved_aside = false;
        if had_config {
            match std::fs::rename(&config, &aside) {
                Ok(()) => moved_aside = true,
                Err(error) => {
                    log::error!("set aside the current config before restore: {error}");
                    failed = true;
                }
            }
        }
        if !failed {
            match std::fs::rename(&incoming_config, &config) {
                Ok(()) => {
                    applied += 1;
                    if moved_aside {
                        let _ = std::fs::remove_dir_all(&aside);
                    }
                }
                Err(error) => {
                    log::error!("restore the config directory: {error}");
                    if moved_aside {
                        let _ = std::fs::rename(&aside, &config);
                    }
                    failed = true;
                }
            }
        }
    }

    let incoming_data = staging.join("data");
    if !failed && incoming_data.is_dir() {
        let data = home.join("data");
        match std::fs::create_dir_all(&data) {
            Ok(()) => {}
            Err(error) => {
                log::error!("create the data directory for restore: {error}");
                failed = true;
            }
        }
        if !failed {
            match std::fs::read_dir(&incoming_data) {
                Ok(entries) => {
                    for entry in entries.flatten() {
                        let source = entry.path();
                        if !source.is_file() {
                            continue;
                        }
                        let Some(name) = source.file_name().map(|name| name.to_os_string()) else {
                            continue;
                        };
                        let target = data.join(&name);
                        let name = name.to_string_lossy().into_owned();
                        if name.to_ascii_lowercase().ends_with(".sqlite") {
                            let _ = std::fs::remove_file(&target);
                            // The archive carries its own sidecars; stale ones
                            // from the replaced database must not survive.
                            let _ = std::fs::remove_file(data.join(format!("{name}-wal")));
                            let _ = std::fs::remove_file(data.join(format!("{name}-shm")));
                        } else {
                            let _ = std::fs::remove_file(&target);
                        }
                        match std::fs::rename(&source, &target) {
                            Ok(()) => applied += 1,
                            Err(error) => {
                                log::error!("restore {name}: {error}");
                                failed = true;
                                break;
                            }
                        }
                    }
                }
                Err(error) => {
                    log::error!("read the staged data directory: {error}");
                    failed = true;
                }
            }
        }
    }

    if failed {
        log::error!("pending restore is incomplete; it will be retried on the next start");
        return;
    }
    if let Err(error) = std::fs::remove_dir_all(&staging) {
        log::warn!("remove the applied restore staging: {error}");
    }
    log::info!("applied {applied} restored item(s) from the pending backup");
}

fn call_with_context(method: &str, signature: &str, label: &str) -> Result<String> {
    let bridge = bridge().context("the backup bridge is unavailable")?;
    let mut env = bridge.vm.attach_current_thread()?;
    let value = env
        .call_static_method(
            &bridge.class,
            method,
            signature,
            &[JValue::Object(bridge.context.as_obj())],
        )
        .map_err(|error| {
            clear_java_exception(&mut env);
            anyhow!("BackupBridge.{label}: {error}")
        })?;
    string_value(&mut env, value, label)
}

fn string_value<'local>(
    env: &mut JNIEnv<'local>,
    value: jni::objects::JValueOwned<'local>,
    label: &str,
) -> Result<String> {
    let object = value.l().map_err(|error| {
        clear_java_exception(env);
        anyhow!("BackupBridge.{label} returned no object: {error}")
    })?;
    if object.is_null() {
        return Err(anyhow!("BackupBridge.{label} returned null"));
    }
    match env.get_string(&JString::from(object)) {
        Ok(text) => Ok(text.to_string_lossy().into_owned()),
        Err(error) => {
            clear_java_exception(env);
            Err(anyhow!("read BackupBridge.{label}: {error}"))
        }
    }
}

fn strip_failure(reply: String) -> Result<String> {
    match reply.strip_prefix('!') {
        Some(message) => Err(anyhow!("{message}")),
        None => Ok(reply),
    }
}

fn bridge() -> Option<&'static Bridge> {
    BRIDGE
        .get_or_init(|| match install() {
            Ok(bridge) => Some(bridge),
            Err(error) => {
                log::error!("backup bridge unavailable: {error:#}");
                None
            }
        })
        .as_ref()
}

fn install() -> Result<Bridge> {
    let context = ndk_context::android_context();
    // SAFETY: android-activity initialized the process JVM before android_main.
    let vm = unsafe { JavaVM::from_raw(context.vm().cast()) }.context("wrap the Android JavaVM")?;
    let app = crate::android_host::app_context().context("the application context is not ready")?;
    let class = {
        let mut env = vm.attach_current_thread()?;
        let loader = env
            .call_method(
                app.as_obj(),
                "getClassLoader",
                "()Ljava/lang/ClassLoader;",
                &[],
            )
            .and_then(|value| value.l())
            .context("read the application class loader")?;
        let class_name = env.new_string(BRIDGE_CLASS)?;
        let class = env
            .call_method(
                &loader,
                "loadClass",
                "(Ljava/lang/String;)Ljava/lang/Class;",
                &[JValue::Object(&class_name)],
            )
            .and_then(|value| value.l())
            .context("load BackupBridge")?;
        env.new_global_ref(class)?
    };
    Ok(Bridge {
        vm,
        class,
        context: app,
    })
}

fn clear_java_exception(env: &mut JNIEnv<'_>) {
    if env.exception_check().unwrap_or(false) {
        let _ = env.exception_describe();
        let _ = env.exception_clear();
    }
}
