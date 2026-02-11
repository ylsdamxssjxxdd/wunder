use crate::args::DesktopArgs;
use crate::runtime::DesktopRuntime;
use anyhow::{anyhow, Context, Result};
use axum::body::Body;
use axum::extract::State;
use axum::http::{header::AUTHORIZATION, HeaderMap, Method, Request, StatusCode};
use axum::middleware::{from_fn_with_state, Next};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, get_service};
use axum::{Extension, Json, Router};
use serde::Serialize;
use serde_json::json;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::path::Path;
use std::sync::Arc;
use tokio::sync::oneshot;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::TraceLayer;
use tracing::{error, info};

#[derive(Clone)]
struct DesktopGuardState {
    token: String,
}

#[derive(Clone)]
struct DesktopWebState {
    runtime: DesktopRuntimeInfo,
    index_html: Option<String>,
}

#[derive(Clone, Serialize)]
pub struct DesktopRuntimeInfo {
    pub mode: &'static str,
    pub bind_addr: String,
    pub web_base: String,
    pub api_base: String,
    pub ws_base: String,
    pub token: String,
    pub desktop_token: String,
    pub user_id: String,
    pub app_dir: String,
    pub workspace_root: String,
    pub temp_root: String,
    pub settings_path: String,
    pub repo_root: String,
    pub frontend_root: Option<String>,
    pub remote_enabled: bool,
    pub remote_connected: bool,
    pub remote_server_base_url: String,
    pub remote_role_name: String,
    pub remote_error: Option<String>,
}

pub struct DesktopBridge {
    runtime_info: DesktopRuntimeInfo,
    shutdown_tx: Option<oneshot::Sender<()>>,
    server_task: Option<tokio::task::JoinHandle<()>>,
}

impl DesktopBridge {
    pub async fn launch(args: &DesktopArgs) -> Result<Self> {
        let runtime = DesktopRuntime::init(args).await?;
        let bind_host = sanitize_host(&args.host)?;
        let bind_addr = format!("{bind_host}:{}", args.port);
        let listener = tokio::net::TcpListener::bind(bind_addr.as_str())
            .await
            .with_context(|| format!("bind desktop bridge failed: {bind_addr}"))?;
        let local_addr = listener
            .local_addr()
            .context("resolve desktop bridge local addr failed")?;
        let public_addr = resolve_public_addr(local_addr);

        let web_base = format!("http://{public_addr}");
        let api_base = format!("{web_base}/wunder");
        let ws_base = format!("ws://{public_addr}/wunder/chat/ws");
        let runtime_info = build_runtime_info(&runtime, local_addr, &web_base, &api_base, &ws_base);
        let web_state = Arc::new(build_web_state(&runtime, runtime_info.clone())?);

        let guarded_api =
            wunder_server::build_desktop_router(runtime.state.clone()).layer(from_fn_with_state(
                Arc::new(DesktopGuardState {
                    token: runtime.desktop_token.clone(),
                }),
                desktop_token_guard,
            ));

        let mut app = Router::new()
            .merge(guarded_api)
            .route("/config.json", get(runtime_config_handler))
            .route("/wunder/desktop/bootstrap", get(bootstrap_handler));

        if let Some(frontend_root) = runtime.frontend_root.as_ref() {
            app = app.merge(build_frontend_router(frontend_root));
        } else {
            app = app
                .route("/", get(frontend_missing_handler))
                .route("/index.html", get(frontend_missing_handler))
                .route("/{*path}", get(frontend_missing_handler));
        }

        app = app
            .layer(Extension(web_state.clone()))
            .layer(TraceLayer::new_for_http());

        let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
        let server = axum::serve(listener, app.into_make_service()).with_graceful_shutdown(async {
            let _ = shutdown_rx.await;
        });
        let server_task = tokio::spawn(async move {
            if let Err(err) = server.await {
                error!("desktop bridge exited with error: {err}");
            }
        });

        info!("wunder-desktop bridge ready: {api_base}");
        Ok(Self {
            runtime_info,
            shutdown_tx: Some(shutdown_tx),
            server_task: Some(server_task),
        })
    }

    pub fn info(&self) -> &DesktopRuntimeInfo {
        &self.runtime_info
    }

    pub fn print_banner(&self, print_token: bool) {
        println!("wunder-desktop bridge ready");
        println!("- api_base: {}", self.runtime_info.api_base);
        println!("- web_base: {}", self.runtime_info.web_base);
        println!("- bind_addr: {}", self.runtime_info.bind_addr);
        println!("- app_dir: {}", self.runtime_info.app_dir);
        println!("- temp_root: {}", self.runtime_info.temp_root);
        println!("- settings_path: {}", self.runtime_info.settings_path);
        println!("- workspace_root: {}", self.runtime_info.workspace_root);
        println!("- repo_root: {}", self.runtime_info.repo_root);
        if let Some(frontend_root) = self.runtime_info.frontend_root.as_ref() {
            println!("- frontend_root: {frontend_root}");
        } else {
            println!("- frontend_root: (not found)");
        }
        println!("- user_id: {}", self.runtime_info.user_id);

        if print_token {
            println!("- desktop_token: {}", self.runtime_info.desktop_token);
        } else {
            println!(
                "- desktop_token: {}",
                mask_token(&self.runtime_info.desktop_token)
            );
            println!("  (use --print-token to print full token)");
        }

        if self.runtime_info.remote_enabled {
            if self.runtime_info.remote_connected {
                println!(
                    "- remote_gateway: connected ({})",
                    self.runtime_info.remote_server_base_url
                );
                println!("- remote_role: {}", self.runtime_info.remote_role_name);
            } else {
                println!(
                    "- remote_gateway: enabled but disconnected ({})",
                    self.runtime_info.remote_server_base_url
                );
                if let Some(message) = self.runtime_info.remote_error.as_ref() {
                    println!("- remote_error: {message}");
                }
            }
        }
    }

