import { elements } from "./elements.js?v=20260110-05";
import { state } from "./state.js";
import { escapeHtml } from "./utils.js?v=20251229-02";
import { getWunderBase } from "./api.js";
import { applyPromptToolError, ensureToolSelectionLoaded, getSelectedToolNames } from "./tools.js?v=20251227-13";
import { notify } from "./notify.js";
import { t } from "./i18n.js?v=20260110-04";

// 渲染系统提示词，并高亮 <tools> 区域内的工具名称与技能名
const renderPromptSegmentWithSkills = (segment, segmentState) => {
  const skillHeaders = new Set(["[Mounted Skills]", "[已挂载技能]"]);
  const lines = segment.split(/\r?\n/);
  const output = lines.map((line) => {
    const trimmed = line.trim();
    if (skillHeaders.has(trimmed)) {
      segmentState.inSkills = true;
      return escapeHtml(line);
    }
    if (trimmed.startsWith("[") && trimmed.endsWith("]") && !skillHeaders.has(trimmed)) {
      segmentState.inSkills = false;
      return escapeHtml(line);
    }
    if (segmentState.inSkills) {
      const match = line.match(/^(\s*-\s+)(.+)$/);
      if (match) {
        return `${escapeHtml(match[1])}<span class="skill-highlight">${escapeHtml(match[2])}</span>`;
      }
    }
    return escapeHtml(line);
  });
  return output.join("\n");
};

// 渲染系统提示词，并高亮 <tools> 区域内的工具名称与技能名
const renderSystemPrompt = (rawText) => {
  if (!rawText) {
    return "";
  }
  const builtinList = Array.isArray(state.toolSelection?.builtin) ? state.toolSelection.builtin : [];
  const knowledgeList = Array.isArray(state.toolSelection?.knowledge) ? state.toolSelection.knowledge : [];
  const userList = Array.isArray(state.toolSelection?.userTools) ? state.toolSelection.userTools : [];
  const sharedList = Array.isArray(state.toolSelection?.sharedTools) ? state.toolSelection.sharedTools : [];
  const builtinToolNames = new Set(builtinList.map((item) => item.name));
  const knowledgeToolNames = new Set(knowledgeList.map((item) => item.name));
  const userToolNames = new Set(userList.map((item) => item.name));
  const sharedToolNames = new Set(sharedList.map((item) => item.name));
  const startTag = "<tools>";
  const endTag = "</tools>";
  let output = "";
  let cursor = 0;
  const skillState = { inSkills: false };
  while (true) {
    const start = rawText.indexOf(startTag, cursor);
    if (start < 0) {
      output += renderPromptSegmentWithSkills(rawText.slice(cursor), skillState);
      break;
    }
    const end = rawText.indexOf(endTag, start + startTag.length);
    if (end < 0) {
      output += renderPromptSegmentWithSkills(rawText.slice(cursor), skillState);
      break;
    }
    output += renderPromptSegmentWithSkills(rawText.slice(cursor, start), skillState);
    output += escapeHtml(startTag);
    const toolsContent = rawText.slice(start + startTag.length, end);
    const lines = toolsContent.split(/\r?\n/);
    const highlighted = lines
      .map((line) => {
        const match = line.match(/"name"\s*:\s*"([^"]+)"/);
        const escapedLine = escapeHtml(line);
        if (!match) {
          return escapedLine;
        }
        const escapedMatch = escapeHtml(match[0]);
        const escapedName = escapeHtml(match[1]);
        let highlightClass = "tool-highlight";
        if (builtinToolNames.has(match[1])) {
          highlightClass = "tool-highlight builtin";
        } else if (knowledgeToolNames.has(match[1])) {
          highlightClass = "tool-highlight knowledge";
        } else if (userToolNames.has(match[1])) {
          highlightClass = "tool-highlight user";
        } else if (sharedToolNames.has(match[1])) {
          highlightClass = "tool-highlight shared";
        }
        const highlightedMatch = escapedMatch.replace(
          escapedName,
          `<span class="${highlightClass}">${escapedName}</span>`
        );
        return escapedLine.replace(escapedMatch, highlightedMatch);
      })
      .join("\n");
    output += highlighted;
    output += escapeHtml(endTag);
    cursor = end + endTag.length;
  }
  return output;
};

// 更新系统提示词构建耗时展示
const updatePromptBuildTime = (value, options = {}) => {
  if (!elements.promptBuildTime) {
    return;
  }
  if (options.loading) {
    elements.promptBuildTime.textContent = t("prompt.buildTime.loading");
    return;
  }
  if (!Number.isFinite(value)) {
    elements.promptBuildTime.textContent = t("prompt.buildTime.empty");
    return;
  }
  const ms = Math.max(0, Number(value));
  const display = ms >= 1000 ? `${(ms / 1000).toFixed(2)} s` : `${ms.toFixed(2)} ms`;
  elements.promptBuildTime.textContent = t("prompt.buildTime.value", { duration: display });
};

// 拉取系统提示词并展示在侧边栏面板
export const loadSystemPrompt = async (options = {}) => {
  const showToast = Boolean(options.showToast);
  const wunderBase = getWunderBase();
  const endpoint = `${wunderBase}/system_prompt`;
  try {
    await ensureToolSelectionLoaded();
  } catch (error) {
    applyPromptToolError(error.message);
  }
  const payload = {
    user_id: String(
      elements.userId?.value || elements.promptUserId?.value || ""
    ).trim(),
    session_id: elements.sessionId.value.trim() || null,
  };
  const toolNames = getSelectedToolNames();
  if (toolNames.length) {
    payload.tool_names = toolNames;
  }

  elements.systemPrompt.textContent = t("common.loading");
  updatePromptBuildTime(null, { loading: true });
  try {
    const response = await fetch(endpoint, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
      },
      body: JSON.stringify(payload),
    });
    if (!response.ok) {
      throw new Error(t("common.requestFailed", { status: response.status }));
    }
    const result = await response.json();
    elements.systemPrompt.innerHTML = renderSystemPrompt(result.prompt || "");
    updatePromptBuildTime(result.build_time_ms);
    state.runtime.promptNeedsRefresh = false;
    if (showToast) {
      notify(t("prompt.refreshSuccess"), "success");
    }
  } catch (error) {
    elements.systemPrompt.textContent = t("prompt.requestError", { message: error.message });
    updatePromptBuildTime(null);
    if (showToast) {
      notify(t("prompt.refreshFailed", { message: error.message }), "error");
    }
  }
};

// 初始化系统提示词面板交互
export const initPromptPanel = () => {
  state.runtime.promptReloadHandler = loadSystemPrompt;
  elements.loadPromptBtn.addEventListener("click", () => loadSystemPrompt({ showToast: true }));
};




