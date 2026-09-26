import { elements } from "./elements.js?v=20260215-01";
import { getWunderBase } from "./api.js";
import { t } from "./i18n.js?v=20260215-01";
import { resolveApiErrorMessage } from "./api-error.js";

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

const parseErrorMessage = async (response) =>
  resolveApiErrorMessage(response, t("auth.login.error.status", { status: response.status }));

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

const wait = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

const prefersReducedMotion = () =>
  typeof window !== "undefined" &&
  typeof window.matchMedia === "function" &&
  window.matchMedia("(prefers-reduced-motion: reduce)").matches;

// 闭锁/失效属于静止状态，不展示顶部读出条；仅在流程态给出反馈。
const AIRLOCK_STATUS_HIDDEN_KEYS = new Set([
  "auth.airlock.status.locked",
  "auth.airlock.status.invalid",
]);

const setAirlockStatus = (key) => {
  const el = elements.airlockStatus;
  if (el) {
    el.textContent = t(key);
  }
  elements.adminLoginModal?.classList.toggle(
    "airlock--status-on",
    !AIRLOCK_STATUS_HIDDEN_KEYS.has(key),
  );
};

const setLoginVisible = (visible, { focus = true } = {}) => {
  const modal = elements.adminLoginModal;
  if (!modal) {
    return;
  }
  if (visible) {
    modal.classList.remove("airlock--opening", "airlock--open");
    modal.classList.add("active");
    modal.setAttribute("aria-hidden", "false");
    if (focus) {
      elements.adminLoginUsername?.focus();
    }
    return;
  }
  modal.classList.remove("active", "airlock--opening");
  modal.setAttribute("aria-hidden", "true");
};

const setLoginError = (message) => {
  if (!elements.adminLoginError) {
    return;
  }
  elements.adminLoginError.textContent = message || "";
  elements.adminLoginError.classList.toggle("active", Boolean(message));
};

const setLoginLoading = (loading) => {
  if (elements.adminLoginBtn) {
    elements.adminLoginBtn.disabled = loading;
    elements.adminLoginBtn.classList.toggle("is-loading", loading);
  }
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
  setAirlockStatus("auth.airlock.status.locked");
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
  setAirlockStatus("auth.airlock.status.opening");
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

// Unblock the app boot immediately; the door animation runs on its own and
// hides the overlay when finished.
const completeLogin = ({ grantBeatMs = 0 } = {}) => {
  setLoginError("");
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
  setLoginError("");
  setLoginLoading(false);
  setAirlockStatus("auth.airlock.status.closing");
  const reducedMotion = prefersReducedMotion();
  modal.classList.remove("airlock--open", "airlock--opening");
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

const performLogin = async () => {
  const username = String(elements.adminLoginUsername?.value || "").trim();
  const password = String(elements.adminLoginPassword?.value || "").trim();
  if (!username || !password) {
    setLoginError(t("auth.login.error.empty"));
    return;
  }
  const wunderBase = getWunderBase();
  if (!wunderBase) {
    setLoginError(t("settings.error.apiBase"));
    return;
  }
  setLoginLoading(true);
  setLoginError("");
  setAirlockStatus("auth.airlock.status.authenticating");
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
      const message = await parseErrorMessage(response);
      setLoginError(t("auth.login.error", { message }));
      setAirlockStatus("auth.airlock.status.locked");
      return;
    }
    const data = await response.json();
    const token = data?.data?.access_token;
    const user = data?.data?.user || null;
    if (!token) {
      setLoginError(t("auth.login.error", { message: t("auth.login.error.generic") }));
      setAirlockStatus("auth.airlock.status.locked");
      return;
    }
    const scope = await resolveAuthScope(token, user);
    if (!scope) {
      clearStoredAuth();
      setLoginError(t("auth.login.notAdmin"));
      setAirlockStatus("auth.airlock.status.locked");
      return;
    }
    writeStoredAuth({ token, user, scope });
    setAirlockStatus("auth.airlock.status.granted");
    completeLogin({ grantBeatMs: 600 });
  } catch (error) {
    const message = error?.message || t("auth.login.error.generic");
    setLoginError(t("auth.login.error", { message }));
    setAirlockStatus("auth.airlock.status.locked");
  } finally {
    setLoginLoading(false);
  }
};

export const initAdminAuth = async () => {
  if (!elements.adminLoginModal) {
    return;
  }
  if (elements.adminLoginUsername && !elements.adminLoginUsername.value.trim()) {
    elements.adminLoginUsername.value = "admin";
  }
  if (elements.adminLoginForm) {
    elements.adminLoginForm.addEventListener("submit", (event) => {
      event.preventDefault();
      performLogin();
    });
  }
  if (elements.adminLogoutBtn) {
    elements.adminLogoutBtn.addEventListener("click", () => {
      void logoutAdmin();
    });
  }
  // Doors are closed from first paint: greet the user before touching storage.
  setLoginVisible(true, { focus: false });
  const stored = readStoredAuth();
  const token = typeof stored.token === "string" ? stored.token.trim() : "";
  if (token) {
    setAirlockStatus("auth.airlock.status.checking");
    const valid = await validateToken(token);
    if (valid) {
      // Refresh with a live session still plays the hatch opening once.
      setAirlockStatus("auth.airlock.status.granted");
      completeLogin({ grantBeatMs: 350 });
      return;
    }
    clearStoredAuth();
    setLoginError(t("auth.airlock.status.invalid"));
    setAirlockStatus("auth.airlock.status.invalid");
    elements.adminLoginUsername?.focus();
    return;
  }
  await waitForLogin();
};

