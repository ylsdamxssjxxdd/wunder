#![allow(dead_code)]
#![allow(clippy::result_large_err)]
// Library entrypoint for integration tests and internal reuse.
mod api;
mod channels;
mod core;
mod gateway;
mod lsp;
mod ops;
mod orchestrator;
pub mod request_limits;
mod sandbox;
mod services;
pub mod storage;

pub use api::{build_desktop_router, build_router};
pub use channels::ChannelHub;
pub use core::{
    approval, approval_registry, auth, command_utils, config, config_store, dpi, exec_policy, i18n,
    path_utils, rustls_provider, schemas, shutdown, state, token_utils,
};
pub use ops::{benchmark, monitor, performance, throughput};
pub use orchestrator::constants as orchestrator_constants;
pub use services::{
    a2a_store, attachment, browser, cron, desktop_lan, doc2md, history, knowledge, llm, mcp,
    memory, org_units, presence, projection, prompting, runtime, sim_lab, skills, swarm, tools,
    user_access, user_prompt_templates, user_store, user_tools, user_world, vector_knowledge,
    workspace,
};
