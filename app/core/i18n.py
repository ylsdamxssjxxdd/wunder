from __future__ import annotations

import contextvars
from typing import Dict, Iterable, Optional

# 统一语言上下文，避免在多线程/协程中被意外覆盖
_LANGUAGE_CONTEXT: contextvars.ContextVar[str] = contextvars.ContextVar(
    "wunder_language", default="zh-CN"
)

# 语言支持与别名映射，确保输入更宽松但输出更规范
DEFAULT_LANGUAGE = "zh-CN"
SUPPORTED_LANGUAGES = {"zh-CN", "en-US"}
_SUPPORTED_LANGUAGE_LIST = ["zh-CN", "en-US"]
_DEFAULT_LANGUAGE_ALIASES = {
    "zh": "zh-CN",
    "zh-cn": "zh-CN",
    "zh-hans": "zh-CN",
    "zh-hans-cn": "zh-CN",
    "en": "en-US",
    "en-us": "en-US",
}
_LANGUAGE_ALIASES = dict(_DEFAULT_LANGUAGE_ALIASES)

# 统一消息字典：按语言存储，便于集中维护翻译
_MESSAGES: Dict[str, Dict[str, str]] = {
    "error.api_key_missing": {
        "zh-CN": "API key 未配置",
        "en-US": "API key is not configured",
    },
    "error.api_key_invalid": {
        "zh-CN": "API key 无效",
        "en-US": "Invalid API key",
    },
    "error.config_file_not_found": {
        "zh-CN": "配置文件不存在: {path}",
        "en-US": "Config file not found: {path}",
    },
    "error.user_id_required": {
        "zh-CN": "user_id 不能为空",
        "en-US": "user_id is required",
    },
    "error.question_required": {
        "zh-CN": "问题不能为空",
        "en-US": "Question is required",
    },
    "error.file_extension_missing": {
        "zh-CN": "文件缺少扩展名",
        "en-US": "File extension is missing",
    },
    "error.unsupported_file_type": {
        "zh-CN": "不支持的文件类型: {extension}",
        "en-US": "Unsupported file type: {extension}",
    },
    "error.empty_parse_result": {
        "zh-CN": "解析结果为空",
        "en-US": "Parsed result is empty",
    },
    "error.converter_unique_markdown_name": {
        "zh-CN": "无法生成唯一的 Markdown 文件名",
        "en-US": "Unable to generate a unique Markdown filename",
    },
    "error.converter_doc2md_failed": {
        "zh-CN": "doc2md 执行失败",
        "en-US": "doc2md execution failed",
    },
    "error.converter_doc2md_no_output": {
        "zh-CN": "doc2md 未生成输出文件",
        "en-US": "doc2md did not produce an output file",
    },
    "error.converter_read_text_failed": {
        "zh-CN": "文本读取失败",
        "en-US": "Failed to read text",
    },
    "error.converter_python_docx_unavailable": {
        "zh-CN": "python-docx 不可用: {detail}",
        "en-US": "python-docx is unavailable: {detail}",
    },
    "error.converter_python_pptx_unavailable": {
        "zh-CN": "python-pptx 不可用: {detail}",
        "en-US": "python-pptx is unavailable: {detail}",
    },
    "error.converter_openpyxl_unavailable": {
        "zh-CN": "openpyxl 不可用: {detail}",
        "en-US": "openpyxl is unavailable: {detail}",
    },
    "error.converter_python_converter_not_found": {
        "zh-CN": "未找到可用的 Python 转换器: {ext}",
        "en-US": "No available Python converter for: {ext}",
    },
    "error.converter_doc2md_convert_failed": {
        "zh-CN": "doc2md 转换失败",
        "en-US": "doc2md conversion failed",
    },
    "error.converter_empty_result": {
        "zh-CN": "转换结果为空",
        "en-US": "Conversion result is empty",
    },
    "error.system_prompt_failed": {
        "zh-CN": "系统提示词构建失败: {detail}",
        "en-US": "Failed to build system prompt: {detail}",
    },
    "error.knowledge_query_required": {
        "zh-CN": "知识库查询不能为空",
        "en-US": "Knowledge query is required",
    },
    "knowledge.fallback_reason": {
        "zh-CN": "词面匹配",
        "en-US": "Lexical match",
    },
    "knowledge.tool.description": {
        "zh-CN": "检索知识库：{name}",
        "en-US": "Search knowledge base: {name}",
    },
    "knowledge.tool.query.description": {
        "zh-CN": "查询内容",
        "en-US": "Query text",
    },
    "knowledge.tool.limit.description": {
        "zh-CN": "返回条数（可选，默认使用系统内置上限）",
        "en-US": "Return limit (optional; defaults to system max).",
    },
    "knowledge.section.full_text": {
        "zh-CN": "全文",
        "en-US": "Full Text",
    },
    "memory.block_prefix": {
        "zh-CN": "[长期记忆]",
        "en-US": "[Long-term Memory]",
    },
    "memory.status.queued": {
        "zh-CN": "排队中",
        "en-US": "Queued",
    },
    "memory.status.running": {
        "zh-CN": "正在处理",
        "en-US": "Processing",
    },
    "memory.status.done": {
        "zh-CN": "已完成",
        "en-US": "Completed",
    },
    "memory.status.failed": {
        "zh-CN": "失败",
        "en-US": "Failed",
    },
    "memory.summary_prompt_fallback": {
        "zh-CN": "请提取对后续对话长期有价值的核心信息，仅输出纯文本列表，每行以“- ”开头，禁止标题与解释。",
        "en-US": "Extract the long-term valuable information for future conversations. Output plain text lines starting with “- ” only, without titles or explanations.",
    },
    "memory.empty_summary": {
        "zh-CN": "暂无摘要。",
        "en-US": "No summary yet.",
    },
    "memory.time_prefix": {
        "zh-CN": "{year}年{month:02d}月{day:02d}日{hour:02d}时{minute:02d}分",
        "en-US": "{year:04d}-{month:02d}-{day:02d} {hour:02d}:{minute:02d}",
    },
    "memory.summary.role.user": {
        "zh-CN": "用户",
        "en-US": "User",
    },
    "memory.summary.role.assistant": {
        "zh-CN": "助手",
        "en-US": "Assistant",
    },
    "memory.summary.role.separator": {
        "zh-CN": "：",
        "en-US": ": ",
    },
    "memory.summary.image_placeholder": {
        "zh-CN": "[图片]",
        "en-US": "[Image]",
    },
    "history.compaction_prefix": {
        "zh-CN": "[上下文摘要]",
        "en-US": "[Context Summary]",
    },
    "history.artifact_prefix": {
        "zh-CN": "[产物索引]",
        "en-US": "[Artifact Index]",
    },
    "history.compaction_prompt_fallback": {
        "zh-CN": "请输出可交接的结构化摘要，包含任务目标、已完成进度、关键决策与约束、关键数据与产物、待办与下一步。若某项为空请写“暂无”。",
        "en-US": "Provide a handoff-ready structured summary covering goal, progress, decisions/constraints, key data/artifacts, and next steps. Use “None” when a section is empty.",
    },
    "history.action.read": {
        "zh-CN": "读取",
        "en-US": "Read",
    },
    "history.action.write": {
        "zh-CN": "写入",
        "en-US": "Write",
    },
    "history.action.replace": {
        "zh-CN": "替换",
        "en-US": "Replace",
    },
    "history.action.edit": {
        "zh-CN": "编辑",
        "en-US": "Edit",
    },
    "history.action.execute": {
        "zh-CN": "执行",
        "en-US": "Execute",
    },
    "history.action.run": {
        "zh-CN": "运行",
        "en-US": "Run",
    },
    "history.action.unknown": {
        "zh-CN": "改动",
        "en-US": "Change",
    },
    "history.failure.unknown_item": {
        "zh-CN": "未知条目",
        "en-US": "Unknown item",
    },
    "history.failure.execution": {
        "zh-CN": "执行失败",
        "en-US": "Execution failed",
    },
    "history.summary.file_reads": {
        "zh-CN": "- 文件读取({count}): {items}",
        "en-US": "- Files read ({count}): {items}",
    },
    "history.summary.file_changes": {
        "zh-CN": "- 文件改动({count}): {items}",
        "en-US": "- Files changed ({count}): {items}",
    },
    "history.summary.command_runs": {
        "zh-CN": "- 命令执行({count}): {items}",
        "en-US": "- Commands executed ({count}): {items}",
    },
    "history.summary.script_runs": {
        "zh-CN": "- 脚本运行({count}): {items}",
        "en-US": "- Scripts run ({count}): {items}",
    },
    "history.summary.failures": {
        "zh-CN": "- 失败记录({count}): {items}",
        "en-US": "- Failures ({count}): {items}",
    },
    "history.items_suffix": {
        "zh-CN": " …等{total}项",
        "en-US": " …and {total} items",
    },
    "compaction.reason.history_threshold": {
        "zh-CN": "历史 token 已达阈值，开始压缩",
        "en-US": "History tokens reached the threshold; starting compaction.",
    },
    "compaction.reason.context_too_long": {
        "zh-CN": "上下文过长，开始压缩",
        "en-US": "Context too long; starting compaction.",
    },
    "compaction.summary_fallback": {
        "zh-CN": "摘要生成失败，已自动裁剪历史。",
        "en-US": "Summary generation failed; history was automatically trimmed.",
    },
    "attachment.label": {
        "zh-CN": "附件",
        "en-US": "Attachment",
    },
    "attachment.label.separator": {
        "zh-CN": "：",
        "en-US": ": ",
    },
    "attachment.default_name": {
        "zh-CN": "附件",
        "en-US": "Attachment",
    },
    "attachment.image_prompt": {
        "zh-CN": "请参考以下图片。",
        "en-US": "Please refer to the following image.",
    },
    "prompt.skills.header": {
        "zh-CN": "[技能使用协议]",
        "en-US": "[Skill Usage]",
    },
    "prompt.skills.rule1": {
        "zh-CN": "1) 技能是可选流程手册，仅在任务匹配其 YAML 前置信息时使用。",
        "en-US": "1) Skills are optional playbooks; use them only when the task matches their YAML frontmatter.",
    },
    "prompt.skills.rule2": {
        "zh-CN": "2) 使用下方列出的 SKILL.md 路径，先用`读取文件`阅读后再使用技能。",
        "en-US": "2) Use the SKILL.md paths listed below. Read them via `读取文件` before applying a skill.",
    },
    "prompt.skills.rule3": {
        "zh-CN": "3) 严格遵循技能流程，不要编造缺失步骤。优先使用随附脚本/模板/资产。",
        "en-US": "3) Follow the skill steps strictly. Do not invent missing steps. Prefer bundled scripts/templates/assets.",
    },
    "prompt.skills.rule4": {
        "zh-CN": "4) 特别注意，随附的脚本/模板/资产默认与 SKILL.md 位于同一目录下。",
        "en-US": "4) Bundled scripts/templates/assets are located in the same directory as SKILL.md by default.",
    },
    "prompt.skills.rule5": {
        "zh-CN": "5) 只有在实际执行了技能步骤后，才可以声明已使用该技能。",
        "en-US": "5) Only claim a skill is used after completing its steps.",
    },
    "prompt.skills.rule6": {
        "zh-CN": "6) 使用 `执行命令` 运行相关技能脚本，并将输出写回工作目录。若需创建技能，也将技能保存到工作目录。工具调用只需传入相对路径，无需输入绝对路径；但技能的 SKILL.md 等文件路径需要使用绝对路径。",
        "en-US": "6) Run skill scripts via `执行命令`, and write outputs back to the working directory. If you create a skill, save it in the working directory as well. Use relative paths for tool calls; absolute paths are unnecessary, except that SKILL.md paths must be absolute.",
    },
    "prompt.skills.list_header": {
        "zh-CN": "[已挂载技能]",
        "en-US": "[Mounted Skills]",
    },
    "prompt.engineer.ptc_guidance": {
        "zh-CN": "- 若已挂载 ptc，优先使用 ptc 完成任务，不需要先写脚本保存到本地然后再去执行，提高效率。",
        "en-US": "- If ptc is available, prefer ptc to complete tasks directly instead of saving scripts first.",
    },
    "error.knowledge_base_name_required": {
        "zh-CN": "知识库名称不能为空",
        "en-US": "Knowledge base name is required",
    },
    "error.knowledge_base_not_found": {
        "zh-CN": "知识库不存在",
        "en-US": "Knowledge base not found",
    },
    "error.knowledge_root_not_found": {
        "zh-CN": "知识库目录不存在",
        "en-US": "Knowledge base directory not found",
    },
    "error.knowledge_root_not_dir": {
        "zh-CN": "知识库目录不是文件夹",
        "en-US": "Knowledge base path is not a directory",
    },
    "error.absolute_path_forbidden": {
        "zh-CN": "不允许使用绝对路径",
        "en-US": "Absolute paths are not allowed",
    },
    "error.path_out_of_bounds": {
        "zh-CN": "路径越界访问被禁止",
        "en-US": "Path traversal is forbidden",
    },
    "error.knowledge_name_invalid_path": {
        "zh-CN": "知识库名称包含非法路径",
        "en-US": "Knowledge base name contains invalid path segments",
    },
    "error.knowledge_base_name_invalid": {
        "zh-CN": "知识库名称不合法，无法生成默认目录",
        "en-US": "Invalid knowledge base name; cannot generate default directory",
    },
    "error.knowledge_path_out_of_bounds": {
        "zh-CN": "知识库路径越界访问被禁止",
        "en-US": "Knowledge base path traversal is forbidden",
    },
    "error.knowledge_root_create_failed": {
        "zh-CN": "无法创建知识库目录: {root}, {detail}",
        "en-US": "Failed to create knowledge base directory: {root}, {detail}",
    },
    "error.base_url_or_model_required": {
        "zh-CN": "base_url 或 model 不能为空",
        "en-US": "base_url or model is required",
    },
    "error.llm_config_required": {
        "zh-CN": "至少需要一套模型配置",
        "en-US": "At least one LLM configuration is required",
    },
    "error.llm_not_configured": {
        "zh-CN": "LLM 未配置，无法生成回复。",
        "en-US": "LLM is not configured; cannot generate a response.",
    },
    "error.llm_config_missing": {
        "zh-CN": "LLM 未配置 base_url 或 api_key。",
        "en-US": "LLM base_url or api_key is not configured.",
    },
    "error.llm_request_failed": {
        "zh-CN": "LLM 请求失败。",
        "en-US": "LLM request failed.",
    },
    "error.llm_stream_interrupted": {
        "zh-CN": "LLM 流式响应中断。",
        "en-US": "LLM streaming response was interrupted.",
    },
    "error.llm_stream_retry_exhausted": {
        "zh-CN": "流式重连失败，已达到最大重试次数。",
        "en-US": "Stream reconnect failed; maximum retries reached.",
    },
    "error.llm_unavailable": {
        "zh-CN": "模型不可用: {detail}",
        "en-US": "LLM unavailable: {detail}",
    },
    "error.llm_call_failed": {
        "zh-CN": "模型调用失败: {detail}",
        "en-US": "LLM call failed: {detail}",
    },
    "probe.provider_unsupported": {
        "zh-CN": "当前 provider 暂不支持探测",
        "en-US": "Current provider does not support probing",
    },
    "probe.failed": {
        "zh-CN": "探测失败: {detail}",
        "en-US": "Probe failed: {detail}",
    },
    "probe.no_context": {
        "zh-CN": "未获取到上下文长度",
        "en-US": "Context length not found",
    },
    "probe.success": {
        "zh-CN": "ok",
        "en-US": "ok",
    },
    "error.skill_name_required": {
        "zh-CN": "技能名称不能为空",
        "en-US": "Skill name is required",
    },
    "error.skill_not_found": {
        "zh-CN": "技能不存在",
        "en-US": "Skill not found",
    },
    "error.skill_not_executable": {
        "zh-CN": "技能不存在或不可执行: {name}",
        "en-US": "Skill not found or not executable: {name}",
    },
    "error.skill_file_not_found": {
        "zh-CN": "技能文件不存在",
        "en-US": "Skill file not found",
    },
    "error.skill_file_read_failed": {
        "zh-CN": "读取技能文件失败: {detail}",
        "en-US": "Failed to read skill file: {detail}",
    },
    "error.skills_dir_missing": {
        "zh-CN": "EVA_SKILLS 目录不存在",
        "en-US": "EVA_SKILLS directory not found",
    },
    "error.skill_delete_restricted": {
        "zh-CN": "仅支持删除 EVA_SKILLS 目录内的技能",
        "en-US": "Only skills under EVA_SKILLS can be deleted",
    },
    "error.skill_delete_failed": {
        "zh-CN": "删除技能失败: {detail}",
        "en-US": "Failed to delete skill: {detail}",
    },
    "error.skill_delete_update_failed": {
        "zh-CN": "技能已删除，但更新配置失败: {detail}",
        "en-US": "Skill deleted but failed to update config: {detail}",
    },
    "message.skill_deleted": {
        "zh-CN": "已删除",
        "en-US": "Deleted",
    },
    "error.skill_upload_zip_only": {
        "zh-CN": "仅支持上传 .zip 压缩包",
        "en-US": "Only .zip archives are supported",
    },
    "skill.description.missing": {
        "zh-CN": "未提供描述",
        "en-US": "No description provided",
    },
    "error.zip_path_invalid": {
        "zh-CN": "压缩包路径非法",
        "en-US": "Invalid archive path",
    },
    "error.zip_path_illegal": {
        "zh-CN": "压缩包包含非法路径",
        "en-US": "Archive contains illegal paths",
    },
    "error.zip_path_out_of_bounds": {
        "zh-CN": "压缩包包含越界路径",
        "en-US": "Archive contains out-of-bounds paths",
    },
    "error.zip_invalid": {
        "zh-CN": "压缩包格式错误",
        "en-US": "Invalid archive format",
    },
    "message.upload_success": {
        "zh-CN": "上传成功",
        "en-US": "Upload succeeded",
    },
    "message.upload_converted": {
        "zh-CN": "上传并转换完成",
        "en-US": "Upload and conversion completed",
    },
    "error.markdown_only": {
        "zh-CN": "仅支持 Markdown 文件",
        "en-US": "Only Markdown files are supported",
    },
    "error.file_not_found": {
        "zh-CN": "文件不存在",
        "en-US": "File not found",
    },
    "message.saved_and_reindexed": {
        "zh-CN": "已保存并刷新索引",
        "en-US": "Saved and reindexed",
    },
    "message.deleted": {
        "zh-CN": "已删除",
        "en-US": "Deleted",
    },
    "message.index_refreshed": {
        "zh-CN": "已刷新索引",
        "en-US": "Index refreshed",
    },
    "error.tool_name_required": {
        "zh-CN": "工具名称不能为空",
        "en-US": "Tool name is required",
    },
    "error.session_not_found": {
        "zh-CN": "会话不存在",
        "en-US": "Session not found",
    },
    "error.session_not_found_or_finished": {
        "zh-CN": "会话不存在或已结束",
        "en-US": "Session not found or already finished",
    },
    "message.cancel_requested": {
        "zh-CN": "已请求终止",
        "en-US": "Cancellation requested",
    },
    "error.session_not_found_or_running": {
        "zh-CN": "会话不存在或仍在运行",
        "en-US": "Session not found or still running",
    },
    "error.session_cancelled": {
        "zh-CN": "会话已取消",
        "en-US": "Session cancelled",
    },
    "error.user_session_busy": {
        "zh-CN": "该用户已有会话正在执行",
        "en-US": "This user already has an active session running",
    },
    "error.task_id_required": {
        "zh-CN": "任务ID不能为空",
        "en-US": "Task ID is required",
    },
    "error.task_not_found": {
        "zh-CN": "任务不存在",
        "en-US": "Task not found",
    },
    "error.param_required": {
        "zh-CN": "参数不能为空",
        "en-US": "Parameters are required",
    },
    "error.content_required": {
        "zh-CN": "内容不能为空",
        "en-US": "Content is required",
    },
    "message.updated": {
        "zh-CN": "已更新",
        "en-US": "Updated",
    },
    "message.cleared": {
        "zh-CN": "已清空",
        "en-US": "Cleared",
    },
    "message.user_deleted": {
        "zh-CN": "已清除用户数据",
        "en-US": "User data cleared",
    },
    "monitor.summary.restarted": {
        "zh-CN": "服务重启，线程未完成。",
        "en-US": "Service restarted; session unfinished.",
    },
    "monitor.summary.finished": {
        "zh-CN": "已完成回复。",
        "en-US": "Response completed.",
    },
    "monitor.summary.received": {
        "zh-CN": "已接收请求，开始处理。",
        "en-US": "Request received; processing.",
    },
    "monitor.summary.tool_call": {
        "zh-CN": "调用工具：{tool}",
        "en-US": "Tool call: {tool}",
    },
    "monitor.summary.model_call": {
        "zh-CN": "调用模型请求",
        "en-US": "LLM request",
    },
    "monitor.summary.exception": {
        "zh-CN": "执行异常",
        "en-US": "Execution error",
    },
    "monitor.summary.cancelled": {
        "zh-CN": "已终止",
        "en-US": "Cancelled",
    },
    "monitor.summary.cancel_requested": {
        "zh-CN": "已请求终止",
        "en-US": "Cancellation requested",
    },
    "monitor.summary.user_deleted_cancel": {
        "zh-CN": "用户已删除，已请求终止",
        "en-US": "User deleted; cancellation requested",
    },
    "error.tool_not_found": {
        "zh-CN": "未找到工具: {name}",
        "en-US": "Tool not found: {name}",
    },
    "error.tool_execution_failed": {
        "zh-CN": "工具执行失败: {name}",
        "en-US": "Tool execution failed: {name}",
    },
    "error.task_required": {
        "zh-CN": "任务不能为空",
        "en-US": "Task is required",
    },
    "error.tool_disabled_or_unavailable": {
        "zh-CN": "工具未启用或不可用",
        "en-US": "Tool is disabled or unavailable",
    },
    "error.max_rounds_no_final_answer": {
        "zh-CN": "未能在最大轮次内生成最终答复。",
        "en-US": "Unable to produce a final answer within the maximum rounds.",
    },
    "response.a2ui_fallback": {
        "zh-CN": "已生成 A2UI 界面并完成渲染。",
        "en-US": "A2UI UI has been generated and rendered.",
    },
    "error.internal_error": {
        "zh-CN": "内部错误",
        "en-US": "Internal error",
    },
    "tool.fs.path_forbidden": {
        "zh-CN": "路径被安全策略禁止。",
        "en-US": "Path is forbidden by security policy.",
    },
    "tool.fs.absolute_forbidden": {
        "zh-CN": "不允许使用绝对路径。",
        "en-US": "Absolute paths are not allowed.",
    },
    "tool.fs.path_out_of_bounds": {
        "zh-CN": "路径越界访问被禁止。",
        "en-US": "Path traversal is forbidden.",
    },
    "tool.read.no_path": {
        "zh-CN": "未提供文件路径。",
        "en-US": "No file path provided.",
    },
    "tool.read.not_found": {
        "zh-CN": "文件不存在。",
        "en-US": "File not found.",
    },
    "tool.read.too_large": {
        "zh-CN": "文件过大，已拒绝读取。",
        "en-US": "File too large to read.",
    },
    "tool.read.empty_file": {
        "zh-CN": "（空文件）",
        "en-US": "(empty file)",
    },
    "tool.read.range_out_of_file": {
        "zh-CN": "（范围 {start}-{end} 超出文件长度 {total}）",
        "en-US": "(range {start}-{end} out of file size {total})",
    },
    "tool.read.empty_result": {
        "zh-CN": "无可读取文件",
        "en-US": "No readable files",
    },
    "tool.list.path_not_found": {
        "zh-CN": "路径不存在。",
        "en-US": "Path not found.",
    },
    "tool.search.empty": {
        "zh-CN": "搜索关键字不能为空。",
        "en-US": "Search query is required.",
    },
    "tool.search.path_not_found": {
        "zh-CN": "路径不存在。",
        "en-US": "Path not found.",
    },
    "tool.replace.file_not_found": {
        "zh-CN": "文件不存在。",
        "en-US": "File not found.",
    },
    "tool.replace.not_found": {
        "zh-CN": "未找到要替换的内容。",
        "en-US": "Text to replace not found.",
    },
    "tool.replace.count_mismatch": {
        "zh-CN": "实际替换数量与期望不一致。",
        "en-US": "Replacement count mismatch.",
    },
    "tool.edit.file_not_found": {
        "zh-CN": "文件不存在。",
        "en-US": "File not found.",
    },
    "tool.edit.invalid_start": {
        "zh-CN": "起始行号非法。",
        "en-US": "Invalid start line.",
    },
    "tool.edit.out_of_range": {
        "zh-CN": "编辑行号超出文件范围。",
        "en-US": "Edit lines out of range.",
    },
    "tool.edit.replace_out_of_range": {
        "zh-CN": "替换范围超出文件行数。",
        "en-US": "Replace range exceeds file length.",
    },
    "tool.edit.delete_out_of_range": {
        "zh-CN": "删除范围超出文件行数。",
        "en-US": "Delete range exceeds file length.",
    },
    "tool.edit.action_unsupported": {
        "zh-CN": "不支持的编辑动作。",
        "en-US": "Unsupported edit action.",
    },
    "tool.exec.command_required": {
        "zh-CN": "命令不能为空。",
        "en-US": "Command is required.",
    },
    "tool.exec.workdir_not_found": {
        "zh-CN": "工作目录不存在。",
        "en-US": "Working directory not found.",
    },
    "tool.exec.workdir_not_dir": {
        "zh-CN": "工作目录不是目录。",
        "en-US": "Working directory is not a directory.",
    },
    "tool.exec.shell_not_allowed": {
        "zh-CN": "不允许使用 shell 执行。",
        "en-US": "Shell execution is not allowed.",
    },
    "tool.exec.command_failed": {
        "zh-CN": "命令执行异常: {detail}",
        "en-US": "Command execution error: {detail}",
    },
    "tool.exec.parse_failed": {
        "zh-CN": "命令解析失败。",
        "en-US": "Command parse failed.",
    },
    "tool.exec.not_allowed": {
        "zh-CN": "命令不在允许列表中。",
        "en-US": "Command not in allow list.",
    },
    "tool.exec.failed": {
        "zh-CN": "命令执行失败。",
        "en-US": "Command execution failed.",
    },
    "tool.a2a.endpoint_required": {
        "zh-CN": "A2A endpoint 不能为空。",
        "en-US": "A2A endpoint is required.",
    },
    "tool.a2a.message_required": {
        "zh-CN": "A2A 消息不能为空。",
        "en-US": "A2A message is required.",
    },
    "tool.a2a.method_unsupported": {
        "zh-CN": "不支持的 A2A 方法: {method}",
        "en-US": "Unsupported A2A method: {method}",
    },
    "tool.a2a.loop_detected": {
        "zh-CN": "检测到委派链路循环，已阻止调用。",
        "en-US": "Delegation loop detected; call blocked.",
    },
    "tool.a2a.depth_exceeded": {
        "zh-CN": "委派深度超过上限({max_depth})。",
        "en-US": "Delegation depth exceeds limit ({max_depth}).",
    },
    "tool.a2a.request_failed": {
        "zh-CN": "A2A 请求失败: {detail}",
        "en-US": "A2A request failed: {detail}",
    },
    "tool.a2a.response_invalid": {
        "zh-CN": "A2A 响应格式错误。",
        "en-US": "Invalid A2A response format.",
    },
    "tool.a2a.http_error": {
        "zh-CN": "A2A 返回错误: status={status}",
        "en-US": "A2A returned error: status={status}",
    },
    "tool.a2a.service_unavailable": {
        "zh-CN": "A2A 服务不可用：{name}",
        "en-US": "A2A service unavailable: {name}",
    },
    "tool.a2a.task_required": {
        "zh-CN": "A2A 任务标识不能为空。",
        "en-US": "A2A task name/id is required.",
    },
    "tool.ptc.filename_required": {
        "zh-CN": "脚本文件名不能为空。",
        "en-US": "Script filename is required.",
    },
    "tool.ptc.content_required": {
        "zh-CN": "脚本内容不能为空。",
        "en-US": "Script content is required.",
    },
    "tool.ptc.filename_invalid": {
        "zh-CN": "脚本文件名不能包含路径。",
        "en-US": "Script filename cannot include paths.",
    },
    "tool.ptc.ext_invalid": {
        "zh-CN": "仅支持 .py 脚本文件。",
        "en-US": "Only .py scripts are supported.",
    },
    "tool.ptc.exec_error": {
        "zh-CN": "脚本执行异常: {detail}",
        "en-US": "Script execution error: {detail}",
    },
    "tool.ptc.exec_failed": {
        "zh-CN": "脚本执行失败。",
        "en-US": "Script execution failed.",
    },
    "tool.spec.final.description": {
        "zh-CN": "返回给用户的最终回复。",
        "en-US": "Final response to return to the user.",
    },
    "tool.spec.final.args.content": {
        "zh-CN": "最终回复内容。",
        "en-US": "Final response content.",
    },
    "tool.spec.a2ui.description": {
        "zh-CN": "生成 A2UI 界面并返回给客户端渲染。",
        "en-US": "Generate A2UI UI and send it to the client for rendering.",
    },
    "tool.spec.a2ui.args.uid": {
        "zh-CN": "UI Surface 标识，用于绑定渲染区域。",
        "en-US": "UI Surface identifier used to bind the rendering area.",
    },
    "tool.spec.a2ui.args.messages": {
        "zh-CN": "A2UI JSON 消息数组（beginRendering/surfaceUpdate/dataModelUpdate/deleteSurface）。",
        "en-US": "Array of A2UI JSON messages (beginRendering/surfaceUpdate/dataModelUpdate/deleteSurface).",
    },
    "tool.spec.a2ui.args.content": {
        "zh-CN": "可选的简短文本说明。",
        "en-US": "Optional short textual note.",
    },
    "tool.spec.a2a.description": {
        "zh-CN": "通过 A2A JSON-RPC 委派任务给外部智能体并获取结果。",
        "en-US": "Delegate a task to an external agent via A2A JSON-RPC and return the result.",
    },
    "tool.spec.a2a.args.endpoint": {
        "zh-CN": "A2A JSON-RPC 接口地址（通常以 /a2a 结尾）。",
        "en-US": "A2A JSON-RPC endpoint (usually ends with /a2a).",
    },
    "tool.spec.a2a.args.method": {
        "zh-CN": "调用方法（SendMessage / SendStreamingMessage / GetTask / SubscribeToTask / ListTasks / CancelTask）。",
        "en-US": "Method to call (SendMessage / SendStreamingMessage / GetTask / SubscribeToTask / ListTasks / CancelTask).",
    },
    "tool.spec.a2a.args.content": {
        "zh-CN": "简要文本问题，未提供 message 时使用。",
        "en-US": "Text prompt used when message is not provided.",
    },
    "tool.spec.a2a.args.message": {
        "zh-CN": "完整 A2A Message（可选，优先使用）。",
        "en-US": "Full A2A Message (optional, preferred when provided).",
    },
    "tool.spec.a2a.args.session_id": {
        "zh-CN": "会话标识（写入 taskId/contextId，默认当前会话）。",
        "en-US": "Session id used for taskId/contextId (defaults to current session).",
    },
    "tool.spec.a2a.args.user_id": {
        "zh-CN": "远端智能体的 userId（可选，默认沿用当前 user_id）。",
        "en-US": "Remote userId (optional, defaults to current user_id).",
    },
    "tool.spec.a2a.args.tool_names": {
        "zh-CN": "远端允许调用的工具列表（可选）。",
        "en-US": "Allowed tool list for the remote agent (optional).",
    },
    "tool.spec.a2a.args.model_name": {
        "zh-CN": "远端模型配置名称（可选）。",
        "en-US": "Remote model configuration name (optional).",
    },
    "tool.spec.a2a.args.blocking": {
        "zh-CN": "SendMessage 是否阻塞等待结果（默认 true）。",
        "en-US": "Whether SendMessage blocks for result (default true).",
    },
    "tool.spec.a2a.args.stream": {
        "zh-CN": "是否使用流式接口（SendStreamingMessage）。",
        "en-US": "Use streaming interface (SendStreamingMessage).",
    },
    "tool.spec.a2a.args.headers": {
        "zh-CN": "额外请求头（如鉴权）。",
        "en-US": "Extra headers (e.g., auth).",
    },
    "tool.spec.a2a.args.timeout": {
        "zh-CN": "请求超时秒数（默认 120）。",
        "en-US": "Timeout in seconds (default 120).",
    },
    "tool.spec.a2a.args.allow_self": {
        "zh-CN": "是否允许调用本地 A2A 入口（默认 false）。",
        "en-US": "Allow calling local A2A endpoint (default false).",
    },
    "tool.spec.a2a.args.delegation_chain": {
        "zh-CN": "委派链路（用于防止循环）。",
        "en-US": "Delegation chain to prevent loops.",
    },
    "tool.spec.a2a.args.max_depth": {
        "zh-CN": "委派链路最大深度（可选）。",
        "en-US": "Max delegation depth (optional).",
    },
    "tool.spec.a2a.args.task_id": {
        "zh-CN": "任务标识（用于 GetTask/SubscribeToTask 等方法）。",
        "en-US": "Task id for GetTask/SubscribeToTask.",
    },
    "tool.spec.a2a.args.task_name": {
        "zh-CN": "任务名称（如 tasks/{id}）。",
        "en-US": "Task name (e.g., tasks/{id}).",
    },
    "tool.spec.a2a_service.description": {
        "zh-CN": "委派给 A2A 智能体 {name} 执行任务并返回结果。",
        "en-US": "Delegate tasks to A2A agent {name} and return the result.",
    },
    "tool.spec.a2a_service.summary.skills": {
        "zh-CN": "技能：{skills}",
        "en-US": "Skills: {skills}",
    },
    "tool.spec.a2a_service.summary.skills_more": {
        "zh-CN": "技能：{names} 等 {count} 项",
        "en-US": "Skills: {names} (total {count})",
    },
    "tool.spec.a2a_service.summary.tools": {
        "zh-CN": "工具：{tools}",
        "en-US": "Tools: {tools}",
    },
    "tool.spec.a2a_service.summary.tool.builtin": {
        "zh-CN": "内置{count}",
        "en-US": "builtin {count}",
    },
    "tool.spec.a2a_service.summary.tool.mcp": {
        "zh-CN": "MCP{count}",
        "en-US": "MCP {count}",
    },
    "tool.spec.a2a_service.summary.tool.a2a": {
        "zh-CN": "A2A{count}",
        "en-US": "A2A {count}",
    },
    "tool.spec.a2a_service.summary.tool.knowledge": {
        "zh-CN": "知识库{count}",
        "en-US": "knowledge {count}",
    },
    "tool.spec.a2a_service.args.content": {
        "zh-CN": "发送给智能体的文本内容。",
        "en-US": "Text message sent to the agent.",
    },
    "tool.spec.a2a_service.args.message": {
        "zh-CN": "可选的完整 A2A Message 对象。",
        "en-US": "Optional full A2A Message payload.",
    },
    "tool.spec.a2a_service.args.session_id": {
        "zh-CN": "会话标识（写入 taskId/contextId）。",
        "en-US": "Session id used for taskId/contextId.",
    },
    "tool.spec.a2a_service.args.user_id": {
        "zh-CN": "远端智能体的 userId（可选，默认沿用当前 user_id）。",
        "en-US": "Remote userId (optional, defaults to current user_id).",
    },
    "tool.spec.a2a_service.args.tool_names": {
        "zh-CN": "远端允许调用的工具列表（可选）。",
        "en-US": "Allowed tool list for the remote agent (optional).",
    },
    "tool.spec.a2a_service.args.model_name": {
        "zh-CN": "远端模型配置名称（可选）。",
        "en-US": "Remote model configuration name (optional).",
    },
    "tool.spec.a2a_service.args.blocking": {
        "zh-CN": "SendMessage 是否阻塞等待结果（默认 true）。",
        "en-US": "Whether SendMessage blocks for result (default true).",
    },
    "tool.spec.a2a_service.args.stream": {
        "zh-CN": "是否使用流式接口（SendStreamingMessage）。",
        "en-US": "Use streaming interface (SendStreamingMessage).",
    },
    "tool.spec.a2a_service.args.timeout": {
        "zh-CN": "请求超时秒数（可选）。",
        "en-US": "Timeout in seconds (optional).",
    },
    "tool.spec.a2a_observe.description": {
        "zh-CN": "观察当前会话内 A2A 任务的状态与结果，并汇总返回。",
        "en-US": "Observe A2A tasks in the current session and return aggregated status/results.",
    },
    "tool.spec.a2a_observe.args.task_ids": {
        "zh-CN": "需要观察的 task_id 列表（可选）。",
        "en-US": "List of task_ids to observe (optional).",
    },
    "tool.spec.a2a_observe.args.tasks": {
        "zh-CN": "任务列表（可选），可传入 task_id/endpoint/service_name。",
        "en-US": "Task list (optional), supports task_id/endpoint/service_name.",
    },
    "tool.spec.a2a_observe.args.endpoint": {
        "zh-CN": "过滤指定的 A2A 端点（可选）。",
        "en-US": "Filter by A2A endpoint (optional).",
    },
    "tool.spec.a2a_observe.args.service_name": {
        "zh-CN": "过滤指定的 A2A 服务名（可选）。",
        "en-US": "Filter by A2A service name (optional).",
    },
    "tool.spec.a2a_observe.args.refresh": {
        "zh-CN": "是否主动刷新远端任务状态（默认 true）。",
        "en-US": "Refresh remote task status (default true).",
    },
    "tool.spec.a2a_observe.args.timeout": {
        "zh-CN": "刷新远端任务的超时秒数（可选）。",
        "en-US": "Timeout seconds for remote refresh (optional).",
    },
    "tool.spec.a2a_wait.description": {
        "zh-CN": "等待 A2A 任务完成或超时返回，可用于长任务休眠。",
        "en-US": "Wait for A2A tasks to finish or timeout, useful for long-running jobs.",
    },
    "tool.spec.a2a_wait.args.wait_s": {
        "zh-CN": "等待秒数（可选，默认 30 秒）。",
        "en-US": "Wait seconds (optional, default 30s).",
    },
    "tool.spec.a2a_wait.args.poll_interval": {
        "zh-CN": "轮询间隔秒数（可选）。",
        "en-US": "Polling interval in seconds (optional).",
    },
    "tool.spec.a2a_wait.args.task_ids": {
        "zh-CN": "需要等待的 task_id 列表（可选）。",
        "en-US": "List of task_ids to wait for (optional).",
    },
    "tool.spec.a2a_wait.args.tasks": {
        "zh-CN": "任务列表（可选），可传入 task_id/endpoint/service_name。",
        "en-US": "Task list (optional), supports task_id/endpoint/service_name.",
    },
    "tool.spec.a2a_wait.args.endpoint": {
        "zh-CN": "过滤指定的 A2A 端点（可选）。",
        "en-US": "Filter by A2A endpoint (optional).",
    },
    "tool.spec.a2a_wait.args.service_name": {
        "zh-CN": "过滤指定的 A2A 服务名（可选）。",
        "en-US": "Filter by A2A service name (optional).",
    },
    "tool.spec.a2a_wait.args.refresh": {
        "zh-CN": "等待期间是否刷新远端任务状态（默认 true）。",
        "en-US": "Refresh remote task status while waiting (default true).",
    },
    "tool.spec.exec.description": {
        "zh-CN": "请求在系统上执行 CLI 命令。当需要进行系统操作或运行特定命令以完成用户任务的任一步骤时使用。默认在工作区根目录执行，可通过 workdir 指定子目录或白名单目录。需要 cd/&& 等 shell 语法时设置 shell=true，可显式传 shell=false 关闭。",
        "en-US": "Run CLI commands on the system when needed to complete the task. Defaults to the workspace root; use workdir for subdirectories or allowed paths. Use shell=true when you need cd/&& or other shell syntax, or pass shell=false to disable.",
    },
    "tool.spec.exec.args.content": {
        "zh-CN": "CLI 命令。",
        "en-US": "CLI command.",
    },
    "tool.spec.exec.args.workdir": {
        "zh-CN": "可选，工作目录，相对工作区或白名单目录的绝对路径。",
        "en-US": "Optional. Working directory under the workspace or an allowed path.",
    },
    "tool.spec.exec.args.timeout": {
        "zh-CN": "可选，命令超时秒数，默认 30 秒。",
        "en-US": "Optional. Command timeout in seconds (default 30).",
    },
    "tool.spec.exec.args.shell": {
        "zh-CN": "可选，使用 shell 执行。",
        "en-US": "Optional. Execute via shell.",
    },
    "tool.spec.ptc.description": {
        "zh-CN": "程序化工具调用：当 CLI 命令过多、解析脆弱或需要结构化处理时，编写并运行临时 Python 脚本。脚本会保存到工作区的 ptc_temp 目录并立即执行，返回 stdout/stderr。",
        "en-US": "Programmatic tool call: write and run a temporary Python script when CLI commands are too many or fragile, or when structured processing is needed. The script is saved to ptc_temp in the workspace and executed immediately, returning stdout/stderr.",
    },
    "tool.spec.ptc.args.filename": {
        "zh-CN": "Python 脚本文件名，例如 helper.py。",
        "en-US": "Python script filename, e.g., helper.py.",
    },
    "tool.spec.ptc.args.workdir": {
        "zh-CN": "相对工作区的工作目录，使用 . 表示根目录。",
        "en-US": "Working directory relative to the workspace; use . for root.",
    },
    "tool.spec.ptc.args.content": {
        "zh-CN": "完整的 Python 脚本内容。",
        "en-US": "Full Python script content.",
    },
    "tool.spec.list.description": {
        "zh-CN": "列出目录下所有直接子文件夹和文件。如果未提供路径，默认使用工程师工作目录。",
        "en-US": "List direct child folders and files in a directory. Defaults to the workspace root if path is omitted.",
    },
    "tool.spec.list.args.path": {
        "zh-CN": "可选，要列出的目录（相对工程师工作目录）。留空则使用当前工作目录。",
        "en-US": "Optional. Directory to list, relative to the workspace root. Empty uses the current directory.",
    },
    "tool.spec.search.description": {
        "zh-CN": "在工程师工作目录下搜索所有文本文件中的查询字符串（不区分大小写）。返回格式为 <path>:<line>:<content>。",
        "en-US": "Search all text files under the workspace for the query string (case-insensitive). Returns <path>:<line>:<content>.",
    },
    "tool.spec.search.args.query": {
        "zh-CN": "要搜索的文本（不区分大小写的字面量）。",
        "en-US": "Search text (case-insensitive literal).",
    },
    "tool.spec.search.args.path": {
        "zh-CN": "可选，限制搜索范围的目录，相对工程师工作目录。",
        "en-US": "Optional. Directory to limit the search, relative to the workspace.",
    },
    "tool.spec.search.args.file_pattern": {
        "zh-CN": "可选，用于过滤文件的 glob，例如 *.cpp 或 src/**.ts。",
        "en-US": "Optional. Glob pattern to filter files, e.g., *.cpp or src/**.ts.",
    },
    "tool.spec.read.description": {
        "zh-CN": "读取指定路径的文件内容，支持批量读取并指定行号范围。",
        "en-US": "Read file contents from specified paths, with batch and line-range support.",
    },
    "tool.spec.read.args.files": {
        "zh-CN": "要读取的文件列表，可选指定行号范围。",
        "en-US": "List of files to read, with optional line ranges.",
    },
    "tool.spec.read.args.files.path": {
        "zh-CN": "要读取的路径。",
        "en-US": "Path to read.",
    },
    "tool.spec.read.args.files.start_line": {
        "zh-CN": "可选起始行（包含）。",
        "en-US": "Optional start line (inclusive).",
    },
    "tool.spec.read.args.files.end_line": {
        "zh-CN": "可选结束行（包含）。",
        "en-US": "Optional end line (inclusive).",
    },
    "tool.spec.read.args.files.line_ranges": {
        "zh-CN": "可选的行号范围列表。",
        "en-US": "Optional list of line ranges.",
    },
    "tool.spec.write.description": {
        "zh-CN": "请求向指定路径写入内容。如果文件已存在，将被提供的内容覆盖。",
        "en-US": "Write content to a path. Existing files are overwritten.",
    },
    "tool.spec.write.args.path": {
        "zh-CN": "要写入的文件路径。",
        "en-US": "File path to write.",
    },
    "tool.spec.write.args.content": {
        "zh-CN": "文件内容。",
        "en-US": "File content.",
    },
    "tool.spec.replace.description": {
        "zh-CN": "请求在现有文件中替换文本。默认只替换一次 old_string 的出现。",
        "en-US": "Replace text in an existing file. By default, only the first occurrence of old_string is replaced.",
    },
    "tool.spec.replace.args.path": {
        "zh-CN": "要编辑的文件路径。",
        "en-US": "File path to edit.",
    },
    "tool.spec.replace.args.old_string": {
        "zh-CN": "要被替换的精确文本（请包含上下文）。",
        "en-US": "Exact text to replace (include context).",
    },
    "tool.spec.replace.args.new_string": {
        "zh-CN": "用于替换 old_string 的精确文本。",
        "en-US": "Exact replacement text.",
    },
    "tool.spec.replace.args.expected_replacements": {
        "zh-CN": "期望替换的次数，省略则默认 1。",
        "en-US": "Expected number of replacements (default 1).",
    },
    "tool.spec.edit.description": {
        "zh-CN": "对现有文本文件应用结构化的行级编辑。",
        "en-US": "Apply structured line-based edits to a text file.",
    },
    "tool.spec.edit.args.path": {
        "zh-CN": "要编辑的文件路径。",
        "en-US": "File path to edit.",
    },
    "tool.spec.edit.args.edits": {
        "zh-CN": "按顺序执行的编辑操作列表。",
        "en-US": "List of edits to apply in order.",
    },
    "tool.spec.edit.args.edits.action": {
        "zh-CN": "要应用的编辑类型。",
        "en-US": "Edit action type.",
    },
    "tool.spec.edit.args.edits.start_line": {
        "zh-CN": "编辑开始的行号（从 1 开始）。",
        "en-US": "Start line number (1-based).",
    },
    "tool.spec.edit.args.edits.end_line": {
        "zh-CN": "编辑结束的行号（包含）。",
        "en-US": "End line number (inclusive).",
    },
    "tool.spec.edit.args.edits.new_content": {
        "zh-CN": "替换或插入的内容。",
        "en-US": "Content to replace or insert.",
    },
    "tool.spec.edit.args.ensure_newline": {
        "zh-CN": "为 true 时确保文件以换行结束。",
        "en-US": "Ensure the file ends with a newline when true.",
    },
    "tool.invoke.user_skill_not_loaded": {
        "zh-CN": "用户技能未加载",
        "en-US": "User skills not loaded",
    },
    "tool.invoke.user_skill_not_found": {
        "zh-CN": "用户技能不存在",
        "en-US": "User skill not found",
    },
    "tool.invoke.user_skill_failed": {
        "zh-CN": "用户技能执行失败: {detail}",
        "en-US": "User skill execution failed: {detail}",
    },
    "tool.invoke.mcp_name_invalid": {
        "zh-CN": "MCP 工具名称格式错误",
        "en-US": "Invalid MCP tool name format",
    },
    "tool.invoke.mcp_server_unavailable": {
        "zh-CN": "MCP 服务未配置或未启用",
        "en-US": "MCP server not configured or enabled",
    },
    "tool.invoke.mcp_result_error": {
        "zh-CN": "MCP 工具返回错误",
        "en-US": "MCP tool returned an error",
    },
    "tool.invoke.mcp_call_failed": {
        "zh-CN": "MCP 工具调用失败: {detail}",
        "en-US": "MCP tool call failed: {detail}",
    },
    "tool.invoke.user_tool_unknown": {
        "zh-CN": "未知的自建工具类型",
        "en-US": "Unknown user tool type",
    },
    "tool.invoke.skill_failed": {
        "zh-CN": "技能执行失败: {detail}",
        "en-US": "Skill execution failed: {detail}",
    },
    "tool.invoke.wunder_run_failed": {
        "zh-CN": "wunder@run 调用失败: {detail}",
        "en-US": "wunder@run call failed: {detail}",
    },
    "workspace.error.path_not_found": {
        "zh-CN": "路径不存在",
        "en-US": "Path not found",
    },
    "workspace.error.path_not_dir": {
        "zh-CN": "路径不是目录",
        "en-US": "Path is not a directory",
    },
    "workspace.error.target_not_dir": {
        "zh-CN": "目标不是目录",
        "en-US": "Target is not a directory",
    },
    "workspace.error.dir_path_required": {
        "zh-CN": "目录路径不能为空",
        "en-US": "Directory path is required",
    },
    "workspace.error.target_exists_not_dir": {
        "zh-CN": "目标已存在且不是目录",
        "en-US": "Target exists and is not a directory",
    },
    "workspace.message.dir_created": {
        "zh-CN": "已创建目录",
        "en-US": "Directory created",
    },
    "workspace.error.source_path_required": {
        "zh-CN": "源路径不能为空",
        "en-US": "Source path is required",
    },
    "workspace.error.destination_path_required": {
        "zh-CN": "目标路径不能为空",
        "en-US": "Destination path is required",
    },
    "workspace.message.path_unchanged": {
        "zh-CN": "路径未变化",
        "en-US": "Path unchanged",
    },
    "workspace.error.source_not_found": {
        "zh-CN": "源路径不存在",
        "en-US": "Source path not found",
    },
    "workspace.error.destination_exists": {
        "zh-CN": "目标路径已存在",
        "en-US": "Destination path already exists",
    },
    "workspace.error.destination_parent_missing": {
        "zh-CN": "目标父目录不存在",
        "en-US": "Destination parent directory does not exist",
    },
    "workspace.error.move_to_self_or_child": {
        "zh-CN": "禁止移动到自身或子目录",
        "en-US": "Cannot move into itself or a child directory",
    },
    "workspace.message.moved": {
        "zh-CN": "已移动",
        "en-US": "Moved",
    },
    "workspace.error.source_destination_same": {
        "zh-CN": "源路径与目标路径相同",
        "en-US": "Source and destination are the same",
    },
    "workspace.error.copy_to_self_or_child": {
        "zh-CN": "禁止复制到自身或子目录",
        "en-US": "Cannot copy into itself or a child directory",
    },
    "workspace.message.copied": {
        "zh-CN": "已复制",
        "en-US": "Copied",
    },
    "workspace.error.batch_paths_missing": {
        "zh-CN": "未提供批量路径",
        "en-US": "Batch paths are required",
    },
    "workspace.error.destination_dir_missing": {
        "zh-CN": "目标目录不存在",
        "en-US": "Destination directory not found",
    },
    "workspace.error.path_required": {
        "zh-CN": "路径不能为空",
        "en-US": "Path is required",
    },
    "workspace.error.batch_action_unsupported": {
        "zh-CN": "不支持的批量操作",
        "en-US": "Unsupported batch action",
    },
    "workspace.error.destination_unready": {
        "zh-CN": "目标目录未准备好",
        "en-US": "Destination directory not ready",
    },
    "workspace.message.batch_success": {
        "zh-CN": "批量操作完成",
        "en-US": "Batch operation completed",
    },
    "workspace.message.batch_partial": {
        "zh-CN": "批量操作部分失败",
        "en-US": "Batch operation partially failed",
    },
    "workspace.error.file_path_required": {
        "zh-CN": "文件路径不能为空",
        "en-US": "File path is required",
    },
    "workspace.error.target_not_file": {
        "zh-CN": "目标不是文件",
        "en-US": "Target is not a file",
    },
    "workspace.message.file_saved": {
        "zh-CN": "已保存文件",
        "en-US": "File saved",
    },
    "workspace.error.workspace_not_found": {
        "zh-CN": "工作区不存在",
        "en-US": "Workspace not found",
    },
    "workspace.error.delete_root_forbidden": {
        "zh-CN": "禁止删除根目录",
        "en-US": "Deleting root directory is forbidden",
    },
    "workspace.tree.empty": {
        "zh-CN": "（空）",
        "en-US": "(empty)",
    },
    "sandbox.error.path_required": {
        "zh-CN": "路径不能为空。",
        "en-US": "Path is required.",
    },
    "sandbox.error.path_out_of_bounds": {
        "zh-CN": "沙盒路径越界，已拒绝。",
        "en-US": "Sandbox path is out of bounds.",
    },
    "sandbox.message.release_not_required": {
        "zh-CN": "共享沙盒无需释放。",
        "en-US": "Shared sandbox release is not required.",
    },
    "sandbox.error.unsupported_tool": {
        "zh-CN": "沙盒不支持该内置工具。",
        "en-US": "Sandbox does not support this built-in tool.",
    },
    "sandbox.error.tool_failed": {
        "zh-CN": "沙盒内工具执行失败: {detail}",
        "en-US": "Sandbox tool execution failed: {detail}",
    },
    "sandbox.error.payload_invalid": {
        "zh-CN": "沙盒请求为空或格式错误。",
        "en-US": "Sandbox payload is empty or invalid.",
    },
    "sandbox.error.image_missing": {
        "zh-CN": "沙盒镜像未配置，无法执行工具。",
        "en-US": "Sandbox image is not configured; cannot execute tool.",
    },
    "sandbox.error.request_failed": {
        "zh-CN": "沙盒请求失败: {detail}",
        "en-US": "Sandbox request failed: {detail}",
    },
    "sandbox.error.response_error": {
        "zh-CN": "沙盒返回异常: status={status} detail={detail}",
        "en-US": "Sandbox response error: status={status} detail={detail}",
    },
    "sandbox.error.response_not_json": {
        "zh-CN": "沙盒返回非 JSON 响应: {detail}",
        "en-US": "Sandbox returned non-JSON response: {detail}",
    },
    "sandbox.error.release_failed": {
        "zh-CN": "沙盒释放请求失败: {detail}",
        "en-US": "Sandbox release request failed: {detail}",
    },
    "sandbox.error.release_response_error": {
        "zh-CN": "沙盒释放失败: status={status} detail={detail}",
        "en-US": "Sandbox release failed: status={status} detail={detail}",
    },
    "mcp.instructions": {
        "zh-CN": "调用 wunder 智能体执行任务并返回最终回复。",
        "en-US": "Call the wunder agent to execute a task and return the final response.",
    },
    "mcp.tool.run.description": {
        "zh-CN": "执行 wunder 智能体任务并返回最终回复。",
        "en-US": "Execute a wunder task and return the final response.",
    },
    "error.mcp_server_not_found": {
        "zh-CN": "MCP Server 不存在: {name}",
        "en-US": "MCP server not found: {name}",
    },
    "error.mcp_server_disabled": {
        "zh-CN": "MCP Server 未启用: {name}",
        "en-US": "MCP server disabled: {name}",
    },
    "error.mcp_tool_not_allowed": {
        "zh-CN": "该工具未被允许调用。",
        "en-US": "This tool is not allowed.",
    },
}


