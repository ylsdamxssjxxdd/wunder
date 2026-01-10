// Rust 入口：挂载鉴权、静态资源与 API 路由。
mod a2a_store;
mod api;
mod attachment;
mod auth;
mod config;
mod config_store;
mod history;
mod i18n;
mod knowledge;
mod llm;
mod mcp;
mod memory;
mod monitor;
mod orchestrator;
mod orchestrator_constants;
mod path_utils;
mod prompting;
mod sandbox;
mod schemas;
mod shutdown;
mod skills;
mod state;
mod storage;
mod throughput;
mod token_utils;
mod tools;
mod user_tools;
mod workspace;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::middleware::{from_fn, from_fn_with_state, Next};
use axum::response::{IntoResponse, Response};
use axum::Router;
use config::Config;
use config_store::ConfigStore;
use futures::FutureExt;
use shutdown::shutdown_signal;
use state::AppState;
use std::any::Any as StdAny;
use std::panic::AssertUnwindSafe;
use std::path::PathBuf;
use std::sync::Arc;
use tower_http::cors::{AllowHeaders, AllowMethods, AllowOrigin, Any, CorsLayer};
use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::TraceLayer;
use tracing::{error, info, warn};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 初始化配置存储，用于鉴权与路由行为保持一致。
    let config_store = ConfigStore::new(ConfigStore::override_path_default());
    let config = config_store.get().await;
    init_tracing(&config);
    let state = Arc::new(AppState::new(config_store.clone(), config.clone())?);

    // 挂载 API 路由与静态资源入口。
    let app = api::build_router(state.clone());
    let app = mount_static_file(app, "web/simple-chat/index.html", "/");
    let app = mount_static(app, "web", "/wunder/web");
    let app = mount_static(app, "docs/ppt", "/wunder/ppt");
    let app = mount_static(app, "docs/ppt-en", "/wunder/ppt-en");

    let cors = build_cors(&config);
    let app = app
        .layer(from_fn_with_state(state.clone(), api_key_guard))
        .layer(from_fn_with_state(state.clone(), language_guard))
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .layer(from_fn(panic_guard))
        .with_state(state.clone());

    let addr = bind_address(&config);
    let listener = tokio::net::TcpListener::bind(addr.as_str()).await?;
    info!("Rust API 服务已启动: http://{addr}");

    let server = axum::serve(listener, app).with_graceful_shutdown(shutdown_signal());
    if let Err(err) = server.await {
        warn!("服务退出异常: {err}");
    }

    Ok(())
}

fn init_tracing(config: &Config) {
    // ???? RUST_LOG ??????????? info?
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

fn mount_static<S>(app: Router<S>, dir: &str, route: &str) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    let path = PathBuf::from(dir);
    if path.exists() {
        // 目录存在时才挂载，避免容器裁剪后启动报错。
        let service = ServeDir::new(path).append_index_html_on_directories(true);
        app.nest_service(route, service)
    } else {
        app
    }
}

fn mount_static_file<S>(app: Router<S>, file: &str, route: &str) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    let path = PathBuf::from(file);
    if path.exists() {
        app.route_service(route, ServeFile::new(path))
    } else {
        app
    }
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
        Some(origins) if origins.iter().any(|value| *value == "*") => {
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
        Some(methods) if methods.iter().any(|value| *value == "*") => {
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
        Some(headers) if headers.iter().any(|value| *value == "*") => {
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
    if !auth::is_protected_path(path) {
        return Ok(next.run(request).await);
    }

    // 按配置校验 API Key，避免误放行敏感接口。
    let config = state.config_store.get().await;
    let expected = match config.api_key() {
        Some(value) => value,
        None => {
            let message = i18n::t("error.api_key_missing");
            return Ok(auth_error(StatusCode::INTERNAL_SERVER_ERROR, &message));
        }
    };

    let provided = auth::extract_api_key(request.headers()).unwrap_or_default();
    if provided != expected {
        let message = i18n::t("error.api_key_invalid");
        return Ok(auth_error(StatusCode::UNAUTHORIZED, &message));
    }

    Ok(next.run(request).await)
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
            if key == "lang" || key == "language" {
                if !value.trim().is_empty() {
                    candidates.push(value.to_string());
                }
            }
        }
    }
    i18n::resolve_language(candidates.iter().map(|value| value.as_str()))
}

fn auth_error(status: StatusCode, message: &str) -> Response<Body> {
    let payload = serde_json::json!({ "detail": { "message": message } });
    Response::builder()
        .status(status)
        .header("content-type", "application/json; charset=utf-8")
        .body(Body::from(payload.to_string()))
        .unwrap_or_else(|_| Response::new(Body::empty()))
}
