import { fetchAdminHiveTeamRuns } from "./api.js";
import { t } from "./i18n.js?v=20260118-07";
import { appendLog } from "./log.js?v=20260108-02";

const byId = (id) => document.getElementById(id);
const SWARM_HIVES_FILTER_KEY = "wunder.admin.swarm.hives.hive_id";

let lastItems = [];
let languageListenerBound = false;

const escapeHtml = (value) =>
  String(value ?? "")
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;");

const readStorage = (key, fallback = "") => {
  try {
    return String(window.localStorage.getItem(key) || fallback);
  } catch (error) {
    return fallback;
  }
};

const writeStorage = (key, value) => {
  try {
    window.localStorage.setItem(key, String(value || ""));
  } catch (error) {
    // ignore storage failures
  }
};

const groupHiveSummary = (items = []) => {
  const grouped = new Map();
  items.forEach((item) => {
    const hiveId = String(item?.hive_id || "default");
    if (!grouped.has(hiveId)) {
      grouped.set(hiveId, { hive_id: hiveId, total: 0, running: 0, failed: 0 });
    }
    const row = grouped.get(hiveId);
    row.total += 1;
    const status = String(item?.status || "").toLowerCase();
    if (["queued", "running", "merging"].includes(status)) {
      row.running += 1;
    }
    if (status === "failed" || status === "timeout") {
      row.failed += 1;
    }
  });
  return Array.from(grouped.values());
};

const renderHiveSummary = (items = []) => {
  const container = byId("swarmHivesSummary");
  if (!container) {
    return;
  }
  const grouped = groupHiveSummary(items);
  if (!grouped.length) {
    container.innerHTML = '<div class="swarm-empty">' + escapeHtml(t("swarm.hives.empty")) + "</div>";
    return;
  }
  container.innerHTML = grouped
    .map((item) => {
      const html = [
        '<div class="swarm-hive-item">',
        "<strong>" + escapeHtml(item.hive_id) + "</strong>",
        "<span>" + escapeHtml(t("swarm.hives.metric.total", { count: item.total })) + "</span>",
        "<span>" + escapeHtml(t("swarm.hives.metric.running", { count: item.running })) + "</span>",
        "<span>" + escapeHtml(t("swarm.hives.metric.failed", { count: item.failed })) + "</span>",
        "</div>",
      ];
      return html.join("");
    })
    .join("");
};

const bindLanguageChange = () => {
  if (languageListenerBound || typeof window === "undefined") {
    return;
  }
  languageListenerBound = true;
  window.addEventListener("wunder:language-changed", () => {
    renderHiveSummary(lastItems);
  });
};

const bindHiveIdFilter = () => {
  const hiveIdInput = byId("swarmHivesHiveId");
  if (!hiveIdInput) {
    return;
  }
  hiveIdInput.value = readStorage(SWARM_HIVES_FILTER_KEY, hiveIdInput.value || "");
  const persistFilter = () => {
    writeStorage(SWARM_HIVES_FILTER_KEY, String(hiveIdInput.value || "").trim());
  };
  hiveIdInput.addEventListener("change", () => {
    persistFilter();
    loadSwarmHives();
  });
  hiveIdInput.addEventListener("keydown", (event) => {
    if (event.key === "Enter") {
      persistFilter();
      loadSwarmHives();
    }
  });
};

export const loadSwarmHives = async () => {
  const userId = String(byId("userId")?.value || "").trim();
  const hiveId = String(byId("swarmHivesHiveId")?.value || "").trim();
  writeStorage(SWARM_HIVES_FILTER_KEY, hiveId);
  if (!userId || !hiveId) {
    lastItems = [];
    renderHiveSummary(lastItems);
    return;
  }
  try {
    const payload = await fetchAdminHiveTeamRuns(hiveId, { user_id: userId, limit: 200 });
    lastItems = Array.isArray(payload?.data?.items) ? payload.data.items : [];
    renderHiveSummary(lastItems);
  } catch (error) {
    appendLog(t("swarm.hives.log.loadFailed", { message: error.message || "-" }));
    lastItems = [];
    renderHiveSummary(lastItems);
  }
};

export const initSwarmHivesPanel = () => {
  const refreshBtn = byId("swarmHivesRefreshBtn");
  if (refreshBtn) {
    refreshBtn.addEventListener("click", () => {
      loadSwarmHives();
    });
  }
  bindHiveIdFilter();
  bindLanguageChange();
};
