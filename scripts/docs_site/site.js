const DOCS_BASE = "/docs/";
const MANIFEST_URL = `${DOCS_BASE}manifest.json`;
const SEARCH_URL = `${DOCS_BASE}search.json`;
const THEME_KEY = "wunder-docs-theme";

const elements = {
  tabs: document.getElementById("docs-tabs"),
  sidebar: document.getElementById("docs-sidebar"),
  breadcrumbs: document.getElementById("docs-breadcrumbs"),
  pageHeader: document.getElementById("docs-page-header"),
  content: document.getElementById("docs-content"),
  pageFooter: document.getElementById("docs-page-footer"),
  toc: document.getElementById("docs-toc"),
  tocWrap: document.getElementById("docs-toc-wrap"),
  searchInput: document.getElementById("docs-search-input"),
  searchResults: document.getElementById("docs-search-results"),
  themeToggle: document.getElementById("docs-theme-toggle"),
  languageSwitcher: document.getElementById("docs-language-switcher"),
  sidebarToggle: document.getElementById("docs-sidebar-toggle"),
};

const state = {
  manifest: null,
  searchIndex: null,
  pageData: null,
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

const waitForMarked = async (timeoutMs = 3000) => {
  const startedAt = Date.now();
  while (Date.now() - startedAt <= timeoutMs) {
    if (globalThis.marked?.parse && globalThis.marked?.Renderer) {
      return globalThis.marked;
    }
    await new Promise((resolve) => setTimeout(resolve, 30));
  }
  return null;
};

const isExternalLink = (href) => /^https?:\/\//i.test(String(href || ""));

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
    elements.themeToggle.textContent = nextTheme === "dark" ? "浅色" : "深色";
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
      return `<a class="${className}" href="${escapeHtml(tab.entry_url)}">${escapeHtml(tab.tab)}</a>`;
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
          const className = page.slug === state.pageData.slug ? "docs-sidebar-link is-active" : "docs-sidebar-link";
          return `<a class="${className}" href="${escapeHtml(page.url)}">${escapeHtml(page.title)}</a>`;
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
};

const renderLanguageSwitcher = () => {
  if (!elements.languageSwitcher) {
    return;
  }
  const currentLanguage = state.pageData.language;
  const languages = state.manifest?.navigation?.languages || [];
  if (!languages.length) {
    elements.languageSwitcher.innerHTML = "";
    return;
  }
  elements.languageSwitcher.innerHTML = languages
    .map((language) => {
      const isActive = language.language === currentLanguage;
      const currentPrefix = `${currentLanguage}/`;
      const targetSlug = state.pageData.slug.startsWith(currentPrefix)
        ? `${language.language}/${state.pageData.slug.slice(currentPrefix.length)}`
        : state.manifest?.site?.home_page;
      const targetPage = getPageBySlug(targetSlug) || getPageBySlug(state.manifest?.site?.home_page);
      const className = isActive ? "docs-language-chip is-active" : "docs-language-chip";
      return `<a class="${className}" href="${escapeHtml(targetPage?.url || DOCS_BASE)}">${escapeHtml(language.label)}</a>`;
    })
    .join("");
};

const renderBreadcrumbs = () => {
  if (!elements.breadcrumbs) {
    return;
  }
  const items = [
    { label: "文档", url: DOCS_BASE },
    { label: state.pageData.tab, url: null },
    { label: state.pageData.group, url: null },
    { label: state.pageData.title, url: null },
  ];
  elements.breadcrumbs.innerHTML = items
    .map((item, index) => {
      const isLast = index === items.length - 1;
      if (isLast || !item.url) {
        return `<span class="docs-breadcrumb-item is-current">${escapeHtml(item.label)}</span>`;
      }
      return `<a class="docs-breadcrumb-item" href="${escapeHtml(item.url)}">${escapeHtml(item.label)}</a>`;
    })
    .join('<span class="docs-breadcrumb-sep">/</span>');
};

