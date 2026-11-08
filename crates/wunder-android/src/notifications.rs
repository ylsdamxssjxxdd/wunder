//! Android notification executor for the runtime's android_notifications tool.
//!
//! The listener service keeps a bounded in-memory queue; this host forwards a
//! canonical action to `NotificationBridge` (Java) on a blocking worker and
//! returns its JSON reply. Missing notification access arrives as a stable
//! error code, not a JNI exception.

use anyhow::{anyhow, Context, Result};
use jni::objects::{GlobalRef, JObject, JString, JValue};
use jni::{JNIEnv, JavaVM};
use serde_json::Value;
use std::sync::Arc;
use wunder_server::tools::{install_android_notifications_host, AndroidNotificationsHost};

const BRIDGE_CLASS: &str = "dev.wunder.android.NotificationBridge";

struct AndroidNotificationHost {
    vm: JavaVM,
    /// Application context; the listener and bridge are process-scoped.
    context: GlobalRef,
    bridge: GlobalRef,
}

impl AndroidNotificationsHost for AndroidNotificationHost {
    fn execute(&self, action: &str, args: &Value) -> Result<Value> {
        let mut env = self
            .vm
            .attach_current_thread()
            .context("attach the notification worker to the JVM")?;
        self.call_bridge(&mut env, action, args)
    }
}

impl AndroidNotificationHost {
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
            .map_err(|error| self.jni_error(env, error, "run NotificationBridge.execute"))?;
        if result.is_null() {
            return Err(anyhow!("NotificationBridge.execute returned null"));
        }
        let text: String = env
            .get_string(&JString::from(result))?
            .to_string_lossy()
            .into_owned();
        serde_json::from_str(&text)
            .with_context(|| format!("decode the NotificationBridge reply: {text}"))
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

/// Install the notification executor once the Android shell exists. A failure
/// is non-fatal: the tool is simply not advertised by the runtime.
pub fn install() {
    if let Err(error) = try_install() {
        log::error!("android notification host unavailable: {error:#}");
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
    let host = AndroidNotificationHost {
        vm,
        context,
        bridge,
    };
    install_android_notifications_host(Arc::new(host));
    log::info!("android notification host installed");
    Ok(())
}

/// Resolve the bridge through the application class loader: app classes are
/// invisible to `FindClass` on a native worker thread.
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
        .context("load NotificationBridge")?;
    Ok(env.new_global_ref(class)?)
}
