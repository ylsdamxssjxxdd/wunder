import marked from "/third/marked.esm.js";

const DOCS_BASE = "/docs/";
const MANIFEST_URL = `${DOCS_BASE}manifest.json`;
const SEARCH_URL = `${DOCS_BASE}search.json`;
const THEME_KEY = "wunder-docs-theme";
const SIDEBAR_SCROLL_KEY = "wunder-docs-sidebar-scroll";

// Save sidebar scroll position before navigating to a new page
const saveSidebarScroll = () => {
  try {
    sessionStorage.setItem(SIDEBAR_SCROLL_KEY, String(elements.sidebar?.scrollTop || 0));
  } catch (e) { /* ignore */ }
};

// Restore sidebar scroll position after page load
const restoreSidebarScroll = () => {
  if (!elements.sidebar) return;
  try {
    const saved = sessionStorage.getItem(SIDEBAR_SCROLL_KEY);
    if (saved) elements.sidebar.scrollTop = parseInt(saved, 10) || 0;
  } catch (e) { /* ignore */ }
};

const THEME_ICONS = {
  light: `
    <svg viewBox="0 0 20 20" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
      <circle cx="10" cy="10" r="3.5"></circle>
      <path d="M10 1.8v2.1M10 16.1v2.1M18.2 10h-2.1M3.9 10H1.8M15.8 4.2l-1.5 1.5M5.7 14.3l-1.5 1.5M15.8 15.8l-1.5-1.5M5.7 5.7 4.2 4.2"></path>
    </svg>
  `,
  dark: `
    <svg viewBox="0 0 20 20" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
      <path d="M15.8 11.7A6.9 6.9 0 0 1 8.3 4.2a7.2 7.2 0 1 0 7.5 7.5Z"></path>
    </svg>
  `,
};

const elements = {
  tabs: document.getElementById("docs-tabs"),
  sidebar: document.getElementById("docs-sidebar"),
  pageHeader: document.getElementById("docs-page-header"),
  content: document.getElementById("docs-content"),
  pageFooter: document.getElementById("docs-page-footer"),
  toc: document.getElementById("docs-toc"),
  tocWrap: document.getElementById("docs-toc-wrap"),
  searchInput: document.getElementById("docs-search-input"),
  searchResults: document.getElementById("docs-search-results"),
  themeToggle: document.getElementById("docs-theme-toggle"),
  languageSwitcher: document.getElementById("docs-language-switcher"),
};

const state = {
  manifest: null,
  searchIndex: null,
  pageData: null,
  pageUrlMap: new Map(),
  tocObserver: null,
  navigationToken: 0,
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
  const holder = document.createElement("span");
  holder.innerHTML = String(value ?? "");
  return holder.textContent || "";
};

const readPageData = () => {
  const node = document.getElementById("docs-page-data");
  if (!node) {
    return null;
  }
  try {
    return JSON.parse(node.textContent || "{}");
  } catch (error) {
    return null;
  }
};

const fetchJson = async (url) => {
  const response = await fetch(url, { cache: "no-store" });
  if (!response.ok) {
    throw new Error(`${response.status} ${response.statusText}`);
  }
  return response.json();
};

const isExternalLink = (href) => /^https?:\/\//i.test(String(href || ""));

const normalizeDocsPath = (pathname) => {
  const path = String(pathname || "").trim() || DOCS_BASE;
  if (!path.startsWith(DOCS_BASE)) {
    return path;
  }
  if (path === DOCS_BASE.slice(0, -1)) {
    return DOCS_BASE;
  }
  if (path.endsWith("/")) {
    return path;
  }
  const tail = path.split("/").pop() || "";
  return tail.includes(".") ? path : `${path}/`;
};

const mergeCurrentSearchParams = (targetUrl) => {
  const merged = new URL(targetUrl.toString());
  const currentUrl = new URL(window.location.href);
  currentUrl.searchParams.forEach((value, key) => {
    if (!merged.searchParams.has(key)) {
      merged.searchParams.set(key, value);
    }
  });
  return merged;
};

