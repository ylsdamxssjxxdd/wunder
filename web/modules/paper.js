import { elements } from "./elements.js?v=20260215-01";
import { t } from "./i18n.js?v=20260215-01";

const MAX_TOC_LEVEL = 3;
let rendered = false;

const resolvePaperBaseUrl = (src) => {
  if (!src) {
    return "";
  }
  try {
    return new URL(src, window.location.href).toString();
  } catch (error) {
    return src;
  }
};

const resolveAssetUrl = (href, baseUrl) => {
  if (!href) {
    return "";
  }
  const raw = String(href).trim();
  if (!raw) {
    return "";
  }
  if (/^(?:[a-z]+:|#)/i.test(raw)) {
    return raw;
  }
  if (!baseUrl) {
    return raw;
  }
  try {
    return new URL(raw, baseUrl).toString();
  } catch (error) {
    return raw;
  }
};

const escapeHtml = (value) =>
  String(value ?? "").replace(/[&<>"']/g, (char) => {
    switch (char) {
      case "&":
        return "&amp;";
      case "<":
        return "&lt;";
      case ">":
        return "&gt;";
      case '"':
        return "&quot;";
      case "'":
        return "&#39;";
      default:
        return char;
    }
  });

const stripHtml = (value) => {
  if (!value) {
    return "";
  }
  const text = String(value);
  if (!text.includes("<")) {
    return text;
  }
  const holder = document.createElement("span");
  holder.innerHTML = text;
  return holder.textContent || "";
};

const resolveHeadingPayload = (value, level, parser) => {
  if (value && typeof value === "object") {
    const token = value;
    const depth = Number.isFinite(token.depth)
      ? token.depth
      : Number.isFinite(token.level)
        ? token.level
        : Number.isFinite(level)
          ? level
          : 1;
    let html = "";
    if (parser?.parseInline && Array.isArray(token.tokens)) {
      html = parser.parseInline(token.tokens);
    } else if (typeof token.text === "string") {
      html = globalThis.marked?.parseInline ? globalThis.marked.parseInline(token.text) : token.text;
    } else if (typeof token.raw === "string") {
      html = token.raw;
    } else {
      html = String(token.text ?? "");
    }
    const text = typeof token.text === "string" && token.text.trim() ? token.text : stripHtml(html);
    return { level: depth, html, text };
  }
  const parsedLevel = Number.isFinite(level) ? level : Number.parseInt(level, 10);
  const safeLevel = Number.isFinite(parsedLevel) ? parsedLevel : 1;
  const text = typeof value === "string" ? value : String(value ?? "");
  return { level: safeLevel, html: text, text };
};

const normalizeMarkdown = (raw) => {
  if (!raw) {
    return "";
  }
  const lines = raw.replace(/\r\n/g, "\n").split("\n");
  while (lines.length && !lines[0].trim()) {
    lines.shift();
  }
  while (lines.length && !lines[lines.length - 1].trim()) {
    lines.pop();
  }
  if (!lines.length) {
    return "";
  }
  let minIndent = null;
  lines.forEach((line) => {
    if (!line.trim()) {
      return;
    }
    const match = line.match(/^[ \t]+/);
    const indent = match ? match[0].length : 0;
    minIndent = minIndent === null ? indent : Math.min(minIndent, indent);
  });
  if (!minIndent) {
    return lines.join("\n");
  }
  return lines.map((line) => line.slice(minIndent)).join("\n");
};

const buildToc = (items) => {
  if (!elements.paperToc) {
    return;
  }
  elements.paperToc.textContent = "";
  const visibleItems = items.filter((item) => item.level <= MAX_TOC_LEVEL);
  if (!visibleItems.length) {
    elements.paperToc.textContent = t("paper.toc.empty");
    return;
  }
  const list = document.createElement("ul");
  list.className = "paper-toc-list";
  visibleItems.forEach((item) => {
    const entry = document.createElement("li");
    entry.className = `paper-toc-item level-${item.level}`;
    const link = document.createElement("a");
    link.href = `#${item.anchor}`;
    link.textContent = item.text;
    entry.appendChild(link);
    list.appendChild(entry);
  });
  elements.paperToc.appendChild(list);
};

const renderMarkdown = (markdown, sourceUrl = "") => {
  if (!elements.paperContent) {
    return;
  }
  const renderer = globalThis.marked?.Renderer ? new globalThis.marked.Renderer() : null;
  if (!renderer || !globalThis.marked?.parse) {
    elements.paperContent.textContent = markdown;
    return;
  }
  const baseUrl = resolvePaperBaseUrl(sourceUrl);
  const tocItems = [];
  let headingIndex = 0;
  renderer.heading = function (text, level) {
    headingIndex += 1;
    const anchor = `paper-section-${headingIndex}`;
    const payload = resolveHeadingPayload(text, level, this?.parser);
    const safeLevel = Number.isFinite(payload.level) ? payload.level : 1;
    tocItems.push({ level: safeLevel, text: payload.text, anchor });
    return `<h${safeLevel} id="${anchor}">${payload.html}</h${safeLevel}>`;
  };
  renderer.image = function (href, title, text) {
    const token = href && typeof href === "object" ? href : { href, title, text };
    const src = resolveAssetUrl(token.href, baseUrl);
    const alt = token.text ? escapeHtml(token.text) : "";
    const titleAttr = token.title ? ` title="${escapeHtml(token.title)}"` : "";
    const resolved = escapeHtml(src);
    return `<img src="${resolved}" alt="${alt}"${titleAttr} loading="lazy" />`;
  };
  const html = globalThis.marked.parse(markdown, {
    renderer,
    gfm: true,
    breaks: true,
    mangle: false,
    headerIds: false,
  });
  elements.paperContent.innerHTML = html;
  buildToc(tocItems);
};

const loadMarkdown = async () => {
  const src = elements.paperPanel?.dataset?.paperSrc || "/docs/paper.md";
  const response = await fetch(src, { cache: "no-store" });
  if (!response.ok) {
    throw new Error(`${response.status} ${response.statusText}`);
  }
  const text = await response.text();
  return { text, src };
};

export const initPaperPanel = async () => {
  if (!elements.paperPanel || !elements.paperContent) {
    return false;
  }
  if (rendered) {
    return true;
  }
  elements.paperContent.textContent = t("paper.loading");
  try {
    const { text, src } = await loadMarkdown();
    const markdown = normalizeMarkdown(text);
    renderMarkdown(markdown, src);
    rendered = true;
    return true;
  } catch (error) {
    elements.paperContent.textContent = t("paper.loadFailed", {
      message: error?.message || String(error),
    });
    rendered = false;
    return false;
  }
};


