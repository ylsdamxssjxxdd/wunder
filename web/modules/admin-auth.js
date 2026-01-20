import { elements } from "./elements.js?v=20260118-07";
import { getWunderBase } from "./api.js";
import { t } from "./i18n.js?v=20260118-07";

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

const isAdminUser = (user) => {
  const roles = Array.isArray(user?.roles) ? user.roles : [];
  return roles.includes("admin") || roles.includes("super_admin");
};

export const getAuthToken = () => {
  const stored = readStoredAuth();
  const token = typeof stored.token === "string" ? stored.token.trim() : "";
  return token;
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

const parseErrorMessage = async (response) => {
  try {
    const data = await response.json();
    const message = data?.detail?.message;
    if (message) {
      return message;
    }
  } catch (error) {
    // Ignore parse errors and fall back to status.
  }
  return t("auth.login.error.status", { status: response.status });
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
    if (!isAdminUser(data?.data)) {
      return false;
    }
    writeStoredAuth({ token, user: data?.data || null });
    return true;
  } catch (error) {
    return false;
  }
};

let loginPromise = null;
let loginResolve = null;

const setLoginVisible = (visible) => {
  if (!elements.adminLoginModal) {
    return;
  }
  elements.adminLoginModal.classList.toggle("active", visible);
  elements.adminLoginModal.setAttribute("aria-hidden", visible ? "false" : "true");
  if (visible) {
    elements.adminLoginUsername?.focus();
  }
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

const waitForLogin = () => {
  if (loginPromise) {
    return loginPromise;
  }
  loginPromise = new Promise((resolve) => {
    loginResolve = resolve;
  });
  setLoginVisible(true);
  return loginPromise;
};

const completeLogin = () => {
  setLoginVisible(false);
  setLoginError("");
  if (loginResolve) {
    loginResolve();
  }
  loginPromise = null;
  loginResolve = null;
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
  try {
    const response = await fetch(`${wunderBase}/auth/login`, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
      },
      body: JSON.stringify({ username, password }),
    });
    if (!response.ok) {
      const message = await parseErrorMessage(response);
      setLoginError(t("auth.login.error", { message }));
      return;
    }
    const data = await response.json();
    const token = data?.data?.access_token;
    const user = data?.data?.user || null;
    if (!token) {
      setLoginError(t("auth.login.error", { message: t("auth.login.error.generic") }));
      return;
    }
    if (!isAdminUser(user)) {
      clearStoredAuth();
      setLoginError(t("auth.login.notAdmin"));
      return;
    }
    writeStoredAuth({ token, user });
    completeLogin();
  } catch (error) {
    const message = error?.message || t("auth.login.error.generic");
    setLoginError(t("auth.login.error", { message }));
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
  const stored = readStoredAuth();
  const token = typeof stored.token === "string" ? stored.token.trim() : "";
  if (token) {
    const valid = await validateToken(token);
    if (valid) {
      completeLogin();
      return;
    }
    clearStoredAuth();
  }
  await waitForLogin();
};
