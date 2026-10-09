# -*- coding: utf-8 -*-
# Core UI literals (chat, thread log, runtime status, channels, cron, settings
# preview) that were introduced without catalog coverage.

ENTRIES = [
    # composer.slint
    ("composer_goal_hint", "/goal  设置目标", "/goal  set a goal"),
    ("composer_new_hint", "/new  新建线程", "/new  new thread"),
    ("composer_stop_hint", "/stop  停止并退出目标态", "/stop  stop and leave goal mode"),
    ("cp_terminal_placeholder", "输入本地命令…", "Type a local command…"),
    ("cp_terminal_exit", "退出终端", "Exit terminal"),
    ("cp_terminal_panel_title", "终端", "Terminal"),
    ("cp_terminal_idle", "空闲", "Idle"),
    ("cp_terminal_clear", "清屏", "Clear"),
    ("cp_terminal_interrupt", "中断", "Interrupt"),
    # thread_log.slint
    ("tl_next_events", "下一页事件", "Next events"),
    ("tl_event_detail", "事件详情", "Event detail"),
    ("tl_export_log", "导出日志", "Export log"),
    ("tl_filter_events", "按关键词筛选事件", "Filter events by keyword"),
    ("tl_no_events", "暂无事件", "No events"),
    ("tl_overview", "概览", "Overview"),
    ("tl_user_turn", "用户轮次 ", "User turn "),
    # message.slint
    ("msg_code", "代码", "Code"),
    ("msg_copied", "已复制", "Copied"),
    ("msg_thought", "已思考", "Thought"),
    ("msg_thinking", "思考中", "Thinking"),
    ("msg_not_loaded", "（暂未加载）", "(not loaded yet)"),
    # runtime_status.slint
    ("rs_queued", "任务已排队", "Task queued"),
    ("rs_failed", "执行失败", "Execution failed"),
    ("rs_running", "正在", "Running"),
    ("rs_queueing", "正在排队", "Queuing"),
    ("rs_waiting", "等待", "Waiting"),
    ("rs_output_ended", "输出已结束", "Output finished"),
    # entity_pages.slint
    ("ep_import_failed", "导入失败", "Import failed"),
    # channels_page.slint
    ("ch_write_test_log", "写入测试日志", "Write test log"),
    ("ch_pending_setup", "待配置", "Pending setup"),
    ("ch_my_accounts", "我的渠道账号", "My channel accounts"),
    ("ch_qr_login", "扫码登录", "Scan to sign in"),
    ("ch_add_new", "新增", "Add"),
    ("ch_runtime_logs", "渠道运行日志", "Channel runtime logs"),
    ("ch_clear_all", "清空", "Clear"),
    ("ch_channel_config", "渠道配置", "Channel config"),
    ("ch_gen_qr", "生成二维码", "Generate QR code"),
    ("ch_edit_config", "编辑配置", "Edit config"),
    ("ch_account_info", "账号信息", "Account info"),
    ("ch_account_connection", "账号连接配置", "Account connection"),
    ("ch_config_status", "配置状态", "Config status"),
    ("ch_no_logs", "暂无运行日志", "No runtime logs"),
    # cron_page.slint
    ("cron_create", "新建定时任务", "New scheduled task"),
    ("cron_empty", "暂无定时任务", "No scheduled tasks"),
    ("cron_subtitle", "查看与管理智能体定时任务", "View and manage agent scheduled tasks"),
    # main.slint preview turn / shared labels
    ("main_preview_prompt", "整理工作目录并说明结果", "Organize the work folder and report"),
    ("main_preview_answer", "已完成目录检查，并整理了可以继续使用的文件。", "Directory checked and usable files organized."),
    ("main_preview_reasoning", "先读取目录清单，确认文件类型，再检查文本内容是否可用，最后汇总结果。", "Read the listing first, confirm file types, verify text readability, then summarize."),
    ("main_preview_step_title1", "读取工作目录", "Read work folder"),
    ("main_preview_step_title2", "检查文件内容", "Check file content"),
    ("main_preview_step1", "已读取并整理文件列表", "File list read and organized"),
    ("main_preview_step2", "已验证文本内容可读取", "Text content verified readable"),
    ("main_preview_detail1", "执行完成\\n找到 2 个可用文件，目录结构正常。", "Done\\nFound 2 usable files, folder structure is fine."),
    ("main_preview_detail2", "执行完成\\n文件内容已读取，未发现格式错误。", "Done\\nFile content read, no format issues."),
    ("main_preview_workflow", "读取工作目录\\n已读取并整理文件列表\\n\\n检查文件内容\\n已验证文本内容可读取", "Read work folder\\nFile list read and organized\\n\\nCheck file content\\nText content verified readable"),
    ("main_command_failed", "命令执行失败", "Command failed"),
    ("model_provider_custom", "自定义（手动填写服务地址）", "Custom (enter base URL manually)"),
    # Static preview fixtures for the two-column layout (slint-viewer only).
    ("ws_preview_default", "默认工作区", "Default workspace"),
    ("ws_preview_docs", "资料工作区", "Reference workspace"),
    ("ws_preview_root_a", "示例/项目", "sample/project"),
    ("ws_preview_root_b", "示例/资料", "sample/reference"),
]
