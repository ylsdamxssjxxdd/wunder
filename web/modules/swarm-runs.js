import { fetchAdminTeamRunDetail, fetchAdminTeamRuns } from "./api.js";
import { t } from "./i18n.js?v=20260118-07";
import { appendLog } from "./log.js?v=20260108-02";

const byId = (id) => document.getElementById(id);
const SWARM_RUNS_HIVE_FILTER_KEY = "wunder.admin.swarm.runs.hive_filter";

let lastRuns = [];
let lastDetailPayload = null;
let selectedRunId = "";
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

const normalizeTaskTotal = (value) => {
  const parsed = Number.parseInt(value, 10);
  return Number.isFinite(parsed) ? parsed : 0;
};

const renderRuns = (items = []) => {
  const list = byId("swarmRunsList");
  if (!list) {
    return;
  }
  if (!Array.isArray(items) || items.length <= 0) {
    list.innerHTML = '<div class="swarm-empty">' + escapeHtml(t("swarm.runs.empty")) + "</div>";
    return;
  }
  list.innerHTML = items
    .map((item) => {
      const teamRunId = String(item?.team_run_id || "").trim();
      const activeClass = teamRunId && teamRunId === selectedRunId ? " is-active" : "";
      const html = [
        '<button class="swarm-run-item' + activeClass + '" type="button" data-run-id="' + escapeHtml(teamRunId) + '">',
        "<span>" + escapeHtml(item?.status || "-") + "</span>",
        "<span>" + escapeHtml(item?.hive_id || "-") + "</span>",
        "<span>" + escapeHtml(t("swarm.runs.tasks", { count: normalizeTaskTotal(item?.task_total) })) + "</span>",
        "</button>",
      ];
      return html.join("");
    })
    .join("");
};

const renderRunDetail = (payload) => {
  const detail = byId("swarmRunDetail");
  if (!detail) {
    return;
  }
  const run = payload?.data?.run || null;
  const tasks = Array.isArray(payload?.data?.tasks) ? payload.data.tasks : [];
  if (!run) {
    detail.innerHTML = '<div class="swarm-empty">' + escapeHtml(t("swarm.runs.detail.empty")) + "</div>";
    return;
  }
  const taskRows = tasks.length
    ? tasks
        .map(
          (task) =>
            '<div class="swarm-task-row"><span>' +
            escapeHtml(task?.agent_id || "-") +
            "</span><span>" +
            escapeHtml(task?.status || "-") +
            "</span></div>"
        )
        .join("")
    : '<div class="swarm-empty">' + escapeHtml(t("swarm.runs.tasks.empty")) + "</div>";
  const html = [
    '<div class="swarm-detail-head">',
    "<strong>" + escapeHtml(run?.team_run_id || "-") + "</strong>",
    "<span>" + escapeHtml(run?.status || "-") + "</span>",
    "</div>",
    '<div class="swarm-detail-meta">' +
      escapeHtml(t("swarm.runs.meta.hive", { hive: run?.hive_id || "-" })) +
      "</div>",
    '<div class="swarm-detail-tasks">' + taskRows + "</div>",
  ];
  detail.innerHTML = html.join("");
};

const syncLanguage = () => {
  renderRuns(lastRuns);
  renderRunDetail(lastDetailPayload);
};

const bindLanguageChange = () => {
  if (languageListenerBound || typeof window === "undefined") {
    return;
  }
  languageListenerBound = true;
  window.addEventListener("wunder:language-changed", () => {
    syncLanguage();
  });
};

const bindRunClicks = () => {
  const list = byId("swarmRunsList");
  if (!list) {
    return;
  }
  list.addEventListener("click", async (event) => {
    const target = event.target.closest("[data-run-id]");
    if (!target) {
      return;
    }
    const teamRunId = String(target.getAttribute("data-run-id") || "").trim();
    if (!teamRunId) {
      return;
    }
    selectedRunId = teamRunId;
    renderRuns(lastRuns);
    try {
      const payload = await fetchAdminTeamRunDetail(teamRunId);
      lastDetailPayload = payload;
      renderRunDetail(payload);
    } catch (error) {
      appendLog(t("swarm.runs.log.loadDetailFailed", { message: error.message || "-" }));
    }
  });
};

const bindHiveFilter = () => {
  const hiveFilterInput = byId("swarmRunsHiveFilter");
  if (!hiveFilterInput) {
    return;
  }
  hiveFilterInput.value = readStorage(SWARM_RUNS_HIVE_FILTER_KEY, hiveFilterInput.value || "");
  const persistFilter = () => {
    writeStorage(SWARM_RUNS_HIVE_FILTER_KEY, String(hiveFilterInput.value || "").trim());
  };
  hiveFilterInput.addEventListener("change", () => {
    persistFilter();
    loadSwarmRuns();
  });
  hiveFilterInput.addEventListener("keydown", (event) => {
    if (event.key === "Enter") {
      persistFilter();
      loadSwarmRuns();
    }
  });
};

export const loadSwarmRuns = async () => {
  const userId = String(byId("userId")?.value || "").trim();
  const hiveId = String(byId("swarmRunsHiveFilter")?.value || "").trim();
  writeStorage(SWARM_RUNS_HIVE_FILTER_KEY, hiveId);
  if (!userId) {
    lastRuns = [];
    lastDetailPayload = null;
    selectedRunId = "";
    renderRuns(lastRuns);
    renderRunDetail(lastDetailPayload);
    return;
  }
  try {
    const payload = await fetchAdminTeamRuns({ user_id: userId, hive_id: hiveId, limit: 200 });
    lastRuns = Array.isArray(payload?.data?.items) ? payload.data.items : [];
    if (!lastRuns.some((item) => String(item?.team_run_id || "").trim() === selectedRunId)) {
      selectedRunId = "";
      lastDetailPayload = null;
    }
    renderRuns(lastRuns);
    renderRunDetail(lastDetailPayload);
  } catch (error) {
    appendLog(t("swarm.runs.log.loadFailed", { message: error.message || "-" }));
    lastRuns = [];
    lastDetailPayload = null;
    selectedRunId = "";
    renderRuns(lastRuns);
    renderRunDetail(lastDetailPayload);
  }
};

export const initSwarmRunsPanel = () => {
  const refreshBtn = byId("swarmRunsRefreshBtn");
  if (refreshBtn) {
    refreshBtn.addEventListener("click", () => {
      loadSwarmRuns();
    });
  }
  bindHiveFilter();
  bindRunClicks();
  bindLanguageChange();
};
