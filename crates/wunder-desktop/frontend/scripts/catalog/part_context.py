# -*- coding: utf-8 -*-
# The context window card opened from the composer's model trigger.

ENTRIES = [
    ("cx_title", "脑图", "Mind map"),
    ("cx_estimate", "估算", "Estimated"),
    ("cx_measured", "已计量", "Measured"),
    ("cx_summary", "已用 {} · 剩余 {} · 承载 {} 轮对话", "{} used · {} free · {} turns", ["pct", "free", "turns"]),
    ("cx_free", "剩余", "Free"),
    ("cx_more_items", "另外 {} 项", "{} more", ["count"]),
    ("cx_item_prompt", "提示词", "Prompt"),
    ("cx_item_reminder", "提醒", "Reminders"),
    ("cx_item_tool", "工具定义", "Tool definition"),
    ("cx_image", "图片 ×{}", "Images ×{}", ["count"]),
    ("cx_turn", "第 {} 轮", "Turn {}", ["count"]),
    ("cx_item_other", "内容", "Content"),
    ("cx_part_system", "系统", "System"),
    ("cx_part_tools", "工具", "Tools"),
    ("cx_part_files", "图片", "Images"),
    ("cx_part_results", "结果", "Results"),
    ("cx_part_chat", "会话", "Chat"),
    (
        "cx_no_request",
        "本线程还没有发出请求；第一次发送后，这里显示它实际占用的构成。",
        "This thread hasn't sent a request yet. After the first send this shows what it was made of.",
    ),
    ("cx_open", "查看上下文", "Open context"),
    (
        "cx_text_released",
        "正文只保留在当前进程内，重启或释放后不可查看。",
        "Text lives only in this process; it is gone after a restart or release.",
    ),
]
