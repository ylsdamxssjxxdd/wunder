// Rust 入口：挂载鉴权、静态资源与 API 路由。
#![cfg_attr(test, allow(dead_code))]
#![allow(clippy::result_large_err)]
mod api;
mod channels;
mod core;
mod gateway;
mod lsp;
mod ops;
mod orchestrator;
mod request_limits;
mod sandbox;
mod services;
mod storage;

pub use channels::ChannelHub;
pub use core::{
    auth, command_utils, config, config_store, exec_policy, i18n, path_utils, schemas, shutdown,
    state, token_utils,
};
pub use ops::{benchmark, monitor, performance, throughput};
pub use orchestrator::constants as orchestrator_constants;
pub use services::{
    a2a_store, attachment, cron, doc2md, history, knowledge, llm, mcp, memory, org_units,
    prompting, sim_lab, skills, swarm, tools, user_access, user_store, user_tools, user_world,
    vector_knowledge, workspace,
};

use axum::body::Body;
use axum::extract::OriginalUri;
use axum::http::{Request, StatusCode};
use axum::middleware::{from_fn, from_fn_with_state, Next};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::get;
use axum::Router;
use config::{Config, McpToolSpec};
use config_store::ConfigStore;
use futures::FutureExt;
use shutdown::shutdown_signal;
use state::AppState;
use std::any::Any as StdAny;
use std::net::SocketAddr;
use std::panic::AssertUnwindSafe;
use std::path::PathBuf;
use std::sync::Arc;
use tower_http::cors::{AllowHeaders, AllowMethods, AllowOrigin, Any, CorsLayer};
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;
use tracing::{error, info, warn};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    core::rustls_provider::install_process_default_provider();
    // 初始化配置存储，用于鉴权与路由行为保持一致。
    let config_store = ConfigStore::new(ConfigStore::override_path_default());
    let config = config_store.get().await;
    init_tracing(&config);
    let server_mode = resolve_server_mode(&config);
    if server_mode == "sandbox" {
        let addr = bind_address(&config);
        let app = sandbox::server::build_router()
            .layer(TraceLayer::new_for_http())
            .layer(from_fn(panic_guard));
        let listener = tokio::net::TcpListener::bind(addr.as_str()).await?;
        info!("Sandbox 服务已启动: http://{addr}");
        let server = axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .with_graceful_shutdown(shutdown_signal());
        if let Err(err) = server.await {
            warn!("Sandbox 服务退出异常: {err}");
        }
        return Ok(());
    }
    let state = Arc::new(AppState::new(config_store.clone(), config.clone())?);
    state.lsp_manager.sync_with_config(&config).await;
    tokio::spawn(hydrate_enabled_mcp_tool_specs(state.clone()));

    // 挂载 API 路由与静态资源入口。
    let app = api::build_router(state.clone());
    let app = mount_simple_chat_disabled(app);
    let app = mount_trailing_slash_redirect(app, "/wunder/ppt", "/wunder/ppt/");
    let app = mount_trailing_slash_redirect(app, "/wunder/ppt-en", "/wunder/ppt-en/");
    let app = mount_static(app, "frontend/src/assets/qq-avatars", "/assets/qq-avatars");
    let app = mount_static(
        app,
        "frontend/src/assets/qq-avatars",
        "/wunder/assets/qq-avatars",
    );
    let app = mount_static(
        app,
        "frontend/src/assets/agent-avatars",
        "/assets/agent-avatars",
    );
    let app = mount_static(
        app,
        "frontend/src/assets/agent-avatars",
        "/wunder/assets/agent-avatars",
    );
    let app = mount_static(app, "web", "/");
    let app = mount_static(app, "docs/ppt", "/wunder/ppt");
    let app = mount_static(app, "docs/ppt-en", "/wunder/ppt-en");

    let cors = build_cors(&config);
    let app = app
        .layer(from_fn_with_state(state.clone(), api_key_guard))
        .layer(from_fn_with_state(state.clone(), language_guard))
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .layer(from_fn(panic_guard));

    let addr = bind_address(&config);
    let listener = tokio::net::TcpListener::bind(addr.as_str()).await?;
    info!("Rust API 服务已启动: http://{addr}");

    let server = axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal());
    if let Err(err) = server.await {
        warn!("服务退出异常: {err}");
    }

    Ok(())
}

