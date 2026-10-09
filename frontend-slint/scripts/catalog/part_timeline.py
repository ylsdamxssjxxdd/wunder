# -*- coding: utf-8 -*-
# Chat timeline (§7): entry labels, tool group bars and the patch diff card.
ENTRIES = [
    # ---- end-of-turn metrics: labels double as the row tooltips ----
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
