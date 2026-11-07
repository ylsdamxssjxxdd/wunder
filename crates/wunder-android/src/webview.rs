//! Android system-WebView browser host.
//!
//! The Java side owns the WebView, the floating window and the JS snapshot
//! scripts. This facade only forwards canonical `browser` actions. No proot,
//! Playwright or packaged Chromium is involved.

use anyhow::{anyhow, Context, Result};
use jni::objects::{GlobalRef, JObject, JString, JValue};
use jni::{JNIEnv, JavaVM};
use serde_json::Value;
use std::sync::Arc;
use wunder_server::browser::install_browser_provider;
use wunder_server::browser::providers::webview::{
    install_webview_host, WebViewHost, WebViewProvider,
};

const BRIDGE_CLASS: &str = "dev.wunder.android.WebViewBridge";

struct AndroidWebViewHost {
    vm: JavaVM,
    context: GlobalRef,
    bridge: GlobalRef,
}

impl WebViewHost for AndroidWebViewHost {
    fn execute(&self, action: &str, args: &Value) -> Result<Value> {
        let mut env = self
            .vm
            .attach_current_thread()
            .context("attach the WebView worker to the JVM")?;
        self.call_bridge(&mut env, action, args)
    }
}

impl AndroidWebViewHost {
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
            .map_err(|error| self.jni_error(env, error, "run WebViewBridge.execute"))?;
        if result.is_null() {
            return Err(anyhow!("WebViewBridge.execute returned null"));
        }
        let text: String = env
            .get_string(&JString::from(result))?
            .to_string_lossy()
            .into_owned();
        serde_json::from_str(&text).with_context(|| format!("decode the WebViewBridge reply: {text}"))
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

/// Install the WebView host and make its provider the active browser surface.
/// A failure is non-fatal: the builtin Playwright provider stays registered,
/// and the browser tool will report that its runtime is unavailable.
pub fn install() {
    if let Err(error) = try_install() {
        log::error!("webview browser host unavailable: {error:#}");
    }
}

fn try_install() -> Result<()> {
    let context = ndk_context::android_context();
    // SAFETY: android-activity initialized the process JVM before the engine
    // thread starts; the raw pointer stays valid for the app lifetime.
    let vm = unsafe { JavaVM::from_raw(context.vm().cast()) }.context("wrap the Android JavaVM")?;
    let context = crate::android_host::app_context().context("the application context is not ready")?;
    let bridge = {
        let mut env = vm.attach_current_thread()?;
        load_bridge_class(&mut env, context.as_obj())?
    };
    install_webview_host(Arc::new(AndroidWebViewHost {
        vm,
        context,
        bridge,
    }));
    install_browser_provider(Arc::new(WebViewProvider::new()));
    log::info!("webview browser provider installed");
    Ok(())
}

/// `FindClass` on a native worker resolves the system class loader by default
/// and cannot see app classes, so resolve the bridge through the application
/// context's own loader once at startup.
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
        .context("load WebViewBridge")?;
    Ok(env.new_global_ref(class)?)
}
