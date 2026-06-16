#![allow(dead_code)]
#![allow(clippy::result_large_err)]
// Library entrypoint for integration tests and internal reuse.
pub mod api;
mod channels;
mod core;
mod gateway;
mod lsp;
mod ops;
mod orchestrator;
pub mod request_limits;
pub mod sandbox;
mod services;
pub mod storage;

pub use api::{build_desktop_router, build_router};
pub use channels::ChannelHub;
pub use core::{
    approval, approval_registry, auth, blocking, bounded_queue, command_utils, config,
    config_store, dpi, drawio_config, exec_policy, i18n, logging, long_task, onlyoffice_config,
    path_utils, repo_assets, runtime_metrics, rustls_provider, schemas, shutdown, state,
    token_utils,
};
pub use ops::{benchmark, monitor, performance, throughput};
pub use orchestrator::constants as orchestrator_constants;
pub use services::{
    a2a_store, admin_skills, attachment, beeroom_realtime, browser, cron, desktop_lan,
    desktop_runtime_recovery, doc2md, drawio, goal, history, knowledge, llm, mcp, memory,
    multimodal_models, onlyoffice, org_units, presence, prompting, ragflow_knowledge, runtime,
    sim_lab, skills, swarm, tools, user_access, user_leveling, user_plaza, user_prompt_templates,
    user_store, user_tools, user_world, vector_knowledge, workspace,
};
pub use wunder_core as stable_core;
