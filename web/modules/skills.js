import { elements } from "./elements.js?v=20260105-02";
import { state } from "./state.js";
import { getWunderBase } from "./api.js";
import { appendLog } from "./log.js?v=20260108-02";
import { syncPromptTools } from "./tools.js?v=20251227-13";
import { notify } from "./notify.js";
import { t } from "./i18n.js?v=20260110-01";

// 拉取技能清单与启用状态
export const loadSkills = async () => {
  const wunderBase = getWunderBase();
  const endpoint = `${wunderBase}/admin/skills`;
  const response = await fetch(endpoint);
  if (!response.ok) {
    throw new Error(t("common.requestFailed", { status: response.status }));
  }
  const result = await response.json();
  state.skills.paths = Array.isArray(result.paths) ? result.paths : [];
  state.skills.skills = Array.isArray(result.skills) ? result.skills : [];
  renderSkills();
};

// 拉取指定技能的 SKILL.md 内容，供详情弹窗完整展示
const loadSkillContent = async (skillName) => {
  if (!skillName) {
    throw new Error(t("skills.nameRequired"));
  }
  const wunderBase = getWunderBase();
  const endpoint = `${wunderBase}/admin/skills/content?name=${encodeURIComponent(skillName)}`;
  const response = await fetch(endpoint);
  if (!response.ok) {
    throw new Error(t("common.requestFailed", { status: response.status }));
  }
  const result = await response.json();
  return String(result.content || "");
};

const normalizeSkillPath = (rawPath) => String(rawPath || "").replace(/\\/g, "/");

const isSkillDeletable = (skill) => {
  const normalized = normalizeSkillPath(skill?.path).toLowerCase();
  return /(^|\/)eva_skills(\/|$)/.test(normalized);
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
    let detail = "";
    try {
      const payload = await response.json();
      detail = payload?.detail?.message || payload?.message || "";
    } catch (error) {
      detail = "";
    }
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

// 打开技能详情弹窗，并按当前技能加载完整内容
const openSkillDetailModal = async (skill) => {
  const currentVersion = ++state.skills.detailVersion;
  elements.skillModalTitle.textContent = skill.name || t("skills.detail.title");
  elements.skillModalMeta.textContent = skill.path || "";
  elements.skillModalContent.textContent = t("common.loading");
  elements.skillModal.classList.add("active");
  try {
    const content = await loadSkillContent(skill.name);
    if (currentVersion !== state.skills.detailVersion) {
      return;
    }
    elements.skillModalContent.textContent = content || t("skills.detail.empty");
  } catch (error) {
    if (currentVersion !== state.skills.detailVersion) {
      return;
    }
    elements.skillModalContent.textContent = t("common.loadFailedWithMessage", {
      message: error.message,
    });
  }
};

// 关闭技能详情弹窗，同时保留上次内容供下次复用
const closeSkillDetailModal = () => {
  elements.skillModal.classList.remove("active");
};

// 渲染技能勾选列表
const renderSkills = () => {
  elements.skillsList.textContent = "";
  if (!state.skills.skills.length) {
    elements.skillsList.textContent = t("skills.list.empty");
    return;
  }
  state.skills.skills.forEach((skill) => {
    const item = document.createElement("div");
    item.className = "skill-item";
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
    label.innerHTML = `<strong>${skill.name}</strong><span class="muted">${skill.description} · ${skill.path}</span>`;
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
      openSkillDetailModal(skill);
    });
    item.appendChild(checkbox);
    item.appendChild(label);
    item.appendChild(deleteButton);
    elements.skillsList.appendChild(item);
  });
};

// 保存技能启用状态与目录配置
const saveSkills = async () => {
  const wunderBase = getWunderBase();
  const endpoint = `${wunderBase}/admin/skills`;
  const enabled = state.skills.skills.filter((skill) => skill.enabled).map((skill) => skill.name);
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
  state.skills.paths = Array.isArray(result.paths) ? result.paths : [];
  state.skills.skills = Array.isArray(result.skills) ? result.skills : [];
  renderSkills();
  syncPromptTools();
};

// 上传技能压缩包并刷新技能列表
const uploadSkillZip = async (file) => {
  if (!file) {
    return;
  }
  const filename = file.name || "";
  if (!filename.toLowerCase().endsWith(".zip")) {
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

// 初始化技能面板交互
export const initSkillsPanel = () => {
  // 技能详情弹窗：支持点击关闭按钮或遮罩层关闭
  elements.skillModalClose.addEventListener("click", closeSkillDetailModal);
  elements.skillModalCloseBtn.addEventListener("click", closeSkillDetailModal);
  elements.skillModal.addEventListener("click", (event) => {
    if (event.target === elements.skillModal) {
      closeSkillDetailModal();
    }
  });
  elements.addSkillBtn.addEventListener("click", () => {
    elements.skillUploadInput.value = "";
    elements.skillUploadInput.click();
  });
  elements.skillUploadInput.addEventListener("change", async () => {
    const file = elements.skillUploadInput.files?.[0];
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
  elements.refreshSkillsBtn.addEventListener("click", async () => {
    try {
      await loadSkills();
      notify(t("skills.refresh.success"), "success");
    } catch (error) {
      elements.skillsList.textContent = t("common.loadFailedWithMessage", {
        message: error.message,
      });
      notify(t("skills.refresh.failed", { message: error.message }), "error");
    }
  });
};




