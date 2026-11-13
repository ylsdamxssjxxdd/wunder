# -*- coding: utf-8 -*-
# Composer (chat input), message row and components labels.

ENTRIES = [
    # ---- composer ----
    ("cp_thinking_auto", "自动", "Auto"),
    ("cp_context_usage", "上下文占用", "Context usage"),
    ("cp_usage_empty", "暂无最近一次请求数据", "No request usage yet"),
    ("cp_usage_prefix", "当前请求上下文占用：", "Current context usage: "),
    ("dk_new_folder", "新建文件夹", "New folder"),
    ("cp_image_prefix", "图片 · ", "Images · "),
    ("cp_remove_attachment", "移除附件", "Remove attachment"),
    ("cp_input_placeholder", "请输入文本", "Enter text"),
    ("cp_preset_questions", "预设问题", "Preset questions"),
    ("cp_preset_q1", "你好，请介绍一下你的能力。", "Hi, please introduce your capabilities."),
    ("cp_preset_q2", "请帮我整理当前内容。", "Please organize the current content."),
    ("cp_commands", "命令", "Commands"),
    ("cp_cmd_help", "/help  查看可用命令", "/help  list available commands"),
    ("cp_cmd_new", "/new  新建线程", "/new  new thread"),
    ("cp_cmd_stop", "/stop  停止并退出目标态", "/stop  stop and leave goal mode"),
    ("cp_cmd_goal", "/goal  设置目标", "/goal  set a goal"),
    ("cp_cmd_compact", "/compact  压缩上下文", "/compact  compact context"),
    ("cp_stop_recording", "停止录音", "Stop recording"),
    ("cp_transcribing", "识别中", "Transcribing"),
    ("cp_record", "录音", "Record"),
    ("cp_screenshot", "截图", "Screenshot"),
    ("cp_stopping", "正在停止", "Stopping"),
    ("cp_stop_generation", "停止生成", "Stop"),
    ("cp_sending", "发送中", "Sending"),
    ("cp_send", "发送", "Send"),

    # ---- more menu (§8.2) ----
    ("cp_more", "更多", "More"),
    ("cp_more_new_thread", "新建线程", "New thread"),
    ("cp_more_upload", "上传文件", "Upload file"),
    ("cp_more_workspace", "打开工作区文件夹", "Open workspace folder"),

    # ---- model picker (§8.3) ----
    ("cp_model", "模型", "Model"),
    ("cp_model_pick", "切换模型", "Switch model"),
    ("cp_model_context", "上下文 Token", "Context tokens"),
    ("cp_voice_input", "语音输入", "Voice input"),
    ("cp_open_cli", "打开命令行", "Open CLI"),
    ("cp_model_search", "搜索模型", "Search models"),
    ("cp_model_empty", "还没有可用模型。请到「设置 → 模型设置」添加。", "No models yet. Add one in Settings → Models."),
    ("cp_model_no_match", "没有匹配的模型", "No matching model"),
    ("cp_model_default_tag", "默认", "Default"),
    ("cp_model_settings", "打开模型设置", "Open model settings"),
    ("cp_model_none", "未选择模型", "No model selected"),
    # ---- 未配置模型引导横幅 ----
    ("cp_model_setup_hint", "还没有配置模型，先添加一个模型再开始对话。", "No model configured yet. Add one to start chatting."),
    ("cp_model_setup_action", "配置模型", "Set up model"),

    # ---- goal banner (§8.1b) ----
    ("cp_error_prefix", "无法", "Unable to "),
    ("cp_fail_prefix", "失败", "Failed"),
    ("cp_goal_label", "目标", "Goal"),
    ("cp_goal_pause", "暂停目标", "Pause goal"),
    ("cp_goal_resume", "继续目标", "Resume goal"),
    ("cp_goal_paused_tag", "已暂停", "Paused"),
    ("cp_goal_edit_hint", "输入新的目标内容，回车保存", "Edit the goal, Enter to save"),
    ("cp_goal_edit", "编辑目标", "Edit goal"),
    ("cp_goal_clear", "清除目标", "Clear goal"),
    # ---- 发送队列 (composer queue strip) ----
    ("cp_queue_title", "等待发送", "Waiting to send"),
    ("cp_queue_hint", "拖动把手调整顺序", "Drag the handle to reorder"),
    ("cp_queue_interject", "插话", "Interject"),
    ("cp_queue_interjected", "已插话", "Interjected"),
    ("cp_queue_edit_hint", "撤下并回填输入框", "Withdraw and refill the input"),
    ("cp_queue_remove_hint", "撤下这条排队轮次", "Withdraw this queued turn"),
    ("cp_queue_remove_waiting_hint", "撤下这条等待发送的消息", "Withdraw this waiting message"),
    ("cp_queue_more", "还有 {} 条未显示", "{} more not shown", ["count"]),
    ("cp_queue_attachment_only", "仅附件", "Attachment only"),
    ("cp_queue_attachments", "附件 {}", "{} attachment(s)", ["count"]),
    ("cp_queue_send", "加入等待发送", "Add to the send queue"),

    # ---- 输入区硬件状态网格 (HostHud) ----
    ("hud_cpu", "CPU", "CPU"),
    ("hud_memory", "内存", "Memory"),
    # 网络与磁盘只报占用率（读数在悬停提示里），单位与速率不再进格子。
    ("hud_network", "网络", "Network"),
    ("hud_disk", "磁盘", "Disk"),
    # 详情面板：四项读数（CPU / 内存 / 网络 / 磁盘）一次列全。
    ("hud_detail_title", "本机状态", "This machine"),
    ("hud_read", "读", "R"),
    ("hud_write", "写", "W"),
    ("hud_cores", "{} 核", "{} cores", ["cores"]),
    ("hud_tasks_short", "任务", "Tasks"),
    ("hud_agents_short", "智能体", "Agents"),
    ("hud_tasks_title", "后台任务", "Running tasks"),
    ("hud_tasks_count", "{} 个运行中", "{} running", ["count"]),
    ("hud_tasks_empty", "没有正在运行的任务", "Nothing is running"),
    ("hud_tasks_more", "还有 {} 条未显示", "{} more not shown", ["count"]),
    ("hud_tasks_stop", "终止任务", "Terminate task"),
    ("hud_tasks_expand", "展开详情", "Show details"),
    ("hud_tasks_collapse", "收起详情", "Hide details"),
    ("hud_tasks_loading", "读取输出…", "Reading output…"),
    ("hud_tasks_no_output", "暂无输出", "No output yet"),
    ("hud_tasks_hint", "后台任务：{} 个命令正在运行", "Background tasks: {} running", ["count"]),

    # ---- status bar (§8.5) ----
    ("cp_status_local", "本地", "Local"),
    ("cp_status_branch", "分支", "Branch"),

    # ---- message row ----
    ("ms_agent_loop", "智能体循环", "Agent loop"),
    ("ms_state_done", "任务完成", "Task complete"),

    # ---- components ----
    ("ui_collapse_sidebar", "收起侧栏", "Collapse sidebar"),
    ("ui_expand_sidebar", "展开侧栏", "Expand sidebar"),
    ("ui_avatar_mine", "我", "Me"),
    ("ui_search_placeholder", "搜索", "Search"),
]