def _normalize_language_list(values: Optional[Iterable[str]]) -> list[str]:
    """整理语言列表，保持顺序并去重。"""
    if not values:
        return list(_SUPPORTED_LANGUAGE_LIST)
    seen: set[str] = set()
    ordered: list[str] = []
    for raw in values:
        cleaned = str(raw or "").strip()
        if not cleaned or cleaned in seen:
            continue
        ordered.append(cleaned)
        seen.add(cleaned)
    return ordered or list(_SUPPORTED_LANGUAGE_LIST)


def _normalize_alias_target(value: Optional[str], supported: set[str]) -> Optional[str]:
    """将别名目标映射为受支持的语言码。"""
    cleaned = str(value or "").strip()
    if not cleaned:
        return None
    if cleaned in supported:
        return cleaned
    lowered = cleaned.lower()
    for lang in supported:
        if lang.lower() == lowered:
            return lang
    return None


def configure_i18n(
    *,
    default_language: Optional[str] = None,
    supported_languages: Optional[Iterable[str]] = None,
    aliases: Optional[Dict[str, str]] = None,
) -> None:
    """更新 i18n 配置，便于与配置文件保持一致。"""
    global DEFAULT_LANGUAGE, SUPPORTED_LANGUAGES, _SUPPORTED_LANGUAGE_LIST, _LANGUAGE_ALIASES
    supported_list = _normalize_language_list(supported_languages)
    supported_set = set(supported_list)
    merged_aliases = dict(_DEFAULT_LANGUAGE_ALIASES)
    if isinstance(aliases, dict):
        for key, value in aliases.items():
            alias_key = str(key or "").strip().lower()
            alias_target = _normalize_alias_target(value, supported_set)
            if not alias_key or not alias_target:
                continue
            merged_aliases[alias_key] = alias_target
    cleaned_default = str(default_language or "").strip()
    if cleaned_default:
        lowered_default = cleaned_default.lower()
        if lowered_default in merged_aliases:
            cleaned_default = merged_aliases[lowered_default]
        else:
            for lang in supported_list:
                if lang.lower() == lowered_default:
                    cleaned_default = lang
                    break
    if not cleaned_default:
        cleaned_default = DEFAULT_LANGUAGE
    if cleaned_default not in supported_set:
        supported_list.insert(0, cleaned_default)
        supported_set.add(cleaned_default)
    DEFAULT_LANGUAGE = cleaned_default
    SUPPORTED_LANGUAGES = supported_set
    _SUPPORTED_LANGUAGE_LIST = supported_list
    for lang in supported_list:
        merged_aliases[lang.lower()] = lang
    _LANGUAGE_ALIASES = merged_aliases


