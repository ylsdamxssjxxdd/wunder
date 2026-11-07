//! Facade over the bundled proot Ubuntu runtime (`UbuntuRuntime.java`).
//!
//! Installation, extraction and the proot probe live on the Java side because
//! they need the AssetManager and a background thread that survives the UI
//! view. Rust only starts the install and polls a compact snapshot; the engine
//! turns that snapshot into the settings card and points `WUNDER_SHELL_BIN`
//! at the generated wrapper.

#[derive(Clone, Debug)]
pub struct Snapshot {
    pub state: String,
    pub progress: i32,
    pub stage: String,
    pub version: String,
    pub error: String,
}

impl Snapshot {
    pub fn installed(&self) -> bool {
        self.state == "installed"
    }

    pub fn installing(&self) -> bool {
        self.state == "installing"
    }
}

#[cfg(target_os = "android")]
mod android {
    use super::Snapshot;
    use crate::android_host;
    use anyhow::{Context as _, Result};
    use jni::objects::{GlobalRef, JClass, JObject, JString, JValue};
    use jni::JNIEnv;
    use std::sync::OnceLock;

    const UBUNTU_CLASS: &str = "dev.wunder.android.UbuntuRuntime";

    static CLASS: OnceLock<GlobalRef> = OnceLock::new();

    /// Resolve `UbuntuRuntime` through the application class loader once per
    /// process. `FindClass` on a native thread cannot see app classes, so the
    /// cached global reference is the only reliable handle.
    fn class() -> Option<&'static GlobalRef> {
        if let Some(class) = CLASS.get() {
            return Some(class);
        }
        let loaded = android_host::with_app(load_class).and_then(Result::ok)?;
        let _ = CLASS.set(loaded);
        CLASS.get()
    }

    fn load_class(env: &mut JNIEnv<'_>, app: &JObject<'_>) -> Result<GlobalRef> {
        let loader = env
            .call_method(app, "getClassLoader", "()Ljava/lang/ClassLoader;", &[])
            .context("read the application class loader")?
            .l()
            .context("application class loader is null")?;
        let name = env.new_string(UBUNTU_CLASS)?;
        let class = env
            .call_method(
                &loader,
                "loadClass",
                "(Ljava/lang/String;)Ljava/lang/Class;",
                &[JValue::Object(&name)],
            )
            .context("load UbuntuRuntime")?
            .l()
            .context("UbuntuRuntime class is null")?;
        Ok(env.new_global_ref(class)?)
    }

    fn as_class<'local>(class: &'static GlobalRef) -> JClass<'local> {
        // SAFETY: `JObject` is a non-owning handle; the global reference keeps the
        // class alive for the whole process, so the alias cannot dangle.
        JClass::from(unsafe { JObject::from_raw(class.as_obj().as_raw()) })
    }

    fn string_method(name: &str) -> Option<String> {
        let class = class()?;
        android_host::with_app(|env, _| -> Option<String> {
            let value = env
                .call_static_method(as_class(class), name, "()Ljava/lang/String;", &[])
                .ok()?;
            let object = value.l().ok()?;
            let string = JString::from(object);
            let text = env.get_string(&string).ok()?;
            Some(text.to_string_lossy().into_owned())
        })?
    }

    /// Detect an existing install. Safe to call on every process start.
    pub fn bind() {
        let Some(class) = class() else {
            log::warn!("ubuntu runtime facade unavailable");
            return;
        };
        let _ = android_host::with_app(|env, app| {
            let result = env.call_static_method(
                as_class(class),
                "bind",
                "(Landroid/content/Context;)V",
                &[JValue::Object(app)],
            );
            if result.is_err() {
                log::warn!("UbuntuRuntime.bind failed");
            }
        });
    }

    /// Start the background install. Repeated calls are ignored by the Java side.
    pub fn install() {
        let Some(class) = class() else {
            return;
        };
        let _ = android_host::with_app(|env, _| {
            let result = env.call_static_method(as_class(class), "install", "()V", &[]);
            if result.is_err() {
                log::warn!("UbuntuRuntime.install failed");
            }
        });
    }

    /// Absolute path of the generated proot shell wrapper.
    pub fn shell_path() -> String {
        string_method("shellPath").unwrap_or_default()
    }

    pub fn snapshot() -> Option<Snapshot> {
        let raw = string_method("snapshot")?;
        let mut parts = raw.split('\u{1f}');
        let state = parts.next()?.to_string();
        let progress = parts.next()?.parse().unwrap_or(0);
        let stage = parts.next()?.to_string();
        let version = parts.next()?.to_string();
        let error = parts.next().unwrap_or_default().to_string();
        Some(Snapshot {
            state,
            progress,
            stage,
            version,
            error,
        })
    }
}

#[cfg(target_os = "android")]
pub use android::{bind, install, shell_path, snapshot};

#[cfg(not(target_os = "android"))]
pub fn bind() {}
#[cfg(not(target_os = "android"))]
pub fn install() {}
#[cfg(not(target_os = "android"))]
pub fn shell_path() -> String {
    String::new()
}
#[cfg(not(target_os = "android"))]
pub fn snapshot() -> Option<Snapshot> {
    None
}
