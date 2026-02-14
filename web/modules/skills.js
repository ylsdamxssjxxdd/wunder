import { state } from "./state.js";
import { getWunderBase } from "./api.js";
import { appendLog } from "./log.js?v=20260108-02";
import { syncPromptTools } from "./tools.js?v=20260214-01";
import { notify } from "./notify.js";
import { escapeHtml } from "./utils.js?v=20251229-02";
import { t } from "./i18n.js?v=20260214-01";
import { resolveApiErrorMessage } from "./api-error.js";

const skillsList = document.getElementById("skillsList");
const refreshSkillsBtn = document.getElementById("refreshSkillsBtn");
const addSkillBtn = document.getElementById("addSkillBtn");
const skillUploadInput = document.getElementById("skillUploadInput");
const skillDetailTitle = document.getElementById("skillDetailTitle");
const skillDetailMeta = document.getElementById("skillDetailMeta");
const skillFileTree = document.getElementById("skillFileTree");
const skillEditorPath = document.getElementById("skillEditorPath");
const skillFileSaveBtn = document.getElementById("skillFileSaveBtn");
const skillFileContent = document.getElementById("skillFileContent");
const skillEditorBody = document.getElementById("skillEditorBody");
const skillFileHighlight = document.getElementById("skillFileHighlight");

const viewState = {
  selectedIndex: -1,
  files: [],
  root: "",
  activeFile: "",
  fileContent: "",
  detailVersion: 0,
  fileVersion: 0,
};

const normalizeSkillPath = (rawPath) => String(rawPath || "").replace(/\\/g, "/");

const resolveDefaultSkillFile = (entries) => {
  if (!Array.isArray(entries)) {
    return "";
  }
  let fallback = "";
  for (const entry of entries) {
    if (!entry || entry.kind === "dir") {
      continue;
    }
    const path = String(entry.path || "");
    if (!path) {
      continue;
    }
    const normalized = normalizeSkillPath(path).toLowerCase();
    if (normalized === "skill.md") {
      return path;
    }
    if (!fallback && normalized.endsWith("/skill.md")) {
      fallback = path;
    }
  }
  return fallback;
};

const HIGHLIGHT_KEYWORDS = new Set([
  "await",
  "break",
  "case",
  "catch",
  "class",
  "const",
  "continue",
  "default",
  "do",
  "else",
  "enum",
  "export",
  "extends",
  "finally",
  "for",
  "fn",
  "function",
  "if",
  "impl",
  "import",
  "in",
  "interface",
  "let",
  "match",
  "new",
  "pub",
  "return",
  "self",
  "static",
  "struct",
  "switch",
  "throw",
  "try",
  "type",
  "use",
  "var",
  "while",
  "yield",
]);

