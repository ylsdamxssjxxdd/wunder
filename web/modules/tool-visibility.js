import { elements } from "./elements.js?v=20260518-01";
import { state } from "./state.js";
import { getWunderBase } from "./api.js";
import { notify } from "./notify.js";
import { t } from "./i18n.js?v=20260518-01";
import { ensureOrgUnitsLoaded, getAllOrgUnitItems } from "./org-units.js?v=20260518-01";

const normalizeNameList = (values) => {
  const seen = new Set();
  const output = [];
  (Array.isArray(values) ? values : []).forEach((value) => {
    const cleaned = String(value || "").trim();
    if (!cleaned || seen.has(cleaned)) {
      return;
    }
    seen.add(cleaned);
    output.push(cleaned);
  });
  return output;
};

const ensureState = () => {
  if (!state.toolVisibility || typeof state.toolVisibility !== "object") {
    state.toolVisibility = {
      tools: [],
      selectedName: "",
      visibilityRules: [],
      selectedUnitIds: [],
    };
  }
};

const buildRuleMap = () =>
  new Map(
    (Array.isArray(state.toolVisibility.visibilityRules) ? state.toolVisibility.visibilityRules : []).map((rule) => [
      String(rule?.name || "").trim(),
      normalizeNameList(rule?.visible_unit_ids || []),
    ])
  );

const selectedTool = () =>
  (Array.isArray(state.toolVisibility.tools) ? state.toolVisibility.tools : []).find(
    (tool) => String(tool?.name || "").trim() === String(state.toolVisibility.selectedName || "").trim()
  ) || null;

const renderDetail = () => {
  const tool = selectedTool();
  const unitMap = new Map(getAllOrgUnitItems().map((item) => [item.unit_id, item.path_name || item.name || item.unit_id]));
  if (!tool) {
    elements.toolVisibilityDetailTitle.textContent = "未选择工具";
    elements.toolVisibilityDetailMeta.textContent = "";
    elements.toolVisibilityUnitsTree.textContent = "";
    return;
  }
  const ruleMap = buildRuleMap();
  const unitIds = ruleMap.get(tool.name) || [];
  if (!state.toolVisibility.editingName || state.toolVisibility.editingName !== tool.name) {
    state.toolVisibility.selectedUnitIds = [...unitIds];
  }
  elements.toolVisibilityDetailTitle.textContent = tool.name;
  elements.toolVisibilityDetailMeta.textContent = unitIds.length
    ? unitIds.map((item) => unitMap.get(item) || item).join(" | ")
    : t("visibility.all");
  renderUnitTree();
};

const renderList = () => {
  elements.toolVisibilityList.textContent = "";
  const items = Array.isArray(state.toolVisibility.tools) ? state.toolVisibility.tools : [];
  if (!items.length) {
    elements.toolVisibilityList.textContent = t("tools.empty.builtin");
    return;
  }
  const ruleMap = buildRuleMap();
  items.forEach((tool) => {
    const row = document.createElement("div");
    row.className = "skill-item";
    if (tool.name === state.toolVisibility.selectedName) {
      row.classList.add("is-active");
    }
    const label = document.createElement("label");
    const title = document.createElement("strong");
    title.textContent = tool.name;
    const meta = document.createElement("span");
    meta.className = "muted";
    meta.textContent = (ruleMap.get(tool.name) || []).length ? t("visibility.scoped") : t("visibility.all");
    label.append(title, meta);
    row.appendChild(label);
    row.addEventListener("click", () => {
      state.toolVisibility.selectedName = tool.name;
      renderList();
      renderDetail();
    });
    elements.toolVisibilityList.appendChild(row);
  });
};

const buildOrgTree = () => {
  const units = Array.isArray(state.orgUnits?.list) ? state.orgUnits.list : [];
  const byParent = new Map();
  units.forEach((unit) => {
    const key = String(unit.parent_id || "");
    if (!byParent.has(key)) {
      byParent.set(key, []);
    }
    byParent.get(key).push(unit);
  });
  byParent.forEach((list) => {
    list.sort((left, right) => String(left.path_name || left.name).localeCompare(String(right.path_name || right.name)));
  });
  const build = (parentId = "") =>
    (byParent.get(parentId) || []).map((unit) => ({
      ...unit,
      children: build(unit.unit_id),
    }));
  return build("");
};

const collectDescendantIds = (node) => {
  const output = [node.unit_id];
  (Array.isArray(node.children) ? node.children : []).forEach((child) => {
    output.push(...collectDescendantIds(child));
  });
  return output;
};