const decorateDocsHref = (href) => {
  const raw = String(href || "").trim();
  if (!raw || raw.startsWith("#")) {
    return raw;
  }
  try {
    const resolved = new URL(raw, window.location.href);
    if (resolved.origin !== window.location.origin) {
      return raw;
    }
    if (!normalizeDocsPath(resolved.pathname).startsWith(DOCS_BASE)) {
      return raw;
    }
    const merged = mergeCurrentSearchParams(resolved);
    return `${merged.pathname}${merged.search}${merged.hash}`;
  } catch (error) {
    return raw;
  }
};

const rebuildPageUrlMap = () => {
  const pageUrlMap = new Map();
  Object.values(state.manifest?.pages || {}).forEach((page) => {
    pageUrlMap.set(normalizeDocsPath(page.url), page.slug);
  });
  state.pageUrlMap = pageUrlMap;
};

const getPageByUrl = (input) => {
  if (!state.manifest) {
    return null;
  }
  try {
    const baseUrl = input instanceof URL ? input : new URL(String(input), window.location.href);
    if (!/^https?:$/i.test(baseUrl.protocol)) {
      return null;
    }
    if (baseUrl.origin !== window.location.origin) {
      return null;
    }
    const mergedUrl = mergeCurrentSearchParams(baseUrl);
    const slug = state.pageUrlMap.get(normalizeDocsPath(mergedUrl.pathname));
    if (!slug) {
      return null;
    }
    const page = getPageBySlug(slug);
    return page ? { page, url: mergedUrl } : null;
  } catch (error) {
    return null;
  }
};

const parsePageSnapshot = (html) => {
  const documentSnapshot = new DOMParser().parseFromString(String(html || ""), "text/html");
  const pageDataNode = documentSnapshot.getElementById("docs-page-data");
  if (!pageDataNode) {
    return null;
  }
  try {
    return {
      pageData: JSON.parse(pageDataNode.textContent || "{}"),
      title: documentSnapshot.title || "",
      description:
        documentSnapshot.querySelector('meta[name="description"]')?.getAttribute("content") || "",
    };
  } catch (error) {
    return null;
  }
};

const loadPageSnapshot = async (url) => {
  const response = await fetch(url.toString(), { cache: "no-store" });
  if (!response.ok) {
    throw new Error(`${response.status} ${response.statusText}`);
  }
  const snapshot = parsePageSnapshot(await response.text());
  if (!snapshot?.pageData) {
    throw new Error("invalid docs page snapshot");
  }
  return snapshot;
};

const syncPageDataNode = () => {
  let node = document.getElementById("docs-page-data");
  if (!node) {
    node = document.createElement("script");
    node.id = "docs-page-data";
    node.type = "application/json";
    document.body.appendChild(node);
  }
  node.textContent = JSON.stringify(state.pageData || {});
};

const syncDocumentMetadata = (snapshot) => {
  document.documentElement.lang = state.pageData?.language || document.documentElement.lang || "zh-CN";
  if (snapshot?.title) {
    document.title = snapshot.title;
  }
  const description = snapshot?.description ?? state.pageData?.summary ?? "";
  const metaDescription = document.querySelector('meta[name="description"]');
  if (metaDescription) {
    metaDescription.setAttribute("content", description);
  }
};

const disconnectTocObserver = () => {
  if (state.tocObserver) {
    state.tocObserver.disconnect();
    state.tocObserver = null;
  }
};

const runWithoutSmoothScroll = (action) => {
  const root = document.documentElement;
  const previous = root.style.scrollBehavior;
  root.style.scrollBehavior = "auto";
  try {
    action();
  } finally {
    if (previous) {
      root.style.scrollBehavior = previous;
    } else {
      root.style.removeProperty("scroll-behavior");
    }
  }
};

