import { elements } from "./elements.js?v=20260112-04";
import { state } from "./state.js";
import { getWunderBase } from "./api.js";
import { getToolInputSchema } from "./utils.js?v=20251229-02";
import { syncPromptTools } from "./tools.js?v=20251227-13";
import { openToolDetailModal } from "./tool-detail.js";
import { notify } from "./notify.js";
import { appendLog } from "./log.js?v=20260108-02";
import { t } from "./i18n.js?v=20260112-03";

// 拉取内置工具清单与启用状态
export const loadBuiltinTools = async () => {
  const wunderBase = getWunderBase();
  const endpoint = `${wunderBase}/admin/tools`;
  const response = await fetch(endpoint);
  if (!response.ok) {
    throw new Error(t("builtin.requestFailed", { status: response.status }));
  }
  const result = await response.json();
  state.builtin.tools = Array.isArray(result.tools) ? result.tools : [];
  renderBuiltinTools();
};

// 渲染内置工具勾选列表
const renderBuiltinTools = () => {
  elements.builtinToolsList.textContent = "";
  if (!state.builtin.tools.length) {
    elements.builtinToolsList.textContent = t("builtin.empty");
    return;
  }
  state.builtin.tools.forEach((tool) => {
    const item = document.createElement("div");
    item.className = "tool-item";
    const checkbox = document.createElement("input");
    checkbox.type = "checkbox";
    checkbox.checked = Boolean(tool.enabled);
    checkbox.addEventListener("change", (event) => {
      tool.enabled = event.target.checked;
      const actionMessage = tool.enabled
        ? t("builtin.enabled", { name: tool.name })
        : t("builtin.disabled", { name: tool.name });
      saveBuiltinTools()
        .then(() => {
          appendLog(actionMessage);
          notify(actionMessage, "success");
        })
        .catch((error) => {
          console.error(t("builtin.saveFailed", { message: error.message }), error);
          notify(t("builtin.saveFailed", { message: error.message }), "error");
        });
    });
    const label = document.createElement("label");
    label.innerHTML = `<strong>${tool.name}</strong><span class="muted">${
      tool.description || ""
    }</span>`;
    // 点击工具条目查看详情，避免与勾选动作冲突
    item.addEventListener("click", (event) => {
      if (event.target === checkbox) {
        return;
      }
      const metaParts = [
        t("builtin.meta.label"),
        checkbox.checked ? t("builtin.meta.enabled") : t("builtin.meta.disabled"),
      ];
      openToolDetailModal({
        title: tool.name || t("tool.detail.title"),
        meta: metaParts.join(" · "),
        description: tool.description || "",
        schema: getToolInputSchema(tool),
      });
    });
    item.appendChild(checkbox);
    item.appendChild(label);
    elements.builtinToolsList.appendChild(item);
  });
};

// 保存内置工具启用状态
const saveBuiltinTools = async () => {
  const wunderBase = getWunderBase();
  const endpoint = `${wunderBase}/admin/tools`;
  const enabled = state.builtin.tools.filter((tool) => tool.enabled).map((tool) => tool.name);
  const response = await fetch(endpoint, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
    },
    body: JSON.stringify({ enabled }),
  });
  if (!response.ok) {
    throw new Error(t("builtin.requestFailed", { status: response.status }));
  }
  const result = await response.json();
  state.builtin.tools = Array.isArray(result.tools) ? result.tools : [];
  renderBuiltinTools();
  syncPromptTools();
};

// 初始化内置工具面板交互
export const initBuiltinPanel = () => {
  elements.refreshBuiltinBtn.addEventListener("click", async () => {
    try {
      await loadBuiltinTools();
      notify(t("builtin.refreshSuccess"), "success");
    } catch (error) {
      elements.builtinToolsList.textContent = t("builtin.refreshFailedList", {
        message: error.message,
      });
      notify(t("builtin.refreshFailed", { message: error.message }), "error");
    }
  });
};