    pub async fn shutdown(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
        if let Some(task) = self.server_task.take() {
            let _ = task.await;
        }
    }
}

fn build_runtime_info(
    runtime: &DesktopRuntime,
    bind_addr: SocketAddr,
    web_base: &str,
    api_base: &str,
    ws_base: &str,
) -> DesktopRuntimeInfo {
    let mut effective_api_base = api_base.to_string();
    let mut effective_ws_base = ws_base.to_string();
    let mut effective_token = runtime.desktop_token.clone();

    if runtime.remote_gateway.enabled {
        if let Some(remote_api_base) = runtime.remote_api_base.as_ref() {
            effective_api_base = remote_api_base.clone();
        }
        if let Some(remote_ws_base) = runtime.remote_ws_base.as_ref() {
            effective_ws_base = remote_ws_base.clone();
        }
    }

    let remote_connected = runtime.remote_api_base.is_some() && runtime.remote_ws_base.is_some();
    if runtime.remote_gateway.enabled && remote_connected {
        effective_token.clear();
    }

    DesktopRuntimeInfo {
        mode: "desktop",
        bind_addr: bind_addr.to_string(),
        web_base: web_base.to_string(),
        api_base: effective_api_base,
        ws_base: effective_ws_base,
        token: effective_token,
        desktop_token: runtime.desktop_token.clone(),
        user_id: runtime.user_id.clone(),
        app_dir: runtime.app_dir.to_string_lossy().to_string(),
        workspace_root: runtime.workspace_root.to_string_lossy().to_string(),
        temp_root: runtime.temp_root.to_string_lossy().to_string(),
        settings_path: runtime.settings_path.to_string_lossy().to_string(),
        repo_root: runtime.repo_root.to_string_lossy().to_string(),
        frontend_root: runtime
            .frontend_root
            .as_ref()
            .map(|path| path.to_string_lossy().to_string()),
        remote_enabled: runtime.remote_gateway.enabled,
        remote_connected,
        remote_server_base_url: runtime.remote_gateway.server_base_url.trim().to_string(),
        remote_role_name: runtime.remote_gateway.role_name.trim().to_string(),
        remote_error: runtime.remote_error.clone(),
    }
}

fn build_web_state(
    runtime: &DesktopRuntime,
    runtime_info: DesktopRuntimeInfo,
) -> Result<DesktopWebState> {
    let index_html = runtime
        .frontend_root
        .as_ref()
        .map(|frontend_root| load_index_with_runtime(frontend_root, &runtime_info))
        .transpose()?;
    Ok(DesktopWebState {
        runtime: runtime_info,
        index_html,
    })
}

fn load_index_with_runtime(
    frontend_root: &Path,
    runtime_info: &DesktopRuntimeInfo,
) -> Result<String> {
    let index_path = frontend_root.join("index.html");
    let template = std::fs::read_to_string(&index_path)
        .with_context(|| format!("read frontend index failed: {}", index_path.display()))?;
    let runtime_json =
        serde_json::to_string(runtime_info).context("serialize desktop runtime payload failed")?;
    let script = format!(
        "<script>(function(){{const cfg={runtime_json};window.__WUNDER_DESKTOP_RUNTIME__=cfg;try{{const localToken=cfg.desktop_token||cfg.token||'';const remoteAuthMode=Boolean(cfg.remote_enabled&&cfg.remote_connected);if(localToken){{localStorage.setItem('wunder_desktop_local_token',localToken);}}if(cfg.user_id){{localStorage.setItem('wunder_desktop_user_id',cfg.user_id);}}if(!remoteAuthMode&&cfg.token){{localStorage.setItem('access_token',cfg.token);}}else if(remoteAuthMode&&localStorage.getItem('access_token')===localToken){{localStorage.removeItem('access_token');}}}}catch(_e){{}}}})();</script>"
    );
    Ok(inject_script_before_head_end(&template, &script))
}

fn inject_script_before_head_end(template: &str, script: &str) -> String {
    if let Some(index) = template.find("</head>") {
        let mut output = String::with_capacity(template.len() + script.len() + 1);
        output.push_str(&template[..index]);
        output.push_str(script);
        output.push('\n');
        output.push_str(&template[index..]);
        return output;
    }
    format!("{script}\n{template}")
}

