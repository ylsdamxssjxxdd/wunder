//! Launch the existing desktop bridge without blocking the first UI frame.
use crate::{chat_api::ConnectionConfig, MainWindow};
use slint::ComponentHandle;
use std::{
    io::{BufRead, BufReader},
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex},
};

pub struct BridgeProcess(Arc<Mutex<Option<Child>>>);
impl Drop for BridgeProcess {
    fn drop(&mut self) {
        if let Ok(mut child) = self.0.lock() {
            if let Some(mut child) = child.take() {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }
}

pub fn start(app: &MainWindow) -> BridgeProcess {
    app.set_conversations(slint::ModelRc::default());
    app.set_messages(slint::ModelRc::default());
    app.set_agents(slint::ModelRc::default());
    app.set_tools(slint::ModelRc::default());
    app.set_models(slint::ModelRc::default());
    app.set_status("正在启动本地运行时…".into());
    let child = Arc::new(Mutex::new(None));
    let process = BridgeProcess(child.clone());
    let weak = app.as_weak();
    std::thread::spawn(move || {
        let result = launch(&child);
        let _ = weak.upgrade_in_event_loop(move |app| match result {
            Ok(connection) => crate::chat_runtime::install(&app, connection),
            Err(error) => {
                app.set_status("本地运行时启动失败".into());
                app.set_dialog_title("本地运行时不可用".into());
                app.set_dialog_text(error.into());
                app.set_dialog_open(true);
            }
        });
    });
    process
}

fn launch(slot: &Arc<Mutex<Option<Child>>>) -> Result<ConnectionConfig, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let parent = exe.parent().ok_or("无法定位程序目录")?;
    let mut candidates = vec![parent.join("wunder-desktop-bridge.exe")];
    for ancestor in parent.ancestors().take(6) {
        candidates.push(ancestor.join("target/release/wunder-desktop-bridge.exe"));
    }
    let bridge = candidates.into_iter().find(|path| path.is_file()).ok_or(
        "请将 wunder-desktop-bridge.exe 放在桌面程序旁，或使用 --connect 连接已启动的本地运行时。",
    )?;
    let mut command = Command::new(&bridge);
    command
        .args(["--port", "0"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .env("TOKIO_WORKER_THREADS", "8")
        .env("RAYON_NUM_THREADS", "8");
    // Keep the bridge's own persistent data defaults so upgrades retain settings.
    command.current_dir(bridge.parent().unwrap_or(parent));
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000); // CREATE_NO_WINDOW, available on Win7.
    }
    let mut process_slot = slot.lock().map_err(|_| "运行时进程状态不可用")?;
    let mut child = command
        .spawn()
        .map_err(|e| format!("无法启动本地运行时：{e}"))?;
    let output = child.stdout.take().ok_or("无法读取运行时启动信息")?;
    *process_slot = Some(child);
    drop(process_slot);
    let (tx, rx) = std::sync::mpsc::sync_channel::<String>(1);
    std::thread::spawn(move || {
        let mut published = false;
        for line in BufReader::new(output).lines().map_while(Result::ok) {
            if !published {
                if let Some(url) = line.strip_prefix("- api_base: ") {
                    let _ = tx.send(url.trim().to_string());
                    published = true;
                }
            }
            // Drain remaining output without logging tokens or local paths.
        }
    });
    let target = rx
        .recv_timeout(std::time::Duration::from_secs(60))
        .map_err(|_| {
            if let Ok(mut child) = slot.lock() {
                if let Some(mut child) = child.take() {
                    let _ = child.kill();
                    let _ = child.wait();
                }
            }
            "本地运行时未能在一分钟内就绪，请检查运行时配置。".to_string()
        })?;
    ConnectionConfig::from_target(&target).map_err(|e| e.to_string())
}
