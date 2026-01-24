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

// 统一的轻量级提示，避免操作后没有反馈
export const notify = (message, type = "info", options = {}) => {
  if (!message) {
    return;
  }
  const container = ensureToastContainer();
  const toast = document.createElement("div");
  toast.className = `toast ${type}`;
  toast.textContent = String(message);
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