fn build_frontend_router(frontend_root: &Path) -> Router {
    Router::new()
        .route(
            "/favicon.svg",
            get_service(ServeFile::new(frontend_root.join("favicon.svg"))),
        )
        .nest_service("/assets", ServeDir::new(frontend_root.join("assets")))
        .nest_service("/third", ServeDir::new(frontend_root.join("third")))
        .nest_service("/doc-icons", ServeDir::new(frontend_root.join("doc-icons")))
        .nest_service(
            "/vscode-icons",
            ServeDir::new(frontend_root.join("vscode-icons")),
        )
        .route("/", get(frontend_index_handler))
        .route("/index.html", get(frontend_index_handler))
        .route("/{*path}", get(frontend_index_handler))
}

fn sanitize_host(host: &str) -> Result<String> {
    let cleaned = host.trim();
    if cleaned.is_empty() {
        return Ok("127.0.0.1".to_string());
    }
    if cleaned.contains(' ') {
        return Err(anyhow!("invalid host: {cleaned}"));
    }
    Ok(cleaned.to_string())
}

fn resolve_public_addr(local_addr: SocketAddr) -> SocketAddr {
    let ip = match local_addr.ip() {
        IpAddr::V4(ip) if ip.is_unspecified() => IpAddr::V4(Ipv4Addr::LOCALHOST),
        IpAddr::V6(ip) if ip.is_unspecified() => IpAddr::V6(Ipv6Addr::LOCALHOST),
        other => other,
    };
    SocketAddr::new(ip, local_addr.port())
}

fn mask_token(token: &str) -> String {
    if token.len() <= 10 {
        return "********".to_string();
    }
    let head = &token[..6];
    let tail = &token[token.len().saturating_sub(4)..];
    format!("{head}****{tail}")
}

async fn runtime_config_handler(
    Extension(state): Extension<Arc<DesktopWebState>>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    Ok(Json(json!({
        "api_base": state.runtime.api_base,
        "ws_base": state.runtime.ws_base,
        "token": state.runtime.token,
        "desktop_token": state.runtime.desktop_token,
        "user_id": state.runtime.user_id,
        "workspace_root": state.runtime.workspace_root,
        "mode": state.runtime.mode,
        "remote_enabled": state.runtime.remote_enabled,
        "remote_connected": state.runtime.remote_connected,
        "remote_server_base_url": state.runtime.remote_server_base_url,
        "remote_role_name": state.runtime.remote_role_name,
        "remote_error": state.runtime.remote_error,
    })))
}

async fn bootstrap_handler(
    Extension(state): Extension<Arc<DesktopWebState>>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    Ok(Json(json!({ "data": state.runtime })))
}

async fn frontend_index_handler(Extension(state): Extension<Arc<DesktopWebState>>) -> Response {
    match state.index_html.as_ref() {
        Some(html) => Html(html.clone()).into_response(),
        None => frontend_missing_response(&state),
    }
}

async fn frontend_missing_handler(Extension(state): Extension<Arc<DesktopWebState>>) -> Response {
    frontend_missing_response(&state)
}

fn frontend_missing_response(state: &DesktopWebState) -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(json!({
            "error": "FRONTEND_NOT_FOUND",
            "message": "frontend assets not found, please place frontend/dist next to wunder-desktop",
            "data": {
                "web_base": state.runtime.web_base,
                "api_base": state.runtime.api_base,
            }
        })),
    )
        .into_response()
}

async fn desktop_token_guard(
    State(state): State<Arc<DesktopGuardState>>,
    request: Request<Body>,
    next: Next,
) -> Result<Response, StatusCode> {
    if request.method() == Method::OPTIONS {
        return Ok(next.run(request).await);
    }

    let provided = extract_request_token(request.headers(), request.uri().query());
    if provided.as_deref() == Some(state.token.as_str()) {
        return Ok(next.run(request).await);
    }

    Ok((
        StatusCode::UNAUTHORIZED,
        axum::Json(json!({
            "error": "UNAUTHORIZED",
            "message": "invalid desktop token"
        })),
    )
        .into_response())
}

fn extract_request_token(headers: &HeaderMap, query: Option<&str>) -> Option<String> {
    if let Some(value) = headers
        .get("x-api-key")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Some(value.to_string());
    }

    if let Some(value) = headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        if let Some(token) = value.strip_prefix("Bearer ") {
            let token = token.trim();
            if !token.is_empty() {
                return Some(token.to_string());
            }
        }
    }

    if let Some(value) = headers
        .get("sec-websocket-protocol")
        .and_then(|value| value.to_str().ok())
    {
        for item in value.split(',') {
            let item = item.trim();
            if let Some(token) = item.strip_prefix("wunder-auth.") {
                let token = token.trim();
                if !token.is_empty() {
                    return Some(token.to_string());
                }
            }
        }
    }

    if let Some(query) = query {
        for (key, value) in url::form_urlencoded::parse(query.as_bytes()) {
            if (key == "access_token" || key == "api_key") && !value.trim().is_empty() {
                return Some(value.to_string());
            }
        }
    }

    None
}
