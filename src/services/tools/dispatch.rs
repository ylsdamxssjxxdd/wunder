use super::catalog::resolve_tool_name;
use super::context::ToolContext;
use super::{
    a2a_observe, a2a_wait, agent_swarm, compact_cron_tool_result, execute_a2a_service,
    execute_command, execute_knowledge_tool, execute_memory_manager_tool, execute_mcp_tool,
    execute_node_invoke, execute_plan_tool, execute_ptc, execute_question_panel_tool,
    execute_skill_call, execute_user_tool, find_knowledge_base, is_a2a_service_tool,
    is_mcp_tool_name, list_files, lsp_query, read_files, search_content, subagent_control,
    user_world_tool, write_file,
};
use super::{apply_patch_tool, browser_tool, desktop_control, read_image_tool, sleep_tool};
use crate::cron::{handle_cron_action, CronActionRequest};
use crate::i18n;
use crate::skills::execute_skill;
use crate::user_store::UserStore;
use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::sync::RwLock;

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
    if is_a2a_service_tool(&canonical) {
        return execute_a2a_service(context, &canonical, args).await;
    }
    if is_mcp_tool_name(&canonical) {
        return execute_mcp_tool(context, &canonical, args).await;
    }
    if let Some(base) = find_knowledge_base(context.config, &canonical) {
        return execute_knowledge_tool(context, &base, args).await;
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
        "最终回复" => Ok(json!({
            "answer": args.get("content").and_then(Value::as_str).unwrap_or("").to_string()
        })),
        "执行命令" => execute_command(context, args).await,
        "ptc" => execute_ptc(context, args).await,
        "列出文件" => list_files(context, args).await,
        "搜索内容" => search_content(context, args).await,
        "读取文件" => read_files(context, args).await,
        read_image_tool::TOOL_READ_IMAGE => read_image_tool::tool_read_image(context, args).await,
        "技能调用" => execute_skill_call(context, args).await,
        "写入文件" => write_file(context, args).await,
        "应用补丁" => apply_patch_tool::apply_patch(context, args).await,
        "LSP查询" => lsp_query(context, args).await,
        "子智能体控制" => subagent_control(context, args).await,
        "\u{667a}\u{80fd}\u{4f53}\u{8702}\u{7fa4}" => agent_swarm(context, args).await,
        "节点调用" => execute_node_invoke(context, args).await,
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
        "a2a观察" => a2a_observe(context, args).await,
        "a2a等待" => a2a_wait(context, args).await,
        "a2ui" => Ok(json!({
            "uid": args.get("uid"),
            "a2ui": args.get("a2ui"),
            "content": args.get("content")
        })),
        "计划面板" => execute_plan_tool(context, args).await,
        "问询面板" => execute_question_panel_tool(context, args).await,
        "定时任务" => {
            let payload: CronActionRequest =
                serde_json::from_value(args.clone()).map_err(|err| anyhow!(err.to_string()))?;
            let user_tool_manager = context
                .user_tool_manager
                .clone()
                .ok_or_else(|| anyhow!(i18n::t("error.internal_error")))?;
            let user_store = Arc::new(UserStore::new(context.storage.clone()));
            let skills = Arc::new(RwLock::new(context.skills.clone()));
            handle_cron_action(
                context.config.clone(),
                context.storage.clone(),
                context.orchestrator.clone(),
                user_store,
                user_tool_manager,
                skills,
                context.user_id,
                Some(context.session_id),
                context.agent_id,
                payload,
            )
            .await
            .map(compact_cron_tool_result)
        }
        sleep_tool::TOOL_SLEEP_WAIT => sleep_tool::tool_sleep_wait(context, args).await,
        "用户世界工具" => user_world_tool(context, args).await,
        "记忆管理" => execute_memory_manager_tool(context, args).await,
        _ => Err(anyhow!("未知内置工具: {canonical}")),
    }
}
