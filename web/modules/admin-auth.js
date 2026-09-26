import { elements } from "./elements.js?v=20260926-04";
import { getWunderBase } from "./api.js";
import { t } from "./i18n.js?v=20260926-04";

const AUTH_STORAGE_KEY = "wunder_admin_auth";

const readStoredAuth = () => {
  try {
    const raw = localStorage.getItem(AUTH_STORAGE_KEY);
    if (!raw) {
      return {};
    }
    const parsed = JSON.parse(raw);
    return parsed && typeof parsed === "object" ? parsed : {};
  } catch (error) {
    return {};
  }
};

const writeStoredAuth = (payload) => {
  const safe = payload && typeof payload === "object" ? payload : {};
  try {
    localStorage.setItem(AUTH_STORAGE_KEY, JSON.stringify(safe));
  } catch (error) {
    // Ignore storage errors to avoid blocking auth flow.
  }
  return safe;
};

const clearStoredAuth = () => {
  try {
    localStorage.removeItem(AUTH_STORAGE_KEY);
  } catch (error) {
    // Ignore storage errors to avoid blocking auth flow.
  }
};

const AUTH_SCOPE_ADMIN = "admin";
const AUTH_SCOPE_LEADER = "leader";

const isAdminUser = (user) => {
  const roles = Array.isArray(user?.roles) ? user.roles : [];
  return roles.includes("admin") || roles.includes("super_admin");
};

const checkLeaderAccess = async (token) => {
  const wunderBase = getWunderBase();
  if (!wunderBase || !token) {
    return false;
  }
  const response = await fetch(`${wunderBase}/admin/org_units`, {
    headers: {
      Authorization: `Bearer ${token}`,
    },
  });
  return response.ok;
};

const resolveAuthScope = async (token, user) => {
  if (!token) {
    return "";
  }
  if (isAdminUser(user)) {
    return AUTH_SCOPE_ADMIN;
  }
  const isLeader = await checkLeaderAccess(token);
  return isLeader ? AUTH_SCOPE_LEADER : "";
};

export const getAuthToken = () => {
  const stored = readStoredAuth();
  const token = typeof stored.token === "string" ? stored.token.trim() : "";
  return token;
};

export const getAuthScope = () => {
  const stored = readStoredAuth();
  if (stored?.scope) {
    return stored.scope;
  }
  if (stored?.user && isAdminUser(stored.user)) {
    return AUTH_SCOPE_ADMIN;
  }
  return "";
};

export const getAuthHeaders = () => {
  const token = getAuthToken();
  if (token) {
    return { Authorization: `Bearer ${token}` };
  }
  const apiKey = String(elements.apiKey?.value || "").trim();
  if (apiKey) {
    return { "X-API-Key": apiKey };
  }
  return undefined;
};

export const applyAuthHeaders = (headers) => {
  if (!headers) {
    return;
  }
  if (headers.has("Authorization") || headers.has("X-API-Key")) {
    return;
  }
  const token = getAuthToken();
  if (token) {
    headers.set("Authorization", `Bearer ${token}`);
    return;
  }
  const apiKey = String(elements.apiKey?.value || "").trim();
  if (apiKey) {
    headers.set("X-API-Key", apiKey);
  }
};

const validateToken = async (token) => {
  const wunderBase = getWunderBase();
  if (!wunderBase) {
    return false;
  }
  try {
    const response = await fetch(`${wunderBase}/auth/me`, {
      headers: {
        Authorization: `Bearer ${token}`,
      },
    });
    if (!response.ok) {
      return false;
    }
    const data = await response.json();
    const user = data?.data || null;
    const scope = await resolveAuthScope(token, user);
    if (!scope) {
      return false;
    }
    writeStoredAuth({ token, user, scope });
    return true;
  } catch (error) {
    return false;
  }
};

let loginPromise = null;
let loginResolve = null;

// Keep in sync with the door slide duration in styles/airlock.css.
const AIRLOCK_OPEN_TOTAL_MS = 1600;
const AIRLOCK_CLOSE_TOTAL_MS = 1450;
const AIRLOCK_REDUCED_MOTION_MS = 150;
const AIRLOCK_CARVE_DRAW_MS = 1050;
const AIRLOCK_CARVE_SETTLE_MS = 520;

