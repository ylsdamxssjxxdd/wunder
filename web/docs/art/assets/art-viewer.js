(function (global) {
  "use strict";

  const enhancedRoots = new WeakMap();
  const liveViewers = new Set();
  let galleryController = null;

  const clamp = (value, min, max) => Math.min(max, Math.max(min, value));

  function button(action, label, text) {
    return `<button type="button" class="art-viewer__button" data-action="${action}" aria-label="${label}" title="${label}">${text}</button>`;
  }

  class PanViewer {
    constructor(host, options) {
      this.host = host;
      this.options = Object.assign({
        src: "",
        title: "设计稿",
        mode: "inline",
        initialFit: "width",
        originalNode: null,
        onClose: null,
        onPrevious: null,
        onNext: null,
        navigationText: ""
      }, options || {});
      this.scale = 1;
      this.x = 0;
      this.y = 0;
      this.naturalWidth = 1;
      this.naturalHeight = 1;
      this.fitMode = this.options.initialFit === "window" ? "window" : "width";
      this.drag = null;
      this.destroyed = false;
      this.ready = false;
      this.abort = new AbortController();
      this.build();
      liveViewers.add(this);
    }

    build() {
      const { host, options } = this;
      const title = options.title || "设计稿";
      host.classList.add("art-viewer");
      host.innerHTML = `
        <div class="art-viewer__toolbar" role="toolbar" aria-label="设计稿查看工具">
          <span class="art-viewer__title">${this.escape(title)}</span>
          ${options.onPrevious ? button("previous", "上一张", "‹") : ""}
          ${options.onNext ? button("next", "下一张", "›") : ""}
          ${options.navigationText ? `<span class="art-viewer__position">${this.escape(options.navigationText)}</span>` : ""}
          <span class="art-viewer__spacer"></span>
          ${button("fit-window", "适配窗口", "适窗")}
          ${button("fit-width", "适配宽度", "适宽")}
          ${button("actual", "原始尺寸 100%", "100%")}
          <span class="art-viewer__divider" aria-hidden="true"></span>
          ${button("zoom-out", "缩小", "−")}
          <output class="art-viewer__zoom" aria-label="当前缩放比例">100%</output>
          ${button("zoom-in", "放大", "+")}
          ${button("reset", "重置视图", "重置")}
          <a class="art-viewer__button art-viewer__source" href="${this.escape(options.src)}" target="_blank" rel="noopener" aria-label="在新页面打开原始 SVG" title="在新页面打开原始 SVG">源文件</a>
          ${options.onClose ? button("close", "关闭查看器", "×") : ""}
        </div>
        <div class="art-viewer__viewport" tabindex="0" role="img" aria-label="${this.escape(title)}，可用鼠标拖动平移，使用工具按钮缩放">
          <img class="art-viewer__canvas" src="${this.escape(options.src)}" alt="${this.escape(title)}" draggable="false" loading="${options.mode === "modal" ? "eager" : "lazy"}">
          <div class="art-viewer__loading">正在载入设计稿…</div>
          <div class="art-viewer__hint">${options.mode === "modal" ? "拖动平移 · 滚轮缩放 · 方向键移动" : "拖动平移 · Ctrl/⌘ + 滚轮缩放"}</div>
          <div class="art-viewer__error" hidden>无法载入此 SVG，请使用“源文件”打开。</div>
        </div>`;
      this.viewport = host.querySelector(".art-viewer__viewport");
      this.image = host.querySelector(".art-viewer__canvas");
      this.zoomOutput = host.querySelector(".art-viewer__zoom");
      this.loading = host.querySelector(".art-viewer__loading");
      this.hint = host.querySelector(".art-viewer__hint");
      this.error = host.querySelector(".art-viewer__error");
      const signal = this.abort.signal;
      host.querySelector(".art-viewer__toolbar").addEventListener("click", (event) => this.onToolbar(event), { signal });
      this.viewport.addEventListener("pointerdown", (event) => this.onPointerDown(event), { signal });
      this.viewport.addEventListener("pointermove", (event) => this.onPointerMove(event), { signal });
      this.viewport.addEventListener("pointerup", (event) => this.onPointerUp(event), { signal });
      this.viewport.addEventListener("pointercancel", (event) => this.onPointerUp(event), { signal });
      this.viewport.addEventListener("lostpointercapture", (event) => this.onPointerUp(event), { signal });
      this.viewport.addEventListener("wheel", (event) => this.onWheel(event), { passive: false, signal });
      this.viewport.addEventListener("keydown", (event) => this.onKeyDown(event), { signal });
      this.viewport.addEventListener("dblclick", () => this.fit(this.fitMode), { signal });
      this.image.addEventListener("dragstart", (event) => event.preventDefault(), { signal });
      this.image.addEventListener("load", () => this.onLoad(), { signal });
      this.image.addEventListener("error", () => this.onError(), { signal });
      if (this.image.complete && this.image.naturalWidth > 0) {
        this.onLoad();
      }
      if (global.ResizeObserver) {
        this.resizeObserver = new ResizeObserver(() => this.onResize());
        this.resizeObserver.observe(this.viewport);
      } else {
        global.addEventListener("resize", () => this.onResize(), { signal });
      }
    }

    escape(value) {
      return String(value ?? "").replace(/[&<>"']/g, (char) => ({
        "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;"
      })[char]);
    }

    onLoad() {
      if (this.destroyed || this.ready) return;
      this.ready = true;
      this.naturalWidth = Math.max(1, this.image.naturalWidth || 1);
      this.naturalHeight = Math.max(1, this.image.naturalHeight || 1);
      this.loading.hidden = true;
      this.fit(this.fitMode);
    }

    onError() {
      this.loading.hidden = true;
      this.error.hidden = false;
      this.image.hidden = true;
    }

    onResize() {
      if (!this.ready || this.destroyed) return;
      if (this.fitMode === "free") {
        this.applyTransform();
      } else {
        this.fit(this.fitMode);
      }
    }

    fitWindowScale() {
      return Math.min(
        this.viewport.clientWidth / this.naturalWidth,
        this.viewport.clientHeight / this.naturalHeight
      ) || 1;
    }

    fit(mode) {
      if (!this.ready) {
        this.fitMode = mode === "actual" ? "actual" : mode;
        return;
      }
      this.fitMode = mode;
      const width = this.viewport.clientWidth;
      const height = this.viewport.clientHeight;
      if (!width || !height) return;
      if (mode === "actual") {
        this.scale = 1;
        this.fitMode = "free";
      } else if (mode === "window") {
        this.scale = this.fitWindowScale();
      } else {
        this.scale = width / this.naturalWidth;
        this.fitMode = "width";
      }
      this.x = (width - this.naturalWidth * this.scale) / 2;
      this.y = (height - this.naturalHeight * this.scale) / 2;
      this.applyTransform();
      this.updateButtons();
    }

    zoomAt(factor, clientX, clientY) {
      if (!this.ready) return;
      const rect = this.viewport.getBoundingClientRect();
      const px = clientX == null ? rect.width / 2 : clientX - rect.left;
      const py = clientY == null ? rect.height / 2 : clientY - rect.top;
      const nextScale = clamp(this.scale * factor, Math.min(0.1, this.fitWindowScale()), 4);
      const imageX = (px - this.x) / this.scale;
      const imageY = (py - this.y) / this.scale;
      this.scale = nextScale;
      this.x = px - imageX * nextScale;
      this.y = py - imageY * nextScale;
      this.fitMode = "free";
      this.applyTransform();
      this.updateButtons();
    }

    panBy(dx, dy) {
      if (!this.ready) return;
      this.x += dx;
      this.y += dy;
      this.fitMode = "free";
      this.applyTransform();
      this.updateButtons();
    }

    applyTransform() {
      const viewportWidth = this.viewport.clientWidth;
      const viewportHeight = this.viewport.clientHeight;
      const width = this.naturalWidth * this.scale;
      const height = this.naturalHeight * this.scale;
      const margin = 24;
      const centerX = (viewportWidth - width) / 2;
      const centerY = (viewportHeight - height) / 2;
      this.x = width <= viewportWidth
        ? clamp(this.x, centerX - margin, centerX + margin)
        : clamp(this.x, viewportWidth - width - margin, margin);
      this.y = height <= viewportHeight
        ? clamp(this.y, centerY - margin, centerY + margin)
        : clamp(this.y, viewportHeight - height - margin, margin);
      this.image.style.width = `${width}px`;
      this.image.style.height = `${height}px`;
      this.image.style.transform = `translate(${this.x}px, ${this.y}px)`;
      this.zoomOutput.value = `${Math.round(this.scale * 100)}%`;
      this.zoomOutput.textContent = `${Math.round(this.scale * 100)}%`;
    }

    updateButtons() {
      const pairs = [
        ["fit-window", this.fitMode === "window"],
        ["fit-width", this.fitMode === "width"],
        ["actual", this.fitMode === "free" && Math.abs(this.scale - 1) < 0.001]
      ];
      pairs.forEach(([action, pressed]) => {
        const node = this.host.querySelector(`[data-action="${action}"]`);
        if (node) node.setAttribute("aria-pressed", String(pressed));
      });
    }

    onToolbar(event) {
      const target = event.target instanceof Element ? event.target.closest("[data-action]") : null;
      if (!target) return;
      const action = target.dataset.action;
      if (action === "fit-window") this.fit("window");
      if (action === "fit-width") this.fit("width");
      if (action === "actual") this.fit("actual");
      if (action === "zoom-in") this.zoomAt(1.2);
      if (action === "zoom-out") this.zoomAt(1 / 1.2);
      if (action === "reset") this.fit(this.options.initialFit === "window" ? "window" : "width");
      if (action === "close" && this.options.onClose) this.options.onClose();
      if (action === "previous" && this.options.onPrevious) this.options.onPrevious();
      if (action === "next" && this.options.onNext) this.options.onNext();
    }

    onPointerDown(event) {
      if (!this.ready || event.button !== 0 || event.isPrimary === false) return;
      event.preventDefault();
      this.viewport.focus({ preventScroll: true });
      this.drag = { id: event.pointerId, x: event.clientX, y: event.clientY };
      this.viewport.setPointerCapture(event.pointerId);
      this.host.classList.add("is-dragging");
      this.hint.hidden = true;
    }

    onPointerMove(event) {
      if (!this.drag || event.pointerId !== this.drag.id) return;
      event.preventDefault();
      const dx = event.clientX - this.drag.x;
      const dy = event.clientY - this.drag.y;
      this.drag.x = event.clientX;
      this.drag.y = event.clientY;
      this.panBy(dx, dy);
    }

    onPointerUp(event) {
      if (!this.drag || event.pointerId !== this.drag.id) return;
      if (this.viewport.hasPointerCapture(event.pointerId)) {
        this.viewport.releasePointerCapture(event.pointerId);
      }
      this.drag = null;
      this.host.classList.remove("is-dragging");
    }

    onWheel(event) {
      const shouldZoom = this.options.mode === "modal" || event.ctrlKey || event.metaKey;
      if (!shouldZoom) return;
      event.preventDefault();
      const factor = clamp(Math.exp(-event.deltaY * 0.002), 0.8, 1.25);
      this.zoomAt(factor, event.clientX, event.clientY);
    }

    onKeyDown(event) {
      if (event.ctrlKey || event.metaKey || event.altKey) return;
      const step = event.shiftKey ? 100 : 40;
      const actions = {
        ArrowLeft: () => this.panBy(step, 0),
        ArrowRight: () => this.panBy(-step, 0),
        ArrowUp: () => this.panBy(0, step),
        ArrowDown: () => this.panBy(0, -step),
        "+": () => this.zoomAt(1.2),
        "=": () => this.zoomAt(1.2),
        "-": () => this.zoomAt(1 / 1.2),
        "0": () => this.fit(this.options.initialFit === "window" ? "window" : "width"),
        "1": () => this.fit("actual"),
        "f": () => this.fit("window"),
        "w": () => this.fit("width")
      };
      const action = actions[event.key];
      if (action) {
        event.preventDefault();
        action();
      }
    }

    destroy(restore = true) {
      if (this.destroyed) return;
      this.destroyed = true;
      this.abort.abort();
      if (this.resizeObserver) this.resizeObserver.disconnect();
      liveViewers.delete(this);
      if (restore && this.options.originalNode) {
        this.host.replaceWith(this.options.originalNode);
      } else {
        this.host.innerHTML = "";
      }
    }
  }

  function enhance(root, options = {}) {
    if (!root) return [];
    destroyEnhancements(root);
    const selector = options.selector || 'img[src$=".svg"]';
    const viewers = [];
    root.querySelectorAll(selector).forEach((image) => {
      if (image.closest(".art-viewer")) return;
      const frame = document.createElement("figure");
      frame.className = "art-viewer-frame";
      image.parentNode.insertBefore(frame, image);
      image.remove();
      if (options.inlineHeight) frame.style.setProperty("--art-viewer-inline-height", options.inlineHeight);
      const viewer = new PanViewer(frame, {
        src: image.currentSrc || image.src,
        title: image.alt || "设计稿",
        mode: "inline",
        initialFit: options.initialFit || "width",
        originalNode: image
      });
      viewers.push(viewer);
    });
    enhancedRoots.set(root, viewers);
    return viewers;
  }

  function destroyEnhancements(root) {
    const viewers = enhancedRoots.get(root) || [];
    viewers.forEach((viewer) => viewer.destroy(true));
    enhancedRoots.delete(root);
  }

  function trapDialogTab(event, dialog) {
    if (event.key !== "Tab") return;
    const nodes = Array.from(dialog.querySelectorAll('button, a[href], [tabindex="0"]'))
      .filter((node) => !node.disabled && node.getClientRects().length > 0);
    if (!nodes.length) return;
    const first = nodes[0];
    const last = nodes[nodes.length - 1];
    if (event.shiftKey && document.activeElement === first) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault();
      first.focus();
    }
  }

  function openGallery({ items, index = 0 }) {
    if (!Array.isArray(items) || !items.length) return null;
    if (galleryController) galleryController.close();
    const previousFocus = document.activeElement;
    const previousOverflow = document.documentElement.style.overflow;
    const overlay = document.createElement("div");
    overlay.className = "art-viewer-modal";
    overlay.innerHTML = '<div class="art-viewer-modal__dialog" role="dialog" aria-modal="true" aria-label="设计稿浏览器"></div>';
    document.body.appendChild(overlay);
    document.documentElement.style.overflow = "hidden";
    const dialog = overlay.firstElementChild;
    const controller = {
      items,
      index: Math.round(clamp(Number(index) || 0, 0, items.length - 1)),
      viewer: null,
      closed: false,
      show(nextIndex) {
        if (this.closed) return;
        this.index = (nextIndex + items.length) % items.length;
        if (this.viewer) this.viewer.destroy(false);
        dialog.innerHTML = "";
        const item = items[this.index];
        dialog.setAttribute("aria-label", `${item.title}，第 ${this.index + 1} 张，共 ${items.length} 张`);
        this.viewer = new PanViewer(dialog, {
          src: item.src,
          title: item.title,
          mode: "modal",
          initialFit: "width",
          navigationText: `${this.index + 1} / ${items.length}`,
          onClose: () => this.close(),
          onPrevious: () => this.show(this.index - 1),
          onNext: () => this.show(this.index + 1)
        });
        const viewport = dialog.querySelector(".art-viewer__viewport");
        if (viewport) viewport.focus({ preventScroll: true });
      },
      close() {
        if (this.closed) return;
        this.closed = true;
        if (this.viewer) this.viewer.destroy(false);
        overlay.remove();
        document.documentElement.style.overflow = previousOverflow;
        if (galleryController === controller) galleryController = null;
        if (previousFocus && previousFocus.isConnected) previousFocus.focus({ preventScroll: true });
      }
    };
    galleryController = controller;
    overlay.addEventListener("click", (event) => {
      if (event.target === overlay) controller.close();
    });
    overlay.addEventListener("keydown", (event) => {
      if (event.key === "Escape") {
        event.preventDefault();
        controller.close();
        return;
      }
      if (event.key === "PageDown") {
        event.preventDefault();
        controller.show(controller.index + 1);
        return;
      }
      if (event.key === "PageUp") {
        event.preventDefault();
        controller.show(controller.index - 1);
        return;
      }
      trapDialogTab(event, dialog);
    });
    controller.show(index);
    return controller;
  }

  function closeGallery() {
    if (galleryController) galleryController.close();
  }

  global.WunderArtViewer = {
    enhance,
    destroyEnhancements,
    openGallery,
    closeGallery,
    get activeViewerCount() {
      return liveViewers.size;
    }
  };
})(window);