const scrollToPageAnchor = (hash, { fallbackToTop = false } = {}) => {
  const targetHash = String(hash || "").trim();
  if (!targetHash) {
    if (fallbackToTop) {
      runWithoutSmoothScroll(() => window.scrollTo(0, 0));
    }
    return;
  }
  const targetId = decodeURIComponent(targetHash.replace(/^#/, ""));
  const target = document.getElementById(targetId);
  if (target) {
    runWithoutSmoothScroll(() => target.scrollIntoView({ block: "start" }));
    return;
  }
  if (fallbackToTop) {
    runWithoutSmoothScroll(() => window.scrollTo(0, 0));
  }
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
      html = marked.parseInline(token.text);
    } else if (typeof token.raw === "string") {
      html = token.raw;
    } else {
      html = String(token.text ?? "");
    }
    const text = typeof token.text === "string" && token.text.trim() ? token.text : stripHtml(html);
    return { level: depth, html, text };
  }
  return {
    level: Number.isFinite(level) ? level : Number.parseInt(level, 10) || 1,
    html: String(value ?? ""),
    text: stripHtml(value),
  };
};

const applyTheme = (theme) => {
  const nextTheme = theme === "dark" ? "dark" : "light";
  document.documentElement.dataset.theme = nextTheme;
  if (elements.themeToggle) {
    elements.themeToggle.innerHTML = nextTheme === "dark" ? THEME_ICONS.dark : THEME_ICONS.light;
    const label = nextTheme === "dark" ? "切换到浅色模式" : "切换到深色模式";
    elements.themeToggle.setAttribute("aria-label", label);
    elements.themeToggle.setAttribute("title", label);
    elements.themeToggle.setAttribute("aria-pressed", String(nextTheme === "dark"));
  }
};

const initializeTheme = () => {
  let storedTheme = "light";
  try {
    storedTheme = localStorage.getItem(THEME_KEY) || "light";
  } catch (error) {
    storedTheme = "light";
  }
  applyTheme(storedTheme);
  elements.themeToggle?.addEventListener("click", () => {
    const currentTheme = document.documentElement.dataset.theme === "dark" ? "dark" : "light";
    const nextTheme = currentTheme === "dark" ? "light" : "dark";
    applyTheme(nextTheme);
    try {
      localStorage.setItem(THEME_KEY, nextTheme);
    } catch (error) {
      // ignore storage failures
    }
  });
};

const getPageBySlug = (slug) => state.manifest?.pages?.[slug] || null;

const getCurrentLanguageNavigation = () => {
  const language = state.pageData?.language;
  return state.manifest?.navigation?.languages?.find((item) => item.language === language) || null;
};

const renderTabs = () => {
  if (!elements.tabs) {
    return;
  }
  const language = getCurrentLanguageNavigation();
  if (!language?.tabs?.length) {
    elements.tabs.innerHTML = "";
    return;
  }
  elements.tabs.innerHTML = language.tabs
    .map((tab) => {
      const isActive = tab.tab === state.pageData.tab;
      const className = isActive ? "docs-tab is-active" : "docs-tab";
      const href = decorateDocsHref(tab.entry_url);
      return `<a class="${className}" href="${escapeHtml(href)}"${isActive ? ' aria-current="page"' : ""}>${escapeHtml(tab.tab)}</a>`;
    })
    .join("");
};

const renderSidebar = () => {
  if (!elements.sidebar) {
    return;
  }
  const language = getCurrentLanguageNavigation();
  const activeTab = language?.tabs?.find((item) => item.tab === state.pageData.tab) || language?.tabs?.[0];
  if (!activeTab) {
    elements.sidebar.innerHTML = "";
    return;
  }
  elements.sidebar.innerHTML = activeTab.groups
    .map((group) => {
      const pageLinks = (group.pages || [])
        .map((page) => {
          const isActive = page.slug === state.pageData.slug;
          const className = isActive ? "docs-sidebar-link is-active" : "docs-sidebar-link";
          const href = decorateDocsHref(page.url);
          return `<a class="${className}" href="${escapeHtml(href)}"${isActive ? ' aria-current="page"' : ""}>${escapeHtml(page.title)}</a>`;
        })
        .join("");
      return `
        <section class="docs-sidebar-group">
          <div class="docs-sidebar-group-title">${escapeHtml(group.group)}</div>
          <div class="docs-sidebar-group-links">${pageLinks}</div>
        </section>
      `;
    })
    .join("");

  restoreSidebarScroll();
};