fn init_tracing(config: &Config) {
    let default_level = config.observability.log_level.trim();
    let default_level = if default_level.is_empty() {
        "info".to_string()
    } else {
        default_level.to_lowercase()
    };
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(default_level));
    tracing_subscriber::fmt().with_env_filter(filter).init();
}

fn bind_address(config: &Config) -> String {
    // 保留环境变量覆盖，便于容器化部署。
    let host = std::env::var("WUNDER_HOST").unwrap_or_else(|_| config.server.host.clone());
    let port = std::env::var("WUNDER_PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(config.server.port);
    format!("{host}:{port}")
}

fn resolve_server_mode(config: &Config) -> String {
    let env_mode = std::env::var("WUNDER_SERVER_MODE").ok();
    let raw = env_mode
        .as_deref()
        .unwrap_or(config.server.mode.as_str())
        .trim();
    if raw.is_empty() {
        "api".to_string()
    } else {
        raw.to_ascii_lowercase()
    }
}

fn should_hydrate_mcp_tool_specs(server: &config::McpServerConfig) -> bool {
    server.enabled
        && server.tool_specs.is_empty()
        && !server.name.trim().is_empty()
        && !server.endpoint.trim().is_empty()
}

fn to_mcp_tool_specs(specs: Vec<schemas::ToolSpec>) -> Vec<McpToolSpec> {
    specs
        .into_iter()
        .map(|spec| McpToolSpec {
            name: spec.name,
            description: spec.description,
            input_schema: serde_yaml::to_value(spec.input_schema)
                .unwrap_or(serde_yaml::Value::Null),
        })
        .collect()
}

async fn hydrate_enabled_mcp_tool_specs(state: Arc<AppState>) {
    for attempt in 1..=5 {
        let config = state.config_store.get().await;
        let timeout_s = if config.mcp.timeout_s > 0 {
            config.mcp.timeout_s.clamp(10, 300)
        } else {
            120
        };
        let pending = config
            .mcp
            .servers
            .iter()
            .filter(|server| should_hydrate_mcp_tool_specs(server))
            .cloned()
            .collect::<Vec<_>>();
        if pending.is_empty() {
            return;
        }

        let mut hydrated = Vec::new();
        for server in pending {
            let result = tokio::time::timeout(
                std::time::Duration::from_secs(timeout_s),
                crate::mcp::fetch_tools(&config, &server),
            )
            .await;
            match result {
                Ok(Ok(specs)) if !specs.is_empty() => {
                    hydrated.push((server.name.clone(), to_mcp_tool_specs(specs)));
                }
                Ok(Ok(_)) => {
                    warn!(
                        "startup MCP tool hydration returned no tools for server {}",
                        server.name
                    );
                }
                Ok(Err(err)) => {
                    warn!(
                        "startup MCP tool hydration failed for server {}: {}",
                        server.name, err
                    );
                }
                Err(_) => {
                    warn!(
                        "startup MCP tool hydration timed out for server {} after {}s",
                        server.name, timeout_s
                    );
                }
            }
        }

        if !hydrated.is_empty() {
            let names = hydrated
                .iter()
                .map(|(name, _)| name.clone())
                .collect::<Vec<_>>();
            if let Err(err) = state
                .config_store
                .update(|config| {
                    for (name, specs) in &hydrated {
                        if let Some(server) = config
                            .mcp
                            .servers
                            .iter_mut()
                            .find(|item| item.name == *name)
                        {
                            if server.tool_specs.is_empty() {
                                server.tool_specs = specs.clone();
                            }
                        }
                    }
                })
                .await
            {
                warn!("failed to persist hydrated MCP tool specs: {err}");
            } else {
                info!("hydrated MCP tool specs at startup: {}", names.join(", "));
            }
        }

        let has_remaining = state
            .config_store
            .get()
            .await
            .mcp
            .servers
            .iter()
            .any(should_hydrate_mcp_tool_specs);
        if !has_remaining {
            return;
        }
        if attempt < 5 {
            tokio::time::sleep(std::time::Duration::from_secs(3)).await;
        }
    }
}

fn mount_static<S>(app: Router<S>, dir: &str, route: &str) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    let mut path = PathBuf::from(dir);
    if !path.exists() && path.is_relative() {
        let manifest_fallback = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(dir);
        if manifest_fallback.exists() {
            path = manifest_fallback;
        }
    }
    if path.exists() {
        // 目录存在时才挂载，避免容器裁剪后启动报错。
        let service = ServeDir::new(path).append_index_html_on_directories(true);
        let trimmed = route.trim_end_matches('/');
        if trimmed.is_empty() {
            app.fallback_service(service)
        } else {
            let nested = Router::new().fallback_service(service);
            app.nest(&format!("{trimmed}/"), nested)
        }
    } else {
        app
    }
}

