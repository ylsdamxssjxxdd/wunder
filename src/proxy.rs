// 代理层：将请求透传给后端服务，并兼容 SSE 等流式输出。
use crate::state::AppState;
use axum::body::{to_bytes, Body};
use axum::http::{HeaderMap, Request, Response, StatusCode};
use futures::StreamExt;
use serde_json::json;
use std::sync::Arc;
use tracing::{error, warn};

pub async fn proxy_handler(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
    request: Request<Body>,
) -> Response<Body> {
    // 缺少后端时直接返回 503，避免无限重试。
    let base_url = match &state.backend_base {
        Some(url) => url.clone(),
        None => return service_unavailable(),
    };

    let path = request
        .uri()
        .path_and_query()
        .map(|value| value.as_str())
        .unwrap_or("/");
    let target_url = format!("{base_url}{path}");

    // 迁移期先做一次性读取，保证表单/文件上传可转发。
    let max_body = proxy_max_body();
    let (parts, body) = request.into_parts();
    let bytes = match to_bytes(body, max_body).await {
        Ok(data) => data,
        Err(err) => {
            error!("读取请求体失败: {err}");
            return bad_gateway();
        }
    };

    let mut builder = state
        .client
        .request(parts.method, target_url)
        .body(bytes);
    builder = apply_headers(builder, &parts.headers);

    let response = match builder.send().await {
        Ok(resp) => resp,
        Err(err) => {
            error!("后端请求失败: {err}");
            return bad_gateway();
        }
    };

    let status = response.status();
    let headers = response.headers().clone();
    // 直接透传流式响应，兼容 SSE 输出。
    let stream = response
        .bytes_stream()
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::Other, err));

    let mut builder = Response::builder().status(status);
    for (name, value) in headers.iter() {
        if name.as_str().eq_ignore_ascii_case("transfer-encoding") {
            continue;
        }
        builder = builder.header(name, value);
    }

    match builder.body(Body::from_stream(stream)) {
        Ok(resp) => resp,
        Err(err) => {
            error!("构建代理响应失败: {err}");
            bad_gateway()
        }
    }
}

fn apply_headers(
    mut builder: reqwest::RequestBuilder,
    headers: &HeaderMap,
) -> reqwest::RequestBuilder {
    // 过滤 Host，避免后端误判来源。
    for (name, value) in headers {
        if name.as_str().eq_ignore_ascii_case("host") {
            continue;
        }
        builder = builder.header(name, value);
    }
    builder
}

fn proxy_max_body() -> usize {
    // 默认限制 100MB，避免无上限缓存导致内存爆涨。
    std::env::var("WUNDER_PROXY_MAX_BODY_BYTES")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(100 * 1024 * 1024)
}

fn service_unavailable() -> Response<Body> {
    let payload = json!({
        "message": "后端服务不可用，请检查 WUNDER_PY_BACKEND_URL 或自动启动配置。"
    });
    json_response(StatusCode::SERVICE_UNAVAILABLE, payload)
}

fn bad_gateway() -> Response<Body> {
    let payload = json!({
        "message": "后端请求失败，请检查 Rust 代理与 Python 服务状态。"
    });
    json_response(StatusCode::BAD_GATEWAY, payload)
}

fn json_response(status: StatusCode, payload: serde_json::Value) -> Response<Body> {
    match Response::builder()
        .status(status)
        .header("content-type", "application/json; charset=utf-8")
        .body(Body::from(payload.to_string()))
    {
        Ok(resp) => resp,
        Err(err) => {
            warn!("构建 JSON 响应失败: {err}");
            Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(Body::empty())
                .unwrap_or_else(|_| Response::new(Body::empty()))
        }
    }
}
