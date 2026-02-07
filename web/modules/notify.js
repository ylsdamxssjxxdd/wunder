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

const TRACE_ID_RE = /(err_[a-z0-9]+)/i;

const resolveToastPayload = (message) => {
  if (message && typeof message === "object" && !Array.isArray(message)) {
    const text = String(message.message || message.text || "").trim();
    const traceId = String(message.traceId || message.trace_id || "").trim();
    const hint = String(message.hint || "").trim();
    return { text, traceId, hint };
  }
  const text = String(message || "").trim();
  const match = text.match(TRACE_ID_RE);
  return { text, traceId: match ? match[1] : "", hint: "" };
};

const copyWithExecCommand = (text) => {
  const textarea = document.createElement("textarea");
  textarea.value = text;
  textarea.setAttribute("readonly", "readonly");
  textarea.style.position = "fixed";
  textarea.style.opacity = "0";
  textarea.style.left = "-9999px";
  document.body.appendChild(textarea);
  textarea.select();
  const ok = document.execCommand("copy");
  document.body.removeChild(textarea);
  if (!ok) {
    throw new Error("copy failed");
  }
};

const copyText = async (text) => {
  if (!text) {
    return;
  }
  if (navigator && navigator.clipboard && navigator.clipboard.writeText) {
    await navigator.clipboard.writeText(text);
    return;
  }
  copyWithExecCommand(text);
};

const buildToastContent = (toast, payload, options) => {
  const message = document.createElement("div");
  message.className = "toast-message";
  message.textContent = payload.text;
  toast.appendChild(message);

  if (!payload.traceId) {
    if (payload.hint) {
      const hint = document.createElement("div");
      hint.className = "toast-hint";
      hint.textContent = payload.hint;
      toast.appendChild(hint);
    }
    return;
  }

  const metaRow = document.createElement("div");
  metaRow.className = "toast-meta";

  const trace = document.createElement("span");
  const traceLabel = String(options.traceLabel || "trace_id").trim() || "trace_id";
  trace.textContent = traceLabel + ": " + payload.traceId;
  metaRow.appendChild(trace);

  const actionButton = document.createElement("button");
  actionButton.type = "button";
  actionButton.className = "toast-action";
  actionButton.textContent = options.actionLabel || "Copy";
  actionButton.addEventListener("click", async () => {
    try {
      if (typeof options.onAction === "function") {
        await options.onAction(payload.traceId);
      } else {
        await copyText(payload.traceId);
      }
      notify(options.actionSuccess || "Trace ID copied", "success", { duration: 1800 });
    } catch (error) {
      notify(options.actionFailed || "Copy failed", "error", { duration: 2200 });
    }
  });
  metaRow.appendChild(actionButton);
  toast.appendChild(metaRow);

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