fn mount_simple_chat_disabled<S>(app: Router<S>) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    app.route("/simple-chat", get(simple_chat_disabled))
        .route("/simple-chat/", get(simple_chat_disabled))
        .route("/simple-chat/{*path}", get(simple_chat_disabled))
}

fn mount_trailing_slash_redirect<S>(app: Router<S>, from: &str, to: &'static str) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    app.route(
        from,
        get(move |uri: OriginalUri| async move {
            let query = uri
                .0
                .query()
                .map(|value| format!("?{value}"))
                .unwrap_or_default();
            Redirect::permanent(&format!("{to}{query}"))
        }),
    )
}

async fn simple_chat_disabled() -> impl IntoResponse {
    (StatusCode::GONE, "simple-chat is temporarily disabled")
}

fn build_cors(config: &Config) -> CorsLayer {
    // 读取配置并转换为 tower-http 的 CORS 规则。
    let mut cors = CorsLayer::new();

    match config
        .cors
        .allow_origins
        .as_ref()
        .map(|value| value.iter().map(|item| item.as_str()).collect::<Vec<_>>())
    {
        Some(origins) if origins.contains(&"*") => {
            cors = cors.allow_origin(Any);
        }
        Some(origins) => {
            let values = origins
                .iter()
                .filter_map(|value| value.parse().ok())
                .collect::<Vec<_>>();
            if !values.is_empty() {
                cors = cors.allow_origin(AllowOrigin::list(values));
            }
        }
        None => {
            cors = cors.allow_origin(Any);
        }
    }

    match config
        .cors
        .allow_methods
        .as_ref()
        .map(|value| value.iter().map(|item| item.as_str()).collect::<Vec<_>>())
    {
        Some(methods) if methods.contains(&"*") => {
            cors = cors.allow_methods(Any);
        }
        Some(methods) => {
            let values = methods
                .iter()
                .filter_map(|value| value.parse().ok())
                .collect::<Vec<_>>();
            if !values.is_empty() {
                cors = cors.allow_methods(AllowMethods::list(values));
            }
        }
        None => {
            cors = cors.allow_methods(Any);
        }
    }

    match config
        .cors
        .allow_headers
        .as_ref()
        .map(|value| value.iter().map(|item| item.as_str()).collect::<Vec<_>>())
    {
        Some(headers) if headers.contains(&"*") => {
            cors = cors.allow_headers(Any);
        }
        Some(headers) => {
            let values = headers
                .iter()
                .filter_map(|value| value.parse().ok())
                .collect::<Vec<_>>();
            if !values.is_empty() {
                cors = cors.allow_headers(AllowHeaders::list(values));
            }
        }
        None => {
            cors = cors.allow_headers(Any);
        }
    }

    if config.cors.allow_credentials.unwrap_or(false) {
        cors = cors.allow_credentials(true);
    }

    cors
}

