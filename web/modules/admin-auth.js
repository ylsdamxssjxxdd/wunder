import { elements } from "./elements.js?v=20261016-01";
import { getWunderBase } from "./api.js";

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
let overlayTransition = 0;

// Safety net only. Normal completion is driven by the final hex animationend
// event, so a frame is not held after the visible wall has gone.
const AIRLOCK_HEX_ANIMATION_FALLBACK_MS = 1600;

// 图案锁口令：左侧第2个(Geburah) → 右侧第2个(Chesed) → 中间第1个(Kether) → 中间第4个(Malkuth)。
const TREE_UNLOCK_SEQUENCE = [5, 4, 1, 10];
const TREE_WRONG_FLASH_MS = 450;
let hexResizeTimer = null;

// Build the repeating emergency-panel wall from bounded DOM nodes. A fixed
// viewport-sized pool keeps the effect cheap while still covering resize.
const buildHexField = () => {
  const field = document.querySelector(".airlock-hex-field");
  if (!field) {
    return;
  }
  const width = window.innerWidth || 1280;
  const height = window.innerHeight || 720;
  // Flat-top regular hexagon: width / height = 2 / sqrt(3). The centers
  // follow the actual honeycomb neighbor distances, so panels touch at their
  // edges without the stretched, rectangular gaps of the old grid.
  const cellWidth = Math.max(136, Math.min(220, width * 0.15));
  const cellHeight = cellWidth * 0.8660254038;
  const colStep = cellWidth * 0.75;
  const rowStep = cellHeight;
  const columns = Math.ceil(width / colStep);
  const rows = Math.ceil(height / rowStep);
  const fragment = document.createDocumentFragment();
  for (let row = -1; row < rows; row += 1) {
    for (let col = -1; col < columns; col += 1) {
      const x = col * colStep;
      // Flat-top honeycomb: neighboring columns are offset by half a cell.
      const y = row * rowStep + (col % 2 === 0 ? 0 : rowStep * 0.5);
      // Offscreen panels must not extend the visible transition's lifetime.
      if (x >= width || x + cellWidth <= 0 || y >= height || y + cellHeight <= 0) {
        continue;
      }
      const cell = document.createElement("div");
      cell.className = "airlock-hex-cell";
      const distance = Math.hypot(
        x + cellWidth / 2 - width / 2,
        y + cellHeight / 2 - height / 2,
      );
      const wave = Math.min(10, Math.round((distance / (Math.hypot(width, height) / 2)) * 10));
      cell.style.width = `${cellWidth}px`;
      cell.style.height = `${cellHeight}px`;
      cell.style.left = `${x}px`;
      cell.style.top = `${y}px`;
      cell.style.setProperty("--hex-delay", `${wave * 52}ms`);
      cell.style.setProperty("--hex-return-delay", `${(10 - wave) * 52}ms`);
      cell.innerHTML = '<i aria-hidden="true"></i><span>EMERGENCY</span><i aria-hidden="true"></i>';
      fragment.appendChild(cell);
    }
  }
  field.replaceChildren(fragment);
  field.classList.remove("is-fading", "is-refilling");
};

const setHexFieldState = (state) => {
  const field = document.querySelector(".airlock-hex-field");
  if (!field) {
    return;
  }
  field.classList.remove("is-fading", "is-refilling");
  if (state === "fading" || state === "refilling") {
    // Restart the animation even when a session is restored repeatedly.
    void field.offsetWidth;
    field.classList.add(state === "fading" ? "is-fading" : "is-refilling");
  }
};

const waitForHexFieldAnimation = (state) => {
  const field = document.querySelector(".airlock-hex-field");
  if (!field || prefersReducedMotion()) {
    return Promise.resolve();
  }
  const expectedName = state === "fading" ? "airlock-hex-fade-out" : "airlock-hex-fade-in";
  const pending = new Set(field.querySelectorAll(".airlock-hex-cell"));
  if (!pending.size) {
    return Promise.resolve();
  }
  return new Promise((resolve) => {
    let done = false;
    const finish = () => {
      if (done) {
        return;
      }
      done = true;
      clearTimeout(fallbackTimer);
      field.removeEventListener("animationend", onAnimationEnd, true);
      field.removeEventListener("animationcancel", onAnimationEnd, true);
      resolve();
    };
    const onAnimationEnd = (event) => {
      const cell = event.target;
      if (
        event.animationName !== expectedName ||
        !cell?.classList?.contains("airlock-hex-cell") ||
        !pending.delete(cell)
      ) {
        return;
      }
      if (!pending.size) {
        finish();
      }
    };
    const fallbackTimer = setTimeout(finish, AIRLOCK_HEX_ANIMATION_FALLBACK_MS);
    field.addEventListener("animationend", onAnimationEnd, true);
    field.addEventListener("animationcancel", onAnimationEnd, true);
  });
};

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
    modal.classList.remove("airlock--open", "airlock--exiting");
    modal.classList.add("active");
    modal.setAttribute("aria-hidden", "false");
    return;
  }
  modal.classList.remove("active");
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