const HIGHLIGHT_TOKEN_REGEX =
  /(\"(?:\\.|[^\"\\])*\"|\'(?:\\.|[^'\\])*\'|`(?:\\.|[^`\\])*`|\/\/.*?$|\/\*[\s\S]*?\*\/|\b\d+(?:\.\d+)?\b|\b[A-Za-z_][A-Za-z0-9_]*\b)/gm;

let skillHighlightTimer = 0;

const highlightInlineCode = (text) => {
  const raw = String(text ?? "");
  if (!raw) {
    return "&nbsp;";
  }
  let result = "";
  let lastIndex = 0;
  for (const match of raw.matchAll(HIGHLIGHT_TOKEN_REGEX)) {
    const token = match[0];
    const index = match.index ?? 0;
    if (index > lastIndex) {
      result += escapeHtml(raw.slice(lastIndex, index));
    }
    let className = "";
    if (token.startsWith("//") || token.startsWith("/*")) {
      className = "code-token-comment";
    } else if (token.startsWith('"') || token.startsWith("'") || token.startsWith("`")) {
      className = "code-token-string";
    } else if (/^\d/.test(token)) {
      className = "code-token-number";
    } else if (HIGHLIGHT_KEYWORDS.has(token)) {
      className = "code-token-keyword";
    }
    if (className) {
      result += `<span class="${className}">${escapeHtml(token)}</span>`;
    } else {
      result += escapeHtml(token);
    }
    lastIndex = index + token.length;
  }
  if (lastIndex < raw.length) {
    result += escapeHtml(raw.slice(lastIndex));
  }
  return result || "&nbsp;";
};

const updateSkillEditorHighlight = () => {
  if (!skillFileHighlight || !skillFileContent) {
    return;
  }
  skillFileHighlight.innerHTML = highlightInlineCode(skillFileContent.value);
  skillFileHighlight.scrollTop = skillFileContent.scrollTop;
  skillFileHighlight.scrollLeft = skillFileContent.scrollLeft;
};

const scheduleSkillEditorHighlight = () => {
  if (!skillFileHighlight) {
    return;
  }
  if (skillHighlightTimer) {
    cancelAnimationFrame(skillHighlightTimer);
  }
  skillHighlightTimer = requestAnimationFrame(() => {
    skillHighlightTimer = 0;
    updateSkillEditorHighlight();
  });
};

const syncSkillEditorScroll = () => {
  if (!skillFileHighlight || !skillFileContent) {
    return;
  }
  skillFileHighlight.scrollTop = skillFileContent.scrollTop;
  skillFileHighlight.scrollLeft = skillFileContent.scrollLeft;
};

const getActiveSkill = () =>
  Number.isInteger(viewState.selectedIndex)
    ? state.skills.skills[viewState.selectedIndex] || null
    : null;

const isSkillDeletable = (skill) => {
  const normalized = normalizeSkillPath(skill?.path).toLowerCase();
  return /(^|\/)eva_skills(\/|$)/.test(normalized);
};

const extractErrorMessage = async (response) => resolveApiErrorMessage(response, "");

const renderSkillDetailHeader = (skill) => {
  if (!skillDetailTitle || !skillDetailMeta) {
    return;
  }
  if (!skill) {
    skillDetailTitle.textContent = t("skills.detail.unselected");
    skillDetailMeta.textContent = "";
    return;
  }
  skillDetailTitle.textContent = skill.name || t("skills.detail.title");
  const metaParts = [];
  if (skill.path) {
    metaParts.push(skill.path);
  }
  if (viewState.root && viewState.root !== skill.path) {
    metaParts.push(viewState.root);
  }
  skillDetailMeta.textContent = metaParts.join(" · ");
};

const setSkillEditorDisabled = (disabled) => {
  if (skillEditorBody) {
    skillEditorBody.classList.toggle("is-disabled", disabled);
  }
  if (skillFileContent) {
    skillFileContent.disabled = disabled;
  }
  if (skillFileSaveBtn) {
    skillFileSaveBtn.disabled = disabled;
  }
};

const renderSkillEditor = () => {
  if (skillEditorPath) {
    skillEditorPath.textContent = viewState.activeFile || t("skills.file.unselected");
  }
  if (skillFileContent) {
    skillFileContent.value = viewState.fileContent || "";
  }
  setSkillEditorDisabled(!viewState.activeFile);
  scheduleSkillEditorHighlight();
};

const showSkillEditorMessage = (message) => {
  if (skillFileContent) {
    skillFileContent.value = message;
  }
  setSkillEditorDisabled(true);
  scheduleSkillEditorHighlight();
};

const renderSkillFileTree = () => {
  if (!skillFileTree) {
    return;
  }
  skillFileTree.textContent = "";
  const skill = getActiveSkill();
  if (!skill) {
    skillFileTree.textContent = t("skills.files.unselected");
    return;
  }
  if (!Array.isArray(viewState.files) || viewState.files.length === 0) {
    skillFileTree.textContent = t("skills.files.empty");
    return;
  }
  viewState.files.forEach((entry) => {
    const path = String(entry?.path || "");
    if (!path) {
      return;
    }
    const kind = entry?.kind === "dir" ? "dir" : "file";
    const item = document.createElement("div");
    item.className = `skill-tree-item is-${kind}`;
    if (kind === "file" && path === viewState.activeFile) {
      item.classList.add("is-active");
    }
    const depth = Math.max(0, path.split("/").length - 1);
    item.style.paddingLeft = `${8 + depth * 14}px`;
    item.title = path;
    const icon = document.createElement("i");
    icon.className = kind === "dir" ? "fa-solid fa-folder" : "fa-regular fa-file-lines";
    const name = document.createElement("span");
    name.className = "skill-tree-name";
    name.textContent = path.split("/").pop() || path;
    item.append(icon, name);
    if (kind === "file") {
      item.addEventListener("click", () => {
        selectSkillFile(path);
      });
    }
    skillFileTree.appendChild(item);
  });
};

const clearSkillDetail = () => {
  viewState.selectedIndex = -1;
  viewState.files = [];
  viewState.root = "";
  viewState.activeFile = "";
  viewState.fileContent = "";
  renderSkillDetailHeader(null);
  renderSkillFileTree();
  renderSkillEditor();
};

const loadSkillFiles = async (skillName) => {
  if (!skillName) {
    throw new Error(t("skills.nameRequired"));
  }
  const wunderBase = getWunderBase();
  const endpoint = `${wunderBase}/admin/skills/files?name=${encodeURIComponent(skillName)}`;
  const response = await fetch(endpoint);
  if (!response.ok) {
    const detail = await extractErrorMessage(response);
    throw new Error(detail || t("common.requestFailed", { status: response.status }));
  }
  return response.json();
};

const loadSkillFileContent = async (skillName, filePath) => {
  if (!skillName) {
    throw new Error(t("skills.nameRequired"));
  }
  if (!filePath) {
    throw new Error(t("skills.file.selectRequired"));
  }
  const wunderBase = getWunderBase();
  const endpoint = `${wunderBase}/admin/skills/file?name=${encodeURIComponent(
    skillName
  )}&path=${encodeURIComponent(filePath)}`;
  const response = await fetch(endpoint);
  if (!response.ok) {
    const detail = await extractErrorMessage(response);
    throw new Error(detail || t("common.requestFailed", { status: response.status }));
  }
  const result = await response.json();
  return String(result.content || "");
};

const saveSkillFileContent = async (skillName, filePath, content) => {
  if (!skillName) {
    throw new Error(t("skills.nameRequired"));
  }
  if (!filePath) {
    throw new Error(t("skills.file.selectRequired"));
  }
  const wunderBase = getWunderBase();
  const endpoint = `${wunderBase}/admin/skills/file`;
  const response = await fetch(endpoint, {
    method: "PUT",
    headers: {
      "Content-Type": "application/json",
    },
    body: JSON.stringify({ name: skillName, path: filePath, content }),
  });
  if (!response.ok) {
    const detail = await extractErrorMessage(response);
    throw new Error(detail || t("common.requestFailed", { status: response.status }));
  }
  return response.json();
};

const selectSkill = async (skill, index) => {
  if (!skill) {
    clearSkillDetail();
    renderSkills();
    return;
  }
  viewState.selectedIndex = index;
  viewState.files = [];
  viewState.root = "";
  viewState.activeFile = "";
  viewState.fileContent = "";
  renderSkills();
  renderSkillDetailHeader(skill);
  renderSkillEditor();
  if (skillFileTree) {
    skillFileTree.textContent = t("common.loading");
  }
  const currentVersion = ++viewState.detailVersion;
  try {
    const payload = await loadSkillFiles(skill.name);
    if (currentVersion !== viewState.detailVersion) {
      return;
    }
    viewState.files = Array.isArray(payload.entries) ? payload.entries : [];
    viewState.root = payload.root || "";
    renderSkillDetailHeader(skill);
    renderSkillFileTree();
    const defaultFile = resolveDefaultSkillFile(viewState.files);
    if (defaultFile) {
      void selectSkillFile(defaultFile);
    }
  } catch (error) {
    if (currentVersion !== viewState.detailVersion) {
      return;
    }
    if (skillFileTree) {
      skillFileTree.textContent = t("skills.files.loadFailed", { message: error.message });
    }
  }
};

const selectSkillFile = async (filePath) => {
  const skill = getActiveSkill();
  if (!skill) {
    notify(t("skills.file.selectSkillRequired"), "warn");
    return;
  }
  const normalized = String(filePath || "");
  if (!normalized) {
    notify(t("skills.file.selectRequired"), "warn");
    return;
  }
  viewState.activeFile = normalized;
  viewState.fileContent = "";
  renderSkillFileTree();
  if (skillEditorPath) {
    skillEditorPath.textContent = normalized;
  }
  showSkillEditorMessage(t("common.loading"));
  const currentVersion = ++viewState.fileVersion;
  try {
    const content = await loadSkillFileContent(skill.name, normalized);
    if (currentVersion !== viewState.fileVersion) {
      return;
    }
    viewState.fileContent = content;
    renderSkillEditor();
  } catch (error) {
    if (currentVersion !== viewState.fileVersion) {
      return;
    }
    showSkillEditorMessage(t("skills.file.readFailed", { message: error.message }));
    notify(t("skills.file.readFailed", { message: error.message }), "error");
  }
};

const renderSkills = () => {
  if (!skillsList) {
    return;
  }
  skillsList.textContent = "";
  if (!state.skills.skills.length) {
    skillsList.textContent = t("skills.list.empty");
    return;
  }
  state.skills.skills.forEach((skill, index) => {
    const item = document.createElement("div");
    item.className = "skill-item";
    if (index === viewState.selectedIndex) {
      item.classList.add("is-active");
    }
    const checkbox = document.createElement("input");
    checkbox.type = "checkbox";
    checkbox.checked = Boolean(skill.enabled);
    checkbox.addEventListener("change", (event) => {
      skill.enabled = event.target.checked;
      const actionMessage = skill.enabled
        ? t("skills.enabled", { name: skill.name })
        : t("skills.disabled", { name: skill.name });
      saveSkills()
        .then(() => {
          appendLog(actionMessage);
          notify(actionMessage, "success");
        })
        .catch((error) => {
          console.error(t("skills.saveFailed", { message: error.message }), error);
          notify(t("skills.saveFailed", { message: error.message }), "error");
        });
    });
    const label = document.createElement("label");
    const title = document.createElement("strong");
    title.textContent = skill.name || "";
    const meta = document.createElement("span");
    meta.className = "muted";
    const metaParts = [];
    if (skill.description) {
      metaParts.push(skill.description);
    }
    if (skill.path) {
      metaParts.push(skill.path);
    }
    meta.textContent = metaParts.join(" · ");
    label.append(title, meta);
    const deleteButton = document.createElement("button");
    deleteButton.type = "button";
    deleteButton.className = "danger btn-with-icon btn-compact skill-delete-btn";
    deleteButton.innerHTML = '<i class="fa-solid fa-trash"></i>';
    const deletable = isSkillDeletable(skill);
    deleteButton.disabled = !deletable;
    deleteButton.title = deletable
      ? t("skills.delete.title")
      : t("skills.delete.restricted");
    deleteButton.addEventListener("click", (event) => {
      event.stopPropagation();
      if (!deletable) {
        notify(t("skills.delete.restricted"), "warn");
        return;
      }
      deleteSkill(skill)
        .then((deletedName) => {
          if (!deletedName) {
            return;
          }
          appendLog(t("skills.deleted", { name: deletedName }));
          notify(t("skills.deleted", { name: deletedName }), "success");
        })
        .catch((error) => {
          console.error(t("skills.deleteFailedMessage", { message: error.message }), error);
          notify(t("skills.deleteFailedMessage", { message: error.message }), "error");
        });
    });
    item.addEventListener("click", (event) => {
      if (event.target === checkbox || deleteButton.contains(event.target)) {
        return;
      }
      selectSkill(skill, index);
    });
    item.appendChild(checkbox);
    item.appendChild(label);
    item.appendChild(deleteButton);
    skillsList.appendChild(item);
  });
};

const saveSkills = async () => {
  const wunderBase = getWunderBase();
  const endpoint = `${wunderBase}/admin/skills`;
  const enabled = state.skills.skills
    .filter((skill) => skill.enabled)
    .map((skill) => skill.name);
  const response = await fetch(endpoint, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
    },
    body: JSON.stringify({ enabled, paths: state.skills.paths }),
  });
  if (!response.ok) {
    throw new Error(t("common.requestFailed", { status: response.status }));
  }
  const result = await response.json();
  const activeName = getActiveSkill()?.name || "";
  state.skills.paths = Array.isArray(result.paths) ? result.paths : [];
  state.skills.skills = Array.isArray(result.skills) ? result.skills : [];
  viewState.selectedIndex = activeName
    ? state.skills.skills.findIndex((item) => item.name === activeName)
    : -1;
  renderSkills();
  syncPromptTools();
};

