import { normalizeApiBase } from "./utils.js?v=20251229-02";

// 获取当前规范化后的 /wunder 根路径
export const getWunderBase = () => {
  if (typeof window !== "undefined" && window.location?.origin) {
    return normalizeApiBase(`${window.location.origin}/wunder`);
  }
  return normalizeApiBase("/wunder");
};






const toQueryString = (params = {}) => {
  const search = new URLSearchParams();
  Object.entries(params).forEach(([key, value]) => {
    if (value === undefined || value === null || value === "") {
      return;
    }
    search.set(key, String(value));
  });
  const encoded = search.toString();
  return encoded ? `?${encoded}` : "";
};

const parseJsonResponse = async (response) => {
  const payload = await response.json().catch(() => ({}));
  if (!response.ok) {
    const message = payload?.error?.message || payload?.detail?.message || `HTTP ${response.status}`;
    throw new Error(message);
  }
  return payload;
};

export const fetchAdminTeamRuns = async (params = {}) => {
  const base = getWunderBase();
  const response = await fetch(`${base}/admin/team_runs${toQueryString(params)}`);
  return parseJsonResponse(response);
};

export const fetchAdminTeamRunDetail = async (teamRunId) => {
  const base = getWunderBase();
  const response = await fetch(`${base}/admin/team_runs/${encodeURIComponent(teamRunId)}`);
  return parseJsonResponse(response);
};

export const fetchAdminHiveTeamRuns = async (hiveId, params = {}) => {
  const base = getWunderBase();
  const response = await fetch(
    `${base}/admin/hives/${encodeURIComponent(hiveId)}/team_runs${toQueryString(params)}`
  );
  return parseJsonResponse(response);
};
