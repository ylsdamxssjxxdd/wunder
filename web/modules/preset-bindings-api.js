// 预设智能体「绑定 / 同步」接口适配层。
// 契约来源：docs/云端易用重构方案.md §12.2.1 第 5)~8) 条（字段级冻结）。
//
// 设计要点：
// 1. 四个接口集中在此封装，业务模块不直接拼 URL；
// 2. 后端未就绪时统一降级为「契约未就绪」状态：返回 ok=false + unavailable=true，
//    不抛异常、不伪造数据、不重复请求（状态一旦判定未就绪，后续调用直接短路）；
// 3. 只读列表接口仍返回原样 items，便于调用方在契约缺字段时继续渲染旧字段。
import { getWunderBase } from "./api.js";
import { resolveApiErrorMessage } from "./api-error.js";

export const CONTRACT_STATE_UNKNOWN = "unknown";
export const CONTRACT_STATE_READY = "ready";
export const CONTRACT_STATE_UNAVAILABLE = "unavailable";

// 预设可声明「允许用户自定义」的字段（与契约 §12.2.1 第 3)/5) 条一致）
export const CUSTOMIZABLE_FIELDS = [
  "system_prompt",
  "welcome",
  "model_name",
  "reasoning_effort",
  "tool_names",
  "approval_mode",
];

// 可自定义字段的界面文案键，供预设面板与用户管理面板复用
export const CUSTOMIZABLE_FIELD_LABEL_KEYS = {
  system_prompt: "presetAgents.customizable.field.systemPrompt",
  welcome: "presetAgents.customizable.field.welcome",
  model_name: "presetAgents.customizable.field.modelName",
  reasoning_effort: "presetAgents.customizable.field.reasoningEffort",
  tool_names: "presetAgents.customizable.field.toolNames",
  approval_mode: "presetAgents.customizable.field.approvalMode",
};

// 绑定列表分页上限；契约要求 page_size 有固定上限，这里统一收敛
export const BINDING_MAX_PAGE_SIZE = 100;
export const BINDING_DEFAULT_PAGE_SIZE = 20;

// 仅这两个状态码代表「端点尚未交付」，其余错误码按普通失败处理
const UNAVAILABLE_STATUS = new Set([404, 501]);

let contractState = CONTRACT_STATE_UNKNOWN;
let contractDetail = "";

export const getContractState = () => contractState;

// 手动刷新面板时允许重置，便于后端上线后无需刷新整页即可恢复
export const resetContractState = () => {
  contractState = CONTRACT_STATE_UNKNOWN;
  contractDetail = "";
};

const markContractReady = () => {
  contractState = CONTRACT_STATE_READY;
  contractDetail = "";
};

const markContractUnavailable = (detail) => {
  contractState = CONTRACT_STATE_UNAVAILABLE;
  contractDetail = String(detail || "").trim();
};

const isKnownUnavailable = (force) => !force && contractState === CONTRACT_STATE_UNAVAILABLE;

const unavailableResult = () => ({
  ok: false,
  unavailable: true,
  contractReady: false,
  message: contractDetail,
});

