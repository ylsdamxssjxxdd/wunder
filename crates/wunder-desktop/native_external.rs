//! Browser navigation for native settings. No web renderer runs in the UI process.

use super::NativeDesktop;
use anyhow::{bail, Context, Result};
use url::Url;

impl NativeDesktop {
    /// Open the bridge's standalone model page using the system browser.
    /// The address comes from the account settings, without session credentials.
    pub fn open_bridge_model(&self, server: &str) -> Result<()> {
        let url = bridge_model_url(server)?;
        open_browser(&url)
    }
}

fn bridge_model_url(server: &str) -> Result<Url> {
    let server = server.trim();
    if server.is_empty() || server.chars().any(char::is_control) {
        bail!("请先在账号页填写有效的舰体服务地址");
    }
    let mut url = Url::parse(server).context("舰体服务地址格式无效")?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        bail!("舰体服务地址须使用 HTTP 或 HTTPS");
    }
    if !url.username().is_empty() || url.password().is_some() {
        bail!("舰体服务地址不能包含账号或密码");
    }
    let base = url.path().trim_end_matches('/');
    let base = base.strip_suffix("/wunder").unwrap_or(base);
    let path = format!("{base}/bridge/wunder-model.html");
    url.set_path(&path);
    url.set_query(None);
    url.set_fragment(None);
    Ok(url)
}

fn open_browser(url: &Url) -> Result<()> {
    #[cfg(windows)]
    {
        // ShellExecute alone normally reuses a browser tab. Ask the registered
        // browser for a new window when it supports that command-line option.
        if let Some(browser) = default_browser() {
            if let Some(argument) = new_window_argument(&browser) {
                use std::os::windows::process::CommandExt;
                if std::process::Command::new(browser)
                    .args([argument, url.as_str()])
                    .creation_flags(0x08000000) // CREATE_NO_WINDOW: no console flash.
                    .spawn()
                    .is_ok()
                {
                    return Ok(());
                }
            }
        }
        #[link(name = "shell32")]
        unsafe extern "system" {
            fn ShellExecuteW(
                window: isize,
                operation: *const u16,
                file: *const u16,
                parameters: *const u16,
                directory: *const u16,
                show: i32,
            ) -> isize;
        }
        let operation: Vec<u16> = "open".encode_utf16().chain(Some(0)).collect();
        let target: Vec<u16> = url.as_str().encode_utf16().chain(Some(0)).collect();
        let result = unsafe {
            ShellExecuteW(
                0,
                operation.as_ptr(),
                target.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                1,
            )
        };
        if result <= 32 {
            bail!("无法打开系统浏览器（错误码 {result}）");
        }
        Ok(())
    }
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        #[cfg(target_os = "linux")]
        if let Some(browser) = default_browser() {
            if let Some(argument) = new_window_argument(&browser) {
                if std::process::Command::new(browser)
                    .args([argument, url.as_str()])
                    .spawn()
                    .is_ok()
                {
                    return Ok(());
                }
            }
        }
        let status = std::process::Command::new(if cfg!(target_os = "macos") {
            "open"
        } else {
            "xdg-open"
        })
        .arg(url.as_str())
        .status()
        .context("无法启动系统浏览器")?;
        if !status.success() {
            bail!("系统浏览器未能打开页面");
        }
        Ok(())
    }
    #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
    {
        let _ = url;
        bail!("当前系统不支持打开浏览器");
    }
}

#[cfg(any(windows, target_os = "linux", test))]
fn new_window_argument(browser: &std::path::Path) -> Option<&'static str> {
    match browser.file_stem()?.to_str()?.to_ascii_lowercase().as_str() {
        "chrome"
        | "google-chrome"
        | "google-chrome-stable"
        | "chromium"
        | "chromium-browser"
        | "msedge"
        | "microsoft-edge"
        | "microsoft-edge-stable"
        | "firefox"
        | "firefox-esr"
        | "brave"
        | "brave-browser"
        | "vivaldi"
        | "vivaldi-stable"
        | "opera" => Some("--new-window"),
        "iexplore" => Some("-new"),
        _ => None,
    }
}

