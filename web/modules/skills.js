import { state } from "./state.js";
import { getWunderBase } from "./api.js";
import { appendLog } from "./log.js?v=20260108-02";
import { syncPromptTools } from "./tools.js?v=20260214-01";
import { notify } from "./notify.js";
import { t } from "./i18n.js?v=20260518-01";
import { resolveApiErrorMessage } from "./api-error.js";
import { getAllOrgUnitItems } from "./org-units.js?v=20260518-01";
import {
  initAdminSkillWorkspace,
  loadAdminSkillWorkspace,
  refreshAdminSkillWorkspaceHeader,
  resetAdminSkillWorkspace,
} from "./admin-skill-workspace.js?v=20260604-01";

const skillsList = document.getElementById("skillsList");
const refreshSkillsBtn = document.getElementById("refreshSkillsBtn");
const addSkillBtn = document.getElementById("addSkillBtn");
const exportSkillBtn = document.getElementById("exportSkillBtn");
const skillVisibilityBtn = document.getElementById("skillVisibilityBtn");
const skillUploadInput = document.getElementById("skillUploadInput");
const SUPPORTED_SKILL_ARCHIVE_SUFFIXES = [
  ".zip",
  ".skill",
  ".rar",
  ".7z",
  ".tar",
  ".tgz",
  ".tar.gz",
  ".tbz2",
  ".tar.bz2",
  ".txz",
  ".tar.xz",
];
const skillDetailTitle = document.getElementById("skillDetailTitle");
const skillDetailMeta = document.getElementById("skillDetailMeta");
const skillFileTree = document.getElementById("skillFileTree");
const skillFileSearchInput = document.getElementById("skillFileSearchInput");

const viewState = {
  selectedIndex: -1,
  fileSearch: "",
  detailVersion: 0,
};

const normalizeSkillDisplayPath = (rawPath) => {
  let normalized = String(rawPath || "").trim();
  if (!normalized) {
    return "";
  }
  normalized = normalized.replace(/^\\\\\?\\UNC\\/i, "\\\\");
  normalized = normalized.replace(/^\\\\\?\\/, "");
  normalized = normalized.replace(/^\/\/\?\//, "");
  normalized = normalized.replace(/^\/\/\.\//, "");
  normalized = normalized.replace(/\\/g, "/");
  if (/^\/[A-Za-z]:\//.test(normalized)) {
    normalized = normalized.slice(1);
  }
  return normalized;
};

const resetSkillFileSearch = () => {
  viewState.fileSearch = "";
  if (skillFileSearchInput) {
    skillFileSearchInput.value = "";
  }
};

const getActiveSkill = () =>
  Number.isInteger(viewState.selectedIndex)
    ? state.skills.skills[viewState.selectedIndex] || null
    : null;

const resolveSkillSource = (skill) => {
  if (!skill) {
    return "custom";
  }
  if (skill.source === "builtin" || skill.builtin === true || skill.readonly === true) {
    return "builtin";
  }
  if (skill.source === "external") {
    return "external";
  }
  return "custom";
};

const isSkillEditable = (skill) => {
  if (!skill) {
    return false;
  }
  if (typeof skill.editable === "boolean") {
    return skill.editable;
  }
  return true;
};

const buildSkillSourceLabel = (skill) => {
  switch (resolveSkillSource(skill)) {
    case "builtin":
      return t("skills.source.builtin");
    case "external":
      return t("skills.source.external");
    default:
      return t("skills.source.custom");
  }
};

const extractErrorMessage = async (response) => resolveApiErrorMessage(response, "");

const buildUploadedSkillSuccessMessage = (originalName, result, fallbackKey) => {
  const normalizedOriginal = String(originalName || "").trim();
  const finalNames = Array.isArray(result?.final_names)
    ? result.final_names.map((item) => String(item || "").trim()).filter(Boolean)
    : [];
  if (
    finalNames.length > 0 &&
    (finalNames.length !== 1 || finalNames[0] !== normalizedOriginal)
  ) {
    return t("skills.upload.renamed", { names: finalNames.join(", ") });
  }
  return t(fallbackKey);
};

const buildVisibilitySummary = () => {
  const rules = Array.isArray(state.skills.visibilityRules) ? state.skills.visibilityRules : [];
  if (!rules.length) {
    return t("visibility.all");
  }
  const unitMap = new Map(getAllOrgUnitItems().map((item) => [item.unit_id, item.path_name || item.name || item.unit_id]));
  return rules
    .map((rule) => `${String(rule.name || "").trim()}: ${(rule.visible_unit_ids || []).map((unitId) => unitMap.get(unitId) || unitId).join(", ") || t("visibility.all")}`)
    .join(" | ");
};

const saveSkillVisibilityRules = async () => {
  const wunderBase = getWunderBase();
  const endpoint = `${wunderBase}/admin/tools`;
  const enabled = Array.isArray(state.builtin?.tools)
    ? state.builtin.tools.filter((tool) => tool.enabled).map((tool) => tool.name)
    : [];
  const response = await fetch(endpoint, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
    },
    body: JSON.stringify({
      enabled,
      visibility_rules: state.skills.visibilityRules,
    }),
  });
  if (!response.ok) {
    throw new Error(t("common.requestFailed", { status: response.status }));
  }
  return response.json();
};

