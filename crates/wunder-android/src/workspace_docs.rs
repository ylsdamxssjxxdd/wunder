//! Workspace catalogue for the system DocumentsProvider.
//!
//! The provider (Java) is read-only and knows only what the engine publishes:
//! the id, display name and root folder of every workspace. The payload is
//! tiny and changes rarely, so an unchanged catalogue is not pushed again.

use anyhow::{Context as _, Result};
use jni::objects::{GlobalRef, JValue};
use jni::{JNIEnv, JavaVM};
use serde_json::json;
use std::sync::{Mutex, OnceLock};
use wunder_desktop::NativeWorkspace;

const REGISTRY_CLASS: &str = "dev.wunder.android.WorkspaceRegistry";

struct Registry {
    vm: JavaVM,
    class: GlobalRef,
}

static REGISTRY: OnceLock<Option<Registry>> = OnceLock::new();
/// Last payload Java acknowledged; guarded so a no-op refresh stays free.
static LAST: Mutex<Option<String>> = Mutex::new(None);

/// Push the catalogue after a refresh. Failures are non-fatal: the provider
/// simply keeps its previous snapshot (or stays empty).
pub fn publish(workspaces: &[NativeWorkspace]) {
    let payload = json!({
        "workspaces": workspaces
            .iter()
            .map(|workspace| json!({
                "id": workspace.workspace_id,
                "name": workspace.name,
                "root": workspace.root_path,
            }))
            .collect::<Vec<_>>(),
    })
    .to_string();
    let unchanged = LAST
        .lock()
        .map(|last| last.as_deref() == Some(payload.as_str()))
        .unwrap_or(false);
    if unchanged {
        return;
    }
    match update(&payload) {
        Ok(()) => {
            if let Ok(mut last) = LAST.lock() {
                *last = Some(payload);
            }
        }
        Err(error) => log::warn!("publish workspaces to the document provider: {error:#}"),
    }
}

fn update(payload: &str) -> Result<()> {
    let registry = registry().context("the workspace registry is unavailable")?;
    let mut env = registry.vm.attach_current_thread()?;
    let text = env.new_string(payload)?;
    if let Err(error) = env.call_static_method(
        &registry.class,
        "update",
        "(Ljava/lang/String;)V",
        &[JValue::Object(&text)],
    ) {
        clear_java_exception(&mut env);
        return Err(anyhow::anyhow!("WorkspaceRegistry.update: {error}"));
    }
    Ok(())
}

fn registry() -> Option<&'static Registry> {
    REGISTRY
        .get_or_init(|| match install() {
            Ok(registry) => Some(registry),
            Err(error) => {
                log::error!("workspace document registry unavailable: {error:#}");
                None
            }
        })
        .as_ref()
}

fn install() -> Result<Registry> {
    let context = ndk_context::android_context();
    // SAFETY: android-activity initialized the process JVM before android_main.
    let vm = unsafe { JavaVM::from_raw(context.vm().cast()) }.context("wrap the Android JavaVM")?;
    let class = {
        let mut env = vm.attach_current_thread()?;
        let app =
            crate::android_host::app_context().context("the application context is not ready")?;
        let loader = env
            .call_method(
                app.as_obj(),
                "getClassLoader",
                "()Ljava/lang/ClassLoader;",
                &[],
            )
            .and_then(|value| value.l())
            .context("read the application class loader")?;
        let class_name = env.new_string(REGISTRY_CLASS)?;
        let class = env
            .call_method(
                &loader,
                "loadClass",
                "(Ljava/lang/String;)Ljava/lang/Class;",
                &[JValue::Object(&class_name)],
            )
            .and_then(|value| value.l())
            .context("load WorkspaceRegistry")?;
        env.new_global_ref(class)?
    };
    Ok(Registry { vm, class })
}

fn clear_java_exception(env: &mut JNIEnv<'_>) {
    if env.exception_check().unwrap_or(false) {
        let _ = env.exception_describe();
        let _ = env.exception_clear();
    }
}