// 图案锁口令：左侧第2个(Geburah) → 右侧第2个(Chesed) → 中间第1个(Kether) → 中间第4个(Malkuth)。
const TREE_UNLOCK_SEQUENCE = [5, 4, 1, 10];
const TREE_WRONG_FLASH_MS = 450;

const wait = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

const prefersReducedMotion = () =>
  typeof window !== "undefined" &&
  typeof window.matchMedia === "function" &&
  window.matchMedia("(prefers-reduced-motion: reduce)").matches;

const setLoginVisible = (visible) => {
  const modal = elements.adminLoginModal;
  if (!modal) {
    return;
  }
  if (visible) {
    modal.classList.remove("airlock--opening", "airlock--open", "airlock--carved");
    modal.classList.add("active");
    modal.setAttribute("aria-hidden", "false");
    return;
  }
  modal.classList.remove("active", "airlock--opening");
  modal.setAttribute("aria-hidden", "true");
};

const startLoginWait = () => {
  loginPromise = new Promise((resolve) => {
    loginResolve = resolve;
  });
  return loginPromise;
};

const waitForLogin = () => {
  if (loginPromise) {
    return loginPromise;
  }
  startLoginWait();
  setLoginVisible(true);
  return loginPromise;
};

// Play the hatch opening on top of the overlay, then hide it once the doors
// have fully slid apart so the admin shell underneath becomes interactive.
const playAirlockOpenSequence = async (grantBeatMs) => {
  const modal = elements.adminLoginModal;
  if (!modal) {
    return;
  }
  const reducedMotion = prefersReducedMotion();
  if (grantBeatMs > 0 && !reducedMotion) {
    await wait(grantBeatMs);
  }
  modal.classList.add("airlock--opening");
  await wait(reducedMotion ? AIRLOCK_REDUCED_MOTION_MS : AIRLOCK_OPEN_TOTAL_MS);
  modal.classList.remove("airlock--opening");
  modal.classList.add("airlock--open");
  modal.setAttribute("aria-hidden", "true");
};

// 口令命中：亮刃沿树描切一圈、凹槽加深定型，随后开门。
const playCarveSequence = async () => {
  const modal = elements.adminLoginModal;
  if (!modal) {
    return;
  }
  const reducedMotion = prefersReducedMotion();
  if (reducedMotion) {
    modal.classList.add("airlock--carved");
    await wait(120);
    return;
  }
  modal.classList.add("airlock--carving");
  await wait(AIRLOCK_CARVE_DRAW_MS);
  modal.classList.remove("airlock--carving");
  modal.classList.add("airlock--carved");
  await wait(AIRLOCK_CARVE_SETTLE_MS);
};

// Unblock the app boot immediately; the door animation runs on its own and
// hides the overlay when finished.
const completeLogin = ({ grantBeatMs = 0 } = {}) => {
  if (loginResolve) {
    loginResolve();
  }
  loginPromise = null;
  loginResolve = null;
  void playAirlockOpenSequence(grantBeatMs);
};

// Sign out: seal the doors over the console, then reload behind them so
// panels and auth scope rebuild from a clean state.
export const logoutAdmin = async () => {
  clearStoredAuth();
  const modal = elements.adminLoginModal;
  if (!modal) {
    location.reload();
    return;
  }
  const reducedMotion = prefersReducedMotion();
  modal.classList.remove("airlock--open", "airlock--opening", "airlock--carved");
  modal.classList.add("active", "airlock--closing", "airlock--open-doors", "airlock--instant");
  modal.setAttribute("aria-hidden", "false");
  // Jump the doors to the open position without a transition, then re-arm
  // transitions and let them slide shut.
  void modal.offsetWidth;
  modal.classList.remove("airlock--instant");
  void modal.offsetWidth;
  if (reducedMotion) {
    modal.classList.remove("airlock--open-doors", "airlock--closing");
    await wait(120);
  } else {
    modal.classList.remove("airlock--open-doors");
    await wait(AIRLOCK_CLOSE_TOTAL_MS);
    modal.classList.remove("airlock--closing");
    await wait(420);
  }
  location.reload();
};