const renderLanguageSwitcher = () => {
  if (!elements.languageSwitcher) {
    return;
  }
  const currentLanguage = state.pageData.language;
  const languages = state.manifest?.navigation?.languages || [];
  if (languages.length < 2) {
    elements.languageSwitcher.innerHTML = "";
    return;
  }

  const otherLanguage = languages.find((lang) => lang.language !== currentLanguage);
  if (!otherLanguage) {
    elements.languageSwitcher.innerHTML = "";
    return;
  }

  const currentPrefix = `${currentLanguage}/`;
  const targetSlug = state.pageData.slug.startsWith(currentPrefix)
    ? `${otherLanguage.language}/${state.pageData.slug.slice(currentPrefix.length)}`
    : state.manifest?.site?.home_page;
  const targetPage = getPageBySlug(targetSlug) || getPageBySlug(state.manifest?.site?.home_page);

  const shortLabel = otherLanguage.language === "zh-CN" ? "zh" : "en";
  const targetHref = decorateDocsHref(targetPage?.url || DOCS_BASE);

  elements.languageSwitcher.innerHTML = `
    <a class="docs-language-toggle" href="${escapeHtml(targetHref)}" title="${escapeHtml(otherLanguage.label)}">
      <span class="docs-language-toggle-icon" aria-hidden="true">
        <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round">
          <circle cx="8" cy="8" r="6.5"/>
          <path d="M1 8h14M8 1a12 12 0 0 0 0 14 12 12 0 0 0 0-14"/>
        </svg>
      </span>
      <span class="docs-language-toggle-text">${escapeHtml(shortLabel)}</span>
    </a>
  `;
};

const renderPageHeader = () => {
  if (!elements.pageHeader) {
    return;
  }
  elements.pageHeader.innerHTML = `
    <div class="docs-page-kicker">${escapeHtml(state.pageData.tab)}</div>
    <h1>${escapeHtml(state.pageData.title)}</h1>
    ${state.pageData.updated_at ? `<div class="docs-page-updated">最后更新：${escapeHtml(state.pageData.updated_at)}</div>` : ""}
    ${state.pageData.summary ? `<p class="docs-page-summary">${escapeHtml(state.pageData.summary)}</p>` : ""}
  `;
};

const postProcessContent = () => {
  if (!elements.content) {
    return;
  }
  elements.content.querySelectorAll("a").forEach((anchor) => {
    const href = anchor.getAttribute("href") || "";
    if (isExternalLink(href)) {
      anchor.setAttribute("target", "_blank");
      anchor.setAttribute("rel", "noopener noreferrer");
      return;
    }
    if (!href.startsWith("#")) {
      anchor.setAttribute("href", decorateDocsHref(href));
    }
  });
};

const renderContent = async () => {
  if (!elements.content) {
    return;
  }
  const renderer = new marked.Renderer();
  let headingIndex = 0;
  renderer.heading = function (text, level) {
    const payload = resolveHeadingPayload(text, level, this?.parser);
    const currentHeading = state.pageData.headings?.[headingIndex] || null;
    headingIndex += 1;
    const headingId = currentHeading?.id || `section-${headingIndex}`;
    const safeLevel = Number.isFinite(currentHeading?.level) ? currentHeading.level : payload.level;
    return `<h${safeLevel} id="${escapeHtml(headingId)}">${payload.html}</h${safeLevel}>`;
  };
  const htmlContent = marked.parse(state.pageData.markdown || "", {
    renderer,
    gfm: true,
    breaks: false,
    mangle: false,
    headerIds: false,
  });
  elements.content.innerHTML = htmlContent;
  postProcessContent();
};

