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
pub use services::archive_extract;
pub use services::companions;
pub use services::memory_fragments;
pub use services::tool_result_display;
pub mod storage;

pub use api::{build_desktop_router, build_router};
pub use channels::ChannelHub;
pub use core::{
    approval, approval_registry, auth, blocking, bounded_queue, command_utils, config,
    config_store, dpi, drawio_config, exec_policy, i18n, logging, long_task, onlyoffice_config,
    path_utils, repo_assets, runtime_metrics, runtime_tuning, rustls_provider, schemas, shutdown,
    state, token_utils,
};
pub use lsp::LspManager;
pub use ops::{benchmark, monitor, performance, throughput};
pub use orchestrator::constants as orchestrator_constants;
pub use services::runtime::thread::{ThreadCancelSettlement, ThreadSubmitOutcome};
pub use services::skill_archive;
pub use services::stress_thread::{validate_stress_params, MAX_MODEL_ROUNDS, MAX_USER_ROUNDS};
pub use services::subagents::{control_parent_subagents, list_parent_subagents};
pub use services::thread_catalog::{
    StatusSignals, ThreadCatalogService, ThreadListQuery, ThreadPage, ThreadPendingReason,
    ThreadSnapshot, ThreadStatus, ThreadWriteAccess,
};
pub use services::thread_change_feeder::{watch_thread_changes, ThreadChangeFrame};
pub use services::tools::command_sessions::{
    find_winpty_library, DesktopTerminalBackend, DesktopTerminalFrame, DesktopTerminalService,
    DesktopTerminalSnapshot, DesktopTerminalStartSpec, DesktopTerminalStatus,
};
pub use services::user_world::UserWorldRealtimeEvent;
pub use services::work_state_reset::{
    reset_user_work_state, ResetWorkStateSession, ResetWorkStateSummary,
};
pub use services::worker_card_settings;
pub use services::{
    admin_skills, agent_management, attachment, browser, cloud, cron, default_agent_protocol,
    default_tool_profile, desktop_lan, desktop_runtime_recovery, doc2md, drawio, goal, history,
    interlink, knowledge, llm, mcp, memory, multimodal_models, onlyoffice, org_units, presence, prompting,
    ragflow_knowledge, runtime, skills, tools, user_access, user_leveling, user_prompt_templates,
    user_store, user_tools, user_world, vector_knowledge, virtual_llm, workspace,
};
pub use wunder_core as stable_core;