const buildQuery = (params = {}) => {
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

const hasOwn = (target, key) =>
  Boolean(target) && typeof target === "object" && Object.prototype.hasOwnProperty.call(target, key);

// 统一请求：返回结构化结果，调用方无需处理异常分支
const requestContract = async (path, { method = "GET", body, query, force = false } = {}) => {
  if (isKnownUnavailable(force)) {
    return unavailableResult();
  }
  let response;
  try {
    response = await fetch(getWunderBase() + path + buildQuery(query), {
      method,
      headers: { "Content-Type": "application/json" },
      body: body === undefined ? undefined : JSON.stringify(body),
    });
  } catch (error) {
    return {
      ok: false,
      unavailable: false,
      contractReady: false,
      message: error?.message || "",
    };
  }
  if (UNAVAILABLE_STATUS.has(response.status)) {
    markContractUnavailable(`${method} ${path} → HTTP ${response.status}`);
    return unavailableResult();
  }
  if (!response.ok) {
    const message = await resolveApiErrorMessage(response, "");
    return { ok: false, unavailable: false, contractReady: false, message };
  }
  let payload = null;
  try {
    payload = await response.json();
  } catch (_error) {
    payload = null;
  }
  const data = payload && typeof payload === "object" ? payload.data : null;
  if (!data || typeof data !== "object" || Array.isArray(data)) {
    markContractUnavailable(`${method} ${path} → 响应缺少 data 包裹`);
    return unavailableResult();
  }
  return { ok: true, unavailable: false, contractReady: true, data, message: "" };
};

// 规范化可自定义标记：缺字段一律视为「不允许用户自定义」
export const normalizeCustomizable = (raw) => {
  const source = raw && typeof raw === "object" && !Array.isArray(raw) ? raw : {};
  const output = {};
  CUSTOMIZABLE_FIELDS.forEach((key) => {
    output[key] = source[key] === true;
  });
  return output;
};

const toInteger = (value, fallback = 0) => {
  const parsed = Number(value);
  return Number.isFinite(parsed) ? Math.trunc(parsed) : fallback;
};

// 3) / 5) 预设列表：新增 bound_users 与 customizable，去掉 sandbox_container_id
export const listPresetAgents = async ({ force = false } = {}) => {
  const result = await requestContract("/admin/preset_agents", { force });
  if (!result.ok) {
    return { ok: false, unavailable: result.unavailable, items: [], message: result.message };
  }
  const rawItems = Array.isArray(result.data.items) ? result.data.items : null;
  if (!rawItems) {
    markContractUnavailable("GET /admin/preset_agents → data.items 缺失");
    return { ...unavailableResult(), items: [] };
  }
  const hasNewFields = rawItems.some(
    (item) => hasOwn(item, "bound_users") || hasOwn(item, "customizable")
  );
  if (rawItems.length && !hasNewFields) {
    markContractUnavailable("GET /admin/preset_agents → 缺少 bound_users / customizable 字段");
  } else if (hasNewFields) {
    markContractReady();
  }
  return {
    ok: true,
    unavailable: false,
    items: rawItems,
    contractReady: hasNewFields,
    message: "",
  };
};

const normalizeBindingItem = (item) => ({
  user_id: String(item?.user_id || item?.userId || "").trim(),
  username: String(item?.username || "").trim(),
  agent_id: String(item?.agent_id || item?.agentId || "").trim(),
  customized: (Array.isArray(item?.customized) ? item.customized : Array.isArray(item?.customized_fields) ? item.customized_fields : [])
    .map((value) => String(value || "").trim())
    .filter(Boolean),
});

// 6) 绑定用户列表（分页 + 关键字）
export const listPresetBindings = async ({
  presetId,
  page = 1,
  pageSize = BINDING_DEFAULT_PAGE_SIZE,
  keyword = "",
  force = false,
} = {}) => {
  const preset = String(presetId || "").trim();
  if (!preset) {
    return { ok: false, unavailable: false, items: [], total: 0, page: 1, pageSize: BINDING_DEFAULT_PAGE_SIZE, message: "" };
  }
  const safePageSize = Math.min(
    BINDING_MAX_PAGE_SIZE,
    Math.max(1, toInteger(pageSize, BINDING_DEFAULT_PAGE_SIZE))
  );
  const safePage = Math.max(1, toInteger(page, 1));
  const result = await requestContract(
    `/admin/preset_agents/${encodeURIComponent(preset)}/bindings`,
    {
      method: "GET",
      query: { page: safePage, page_size: safePageSize, keyword: String(keyword || "").trim() },
      force,
    }
  );
  if (!result.ok) {
    return {
      ok: false,
      unavailable: result.unavailable,
      items: [],
      total: 0,
      page: safePage,
      pageSize: safePageSize,
      message: result.message,
    };
  }
  const rawItems = result.data.items;
  if (!Array.isArray(rawItems)) {
    markContractUnavailable("GET /admin/preset_agents/{id}/bindings → data.items 缺失");
    return { ...unavailableResult(), items: [], total: 0, page: safePage, pageSize: safePageSize };
  }
  markContractReady();
  return {
    ok: true,
    unavailable: false,
    items: rawItems.map(normalizeBindingItem).filter((item) => item.user_id),
    total: Math.max(0, toInteger(result.data.total, rawItems.length)),
    page: safePage,
    pageSize: safePageSize,
    message: "",
  };
};

// 7) 绑定 / 换绑 / 解绑。解绑必须显式给出 new_preset_id，保证用户始终有且仅有一个智能体。
export const mutatePresetBindings = async ({
  presetId,
  userIds = [],
  action = "bind",
  newPresetId = "",
  force = false,
} = {}) => {
  const preset = String(presetId || "").trim();
  const users = (Array.isArray(userIds) ? userIds : []).map((value) => String(value || "").trim()).filter(Boolean);
  const normalizedAction = action === "unbind" ? "unbind" : "bind";
  const targetPreset = String(newPresetId || "").trim();
  if (!preset || !users.length) {
    return { ok: false, unavailable: false, result: null, message: "" };
  }
  if (normalizedAction === "unbind" && !targetPreset) {
    return { ok: false, unavailable: false, result: null, message: "" };
  }
  const body = { preset_id: preset, user_ids: users, action: normalizedAction };
  if (targetPreset) {
    body.new_preset_id = targetPreset;
  }
  const result = await requestContract("/admin/preset_agents/bindings", {
    method: "POST",
    body,
    force,
  });
  if (!result.ok) {
    return { ok: false, unavailable: result.unavailable, result: null, message: result.message };
  }
  if (!hasOwn(result.data, "affected_users")) {
    markContractUnavailable("POST /admin/preset_agents/bindings → 缺少 affected_users 字段");
    return { ...unavailableResult(), result: null };
  }
  markContractReady();
  return {
    ok: true,
    unavailable: false,
    changed: true,
    result: {
      preset_id: String(result.data.preset_id || preset),
      affected_users: Math.max(0, toInteger(result.data.affected_users, users.length)),
      created_agents: Math.max(0, toInteger(result.data.created_agents, 0)),
      rebound_agents: Math.max(0, toInteger(result.data.rebound_agents, 0)),
    },
    message: "",
  };
};

// 8) 同步：dry_run=true 先预览影响面，确认后再以 dry_run=false 落库
export const syncPresetAgents = async ({ presetId, mode = "safe", dryRun = true, force = false } = {}) => {
  const preset = String(presetId || "").trim();
  if (!preset) {
    return { ok: false, unavailable: false, result: null, message: "" };
  }
  const normalizedMode = mode === "force" ? "force" : "safe";
  const result = await requestContract("/admin/preset_agents/sync", {
    method: "POST",
    body: { preset_id: preset, mode: normalizedMode, dry_run: dryRun === true },
    force,
  });
  if (!result.ok) {
    return { ok: false, unavailable: result.unavailable, result: null, message: result.message };
  }
  if (!hasOwn(result.data, "affected_users")) {
    markContractUnavailable("POST /admin/preset_agents/sync → 缺少 affected_users 字段（旧版同步语义）");
    return { ...unavailableResult(), result: null };
  }
  markContractReady();
  return {
    ok: true,
    unavailable: false,
    result: {
      preset_id: String(result.data.preset_id || preset),
      mode: String(result.data.mode || normalizedMode) === "force" ? "force" : "safe",
      dry_run: result.data.dry_run === true,
      affected_users: Math.max(0, toInteger(result.data.affected_users, 0)),
      updated_agents: Math.max(0, toInteger(result.data.updated_agents, 0)),
      skipped_customized: Math.max(0, toInteger(result.data.skipped_customized, 0)),
      created_agents: Math.max(0, toInteger(result.data.created_agents, 0)),
    },
    message: "",
  };
};

// 9) 用户列表（管理侧）绑定字段：缺失时返回空值，由界面显示「-」并标注契约未就绪
export const normalizeUserBindingFields = (item) => {
  const customized = Array.isArray(item?.customized_fields)
    ? item.customized_fields
    : Array.isArray(item?.customized)
      ? item.customized
      : [];
  const presetId = String(item?.preset_id || item?.presetId || "").trim();
  const agentId = String(item?.agent_id || item?.agentId || "").trim();
  return {
    preset_id: presetId,
    agent_id: agentId,
    customized_fields: customized.map((value) => String(value || "").trim()).filter(Boolean),
    available: Boolean(presetId || agentId || customized.length),
  };
};