const renderUnitTree = () => {
  const container = elements.toolVisibilityUnitsTree;
  if (!container) {
    return;
  }
  container.textContent = "";
  const allNodes = buildOrgTree();
  const allIds = new Set();
  const collectAll = (node) => {
    allIds.add(node.unit_id);
    (Array.isArray(node.children) ? node.children : []).forEach(collectAll);
  };
  allNodes.forEach(collectAll);
  const selected = new Set(
    (state.toolVisibility.selectedUnitIds || []).length ? state.toolVisibility.selectedUnitIds : Array.from(allIds)
  );
  const toggleNode = (node, checked) => {
    const ids = collectDescendantIds(node);
    if (checked) {
      ids.forEach((id) => selected.add(id));
    } else {
      ids.forEach((id) => selected.delete(id));
    }
    state.toolVisibility.editingName = selectedTool()?.name || "";
    state.toolVisibility.selectedUnitIds = Array.from(selected);
    renderUnitTree();
    const tool = selectedTool();
    const unitMap = new Map(getAllOrgUnitItems().map((item) => [item.unit_id, item.path_name || item.name || item.unit_id]));
    elements.toolVisibilityDetailMeta.textContent = state.toolVisibility.selectedUnitIds.length
      ? state.toolVisibility.selectedUnitIds.map((item) => unitMap.get(item) || item).join(" | ")
      : t("visibility.all");
  };
  const renderNode = (node, depth) => {
    const row = document.createElement("div");
    row.className = "tool-item";
    row.style.paddingLeft = `${8 + depth * 14}px`;
    const checkbox = document.createElement("input");
    checkbox.type = "checkbox";
    checkbox.checked = selected.has(node.unit_id);
    checkbox.addEventListener("change", () => toggleNode(node, checkbox.checked));
    const label = document.createElement("label");
    label.innerHTML = `<strong>${node.name || node.unit_id}</strong><span class="muted">${node.path_name || node.unit_id}</span>`;
    row.appendChild(checkbox);
    row.appendChild(label);
    container.appendChild(row);
    (Array.isArray(node.children) ? node.children : []).forEach((child) => renderNode(child, depth + 1));
  };
  allNodes.forEach((node) => renderNode(node, 0));
};

const extractAllOpenTools = (payload) => {
  const result = [];
  const pushItems = (items) => {
    (Array.isArray(items) ? items : []).forEach((item) => {
      const name = String(item?.name || "").trim();
      if (!name) {
        return;
      }
      result.push({ name, description: String(item?.description || "").trim() });
    });
  };
  pushItems(payload?.tools);
  pushItems(payload?.skills);
  return result;
};

export const loadToolVisibilityPanel = async () => {
  ensureState();
  await ensureOrgUnitsLoaded({ silent: true });
  const wunderBase = getWunderBase();
  const [toolsResponse, skillsResponse] = await Promise.all([
    fetch(`${wunderBase}/admin/tools`),
    fetch(`${wunderBase}/admin/skills`),
  ]);
  if (!toolsResponse.ok || !skillsResponse.ok) {
    throw new Error(t("common.loadFailed"));
  }
  const toolsPayload = await toolsResponse.json();
  const skillsPayload = await skillsResponse.json();
  state.toolVisibility.tools = normalizeNameList([
    ...extractAllOpenTools(toolsPayload).map((item) => item.name),
    ...extractAllOpenTools(skillsPayload).map((item) => item.name),
  ]).map((name) => ({ name }));
  state.toolVisibility.visibilityRules = Array.isArray(toolsPayload?.visibility?.rules)
    ? toolsPayload.visibility.rules
    : [];
  if (
    !state.toolVisibility.selectedName ||
    !state.toolVisibility.tools.some((item) => item.name === state.toolVisibility.selectedName)
  ) {
    state.toolVisibility.selectedName = state.toolVisibility.tools[0]?.name || "";
  }
  state.toolVisibility.editingName = "";
  renderList();
  renderDetail();
};

export const saveToolVisibilityPanel = async () => {
  ensureState();
  const tool = selectedTool();
  if (!tool) {
    return;
  }
  const tree = buildOrgTree();
  const allIds = [];
  const collectAll = (node) => {
    allIds.push(node.unit_id);
    (Array.isArray(node.children) ? node.children : []).forEach(collectAll);
  };
  tree.forEach(collectAll);
  const unitIds = normalizeNameList(state.toolVisibility.selectedUnitIds || []);
  const normalizedUnitIds =
    unitIds.length === allIds.length ? [] : unitIds;
  const nextRules = (Array.isArray(state.toolVisibility.visibilityRules) ? state.toolVisibility.visibilityRules : [])
    .filter((rule) => String(rule?.name || "").trim() !== tool.name);
  if (normalizedUnitIds.length) {
    nextRules.push({ name: tool.name, visible_unit_ids: normalizedUnitIds });
  }
  const wunderBase = getWunderBase();
  const toolsResponse = await fetch(`${wunderBase}/admin/tools`);
  if (!toolsResponse.ok) {
    throw new Error(t("common.loadFailed"));
  }
  const toolsPayload = await toolsResponse.json();
  const enabled = Array.isArray(toolsPayload?.enabled) ? toolsPayload.enabled : [];
  const response = await fetch(`${wunderBase}/admin/tools`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      enabled,
      visibility_rules: nextRules,
    }),
  });
  if (!response.ok) {
    throw new Error(t("common.saveFailed"));
  }
  state.toolVisibility.visibilityRules = nextRules;
  state.toolVisibility.editingName = "";
  renderList();
  renderDetail();
  notify(t("common.save"), "success");
};

export const initToolVisibilityPanel = () => {
  ensureState();
  elements.toolVisibilityRefreshBtn?.addEventListener("click", async () => {
    try {
      await loadToolVisibilityPanel();
      notify(t("common.refresh"), "success");
    } catch (error) {
      notify(error.message, "error");
    }
  });
  elements.toolVisibilitySaveBtn?.addEventListener("click", async () => {
    try {
      await saveToolVisibilityPanel();
    } catch (error) {
      notify(error.message, "error");
    }
  });
};
