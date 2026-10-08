let toastContainer = null;

const ensureToastContainer = () => {
  if (toastContainer) {
    return toastContainer;
  }
  toastContainer = document.getElementById("toastContainer");
  if (!toastContainer) {
    toastContainer = document.createElement("div");
    toastContainer.id = "toastContainer";
    toastContainer.className = "toast-container";
    document.body.appendChild(toastContainer);
  }
  return toastContainer;
};

const resolveToastPayload = (message) => {
  if (message && typeof message === "object" && !Array.isArray(message)) {
    const text = String(message.message || message.text || "").trim();
    const hint = String(message.hint || "").trim();
    return { text, hint };
  }
  const text = String(message || "").trim();
  return { text, hint: "" };
};

const buildToastContent = (toast, payload, options) => {
  const message = document.createElement("div");
  message.className = "toast-message";
  message.textContent = payload.text;
  toast.appendChild(message);

  if (payload.hint) {
    const hint = document.createElement("div");
    hint.className = "toast-hint";
    hint.textContent = payload.hint;
    toast.appendChild(hint);
  }
};

// 统一的轻量级提示，避免操作后没有反馈
export const notify = (message, type = "info", options = {}) => {
  const payload = resolveToastPayload(message);
  if (!payload.text) {
    return;
  }
  const container = ensureToastContainer();
  const toast = document.createElement("div");
  toast.className = "toast " + type;
  buildToastContent(toast, payload, options);
  container.appendChild(toast);

  const duration = Number.isFinite(options.duration) ? options.duration : 2600;
  const hideDelay = Math.max(600, duration);
  window.setTimeout(() => {
    toast.classList.add("hide");
    window.setTimeout(() => {
      toast.remove();
    }, 280);
  }, hideDelay);
};

