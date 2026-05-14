const TOOL_ICON_RULES = [
  { keywords: ["用户世界工具", "user_world", "user world"], icon: "fa-earth-asia" },
  { keywords: ["会话让出", "sessions_yield", "session yield", "yield"], icon: "fa-share-from-square" },
  { keywords: ["自我状态", "self_status", "self status"], icon: "fa-gauge-high" },
  { keywords: ["桌面控制器", "desktop_controller", "desktop controller"], icon: "fa-computer-mouse" },
  { keywords: ["桌面监视器", "桌面监控", "desktop_monitor", "desktop monitor"], icon: "fa-display" },
  { keywords: ["计划面板", "计划看板", "update_plan", "plan board"], icon: "fa-table-columns" },
  { keywords: ["问询面板", "question_panel", "ask_panel", "question panel"], icon: "fa-circle-question" },
  { keywords: ["目标态", "目标工具", "goal", "goal mode"], icon: "fa-bullseye" },
  { keywords: ["浏览器", "browser", "browser_navigate", "browser_click", "browser_type", "browser_screenshot", "browser_read_page"], icon: "fa-window-maximize" },
  { keywords: ["节点调用", "node.invoke", "node_invoke", "node invoke", "gateway_invoke"], icon: "fa-diagram-project" },
  { keywords: ["技能调用", "skill_call", "skill_get"], icon: "fa-book-open" },
  { keywords: ["智能体蜂群", "agent_swarm", "swarm_control"], icon: "fa-bee" },
  { keywords: ["子智能体控制", "subagent_control"], icon: "fa-diagram-project" },
  { keywords: ["会话线程控制", "thread_control", "session_thread"], icon: "fa-code-branch" },
  { keywords: ["网页抓取", "web_fetch", "web fetch", "webfetch"], icon: "fa-globe" },
  { keywords: ["a2a观察", "a2a_observe"], icon: "fa-glasses" },
  { keywords: ["a2a等待", "a2a_wait"], icon: "fa-clock" },
  { keywords: ["休眠等待", "sleep_wait", "sleep", "pause"], icon: "fa-hourglass-half" },
  { keywords: ["记忆管理", "memory_manager", "memory_manage", "memory manager"], icon: "fa-memory" },
  { keywords: ["a2ui"], icon: "fa-image" },
  { keywords: ["读图工具", "read_image", "read image", "view_image", "view image"], icon: "fa-eye" },
  { keywords: ["声转文", "语音转文", "transcribe_speech", "transcribe speech", "speech_to_text", "speech to text", "asr", "audio transcription"], icon: "fa-microphone-lines" },
  { keywords: ["文转声", "语音生成", "generate_speech", "speech generation", "text_to_speech", "text to speech", "tts"], icon: "fa-wave-square" },
  { keywords: ["图像生成", "绘图生成", "generate_image", "image generation", "text_to_image", "text to image"], icon: "fa-paintbrush" },
  { keywords: ["视频生成", "generate_video", "video generation", "text_to_video", "text to video"], icon: "fa-film" },
  { keywords: ["渠道工具", "channel_tool", "channel tool", "channel_send", "channel_contacts"], icon: "fa-comments" },
  { keywords: ["列出文件", "list files", "list_file", "list_files"], icon: "fa-folder-open" },
  { keywords: ["读取文件", "read file", "read_file"], icon: "fa-file-lines" },
  { keywords: ["写入文件", "write file", "write_file"], icon: "fa-file-circle-plus" },
  { keywords: ["文本编辑", "text edit", "text_editor", "text editor", "edit_file2"], icon: "fa-file-pen" },
  { keywords: ["应用补丁", "apply patch", "apply_patch"], icon: "fa-pen-to-square" },
  { keywords: ["LSP查询", "lsp query", "lsp"], icon: "fa-code" },
  { keywords: ["执行命令", "运行命令", "run command", "execute command", "execute_command", "shell"], icon: "fa-terminal" },
  { keywords: ["ptc", "programmatic_tool_call"], icon: "fa-code" },
  { keywords: ["定时任务", "计划任务", "cron", "schedule", "scheduled", "timer", "schedule_task"], icon: "fa-clock" },
  { keywords: ["搜索", "检索", "search", "query", "retrieve", "search_content"], icon: "fa-magnifying-glass" },
  { keywords: ["知识", "knowledge", "rag", "vector", "embedding", "document", "kb"], icon: "fa-database" },
  { keywords: ["mcp", "connector", "integration", "endpoint"], icon: "fa-plug" },
  { keywords: ["shared", "share"], icon: "fa-wrench" },
  { keywords: ["最终回复", "final answer", "final response", "final_response"], icon: "fa-paper-plane" }
];

const normalizeText = (value) => String(value || "").trim().toLowerCase();
const normalizeMatchKey = (value) => normalizeText(value).replace(/[\s_.\-:/\\@]+/g, "");

const buildSearchText = (input) => {
  if (Array.isArray(input)) return input.map((item) => String(item || "").trim()).filter(Boolean).join(" ");
  if (typeof input === "string") return input;
  if (!input || typeof input !== "object") return "";
  return [input.name, input.runtimeName, input.description, input.hint, input.category, input.group, input.source]
    .map((item) => String(item || "").trim())
    .filter(Boolean)
    .join(" ");
};

const buildCategoryKey = (input) => {
  if (!input || typeof input !== "object" || Array.isArray(input)) return "";
  return normalizeText(input.category || input.group || input.source);
};

const matchesKeyword = (text, normalizedText, keyword) => {
  const lowerKeyword = normalizeText(keyword);
  if (!lowerKeyword) return false;
  if (text.includes(lowerKeyword)) return true;
  const normalizedKeyword = normalizeMatchKey(lowerKeyword);
  return Boolean(normalizedKeyword && normalizedText.includes(normalizedKeyword));
};

export const resolveToolIconClass = (input) => {
  const rawText = buildSearchText(input);
  const text = normalizeText(rawText);
  const normalizedText = normalizeMatchKey(rawText);
  const categoryKey = buildCategoryKey(input);
  if (!text && !categoryKey) return "fa-toolbox";
  if (text === "wunder@excute" || text.endsWith("@wunder@excute")) return "fa-dragon";
  if (text === "wunder@doc2md" || text.endsWith("@wunder@doc2md")) return "fa-file-lines";
  for (const rule of TOOL_ICON_RULES) {
    if (rule.keywords.some((keyword) => matchesKeyword(text, normalizedText, keyword))) {
      return rule.icon;
    }
  }
  if (categoryKey === "mcp" || rawText.includes("@")) return "fa-plug";
  if (categoryKey === "knowledge") return "fa-database";
  if (categoryKey === "skill") return "fa-book";
  if (categoryKey === "shared" || categoryKey === "user") return "fa-wrench";
  return "fa-toolbox";
};
