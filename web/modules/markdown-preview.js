const BROKEN_TABLE_DIVIDER_REGEX = /^[\s|:-]+$/;
export const normalizePreviewExternalUrl = (value = "") => String(value || "").trim();

const splitPreviewTableRow = (row = "") =>
  String(row || "")
    .replace(/^\|/, "")
    .replace(/\|$/, "")
    .split("|")
    .map((cell) => cell.trim());

const looksLikePreviewDividerRow = (row = "") => {
  const trimmed = String(row || "").trim();
  const pipeCount = (trimmed.match(/\|/g) || []).length;
  if (
    !trimmed ||
    !trimmed.includes("|") ||
    !trimmed.includes("-") ||
    (pipeCount < 2 && !(trimmed.startsWith("|") && trimmed.endsWith("|")))
  ) {
    return false;
  }
  return BROKEN_TABLE_DIVIDER_REGEX.test(trimmed);
};

const looksLikePreviewTableRow = (row = "") => {
  const trimmed = String(row || "").trim();
  if (!trimmed.includes("|")) return false;
  const pipeCount = (trimmed.match(/\|/g) || []).length;
  if (pipeCount < 2 && !(trimmed.startsWith("|") && trimmed.endsWith("|"))) return false;
  if (looksLikePreviewDividerRow(trimmed)) return false;
  return splitPreviewTableRow(trimmed).length >= 2;
};

const preserveMalformedPreviewTables = (content = "") => {
  if (!content.includes("|")) return content;
  const lines = content.split("\n");
  let activeFence = "";
  for (let index = 0; index < lines.length; index += 1) {
    const trimmed = lines[index].trim();
    const fenceMatch = trimmed.match(/^([`~]{3,})/);
    if (fenceMatch) {
      const marker = fenceMatch[1];
      if (!activeFence) {
        activeFence = marker;
      } else if (marker[0] === activeFence[0] && marker.length >= activeFence.length) {
        activeFence = "";
      }
      continue;
    }
    if (activeFence || !isMalformedPreviewTableStart(lines, index)) continue;
    let end = index + 1;
    while (end + 1 < lines.length && looksLikePreviewTableContinuation(lines[end + 1])) {
      end += 1;
    }
    for (let row = index; row <= end; row += 1) {
      lines[row] = escapePreviewMarkdownLiteralLine(lines[row]);
    }
    index = end;
  }
  return lines.join("\n");
};

const isMalformedPreviewTableStart = (lines, index) => {
  const headerRow = String(lines[index] || "").trim();
  if (!looksLikePreviewTableRow(headerRow)) return false;
  const previousLine = String(lines[index - 1] || "").trim();
  if (looksLikePreviewDividerRow(previousLine) || looksLikePreviewTableRow(previousLine)) {
    return false;
  }
  const headerCells = splitPreviewTableRow(headerRow);
  if (headerCells.length < 2 || headerCells.every((cell) => !cell)) return false;
  const nextLine = String(lines[index + 1] || "").trim();
  if (!nextLine) return false;
  if (looksLikePreviewDividerRow(nextLine)) {
    return splitPreviewTableRow(nextLine).length !== headerCells.length;
  }
  return looksLikePreviewTableRow(nextLine);
};

const looksLikePreviewTableContinuation = (row = "") => {
  const trimmed = String(row || "").trim();
  if (!trimmed) return false;
  return looksLikePreviewDividerRow(trimmed) || looksLikePreviewTableRow(trimmed);
};

const escapePreviewMarkdownLiteralLine = (line = "") =>
  String(line || "")
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/\\/g, "\\\\")
    .replace(/([`*_{}\[\]()#+.!|~-])/g, "\\$1");

export const normalizeMarkdownForWebPreview = (content = "") =>
  preserveMalformedPreviewTables(normalizePreviewExternalUrl(String(content || "").replace(/\r\n/g, "\n")));

const buildPreviewImageFallback = (src = "", alt = "") => {
  const fallback = document.createElement("span");
  fallback.className = "markdown-preview-raw-fallback";
  fallback.textContent = `![${String(alt || "").trim()}](${String(src || "").trim()})`;
  return fallback;
};

const enhancePreviewTables = (container) => {
  container.querySelectorAll("table").forEach((table) => {
    if (table.closest(".markdown-preview-table")) {
      return;
    }
    const wrapper = document.createElement("div");
    wrapper.className = "markdown-preview-table";
    table.parentNode?.insertBefore(wrapper, table);
    wrapper.appendChild(table);
  });
};

const enhancePreviewLinks = (container) => {
  container.querySelectorAll("a[href]").forEach((link) => {
    const href = normalizePreviewExternalUrl(link.getAttribute("href") || "");
    if (!href || !/^https?:\/\//i.test(href)) {
      return;
    }
    link.setAttribute("href", href);
    link.setAttribute("target", "_blank");
    link.setAttribute("rel", "noreferrer noopener");
  });
};

const enhancePreviewImages = (container) => {
  container.querySelectorAll("img").forEach((img) => {
    if (img.dataset.previewBound === "true") {
      return;
    }
    const rawSrc = img.getAttribute("src") || "";
    const normalizedSrc = normalizePreviewExternalUrl(rawSrc);
    img.dataset.previewBound = "true";
    img.loading = "lazy";
    img.decoding = "async";
    img.referrerPolicy = "no-referrer";
    if (normalizedSrc && normalizedSrc !== rawSrc) {
      img.setAttribute("src", normalizedSrc);
    }
    img.addEventListener(
      "error",
      () => {
        const fallback = buildPreviewImageFallback(
          normalizedSrc || img.currentSrc || img.src || "",
          String(img.getAttribute("alt") || "").trim()
        );
        img.replaceWith(fallback);
      },
      { once: true }
    );
  });
};

export const enhanceRenderedMarkdown = (container) => {
  if (!container) {
    return;
  }
  enhancePreviewTables(container);
  enhancePreviewLinks(container);
  enhancePreviewImages(container);
};
