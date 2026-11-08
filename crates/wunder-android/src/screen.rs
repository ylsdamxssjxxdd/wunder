//! Android screen-capture executor for the runtime's android_screen tool.
//!
//! JNI work happens on the engine's blocking workers, never on the UI thread.
//! The Java bridge owns the MediaProjection grant, the foreground service and
//! the PNG file; this host only forwards one canonical action and returns the
//! device's JSON reply. Every failure is already a JSON value on the Java side,
//! so JNI errors are the only reason to surface an `Err` here.

use anyhow::{anyhow, Context, Result};
use jni::objects::{GlobalRef, JObject, JString, JValue};
use jni::{JNIEnv, JavaVM};
use serde_json::Value;
use std::sync::Arc;
use wunder_server::tools::{install_android_screen_host, AndroidScreenHost};

const BRIDGE_CLASS: &str = "dev.wunder.android.ScreenCaptureBridge";

struct AndroidScreenCaptureHost {
    vm: JavaVM,
    /// Application context; the bridge resolves the current activity itself
    /// because activities are recreated while the host stays process-scoped.
    context: GlobalRef,
    bridge: GlobalRef,
}

impl AndroidScreenHost for AndroidScreenCaptureHost {
    fn execute(&self, action: &str, args: &Value) -> Result<Value> {
        let mut env = self
            .vm
            .attach_current_thread()
            .context("attach the screen-capture worker to the JVM")?;
        self.call_bridge(&mut env, action, args)
    }
}

impl AndroidScreenCaptureHost {
    fn call_bridge(&self, env: &mut JNIEnv<'_>, action: &str, args: &Value) -> Result<Value> {
        let context = self.context.as_obj();
        let action_string = env.new_string(action)?;
        let args_json = serde_json::to_string(args)?;
        let args_string = env.new_string(args_json)?;
        let result = env
            .call_static_method(
                &self.bridge,
                "execute",
                "(Landroid/content/Context;Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;",
                &[
                    JValue::Object(context),
                    JValue::Object(&action_string),
                    JValue::Object(&args_string),
                ],
            )
            .and_then(|value| value.l())
            .map_err(|error| self.jni_error(env, error, "run ScreenCaptureBridge.execute"))?;
        if result.is_null() {
            return Err(anyhow!("ScreenCaptureBridge.execute returned null"));
        }
        let text: String = env
            .get_string(&JString::from(result))?
            .to_string_lossy()
            .into_owned();
        serde_json::from_str(&text)
            .with_context(|| format!("decode the ScreenCaptureBridge reply: {text}"))
    }

    fn jni_error(
        &self,
        env: &mut JNIEnv<'_>,
        error: jni::errors::Error,
        step: &str,
    ) -> anyhow::Error {
        if matches!(error, jni::errors::Error::JavaException) {
            let _ = env.exception_describe();
            let _ = env.exception_clear();
        }
        anyhow!("{step}: {error}")
    }
}

/// Install the screen-capture executor once the Android shell exists. A
/// failure is non-fatal: the runtime simply does not offer the tool because
/// no host is installed.
pub fn install() {
    if let Err(error) = try_install() {
        log::error!("android screen host unavailable: {error:#}");
    }
}

fn try_install() -> Result<()> {
    let context = ndk_context::android_context();
    // SAFETY: android-activity initialized the process JVM before
    // `android_main`; the raw pointer stays valid for the app lifetime.
    let vm = unsafe { JavaVM::from_raw(context.vm().cast()) }.context("wrap the Android JavaVM")?;
    let context =
        crate::android_host::app_context().context("the application context is not ready")?;
    let bridge = {
        let mut env = vm.attach_current_thread()?;
        load_bridge_class(&mut env, context.as_obj())?
    };
    let host = AndroidScreenCaptureHost {
        vm,
        context,
        bridge,
    };
    install_android_screen_host(Arc::new(host));
    log::info!("android screen host installed");
    Ok(())
}

/// `FindClass` on a native worker resolves the system class loader by default
/// and cannot see app classes, so resolve the bridge through the application's
/// own loader once at startup.
fn load_bridge_class(env: &mut JNIEnv<'_>, context: &JObject<'_>) -> Result<GlobalRef> {
    let loader = env
        .call_method(context, "getClassLoader", "()Ljava/lang/ClassLoader;", &[])
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
        .context("load ScreenCaptureBridge")?;
    Ok(env.new_global_ref(class)?)
}
