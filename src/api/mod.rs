// API 路由汇总入口，按领域拆分以保持结构清晰。
pub mod a2a;
pub mod admin;
pub mod core;
pub mod evaluation;
pub mod user_tools;
pub mod workspace;

use crate::state::AppState;
use axum::Router;
use std::sync::Arc;

pub fn build_router(state: Arc<AppState>) -> Router<Arc<AppState>> {
    Router::new()
        .merge(core::router())
        .merge(workspace::router())
        .merge(admin::router())
        .merge(evaluation::router())
        .merge(user_tools::router())
        .merge(a2a::router())
        .merge(crate::mcp::router(state.clone()))
        .with_state(state)
}
