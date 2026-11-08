//! System share / VIEW intake.
//!
//! Java owns intent handling: the activity receives a share before the Rust
//! engine exists on a cold start, and file copies must not block the UI
//! thread. This module only drains the static queue from the render tick and
//! decodes the JSON record into typed values for the engine.

use anyhow::{Context as _, Result};
use jni::objects::{GlobalRef, JString, JValue};
use jni::{JNIEnv, JavaVM};
use serde_json::Value;
use std::sync::OnceLock;

const BRIDGE_CLASS: &str = "dev.wunder.android.IntentBridge";
const MAX_FILES: usize = 8;

/// One normalised share the engine should act on.
pub enum SharedIntent {
    /// Shared text body, already bounded by Java.
    Text(String),
    /// An http(s) link opened with ACTION_VIEW.
    Link(String),
    /// Files copied into the app sandbox, plus an optional shared caption.
    Files {
        files: Vec<String>,
        text: Option<String>,
    },
}

struct Bridge {
    vm: JavaVM,
    class: GlobalRef,
}

static BRIDGE: OnceLock<Option<Bridge>> = OnceLock::new();

/// Drain one queued share. `None` also covers "Java has not delivered one yet".
pub fn take() -> Option<SharedIntent> {
    let bridge = bridge()?;
    let mut env = match bridge.vm.attach_current_thread() {
        Ok(env) => env,
        Err(error) => {
            log::warn!("attach for a shared intent: {error}");
            return None;
        }
    };
    let value = match env.call_static_method(
        &bridge.class,
        "takeSharedIntent",
        "()Ljava/lang/String;",
        &[],
    ) {
        Ok(value) => value,
        Err(error) => {
            clear_java_exception(&mut env);
            log::warn!("take the shared intent: {error}");
            return None;
        }
    };
    let Ok(object) = value.l() else {
        clear_java_exception(&mut env);
        return None;
    };
    if object.is_null() {
        return None;
    }
    let raw = match env.get_string(&JString::from(object)) {
        Ok(text) => text.to_string_lossy().into_owned(),
        Err(error) => {
            clear_java_exception(&mut env);
            log::warn!("read the shared intent record: {error}");
            return None;
        }
    };
    decode(&raw)
}

fn decode(raw: &str) -> Option<SharedIntent> {
    let value: Value = serde_json::from_str(raw).ok()?;
    match value.get("kind")?.as_str()? {
        "text" => {
            let text = value.get("text")?.as_str()?.to_string();
            (!text.trim().is_empty()).then_some(SharedIntent::Text(text))
        }
        "view" => {
            let url = value.get("url")?.as_str()?.to_string();
            (!url.trim().is_empty()).then_some(SharedIntent::Link(url))
        }
        "files" => {
            let files = value
                .get("files")?
                .as_array()?
                .iter()
                .filter_map(Value::as_str)
                .filter(|path| !path.is_empty())
                .take(MAX_FILES)
                .map(str::to_string)
                .collect::<Vec<_>>();
            if files.is_empty() {
                return None;
            }
            let text = value
                .get("text")
                .and_then(Value::as_str)
                .map(str::to_string)
                .filter(|text| !text.trim().is_empty());
            Some(SharedIntent::Files { files, text })
        }
        _ => None,
    }
}

fn bridge() -> Option<&'static Bridge> {
    BRIDGE
        .get_or_init(|| match install() {
            Ok(bridge) => Some(bridge),
            Err(error) => {
                log::error!("shared intent bridge unavailable: {error:#}");
                None
            }
        })
        .as_ref()
}

fn install() -> Result<Bridge> {
    let context = ndk_context::android_context();
    // SAFETY: android-activity initialized the process JVM before android_main.
    let vm = unsafe { JavaVM::from_raw(context.vm().cast()) }.context("wrap the Android JavaVM")?;
    let class = {
        let mut env = vm.attach_current_thread()?;
        let app =
            crate::android_host::app_context().context("the application context is not ready")?;
        // FindClass on a native worker resolves the system loader; app classes
        // must be loaded through the application's own loader.
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
            .context("load IntentBridge")?;
        env.new_global_ref(class)?
    };
    Ok(Bridge { vm, class })
}

fn clear_java_exception(env: &mut JNIEnv<'_>) {
    if env.exception_check().unwrap_or(false) {
        let _ = env.exception_describe();
        let _ = env.exception_clear();
    }
}