def get_default_language() -> str:
    """读取默认语言码。"""
    return DEFAULT_LANGUAGE


def get_supported_languages() -> list[str]:
    """读取支持语言列表。"""
    return list(_SUPPORTED_LANGUAGE_LIST)


def get_language_aliases() -> Dict[str, str]:
    """读取语言别名映射。"""
    return dict(_LANGUAGE_ALIASES)


def get_i18n_config() -> Dict[str, object]:
    """导出当前 i18n 配置，用于接口响应。"""
    return {
        "default_language": DEFAULT_LANGUAGE,
        "supported_languages": list(_SUPPORTED_LANGUAGE_LIST),
        "aliases": dict(_LANGUAGE_ALIASES),
    }


def _normalize_language_code(value: str) -> Optional[str]:
    """将语言代码规范化为系统支持的格式。"""
    if not value:
        return None
    cleaned = value.strip()
    if not cleaned:
        return None
    lowered = cleaned.lower()
    if lowered in _LANGUAGE_ALIASES:
        return _LANGUAGE_ALIASES[lowered]
    # 保留完整区域码（如 en-US / zh-CN）
    if cleaned in SUPPORTED_LANGUAGES:
        return cleaned
    return None


def normalize_language(raw: Optional[str], *, fallback: bool = True) -> str:
    """解析语言输入，返回规范化语言码。"""
    if not raw:
        return DEFAULT_LANGUAGE if fallback else ""
    # 兼容 Accept-Language 这种多语言输入
    for part in str(raw).split(","):
        code = part.split(";")[0].strip()
        normalized = _normalize_language_code(code)
        if normalized:
            return normalized
    return DEFAULT_LANGUAGE if fallback else ""