// ---- 卡巴拉图案锁 ----

let treeProgress = 0;
let treeUnlocking = false;
let treeResetTimer = null;

const resetTreeLights = () => {
  document.querySelectorAll(".airlock-node.lit, .airlock-node.wrong").forEach((node) => {
    node.classList.remove("lit", "wrong");
  });
  treeProgress = 0;
};

// 认证失败：已点亮的质点闪红一次后全部重置（无文字提示）。
const failLogin = () => {
  treeUnlocking = false;
  document.querySelectorAll(".airlock-node.lit").forEach((node) => {
    node.classList.remove("lit");
    node.classList.add("wrong");
  });
  clearTimeout(treeResetTimer);
  treeResetTimer = setTimeout(resetTreeLights, TREE_WRONG_FLASH_MS);
};

// 统一登录入口：图案锁命中后以默认管理员凭据请求后端会话。
const performLogin = async (username, password) => {
  const wunderBase = getWunderBase();
  if (!wunderBase) {
    failLogin();
    return;
  }
  try {
    const response = await fetch(`${wunderBase}/auth/login`, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        "X-Wunder-Session-Scope": "admin_web",
      },
      body: JSON.stringify({ username, password }),
    });
    if (!response.ok) {
      failLogin();
      return;
    }
    const data = await response.json();
    const token = data?.data?.access_token;
    const user = data?.data?.user || null;
    if (!token) {
      failLogin();
      return;
    }
    const scope = await resolveAuthScope(token, user);
    if (!scope) {
      clearStoredAuth();
      failLogin();
      return;
    }
    writeStoredAuth({ token, user, scope });
    // 先播树深刻入门的动画，再沿树中线开门。
    await playCarveSequence();
    completeLogin({ grantBeatMs: 0 });
  } catch (error) {
    failLogin();
  }
};

const handleTreeNodeClick = (event) => {
  // 登录流程已结束（等待开门/已进入）或正在认证时忽略点击。
  if (!loginPromise || treeUnlocking) {
    return;
  }
  const nodeId = Number(event.currentTarget?.dataset?.node);
  if (!nodeId) {
    return;
  }
  const expected = TREE_UNLOCK_SEQUENCE[treeProgress];
  if (nodeId !== expected) {
    // 中柱质点两半同步闪红，避免看起来只剩一半。
    document.querySelectorAll(`.airlock-node[data-node="${nodeId}"]`).forEach((node) => {
      node.classList.add("wrong");
    });
    // 稍作停留让用户看到红光，再统一重置。
    setTimeout(resetTreeLights, TREE_WRONG_FLASH_MS);
    return;
  }
  // 中柱质点在两扇门上各有一半，点亮时全部同步，视觉上是一个整圆。
  document.querySelectorAll(`.airlock-node[data-node="${nodeId}"]`).forEach((node) => {
    node.classList.add("lit");
  });
  treeProgress += 1;
  if (treeProgress === TREE_UNLOCK_SEQUENCE.length) {
    treeUnlocking = true;
    void performLogin("admin", "admin");
  }
};

export const initAdminAuth = async () => {
  if (!elements.adminLoginModal) {
    return;
  }
  document.querySelectorAll(".airlock-node").forEach((node) => {
    node.addEventListener("click", handleTreeNodeClick);
  });
  if (elements.adminLogoutBtn) {
    elements.adminLogoutBtn.addEventListener("click", () => {
      void logoutAdmin();
    });
  }
  // Doors are closed from first paint: greet the user before touching storage.
  setLoginVisible(true);
  const stored = readStoredAuth();
  const token = typeof stored.token === "string" ? stored.token.trim() : "";
  if (token) {
    const valid = await validateToken(token);
    if (valid) {
      // Refresh with a live session still plays the hatch opening once.
      completeLogin({ grantBeatMs: 350 });
      return;
    }
    clearStoredAuth();
  }
  await waitForLogin();
};