const renderPageHeader = () => {
  if (!elements.pageHeader) {
    return;
  }
  const readWhen = (state.pageData.read_when || [])
    .map((item) => `<li>${escapeHtml(item)}</li>`)
    .join("");
  const sourceDocs = (state.pageData.source_docs || [])
    .map((item) => `<li><code>${escapeHtml(item)}</code></li>`)
    .join("");
  elements.pageHeader.innerHTML = `
    <div class="docs-page-kicker">${escapeHtml(state.pageData.tab)} / ${escapeHtml(state.pageData.group)}</div>
    <h1>${escapeHtml(state.pageData.title)}</h1>
    ${state.pageData.summary ? `<p class="docs-page-summary">${escapeHtml(state.pageData.summary)}</p>` : ""}
    <div class="docs-page-meta">
      ${
        readWhen
          ? `<section class="docs-page-meta-block"><div class="docs-page-meta-title">适合什么时候读</div><ul>${readWhen}</ul></section>`
          : ""
      }
      ${
        sourceDocs
          ? `<section class="docs-page-meta-block"><div class="docs-page-meta-title">来源文档</div><ul>${sourceDocs}</ul></section>`
          : ""
      }
    </div>
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
    }
  });
};

const renderContent = async () => {
  if (!elements.content) {
    return;
  }
  const marked = await waitForMarked();
  if (!marked) {
    elements.content.innerHTML = `<pre>${escapeHtml(state.pageData.markdown || "")}</pre>`;
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
    elements.toc.innerHTML = '<div class="docs-toc-empty">当前页没有二级目录</div>';
    return;
  }
  elements.toc.innerHTML = headings
    .map(
      (item) =>
        `<a class="docs-toc-link level-${Number(item.level)}" href="#${escapeHtml(item.id)}">${escapeHtml(item.text)}</a>`,
    )
    .join("");
};

const bindTocObserver = () => {
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
    cards.push(`
      <a class="docs-pagination-card is-prev" href="${escapeHtml(previousPage.url)}">
        <span class="docs-pagination-label">上一篇</span>
        <strong>${escapeHtml(previousPage.title)}</strong>
      </a>
    `);
  }
  if (nextPage) {
    cards.push(`
      <a class="docs-pagination-card is-next" href="${escapeHtml(nextPage.url)}">
        <span class="docs-pagination-label">下一篇</span>
        <strong>${escapeHtml(nextPage.title)}</strong>
      </a>
    `);
  }
  elements.pageFooter.innerHTML = cards.join("");
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
        <a class="docs-search-result" href="${escapeHtml(entry.url)}">
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
  document.addEventListener("click", (event) => {
    if (!elements.searchResults?.contains(event.target) && event.target !== elements.searchInput) {
      hideSearchResults();
    }
  });
};

const bindSidebarToggle = () => {
  if (!elements.sidebarToggle) {
    return;
  }
  elements.sidebarToggle.addEventListener("click", () => {
    document.body.classList.toggle("docs-sidebar-open");
  });
  document.addEventListener("click", (event) => {
    if (window.innerWidth > 1180) {
      return;
    }
    const clickedInsideSidebar = elements.sidebar?.contains(event.target);
    const clickedToggle = elements.sidebarToggle?.contains(event.target);
    if (!clickedInsideSidebar && !clickedToggle) {
      document.body.classList.remove("docs-sidebar-open");
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
  bindSidebarToggle();
  try {
    state.manifest = await fetchJson(MANIFEST_URL);
  } catch (error) {
    renderPageHeader();
    await renderContent();
    renderToc();
    return;
  }
  renderTabs();
  renderSidebar();
  renderLanguageSwitcher();
  renderBreadcrumbs();
  renderPageHeader();
  await renderContent();
  renderToc();
  renderPageFooter();
  bindTocObserver();
};

bootstrap();
