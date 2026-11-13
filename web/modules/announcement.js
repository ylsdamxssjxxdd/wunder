// 系统消息：管理员查看、编辑并推送面向全体用户的公告。
import { elements } from "./elements.js?v=20261113-01";
import { getWunderBase } from "./api.js";
import { notify } from "./notify.js";
import { t } from "./i18n.js?v=20261113-01";
import { formatTimestamp } from "./utils.js";

let current = null;
let loading = false;
let saving = false;
let initialized = false;

const refs = () => ({
  title: elements.announcementTitle,
  content: elements.announcementContent,
  enabled: elements.announcementEnabled,
  badge: elements.announcementStatusBadge,
  status: elements.announcementStatus,
  saveBtn: elements.announcementSaveBtn,
  refreshBtn: elements.announcementRefreshBtn,
});

const requestFailed = async (response) => {
  let detail = "";
  try {
    const body = await response.json();
    detail = body?.error?.message || body?.detail?.message || body?.message || "";
  } catch {
    detail = "";
  }
  return new Error(t("announcement.toast.requestFailed", { status: response.status }) + (detail ? ` ${detail}` : ""));
};

const isPublished = (record) =>
  Boolean(record?.enabled) && Boolean(String(record?.content || "").trim());

const render = () => {
  const { title, content, enabled, badge, status } = refs();
  const record = current;
  if (title) {
    title.value = record?.title || "";
  }
  if (content) {
    content.value = record?.content || "";
  }
  if (enabled) {
    enabled.checked = isPublished(record);
  }
  const hasContent = Boolean(String(record?.content || "").trim());
  const published = isPublished(record);
  const time = formatTimestamp(record?.updated_at);
  if (badge) {
    badge.textContent = published
      ? t("announcement.status.publishedBadge")
      : hasContent
        ? t("announcement.status.disabledBadge")
        : t("announcement.status.empty");
  }
  if (status) {
    if (!hasContent) {
      status.textContent = t("announcement.status.emptyHint");
    } else {
      status.textContent = t(
        published ? "announcement.status.published" : "announcement.status.disabled",
        { time }
      );
    }
  }
};

const loadAnnouncement = async ({ silent = false } = {}) => {
  if (loading) {
    return;
  }
  loading = true;
  const { badge, status } = refs();
  if (!silent && badge) {
    badge.textContent = t("common.loading");
  }
  if (!silent && status) {
    status.textContent = t("common.loading");
  }
  try {
    const response = await fetch(`${getWunderBase()}/admin/announcement`);
    if (!response.ok) {
      throw await requestFailed(response);
    }
    const payload = await response.json();
    current = payload?.announcement || null;
    render();
  } finally {
    loading = false;
  }
};

const saveAnnouncement = async () => {
  if (saving) {
    return;
  }
  const { title, content, enabled, saveBtn } = refs();
  saving = true;
  if (saveBtn) {
    saveBtn.disabled = true;
  }
  try {
    const response = await fetch(`${getWunderBase()}/admin/announcement`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        title: title?.value || "",
        content: content?.value || "",
        enabled: Boolean(enabled?.checked),
      }),
    });
    if (!response.ok) {
      throw await requestFailed(response);
    }
    const payload = await response.json();
    current = payload?.announcement || null;
    render();
    notify(t("announcement.toast.saved"), "success");
  } finally {
    saving = false;
    if (saveBtn) {
      saveBtn.disabled = false;
    }
  }
};

export const initAnnouncementPanel = () => {
  const { saveBtn, refreshBtn } = refs();
  if (initialized) {
    return;
  }
  initialized = true;
  refreshBtn?.addEventListener("click", () => {
    loadAnnouncement().catch((error) =>
      notify(t("announcement.toast.loadFailed", { message: error.message }), "error")
    );
  });
  saveBtn?.addEventListener("click", () => {
    saveAnnouncement().catch((error) =>
      notify(t("announcement.toast.saveFailed", { message: error.message }), "error")
    );
  });
  window.addEventListener("wunder:language-changed", render);
  render();
};

export const loadAnnouncementPanel = async () => {
  try {
    await loadAnnouncement();
  } catch (error) {
    notify(t("announcement.toast.loadFailed", { message: error.message }), "error");
  }
};