const deleteSkill = async (skill) => {
  const skillName = String(skill?.name || "").trim();
  if (!skillName) {
    throw new Error(t("skills.nameRequired"));
  }
  if (!window.confirm(t("skills.deleteConfirm", { name: skillName }))) {
    return null;
  }
  const wunderBase = getWunderBase();
  const endpoint = `${wunderBase}/admin/skills?name=${encodeURIComponent(skillName)}`;
  const response = await fetch(endpoint, { method: "DELETE" });
  if (!response.ok) {
    const detail = await extractErrorMessage(response);
    if (response.status === 404) {
      throw new Error(detail || t("skills.notFound"));
    }
    if (detail) {
      throw new Error(detail);
    }
    throw new Error(t("skills.deleteFailed", { status: response.status }));
  }
  await loadSkills();
  syncPromptTools();
  return skillName;
};

const uploadSkillZip = async (file) => {
  if (!file) {
    return;
  }
  const filename = file.name || "";
  const lowerName = filename.toLowerCase();
  if (!(lowerName.endsWith(".zip") || lowerName.endsWith(".skill"))) {
    throw new Error(t("skills.upload.zipOnly"));
  }
  const wunderBase = getWunderBase();
  const endpoint = `${wunderBase}/admin/skills/upload`;
  const form = new FormData();
  form.append("file", file, filename);
  const response = await fetch(endpoint, {
    method: "POST",
    body: form,
  });
  if (!response.ok) {
    throw new Error(t("skills.upload.failed", { message: response.status }));
  }
  await loadSkills();
  syncPromptTools();
};