def resolve_language(candidates: Iterable[Optional[str]]) -> str:
    """从候选语言中挑选第一个可用值。"""
    for value in candidates:
        if value is None:
            continue
        text = str(value).strip()
        if not text:
            continue
        normalized = normalize_language(text, fallback=False)
        if normalized:
            return normalized
    return DEFAULT_LANGUAGE


def set_language(language: str) -> contextvars.Token:
    """设置当前上下文的语言，返回可恢复的 token。"""
    normalized = normalize_language(language, fallback=True)
    return _LANGUAGE_CONTEXT.set(normalized)


def reset_language(token: contextvars.Token) -> None:
    """恢复此前保存的语言上下文。"""
    try:
        _LANGUAGE_CONTEXT.reset(token)
    except Exception:
        return


def get_language() -> str:
    """读取当前上下文语言。"""
    return _LANGUAGE_CONTEXT.get()


def t(key: str, **kwargs) -> str:
    """按当前语言输出翻译后的消息。"""
    if not key:
        return ""
    language = get_language()
    entry = _MESSAGES.get(key, {})
    template = entry.get(language) or entry.get(DEFAULT_LANGUAGE) or key
    if kwargs:
        try:
            return template.format(**kwargs)
        except Exception:
            return template
    return template


def get_known_prefixes(key: str) -> list[str]:
    """读取指定 key 的所有语言前缀，便于兼容历史数据。"""
    entry = _MESSAGES.get(key, {})
    prefixes = [value for value in entry.values() if isinstance(value, str)]
    # 去重保持顺序，避免重复判断
    seen = set()
    ordered: list[str] = []
    for item in prefixes:
        if item in seen:
            continue
        seen.add(item)
        ordered.append(item)
    return ordered
