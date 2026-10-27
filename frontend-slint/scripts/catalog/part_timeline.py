# AI生成
# -*- coding: utf-8 -*-
# Chat timeline (§7): entry labels, tool group bars and the patch diff card.
ENTRIES = [
    ("tl_status_waiting", "等待确认", "Waiting for confirmation"),
    ("tl_status_error", "任务失败", "Task failed"),
    ("tl_status_stopped", "已停止", "Stopped"),
    ("tl_status_queued", "排队中", "Queued"),
    ("tl_status_compacting", "上下文压缩中", "Compacting context"),
    ("tl_status_subagent", "子智能体执行中", "Subagent running"),
    ("tl_status_tool", "使用工具中", "Using tools"),
    ("tl_status_outputting", "模型输出中", "Model outputting"),
    ("tl_status_requesting", "请求中", "Requesting"),
    ("tl_status_running", "处理中", "Processing"),
    ("tl_process_loading", "正在加载…", "Loading…"),
    ("tl_process_retry", "加载失败，重试", "Loading failed. Retry"),
    # ---- end-of-turn metrics: labels double as the row tooltips ----
    ("tl_stat_round", "用户轮次", "User round"),
    ("tl_user_round", "第 {} 个用户轮次", "User round {}", ["round"]),
    ("tl_stat_credits", "额度", "Quota"),
    ("tl_stat_speed_hint", "有效模型输出 token / 生成流式时间；不含等待和工具执行，优先使用本轮已测量的汇总速度。无法可靠测量时显示 -。", "Measured output tokens per streaming second, excluding wait and tool execution. Uses the turn aggregate when available; - means unavailable."),
    ("tl_stat_context_hint", "最近一次模型请求观测的上下文占用，不是累计消耗；压缩后重新观测，可能下降。", "Context occupancy observed for the latest model request, rather than cumulative consumption. It may decrease after compaction."),
    ("tl_stat_duration", "耗时", "Duration"),
    ("tl_stat_speed", "生成速度", "Generation speed"),
    ("tl_stat_context", "上下文占用", "Context occupancy"),
    ("tl_stat_quota", "上下文消耗", "Consumed tokens"),
    ("tl_stat_tools", "工具调用次数", "Tool calls"),

    ("tl_thought_done", "已思考", "Thought"),
    ("tl_thought_running", "正在思考…", "Thinking…"),
    ("tl_thought_failed", "思考中断", "Thinking interrupted"),
    ("tl_tool_calls", "执行工具 {} 次", "Ran {} tools", ["count"]),
    ("tl_entry_output", "输出", "Output"),
    ("tl_patch_pending", "待应用", "Pending"),
    ("tl_patch_lines_omitted", "另有 {} 行未展示", "{} more lines hidden", ["count"]),
    ("tl_patch_files_omitted", "另有 {} 个文件未展示", "{} more files hidden", ["count"]),
    # Patch diff card (§7.4).
    ("pd_changed_hunk", "变更片段", "Changed hunk"),
    ("pd_action_update", "更新", "Update"),
    ("pd_action_add", "新增", "Add"),
    ("pd_action_delete", "删除", "Delete"),
    ("pd_action_move", "移动", "Move"),
    ("pd_preview_path", "示例/补丁文件.rs", "sample/patched-file.rs"),
]