const saveSkillFile = async () => {
  const skill = getActiveSkill();
  if (!skill) {
    notify(t("skills.file.selectSkillRequired"), "warn");
    return;
  }
  if (!viewState.activeFile) {
    notify(t("skills.file.selectRequired"), "warn");
    return;
  }
  try {
    const content = skillFileContent ? skillFileContent.value : viewState.fileContent;
    const result = await saveSkillFileContent(skill.name, viewState.activeFile, content);
    viewState.fileContent = content;
    appendLog(t("skills.file.saveSuccess"));
    notify(t("skills.file.saveSuccess"), "success");
    if (result?.reloaded) {
      await loadSkills();
    }
  } catch (error) {
    notify(t("skills.file.saveFailed", { message: error.message }), "error");
  }
};

// Pull skills list and render left sidebar.
export const loadSkills = async () => {
  const wunderBase = getWunderBase();
  const endpoint = `${wunderBase}/admin/skills`;
  const response = await fetch(endpoint);
  if (!response.ok) {
    throw new Error(t("common.requestFailed", { status: response.status }));
  }
  const result = await response.json();
  const previousName = getActiveSkill()?.name || "";
  state.skills.paths = Array.isArray(result.paths) ? result.paths : [];
  state.skills.skills = Array.isArray(result.skills) ? result.skills : [];
  viewState.selectedIndex = previousName
    ? state.skills.skills.findIndex((item) => item.name === previousName)
    : -1;
  renderSkills();
  if (viewState.selectedIndex < 0) {
    clearSkillDetail();
  } else {
    renderSkillDetailHeader(getActiveSkill());
    renderSkillFileTree();
    renderSkillEditor();
  }
};