const downloadBlob = (blob, filename) => {
  const url = URL.createObjectURL(blob);
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = filename;
  anchor.rel = "noreferrer";
  document.body.appendChild(anchor);
  anchor.click();
  anchor.remove();
  URL.revokeObjectURL(url);
};

const downloadCurrentSkill = async () => {
  const skill = getActiveSkill();
  if (!skill) {
    notify(t("skills.file.selectSkillRequired"), "warn");
    return;
  }
  const skillName = String(skill.name || "").trim();
  if (!skillName) {
    notify(t("skills.file.selectRequired"), "warn");
    return;
  }
  const wunderBase = getWunderBase();
  const endpoint = `${wunderBase}/admin/skills/export?name=${encodeURIComponent(skillName)}`;
  const response = await fetch(endpoint);
  if (!response.ok) {
    const detail = await extractErrorMessage(response);
    throw new Error(detail || t("common.requestFailed", { status: response.status }));
  }
  const blob = await response.blob();
  const filename = `${skillName}.zip`;
  downloadBlob(blob, filename);
};

const renderSkillDetailHeader = (skill) => {
  if (!skillDetailTitle || !skillDetailMeta) {
    return;
  }
  if (!skill) {
    skillDetailTitle.textContent = t("skills.detail.unselected");
    skillDetailMeta.textContent = "";
    skillDetailMeta.title = "";
    return;
  }
  skillDetailTitle.textContent = skill.name || t("skills.detail.title");
  const displayPath = normalizeSkillDisplayPath(skill.path);
  const metaParts = [];
  if (!isSkillEditable(skill)) {
    metaParts.push(t("skills.readonly.hint"));
  }
  if (displayPath) {
    metaParts.push(displayPath);
  }
  skillDetailMeta.textContent = metaParts.join(" · ");
  skillDetailMeta.title = displayPath;
};

const clearSkillDetail = () => {
  viewState.selectedIndex = -1;
  resetSkillFileSearch();
  resetAdminSkillWorkspace();
};

