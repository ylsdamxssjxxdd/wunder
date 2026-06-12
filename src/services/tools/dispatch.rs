use super::a2a_tool;
use super::catalog::resolve_tool_name;
use super::channel_tool;
use super::command_tool;
use super::context::ToolContext;
use super::file_tool;
use super::knowledge_tool;
use super::lsp_tool;
use super::multimodal_generation_tool;
use super::node_invoke_tool;
use super::panel_tools;
use super::schedule_task_tool;
use super::search_content_tool::search_content;
use super::sessions_yield_tool;
use super::skill_call;
use super::user_world_tool;
use super::{
    agent_swarm, edit_file2, execute_mcp_tool, execute_memory_manager_tool,
    execute_thread_control_tool, execute_user_tool, is_mcp_tool_name, self_status_tool,
    subagent_control,
};
use super::{
    apply_patch_tool, browser_tool, desktop_control, read_image_tool, sleep_tool, web_fetch_tool,
    web_search_tool,
};
use crate::services::goal;
use crate::skills::execute_skill;
use anyhow::{anyhow, Result};
use serde_json::{json, Value};

/// 工具调度入口：优先处理 A2A 与 MCP，再回落到内置工具。
pub async fn execute_tool(context: &ToolContext<'_>, name: &str, args: &Value) -> Result<Value> {
    let _ = context.session_id;
    let canonical = resolve_tool_name(name);
    if let Some(bindings) = context.user_tool_bindings {
        if let Some(alias) = bindings.alias_map.get(&canonical) {
            return execute_user_tool(context, alias, args).await;
        }
    }
    if let Some(skill) = context.skills.get(&canonical) {
        let result = execute_skill(&skill, args, 60).await?;
        context.workspace.mark_tree_dirty(context.workspace_id);
        return Ok(result);
    }
    if a2a_tool::is_a2a_service_tool(&canonical) {
        return a2a_tool::execute_a2a_service_tool(context, &canonical, args).await;
    }
    if is_mcp_tool_name(&canonical) {
        return execute_mcp_tool(context, &canonical, args).await;
    }
    if let Some(base) = knowledge_tool::find_knowledge_base(context.config, &canonical) {
        return knowledge_tool::execute_knowledge_tool(context, &base, args).await;
    }
    execute_builtin_tool(context, &canonical, args).await
}

pub async fn execute_builtin_tool(
    context: &ToolContext<'_>,
    name: &str,
    args: &Value,
) -> Result<Value> {
    let canonical = resolve_tool_name(name);
    match canonical.as_str() {
        canonical if goal::is_goal_tool_name(canonical) => {
            goal::execute_goal_tool(context, &canonical, args).await
        }
        self_status_tool::TOOL_SELF_STATUS => {
            self_status_tool::execute_self_status_tool(context, args).await
        }
        "最终回复" => Ok(json!({
            "answer": args.get("content").and_then(Value::as_str).unwrap_or("").to_string()
        })),
        "执行命令" => command_tool::execute_command(context, args).await,
        "ptc" => command_tool::execute_ptc(context, args).await,
        "列出文件" => file_tool::list_files(context, args).await,
        "搜索内容" => search_content(context, args).await,
        "读取文件" => file_tool::read_files(context, args).await,
        read_image_tool::TOOL_READ_IMAGE => read_image_tool::tool_read_image(context, args).await,
        multimodal_generation_tool::TOOL_GENERATE_SPEECH => {
            multimodal_generation_tool::tool_generate_speech(context, args).await
        }
        multimodal_generation_tool::TOOL_TRANSCRIBE_SPEECH => {
            multimodal_generation_tool::tool_transcribe_speech(context, args).await
        }
        multimodal_generation_tool::TOOL_GENERATE_IMAGE => {
            multimodal_generation_tool::tool_generate_image(context, args).await
        }
        multimodal_generation_tool::TOOL_GENERATE_VIDEO => {
            multimodal_generation_tool::tool_generate_video(context, args).await
        }
        "技能调用" => skill_call::execute_skill_call(context, args).await,
        "写入文件" => file_tool::write_file(context, args).await,
        "文本编辑" => edit_file2(context, args).await,
        "应用补丁" => apply_patch_tool::apply_patch(context, args).await,
        "LSP查询" => lsp_tool::lsp_query(context, args).await,
        "子智能体控制" => subagent_control(context, args).await,
        "会话线程控制" => execute_thread_control_tool(context, args).await,
        "\u{667a}\u{80fd}\u{4f53}\u{8702}\u{7fa4}" => agent_swarm(context, args).await,
        "节点调用" => node_invoke_tool::execute_node_invoke_tool(context, args).await,
        web_search_tool::TOOL_WEB_SEARCH => web_search_tool::tool_web_search(context, args).await,
        web_fetch_tool::TOOL_WEB_FETCH => web_fetch_tool::tool_web_fetch(context, args).await,
        browser_tool::TOOL_BROWSER => {
            browser_tool::tool_browser(context, browser_tool::TOOL_BROWSER, args).await
        }
        browser_tool::TOOL_BROWSER_NAVIGATE => {
            browser_tool::tool_browser_navigate(context, args).await
        }
        browser_tool::TOOL_BROWSER_CLICK => browser_tool::tool_browser_click(context, args).await,
        browser_tool::TOOL_BROWSER_TYPE => browser_tool::tool_browser_type(context, args).await,
        browser_tool::TOOL_BROWSER_SCREENSHOT => {
            browser_tool::tool_browser_screenshot(context, args).await
        }
        browser_tool::TOOL_BROWSER_READ_PAGE => {
            browser_tool::tool_browser_read_page(context, args).await
        }
        browser_tool::TOOL_BROWSER_CLOSE => browser_tool::tool_browser_close(context, args).await,
        desktop_control::TOOL_DESKTOP_CONTROLLER => {
            desktop_control::tool_desktop_controller(context, args).await
        }
        desktop_control::TOOL_DESKTOP_MONITOR => {
            desktop_control::tool_desktop_monitor(context, args).await
        }
        "a2a观察" => a2a_tool::a2a_observe(context, args).await,
        "a2a等待" => a2a_tool::a2a_wait(context, args).await,
        "a2ui" => Ok(json!({
            "uid": args.get("uid"),
            "a2ui": args.get("a2ui"),
            "content": args.get("content")
        })),
        sessions_yield_tool::TOOL_SESSIONS_YIELD => {
            sessions_yield_tool::execute_sessions_yield_tool(context, args).await
        }
        "计划面板" => panel_tools::execute_plan_tool(context, args).await,
        "问询面板" => panel_tools::execute_question_panel_tool(context, args).await,
        "定时任务" => schedule_task_tool::execute_schedule_task_tool(context, args).await,
        sleep_tool::TOOL_SLEEP_WAIT => sleep_tool::tool_sleep_wait(context, args).await,
        "用户世界工具" => user_world_tool::execute_user_world_tool(context, args).await,
        channel_tool::TOOL_CHANNEL => channel_tool::channel_tool(context, args).await,
        "记忆管理" => execute_memory_manager_tool(context, args).await,
        _ => Err(anyhow!("未知内置工具: {canonical}")),
    }
}