// Only the hex wall participates in the transition. The static lock is
// already hidden when authentication succeeds.
const playOverlayExitSequence = async (transition) => {
  const modal = elements.adminLoginModal;
  if (!modal) {
    return;
  }
  // Subscribe before starting, including when animations complete in one frame.
  const animation = waitForHexFieldAnimation("fading");
  setHexFieldState("fading");
  await animation;
  if (transition !== overlayTransition) {
    return;
  }
  modal.classList.add("airlock--open");
  modal.setAttribute("aria-hidden", "true");
};

// Unblock the app boot immediately; the overlay disposes itself after the
// hex wall has disappeared.
const completeLogin = () => {
  clearTimeout(hexResizeTimer);
  // Hide the tree synchronously, before the first hex fades or app boot resumes.
  elements.adminLoginModal?.classList.add("airlock--exiting");
  void playOverlayExitSequence(++overlayTransition);
  if (loginResolve) {
    loginResolve();
  }
  loginPromise = null;
  loginResolve = null;
};

// Sign out: restore the hex wall and return to the in-page lock state. A full
// document reload causes a black flash after the animation and is unnecessary
// because all auth reads come from the cleared stored session.
export const logoutAdmin = async () => {
  clearStoredAuth();
  clearTimeout(hexResizeTimer);
  const transition = ++overlayTransition;
  const modal = elements.adminLoginModal;
  if (!modal) {
    return;
  }
  // Restore a clean, static tree before showing it. Rebuild the bounded wall
  // here in case the viewport changed while the overlay was hidden.
  treeUnlocking = false;
  resetTreeLights();
  buildHexField();
  setLoginVisible(true);
  const animation = waitForHexFieldAnimation("refilling");
  setHexFieldState("refilling");
  await animation;
  if (transition !== overlayTransition) {
    return;
  }
  setHexFieldState("locked");
  startLoginWait();
};

// ---- 卡巴拉图案锁 ----

let treeProgress = 0;
let treeUnlocking = false;
let treeResetTimer = null;

const resetTreeLights = () => {
  clearTimeout(treeResetTimer);
  treeResetTimer = null;
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
    completeLogin();
  } catch (error) {
    failLogin();
  }
};

const handleTreeNodeClick = (event) => {
  // 登录流程已结束或正在认证时忽略点击。
  if (!loginPromise || treeUnlocking) {
    return;
  }
  const nodeId = Number(event.currentTarget?.dataset?.node);
  if (!nodeId) {
    return;
  }
  const expected = TREE_UNLOCK_SEQUENCE[treeProgress];
  if (nodeId !== expected) {
    // 点错后短暂提示，再统一重置。
    document.querySelectorAll(`.airlock-node[data-node="${nodeId}"]`).forEach((node) => {
      node.classList.add("wrong");
    });
    clearTimeout(treeResetTimer);
    treeResetTimer = setTimeout(resetTreeLights, TREE_WRONG_FLASH_MS);
    return;
  }
  // 点亮当前质点，保持完整生命树静态显示。
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
  buildHexField();
  window.addEventListener("resize", () => {
    const field = document.querySelector(".airlock-hex-field");
    if (!field || field.matches(".is-fading, .is-refilling")) {
      return;
    }
    clearTimeout(hexResizeTimer);
    hexResizeTimer = setTimeout(buildHexField, 120);
  }, { passive: true });
  document.querySelectorAll(".airlock-node").forEach((node) => {
    node.addEventListener("click", handleTreeNodeClick);
  });
  if (elements.adminLogoutBtn) {
    elements.adminLogoutBtn.addEventListener("click", () => {
      void logoutAdmin();
    });
  }
  // The lock is visible from first paint, before validating stored auth.
  setLoginVisible(true);
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
