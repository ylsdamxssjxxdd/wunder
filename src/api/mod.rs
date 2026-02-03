// API 路由汇总入口，按领域拆分以保持结构清晰。
pub mod a2a;
pub mod admin;
pub(crate) mod attachment_convert;
pub mod auth;
pub mod channel;
pub mod chat;
pub mod chat_ws;
pub mod core;
pub mod core_ws;
pub mod doc2md;
pub mod evaluation;
pub mod temp_dir;
pub mod user_agents;
pub mod user_context;
pub mod user_tools;
pub mod workspace;
pub(crate) mod ws_helpers;

use crate::state::AppState;
use axum::Router;
use std::sync::Arc;

pub fn build_router(state: Arc<AppState>) -> Router<Arc<AppState>> {
    Router::new()
        .merge(auth::router())
        .merge(channel::router())
        .merge(chat::router())
        .merge(chat_ws::router())
        .merge(core_ws::router())
        .merge(core::router())
        .merge(doc2md::router())
        .merge(temp_dir::router())
        .merge(workspace::router())
        .merge(admin::router())
        .merge(evaluation::router())
        .merge(user_tools::router())
        .merge(user_agents::router())
        .merge(a2a::router())
        .merge(crate::mcp::router(state.clone()))
        .with_state(state)
}