const renderToc = () => {
  if (!elements.toc || !elements.tocWrap) {
    return;
  }
  const headings = (state.pageData.headings || []).filter((item) => Number(item.level) <= 3);
  if (!headings.length) {
    elements.toc.innerHTML = '<div class="docs-toc-empty">当前页没有目录</div>';
    return;
  }
  elements.toc.innerHTML = headings
    .map(
      (item) =>
        `<a class="docs-toc-link level-${Number(item.level)}" href="#${escapeHtml(item.id)}">${escapeHtml(item.text)}</a>`,
    )
    .join("");
};

const bindDocsNavigation = () => {
  document.addEventListener("click", (event) => {
    const anchor = event.target instanceof Element ? event.target.closest("a[href]") : null;
    if (!anchor) {
      return;
    }
    if (
      event.defaultPrevented ||
      event.button !== 0 ||
      event.metaKey ||
      event.ctrlKey ||
      event.shiftKey ||
      event.altKey
    ) {
      return;
    }
    const href = anchor.getAttribute("href") || "";
    if (!href) {
      return;
    }
    if (href.startsWith("#")) {
      const nextHash = href === "#" ? "" : href;
      if (nextHash === window.location.hash) {
        scrollToPageAnchor(nextHash, { fallbackToTop: true });
        event.preventDefault();
        return;
      }
      history.pushState({ hash: nextHash }, "", nextHash || window.location.pathname + window.location.search);
      scrollToPageAnchor(nextHash, { fallbackToTop: true });
      event.preventDefault();
      return;
    }
    const targetAttr = String(anchor.getAttribute("target") || "").trim().toLowerCase();
    if (targetAttr && targetAttr !== "_self") {
      return;
    }
    const target = getPageByUrl(anchor.href);
    if (!target) {
      return;
    }
    event.preventDefault();
    navigateToPage(target.url, { saveScroll: true }).catch(() => {
      window.location.href = target.url.toString();
    });
  });

  window.addEventListener("popstate", () => {
    const currentUrl = new URL(window.location.href);
    const target = getPageByUrl(currentUrl);
    if (!target) {
      window.location.reload();
      return;
    }
    if (
      target.page.slug === state.pageData?.slug &&
      normalizeDocsPath(currentUrl.pathname) === normalizeDocsPath(window.location.pathname)
    ) {
      scrollToPageAnchor(currentUrl.hash, { fallbackToTop: true });
      return;
    }
    navigateToPage(target.url, {
      replaceHistory: true,
      saveScroll: false,
      preserveSidebarScroll: true,
      isPopState: true,
    }).catch(() => {
      window.location.reload();
    });
  });
};

