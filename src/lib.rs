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
mod sandbox;
mod services;
pub mod storage;

pub use api::{build_desktop_router, build_router};
pub use channels::ChannelHub;
pub use core::{
    approval, auth, command_utils, config, config_store, exec_policy, i18n, path_utils, schemas,
    shutdown, state, token_utils,
};
pub use ops::{evaluation, evaluation_runner, monitor, performance, throughput};
pub use orchestrator::constants as orchestrator_constants;
pub use services::{
    a2a_store, attachment, cron, doc2md, history, knowledge, llm, mcp, memory, org_units,
    prompting, sim_lab, skills, swarm, tools, user_access, user_store, user_tools, user_world,
    vector_knowledge, workspace,
};
