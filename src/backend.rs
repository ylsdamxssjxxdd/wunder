// 迁移过渡期后端托管逻辑：可自动拉起 Python 服务并探活。
use anyhow::{anyhow, Result};
use reqwest::Client;
use std::env;
use std::time::Duration;
use tokio::process::{Child, Command};
use tokio::time::sleep;
use tracing::{info, warn};

pub struct BackendHandle {
    base_url: Option<String>,
    child: Option<Child>,
}

impl BackendHandle {
    pub async fn init() -> Result<Self> {
        // 优先使用显式配置的后端地址，避免意外启动 Python 进程。
        if let Some(url) = backend_url_from_env() {
            info!("使用外部后端: {url}");
            return Ok(Self {
                base_url: Some(url),
                child: None,
            });
        }

        if !autostart_enabled() {
            // 自动启动关闭时保持空后端，交由调用方处理不可用提示。
            warn!("未配置后端地址且未启用自动启动，接口将返回不可用状态。");
            return Ok(Self {
                base_url: None,
                child: None,
            });
        }

        let port = backend_port();
        let mut child = spawn_python_backend(port)?;
        let url = format!("http://127.0.0.1:{port}");
        // 简单轮询后端健康检查，避免启动即请求失败。
        wait_backend_ready(&url).await;

        Ok(Self {
            base_url: Some(url),
            child: Some(child),
        })
    }

    pub fn base_url(&self) -> Option<&str> {
        self.base_url.as_deref()
    }

    pub async fn shutdown(&mut self) {
        if let Some(child) = self.child.as_mut() {
            if let Err(err) = child.kill().await {
                warn!("停止后端失败: {err}");
            }
        }
        self.child = None;
    }
}

fn backend_url_from_env() -> Option<String> {
    // 允许通过环境变量直接指定后端地址，支持独立 Python 服务。
    let url = env::var("WUNDER_PY_BACKEND_URL").ok()?.trim().to_string();
    if url.is_empty() {
        None
    } else {
        Some(url.trim_end_matches('/').to_string())
    }
}

fn autostart_enabled() -> bool {
    // 通过环境变量控制是否自动拉起 Python 后端。
    env::var("WUNDER_PY_BACKEND_AUTOSTART")
        .map(|value| value.trim().eq_ignore_ascii_case("true"))
        .unwrap_or(true)
}

fn backend_port() -> u16 {
    // 默认端口与 Rust 监听端口错开，避免冲突。
    env::var("WUNDER_PY_BACKEND_PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(18000)
}

fn spawn_python_backend(port: u16) -> Result<Child> {
    // 使用 uvicorn 启动现有 Python 服务，实现迁移期功能复用。
    let mut command = Command::new("python3");
    command
        .arg("-m")
        .arg("uvicorn")
        .arg("app.asgi:app")
        .arg("--host")
        .arg("0.0.0.0")
        .arg("--port")
        .arg(port.to_string())
        .env("PYTHONUNBUFFERED", "1")
        .current_dir(env::current_dir()?);

    let child = command.spawn().map_err(|err| {
        anyhow!("启动 Python 后端失败，请确认已安装 uvicorn: {err}")
    })?;
    Ok(child)
}

async fn wait_backend_ready(base_url: &str) {
    // 通过 i18n 接口探活，避免占用业务入口。
    let client = Client::new();
    let check_url = format!("{base_url}/wunder/i18n");
    for _ in 0..30 {
        match client.get(&check_url).send().await {
            Ok(response) if response.status().is_success() => {
                info!("Python 后端已就绪: {check_url}");
                return;
            }
            Ok(_) | Err(_) => {
                sleep(Duration::from_secs(1)).await;
            }
        }
    }
    warn!("Python 后端启动超时，将继续运行但请求可能失败。");
}