// Initialize skill panel interactions.
export const initSkillsPanel = () => {
  addSkillBtn?.addEventListener("click", () => {
    if (!skillUploadInput) {
      return;
    }
    skillUploadInput.value = "";
    skillUploadInput.click();
  });
  skillUploadInput?.addEventListener("change", async () => {
    const file = skillUploadInput.files?.[0];
    if (!file) {
      return;
    }
    try {
      await uploadSkillZip(file);
      appendLog(t("skills.upload.success"));
      notify(t("skills.upload.success"), "success");
    } catch (error) {
      appendLog(t("skills.upload.failed", { message: error.message }));
      notify(t("skills.upload.failed", { message: error.message }), "error");
    }
  });
  refreshSkillsBtn?.addEventListener("click", async () => {
    try {
      await loadSkills();
      notify(t("skills.refresh.success"), "success");
    } catch (error) {
      if (skillsList) {
        skillsList.textContent = t("common.loadFailedWithMessage", {
          message: error.message,
        });
      }
      notify(t("skills.refresh.failed", { message: error.message }), "error");
    }
  });
  skillFileSaveBtn?.addEventListener("click", saveSkillFile);
  skillFileContent?.addEventListener("input", () => {
    viewState.fileContent = skillFileContent.value;
    scheduleSkillEditorHighlight();
  });
  skillFileContent?.addEventListener("scroll", syncSkillEditorScroll);
  renderSkillDetailHeader(getActiveSkill());
  renderSkillFileTree();
  renderSkillEditor();
};
