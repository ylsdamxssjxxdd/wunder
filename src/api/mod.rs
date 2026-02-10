// API 路由聚合入口，按领域拆分以保持结构清晰。
pub mod a2a;
pub mod admin;
pub mod admin_sim_lab;
pub mod admin_swarm;
pub(crate) mod attachment_convert;
pub mod auth;
pub mod channel;
pub mod chat;
pub mod chat_ws;
pub mod core;
pub mod core_ws;
pub mod cron;
pub mod doc2md;
pub(crate) mod errors;
pub mod evaluation;
pub mod external_links;
pub mod gateway_ws;
pub mod team_runs;
pub mod temp_dir;
pub mod user_agents;
pub mod user_channels;
pub mod user_context;
pub mod user_tools;
pub mod workspace;
pub(crate) mod ws_helpers;
pub(crate) mod ws_log;

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
        .merge(cron::router())
        .merge(doc2md::router())
        .merge(gateway_ws::router())
        .merge(temp_dir::router())
        .merge(workspace::router())
        .merge(admin::router())
        .merge(admin_swarm::router())
        .merge(admin_sim_lab::router())
        .merge(evaluation::router())
        .merge(external_links::router())
        .merge(user_tools::router())
        .merge(user_agents::router())
        .merge(team_runs::router())
        .merge(user_channels::router())
        .merge(a2a::router())
        .merge(crate::mcp::router(state.clone()))
        .with_state(state)
}

/// Build a reduced router for local desktop mode.
///
/// It intentionally omits admin/channel/gateway/cron routes to keep the local
/// surface minimal while still reusing the same orchestrator/tooling pipeline.
pub fn build_desktop_router(state: Arc<AppState>) -> Router<Arc<AppState>> {
    Router::new()
        .merge(chat::router())
        .merge(chat_ws::router())
        .merge(core_ws::router())
        .merge(core::router())
        .merge(temp_dir::router())
        .merge(workspace::router())
        .merge(user_tools::router())
        .merge(user_agents::router())
        .merge(user_channels::router())
        .merge(crate::mcp::router(state.clone()))
        .with_state(state)
}