const selectSkill = async (skill, index) => {
  if (!skill) {
    clearSkillDetail();
    renderSkills();
    return;
  }
  viewState.selectedIndex = index;
  resetSkillFileSearch();
  renderSkills();
  renderSkillDetailHeader(skill);
  const currentVersion = ++viewState.detailVersion;
  try {
    await loadAdminSkillWorkspace(skill, { resetSearch: true });
    if (currentVersion !== viewState.detailVersion) {
      return;
    }
    refreshAdminSkillWorkspaceHeader();
  } catch (error) {
    if (currentVersion !== viewState.detailVersion) {
      return;
    }
    if (skillFileTree) {
      skillFileTree.textContent = t("skills.files.loadFailed", { message: error.message });
    }
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
    const titleRow = document.createElement("div");
    titleRow.className = "skill-title-row";
    const title = document.createElement("strong");
    title.textContent = skill.name || "";
    const sourceTag = document.createElement("span");
    sourceTag.className = `skill-source-tag is-${resolveSkillSource(skill)}`;
    sourceTag.textContent = buildSkillSourceLabel(skill);
    const meta = document.createElement("span");
    meta.className = "muted";
    const metaParts = [];
    if (skill.description) {
      metaParts.push(skill.description);
    }
    if (skill.path) {
      metaParts.push(normalizeSkillDisplayPath(skill.path));
    }
    meta.textContent = metaParts.join(" · ");
    titleRow.append(title, sourceTag);
    label.append(titleRow, meta);
    const deleteButton = document.createElement("button");
    deleteButton.type = "button";
    deleteButton.className = "danger btn-with-icon btn-compact skill-delete-btn";
    deleteButton.innerHTML = '<i class="fa-solid fa-trash"></i>';
    const deletable = isSkillEditable(skill);
    deleteButton.disabled = !deletable;
    deleteButton.title = deletable
      ? t("skills.delete.title")
      : t("skills.readonly.hint");
    deleteButton.addEventListener("click", (event) => {
      event.stopPropagation();
      if (!deletable) {
        notify(t("skills.readonly.hint"), "warn");
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
  if (!SUPPORTED_SKILL_ARCHIVE_SUFFIXES.some((suffix) => lowerName.endsWith(suffix))) {
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
  const result = await response.json();
  await loadSkills();
  syncPromptTools();
  return result;
};

// Pull skills list and render left sidebar.
export const loadSkills = async (options = {}) => {
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
  state.skills.visibilityRules = Array.isArray(result?.visibility?.rules) ? result.visibility.rules : [];
  viewState.selectedIndex = previousName
    ? state.skills.skills.findIndex((item) => item.name === previousName)
    : -1;
  renderSkills();
  if (viewState.selectedIndex < 0) {
    clearSkillDetail();
  } else {
    renderSkillDetailHeader(getActiveSkill());
    if (!options.skipWorkspaceReload) {
      await loadAdminSkillWorkspace(getActiveSkill(), { preservePath: true });
    } else {
      refreshAdminSkillWorkspaceHeader();
    }
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
  exportSkillBtn?.addEventListener("click", async () => {
    try {
      await downloadCurrentSkill();
      notify(t("skills.export.success"), "success");
    } catch (error) {
      notify(t("skills.export.failed", { message: error.message }), "error");
    }
  });
  skillUploadInput?.addEventListener("change", async () => {
    const file = skillUploadInput.files?.[0];
    if (!file) {
      return;
    }
    try {
      const result = await uploadSkillZip(file);
      const message = buildUploadedSkillSuccessMessage(file.name, result, "skills.upload.success");
      appendLog(message);
      notify(message, "success");
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
  skillVisibilityBtn?.addEventListener("click", () => {
    const current = JSON.stringify(state.skills.visibilityRules || [], null, 2);
    const raw = window.prompt('Rules JSON: [{"name":"tool_name","visible_unit_ids":["unit_1"]}]', current);
    if (raw === null) {
      return;
    }
    try {
      const parsed = JSON.parse(raw);
      state.skills.visibilityRules = Array.isArray(parsed) ? parsed : [];
      saveSkillVisibilityRules()
        .then(() => {
          notify(buildVisibilitySummary(), "success");
        })
        .catch((error) => {
          notify(t("skills.saveFailed", { message: error.message }), "error");
        });
    } catch (error) {
      notify(t("skills.saveFailed", { message: error.message }), "error");
    }
  });
  initAdminSkillWorkspace({
    isSkillEditable,
    onSkillMetadataChanged: () => loadSkills({ skipWorkspaceReload: true }),
  });
  refreshAdminSkillWorkspaceHeader();
};
