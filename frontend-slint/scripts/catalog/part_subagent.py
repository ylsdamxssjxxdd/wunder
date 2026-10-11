# -*- coding: utf-8 -*-
# Subagent cards in the chat timeline and the child-thread detail dialog,
# which reuses the main timeline view to render the child thread's turns.
ENTRIES = [
    ("sub_card_title", "子智能体", "Subagent"),
    ("sub_status_running", "运行中", "Running"),
    ("sub_status_done", "已完成", "Completed"),
    ("sub_status_failed", "失败", "Failed"),
    ("sub_interrupt", "中断", "Interrupt"),
    ("sub_detail_title", "子智能体线程", "Subagent thread"),
    ("sub_detail_task_line", "任务：{}", "Task: {}", ["task"]),
    ("sub_detail_metrics", "{} 次工具调用 · {} 次模型请求", "{} tool calls · {} model requests",
     ["tools", "requests"]),
    ("sub_detail_tokens", "上下文 {} tok", "Context {} tok", ["tokens"]),
    ("sub_detail_empty", "子线程暂无内容", "The child thread has no content yet"),
    ("sub_detail_load_failed", "子线程加载失败", "Failed to load the child thread"),
    ("sub_detail_loading", "正在加载子线程…", "Loading the child thread…"),
]