const bindTocObserver = () => {
  disconnectTocObserver();
  if (!elements.content || !elements.toc) {
    return;
  }
  const headings = Array.from(elements.content.querySelectorAll("h1, h2, h3"));
  if (!headings.length || !("IntersectionObserver" in window)) {
    return;
  }
  const links = new Map();
  elements.toc.querySelectorAll(".docs-toc-link").forEach((link) => {
    links.set(link.getAttribute("href")?.replace(/^#/, ""), link);
  });
  const activate = (id) => {
    links.forEach((link, key) => {
      link.classList.toggle("is-active", key === id);
    });
  };
  const observer = new IntersectionObserver(
    (entries) => {
      const visibleEntry = entries
        .filter((entry) => entry.isIntersecting)
        .sort((left, right) => left.boundingClientRect.top - right.boundingClientRect.top)[0];
      if (visibleEntry?.target?.id) {
        activate(visibleEntry.target.id);
      }
    },
    {
      rootMargin: "-15% 0px -70% 0px",
      threshold: [0, 1],
    },
  );
  state.tocObserver = observer;
  headings.forEach((heading) => observer.observe(heading));
  if (headings[0]?.id) {
    activate(headings[0].id);
  }
};

const renderPageFooter = () => {
  if (!elements.pageFooter) {
    return;
  }
  const previousPage = getPageBySlug(state.pageData.prev_slug);
  const nextPage = getPageBySlug(state.pageData.next_slug);
  const cards = [];
  if (previousPage) {
    const previousHref = decorateDocsHref(previousPage.url);
    cards.push(`
      <a class="docs-pagination-card is-prev" href="${escapeHtml(previousHref)}">
        <span class="docs-pagination-label">上一篇</span>
        <strong>${escapeHtml(previousPage.title)}</strong>
      </a>
    `);
  }
  if (nextPage) {
    const nextHref = decorateDocsHref(nextPage.url);
    cards.push(`
      <a class="docs-pagination-card is-next" href="${escapeHtml(nextHref)}">
        <span class="docs-pagination-label">下一篇</span>
        <strong>${escapeHtml(nextPage.title)}</strong>
      </a>
    `);
  }
  elements.pageFooter.innerHTML = cards.join("");
};

const renderPage = async () => {
  renderTabs();
  renderSidebar();
  renderLanguageSwitcher();
  renderPageHeader();
  await renderContent();
  renderToc();
  renderPageFooter();
  bindTocObserver();
};

const navigateToPage = async (
  targetUrl,
  {
    replaceHistory = false,
    saveScroll = true,
    preserveSidebarScroll = false,
    isPopState = false,
  } = {},
) => {
  if (!state.manifest) {
    window.location.href = targetUrl.toString();
    return;
  }
  const resolvedUrl = mergeCurrentSearchParams(targetUrl instanceof URL ? targetUrl : new URL(targetUrl, window.location.href));
  const resolvedPath = normalizeDocsPath(resolvedUrl.pathname);
  const currentPath = normalizeDocsPath(window.location.pathname);
  if (resolvedPath === currentPath && resolvedUrl.hash !== window.location.hash) {
    if (!isPopState) {
      history.pushState({ slug: state.pageData?.slug || "", hash: resolvedUrl.hash }, "", resolvedUrl);
    }
    scrollToPageAnchor(resolvedUrl.hash, { fallbackToTop: true });
    return;
  }

  const target = getPageByUrl(resolvedUrl);
  if (!target) {
    window.location.href = resolvedUrl.toString();
    return;
  }
  if (
    target.page.slug === state.pageData?.slug &&
    resolvedUrl.hash === window.location.hash &&
    normalizeDocsPath(resolvedUrl.pathname) === normalizeDocsPath(window.location.pathname)
  ) {
    return;
  }

  const token = (state.navigationToken += 1);
  const sidebarScrollTop = elements.sidebar?.scrollTop ?? 0;
  if (saveScroll) {
    saveSidebarScroll();
  }

  let snapshot;
  try {
    snapshot = await loadPageSnapshot(resolvedUrl);
  } catch (error) {
    throw error;
  }
  if (token !== state.navigationToken) {
    return;
  }

  state.pageData = snapshot.pageData;
  syncPageDataNode();
  syncDocumentMetadata(snapshot);
  await renderPage();

  if (preserveSidebarScroll && elements.sidebar) {
    elements.sidebar.scrollTop = sidebarScrollTop;
  } else {
    restoreSidebarScroll();
  }

  hideSearchResults();
  if (elements.searchInput) {
    elements.searchInput.blur();
  }
  if (!isPopState) {
    const method = replaceHistory ? "replaceState" : "pushState";
    history[method]({ slug: state.pageData.slug, hash: resolvedUrl.hash }, "", resolvedUrl);
  } else if (replaceHistory) {
    history.replaceState({ slug: state.pageData.slug, hash: resolvedUrl.hash }, "", resolvedUrl);
  }
  scrollToPageAnchor(resolvedUrl.hash, { fallbackToTop: true });
};

const scoreSearchResult = (entry, keyword) => {
  const query = keyword.toLowerCase();
  let score = 0;
  const title = String(entry.title || "").toLowerCase();
  const summary = String(entry.summary || "").toLowerCase();
  const headings = Array.isArray(entry.headings) ? entry.headings.join(" ").toLowerCase() : "";
  const text = String(entry.text || "").toLowerCase();
  if (title === query) {
    score += 200;
  } else if (title.startsWith(query)) {
    score += 120;
  } else if (title.includes(query)) {
    score += 80;
  }
  if (summary.includes(query)) {
    score += 40;
  }
  if (headings.includes(query)) {
    score += 35;
  }
  if (text.includes(query)) {
    score += 15;
  }
  return score;
};

const ensureSearchIndex = async () => {
  if (state.searchIndex) {
    return state.searchIndex;
  }
  state.searchIndex = await fetchJson(SEARCH_URL);
  return state.searchIndex;
};

const hideSearchResults = () => {
  if (elements.searchResults) {
    elements.searchResults.hidden = true;
    elements.searchResults.innerHTML = "";
  }
};

const renderSearchResults = async (keyword) => {
  if (!elements.searchResults) {
    return;
  }
  const query = String(keyword || "").trim();
  if (!query) {
    hideSearchResults();
    return;
  }
  const entries = await ensureSearchIndex();
  const results = (entries || [])
    .map((entry) => ({ ...entry, score: scoreSearchResult(entry, query) }))
    .filter((entry) => entry.score > 0)
    .sort((left, right) => right.score - left.score)
    .slice(0, 8);
  if (!results.length) {
    elements.searchResults.hidden = false;
    elements.searchResults.innerHTML = '<div class="docs-search-empty">没有找到匹配页面</div>';
    return;
  }
  elements.searchResults.hidden = false;
  elements.searchResults.innerHTML = results
    .map(
      (entry) => `
        <a class="docs-search-result" href="${escapeHtml(decorateDocsHref(entry.url))}">
          <strong>${escapeHtml(entry.title)}</strong>
          <span>${escapeHtml(entry.tab)} / ${escapeHtml(entry.group)}</span>
          ${entry.summary ? `<em>${escapeHtml(entry.summary)}</em>` : ""}
        </a>
      `,
    )
    .join("");
};

const bindSearch = () => {
  if (!elements.searchInput) {
    return;
  }
  elements.searchInput.addEventListener("input", () => {
    renderSearchResults(elements.searchInput.value).catch(() => hideSearchResults());
  });
  elements.searchInput.addEventListener("focus", () => {
    if (elements.searchInput.value.trim()) {
      renderSearchResults(elements.searchInput.value).catch(() => hideSearchResults());
    }
  });
  elements.searchInput.addEventListener("keydown", (event) => {
    if (event.key === "Escape") {
      hideSearchResults();
      elements.searchInput.blur();
    }
  });
  document.addEventListener("keydown", (event) => {
    if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "k") {
      event.preventDefault();
      elements.searchInput.focus();
      elements.searchInput.select();
    }
  });
  document.addEventListener("click", (event) => {
    if (!elements.searchResults?.contains(event.target) && event.target !== elements.searchInput) {
      hideSearchResults();
    }
  });
};

const bootstrap = async () => {
  state.pageData = readPageData();
  if (!state.pageData) {
    return;
  }
  initializeTheme();
  bindSearch();
  bindDocsNavigation();
  try {
    state.manifest = await fetchJson(MANIFEST_URL);
  } catch (error) {
    renderPageHeader();
    await renderContent();
    renderToc();
    bindTocObserver();
    return;
  }
  rebuildPageUrlMap();
  syncPageDataNode();
  syncDocumentMetadata({
    title: document.title,
    description: document.querySelector('meta[name="description"]')?.getAttribute("content") || "",
  });
  await renderPage();
  history.replaceState(
    { slug: state.pageData.slug, hash: window.location.hash },
    "",
    mergeCurrentSearchParams(new URL(window.location.href)),
  );
  scrollToPageAnchor(window.location.hash);
};

bootstrap();