async fn api_key_guard(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
    request: Request<Body>,
    next: Next,
) -> Result<Response, StatusCode> {
    if request.method() == axum::http::Method::OPTIONS {
        return Ok(next.run(request).await);
    }

    let path = request.uri().path();
    if !auth::is_admin_path(path) {
        return Ok(next.run(request).await);
    }

    let headers = request.headers();
    let config = state.config_store.get().await;
    let expected = config.api_key();
    if let Some(expected) = expected.as_ref() {
        let provided = auth::extract_api_key(headers).unwrap_or_default();
        if provided == *expected {
            return Ok(next.run(request).await);
        }
    }

    if let Some(token) = auth::extract_bearer_token(headers) {
        let user_store = state.user_store.clone();
        let token_for_lookup = token.clone();
        if let Ok(Ok(Some(user))) =
            tokio::task::spawn_blocking(move || user_store.authenticate_token(&token_for_lookup))
                .await
        {
            if crate::user_store::UserStore::is_admin(&user) {
                return Ok(next.run(request).await);
            }
            if auth::is_leader_path(path) {
                let user_store = state.user_store.clone();
                let units =
                    match tokio::task::spawn_blocking(move || user_store.list_org_units()).await {
                        Ok(Ok(units)) => units,
                        Ok(Err(err)) => {
                            let message = format!("org unit lookup failed: {err}");
                            return Ok(auth_error(StatusCode::INTERNAL_SERVER_ERROR, &message));
                        }
                        Err(err) => {
                            let message = format!("org unit lookup join failed: {err}");
                            return Ok(auth_error(StatusCode::INTERNAL_SERVER_ERROR, &message));
                        }
                    };
                if units
                    .iter()
                    .any(|unit| unit.leader_ids.iter().any(|id| id == &user.user_id))
                {
                    return Ok(next.run(request).await);
                }
            }
        }
    }

    if expected.is_none() {
        let message = i18n::t("error.api_key_missing");
        return Ok(auth_error(StatusCode::INTERNAL_SERVER_ERROR, &message));
    }

    let message = i18n::t("error.api_key_invalid");
    Ok(auth_error(StatusCode::UNAUTHORIZED, &message))
}

async fn language_guard(
    _state: axum::extract::State<Arc<AppState>>,
    request: Request<Body>,
    next: Next,
) -> Result<Response, StatusCode> {
    let language = resolve_language_from_request(&request);
    let response =
        i18n::with_language(language.clone(), async move { next.run(request).await }).await;
    let mut response = response;
    if !response.headers().contains_key("content-language") {
        if let Ok(value) = language.parse() {
            response.headers_mut().insert("content-language", value);
        }
    }
    Ok(response)
}

async fn panic_guard(request: Request<Body>, next: Next) -> Result<Response, StatusCode> {
    let method = request.method().clone();
    let path = request.uri().path().to_string();
    let language = resolve_language_from_request(&request);
    let result = AssertUnwindSafe(next.run(request)).catch_unwind().await;
    match result {
        Ok(response) => Ok(response),
        Err(panic) => {
            let detail = panic_message(panic.as_ref());
            error!("panic while handling {method} {path}: {detail}");
            let message = i18n::with_language(language, async { i18n::t("error.internal") }).await;
            Ok((StatusCode::INTERNAL_SERVER_ERROR, message).into_response())
        }
    }
}

fn panic_message(panic: &(dyn StdAny + Send)) -> String {
    if let Some(message) = panic.downcast_ref::<&str>() {
        return message.to_string();
    }
    if let Some(message) = panic.downcast_ref::<String>() {
        return message.clone();
    }
    "unknown panic".to_string()
}

fn resolve_language_from_request(request: &Request<Body>) -> String {
    let headers = request.headers();
    let mut candidates: Vec<String> = Vec::new();
    if let Some(value) = headers
        .get("x-wunder-language")
        .and_then(|v| v.to_str().ok())
    {
        candidates.push(value.to_string());
    }
    if let Some(value) = headers.get("accept-language").and_then(|v| v.to_str().ok()) {
        candidates.push(value.to_string());
    }
    if let Some(value) = headers
        .get("content-language")
        .and_then(|v| v.to_str().ok())
    {
        candidates.push(value.to_string());
    }
    if let Some(query) = request.uri().query() {
        for (key, value) in url::form_urlencoded::parse(query.as_bytes()) {
            if (key == "lang" || key == "language") && !value.trim().is_empty() {
                candidates.push(value.to_string());
            }
        }
    }
    i18n::resolve_language(candidates.iter().map(|value| value.as_str()))
}

fn auth_error(status: StatusCode, message: &str) -> Response {
    api::errors::error_response(status, message)
}
