mod args;
mod runtime;

use anyhow::{anyhow, Context, Result};
use args::DesktopArgs;
use axum::body::Body;
use axum::extract::State;
use axum::http::{header::AUTHORIZATION, HeaderMap, Method, Request, StatusCode};
use axum::middleware::{from_fn_with_state, Next};
use axum::response::{IntoResponse, Response};
use clap::Parser;
use runtime::DesktopRuntime;
use serde_json::json;
use std::net::SocketAddr;
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;
use tracing::info;
use tracing_subscriber::EnvFilter;

#[derive(Clone)]
struct DesktopGuardState {
    token: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();
    let args = DesktopArgs::parse();
    let runtime = DesktopRuntime::init(&args).await?;

    let guard_state = Arc::new(DesktopGuardState {
        token: runtime.desktop_token.clone(),
    });

    let app = wunder_server::build_desktop_router(runtime.state.clone())
        .layer(from_fn_with_state(guard_state, desktop_token_guard))
        .layer(TraceLayer::new_for_http())
        .layer(build_cors())
        .with_state(runtime.state.clone());

    let bind_host = sanitize_host(&args.host)?;
    let bind_addr = format!("{bind_host}:{}", args.port);
    let listener = tokio::net::TcpListener::bind(bind_addr.as_str())
        .await
        .with_context(|| format!("bind desktop bridge failed: {bind_addr}"))?;
    let local_addr = listener
        .local_addr()
        .context("resolve desktop bridge local addr failed")?;

    print_runtime_banner(&runtime, local_addr, args.print_token);

    let server = axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(wunder_server::shutdown::shutdown_signal());

    if let Err(err) = server.await {
        return Err(anyhow!("desktop bridge exited with error: {err}"));
    }

    Ok(())
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let _ = tracing_subscriber::fmt().with_env_filter(filter).try_init();
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

fn build_cors() -> CorsLayer {
    CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any)
}

fn print_runtime_banner(runtime: &DesktopRuntime, addr: SocketAddr, print_token: bool) {
    let api_base = format!("http://{addr}/wunder");
    info!("wunder-desktop ready: {api_base}");

    println!("wunder-desktop ready");
    println!("- api_base: {api_base}");
    println!("- app_dir: {}", runtime.app_dir.to_string_lossy());
    println!("- temp_root: {}", runtime.temp_root.to_string_lossy());
    println!(
        "- workspace_root: {}",
        runtime.workspace_root.to_string_lossy()
    );
    println!("- repo_root: {}", runtime.repo_root.to_string_lossy());
    println!("- user_id: {}", runtime.user_id);

    if print_token {
        println!("- desktop_token: {}", runtime.desktop_token);
    } else {
        println!("- desktop_token: {}", mask_token(&runtime.desktop_token));
        println!("  (use --print-token to print full token)");
    }
}

fn mask_token(token: &str) -> String {
    if token.len() <= 10 {
        return "********".to_string();
    }
    let head = &token[..6];
    let tail = &token[token.len().saturating_sub(4)..];
    format!("{head}****{tail}")
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
