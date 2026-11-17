/**
 * 工作过程行首的工具名：一格两字。
 *
 * 行里已经有工具图标与状态点，四字名会把目标/摘要挤出第一行。这张表与桌面版、
 * 安卓版读的是同一套词（`crates/wunder-desktop/frontend/src/timeline_text.rs` 的
 * `tool_label`，安卓版在 `project.rs` 有同一份），改词时三端一起改，不要只改这里。
 *
 * 命令类工具（执行命令 / 命令会话 / 写 stdin）由 `commandToolLabel` 统一成「执行」，
 * 这里只为兜底也把它们列出来，免得某条记录没有交互信息时露出四字名。
 */
const LABELS: Record<string, string> = {
  '': '工具',
  '执行命令': '执行',
  '运行命令': '执行',
  'exec_command': '执行',
  'execute_command': '执行',
  'run_command': '执行',
  'shell': '执行',
  '命令会话': '执行',
  '命令交互': '执行',
  'command_session': '执行',
  'write_stdin': '执行',
  'write_command_stdin': '执行',
  '读取文件': '读取',
  'read_file': '读取',
  'read files': '读取',
  '写入文件': '写入',
  'write_file': '写入',
  '文本编辑': '编辑',
  '编辑文件': '编辑',
  'apply_patch': '编辑',
  '应用补丁': '编辑',
  'edit_file': '编辑',
  'edit_file2': '编辑',
  'text_editor': '编辑',
  'text_edit': '编辑',
  'str_replace_editor': '编辑',
  'str_replace_based_edit_tool': '编辑',
  '列出文件': '列出',
  'list_files': '列出',
  'list_file': '列出',
  '文件名匹配': '匹配',
  'glob': '匹配',
  'glob_files': '匹配',
  '搜索内容': '搜索',
  'search_content': '搜索',
  'search_files': '搜索',
  '网页搜索': '联网',
  'web_search': '联网',
  'web_search_tool': '联网',
  'web search': '联网',
  'websearch': '联网',
  '网页抓取': '网页',
  'web_fetch': '网页',
  'web_fetch_tool': '网页',
  'web fetch': '网页',
  'webfetch': '网页',
  '问询面板': '问询',
  'question_panel': '问询',
  'ask_panel': '问询',
  '计划面板': '计划',
  'update_plan': '计划',
  'plan_update': '计划',
  '读图工具': '看图',
  '读图': '看图',
  'read_image': '看图',
  'view_image': '看图',
  '技能调用': '技能',
  'skill_call': '技能',
  'skill_get': '技能',
  '定时任务': '定时',
  'schedule_task': '定时',
  '上下文压缩': '压缩',
  'context_compaction': '压缩',
  'compact': '压缩',
  // 英文界面下压缩条目的标题来自 i18n 文案，不是工具标识名。
  'context compaction': '压缩',
  'context_compaction': '压缩',
  '子智能体控制': '子代理',
  'subagent_control': '子代理',
  '会话线程控制': '线程',
  '线程控制': '线程',
  'thread_control': '线程',
  'session_thread': '线程',
  '记忆管理': '记忆',
  'memory_manager': '记忆',
  '浏览器': '浏览',
  'browser': '浏览',
  '桌面控制': '桌面',
  'desktop_control': '桌面',
  'desktop': '桌面',
  'goal': '目标',
  '语音生成': '语音',
  'generate_speech': '语音',
  '声转文': '转写',
  'transcribe_speech': '转写',
  '图像生成': '图像',
  '绘图生成': '图像',
  'generate_image': '图像',
  '视频生成': '视频',
  'generate_video': '视频'
};

/** 行首标签：命中词表用两字词，没命中就保留工具自己的名字。 */
export const toolRowLabel = (name: string): string => {
  const key = String(name ?? '').trim();
  const mapped = LABELS[key] ?? LABELS[key.toLowerCase()];
  return mapped ?? key;
};