#[cfg(windows)]
fn default_browser() -> Option<std::path::PathBuf> {
    #[link(name = "shlwapi")]
    unsafe extern "system" {
        fn AssocQueryStringW(
            flags: u32,
            kind: i32,
            association: *const u16,
            extra: *const u16,
            output: *mut u16,
            length: *mut u32,
        ) -> i32;
    }
    let association: Vec<u16> = "https".encode_utf16().chain(Some(0)).collect();
    let mut length = 0;
    unsafe {
        AssocQueryStringW(
            0x1000,
            2,
            association.as_ptr(),
            std::ptr::null(),
            std::ptr::null_mut(),
            &mut length,
        );
    }
    if length == 0 || length > 32_768 {
        return None;
    }
    let mut output = vec![0u16; length as usize];
    if unsafe {
        AssocQueryStringW(
            0x1000,
            2,
            association.as_ptr(),
            std::ptr::null(),
            output.as_mut_ptr(),
            &mut length,
        )
    } != 0
    {
        return None;
    }
    use std::os::windows::ffi::OsStringExt;
    let end = output.iter().position(|value| *value == 0)?;
    Some(std::ffi::OsString::from_wide(&output[..end]).into())
}

#[cfg(target_os = "linux")]
fn default_browser() -> Option<std::path::PathBuf> {
    let result = std::process::Command::new("xdg-settings")
        .args(["get", "default-web-browser"])
        .output()
        .ok()?;
    if !result.status.success() {
        return None;
    }
    let desktop = String::from_utf8(result.stdout).ok()?;
    let executable = desktop.trim().strip_suffix(".desktop")?;
    // Only known desktop IDs map directly to executable names. Custom launchers
    // still use xdg-open, which honors their registered Exec configuration.
    let executable = match executable {
        "org.mozilla.firefox" => "firefox",
        "com.google.Chrome" => "google-chrome",
        "com.microsoft.Edge" => "microsoft-edge",
        "com.brave.Browser" => "brave-browser",
        name if !name.contains(['/', '\\']) => name,
        _ => return None,
    };
    let path = std::path::PathBuf::from(executable);
    new_window_argument(&path)?;
    Some(path)
}

#[cfg(test)]
mod tests {
    use super::{bridge_model_url, new_window_argument};

    #[test]
    fn uses_browser_specific_new_window_arguments() {
        use std::path::Path;
        assert_eq!(
            new_window_argument(Path::new("msedge.exe")),
            Some("--new-window")
        );
        assert_eq!(
            new_window_argument(Path::new("firefox")),
            Some("--new-window")
        );
        assert_eq!(new_window_argument(Path::new("iexplore.exe")), Some("-new"));
        assert_eq!(new_window_argument(Path::new("fixture-browser")), None);
    }

    #[test]
    fn resolves_origin_api_base_and_proxy_prefix() {
        for (input, expected) in [
            (
                "https://fixture.invalid",
                "https://fixture.invalid/bridge/wunder-model.html",
            ),
            (
                "http://fixture.invalid:8123/wunder/?discard=1#discard",
                "http://fixture.invalid:8123/bridge/wunder-model.html",
            ),
            (
                "https://fixture.invalid/prefix/wunder",
                "https://fixture.invalid/prefix/bridge/wunder-model.html",
            ),
            (
                "http://[::1]:8123/prefix/",
                "http://[::1]:8123/prefix/bridge/wunder-model.html",
            ),
        ] {
            assert_eq!(bridge_model_url(input).unwrap().as_str(), expected);
        }
    }

    #[test]
    fn refuses_non_web_addresses_and_embedded_credentials() {
        for input in [
            "",
            "fixture.invalid",
            "file:///fixture",
            "javascript:alert(0)",
            "https://fixture-user@fixture.invalid",
            "https://fixture.\ninvalid/",
        ] {
            assert!(bridge_model_url(input).is_err());
        }
    }
}
