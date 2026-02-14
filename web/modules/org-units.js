import { elements } from "./elements.js?v=20260214-01";
import { state } from "./state.js";
import { getWunderBase } from "./api.js";
import { appendLog } from "./log.js?v=20260108-02";
import { notify } from "./notify.js";
import { t } from "./i18n.js?v=20260214-01";

const ensureOrgUnitsState = () => {
  if (!state.orgUnits) {
    state.orgUnits = {
      list: [],
      tree: [],
      selectedId: "",
      loaded: false,
      lastUpdated: 0,
      collapsed: new Set(),
    };
  } else if (!(state.orgUnits.collapsed instanceof Set)) {
    const collapsed = Array.isArray(state.orgUnits.collapsed) ? state.orgUnits.collapsed : [];
    state.orgUnits.collapsed = new Set(collapsed);
  }
};

const ensureOrgUnitElements = () => {
  const requiredKeys = [
    "orgUnitRefreshBtn",
    "orgUnitCreateBtn",
    "orgUnitExpandAllBtn",
    "orgUnitCollapseAllBtn",
    "orgUnitTree",
    "orgUnitDetailTitle",
    "orgUnitDetailMeta",
    "orgUnitFormName",
    "orgUnitFormParent",
    "orgUnitFormSortOrder",
    "orgUnitFormLeaders",
    "orgUnitFormPath",
    "orgUnitFormLevel",
    "orgUnitSaveBtn",
    "orgUnitDeleteBtn",
    "orgUnitAddChildBtn",
    "orgUnitModal",
    "orgUnitModalTitle",
    "orgUnitModalClose",
    "orgUnitModalCancel",
    "orgUnitModalSave",
    "orgUnitModalName",
    "orgUnitModalParent",
    "orgUnitModalSortOrder",
    "orgUnitModalLeaders",
  ];
  const missing = requiredKeys.filter((key) => !elements[key]);
  if (missing.length) {
    appendLog(t("orgUnits.domMissing", { nodes: missing.join(", ") }));
    return false;
  }
  return true;
};

const normalizeOrgUnit = (item) => {
  const unitId = String(item?.unit_id || item?.unitId || "").trim();
  const parentId = String(item?.parent_id || item?.parentId || "").trim();
  const leaderIds = Array.isArray(item?.leader_ids)
    ? item.leader_ids
    : Array.isArray(item?.leaderIds)
    ? item.leaderIds
    : [];
  return {
    unit_id: unitId,
    parent_id: parentId || null,
    name: String(item?.name || "").trim(),
    level: Number(item?.level || 0),
    path: String(item?.path || "").trim(),
    path_name: String(item?.path_name || item?.pathName || "").trim(),
    sort_order: Number.isFinite(Number(item?.sort_order ?? item?.sortOrder))
      ? Number(item?.sort_order ?? item?.sortOrder)
      : 0,
    leader_ids: leaderIds.map((id) => String(id || "").trim()).filter(Boolean),
  };
};

const normalizeTree = (nodes) =>
  (Array.isArray(nodes) ? nodes : []).map((node) => {
    const normalized = normalizeOrgUnit(node);
    normalized.children = normalizeTree(node?.children || []);
    return normalized;
  });

const flattenTree = (nodes, output = []) => {
  (Array.isArray(nodes) ? nodes : []).forEach((node) => {
    output.push(node);
    if (node.children?.length) {
      flattenTree(node.children, output);
    }
  });
  return output;
};

const buildUnitMap = (units) => {
  const map = new Map();
  (units || []).forEach((unit) => {
    if (unit?.unit_id) {
      map.set(unit.unit_id, unit);
    }
  });
  return map;
};

const buildUnitOptions = (units, options = {}) => {
  const includeRoot = options.includeRoot === true;
  const excluded = new Set(options.excludeIds || []);
  const rootLabel = options.rootLabel || t("orgUnits.option.root");
  const output = [];
  if (includeRoot) {
    output.push({ value: "", label: rootLabel });
  }
  units.forEach((unit) => {
    if (!unit?.unit_id || excluded.has(unit.unit_id)) {
      return;
    }
    output.push({ value: unit.unit_id, label: unit.path_name || unit.name || unit.unit_id });
  });
  return output;
};

const syncSelectOptions = (select, options, selected) => {
  if (!select) {
    return;
  }
  select.textContent = "";
  options.forEach((option) => {
    const node = document.createElement("option");
    node.value = option.value;
    node.textContent = option.label;
    select.appendChild(node);
  });
  select.value = selected ?? "";
};

const parseLeaderIds = (raw) => {
  if (!raw) {
    return [];
  }
  return String(raw)
    .split(/[,，\n\r]+/)
    .map((value) => value.trim())
    .filter(Boolean);
};

const openModal = (modal) => {
  if (modal) {
    modal.classList.add("active");
  }
};

const closeModal = (modal) => {
  if (modal) {
    modal.classList.remove("active");
  }
};

const resolveSelectedUnit = () =>
  state.orgUnits.list.find((unit) => unit.unit_id === state.orgUnits.selectedId) || null;

const computeDescendantIds = (unit, units) => {
  if (!unit?.path) {
    return new Set();
  }
  const prefix = `${unit.path}/`;
  const output = new Set();
  units.forEach((item) => {
    if (item.path && item.path.startsWith(prefix)) {
      output.add(item.unit_id);
    }
  });
  return output;
};

const collapseAllOrgUnits = () => {
  const tree = state.orgUnits.tree || [];
  if (!tree.length) {
    return;
  }
  const collapsed = new Set();
  const stack = [...tree];
  while (stack.length) {
    const node = stack.pop();
    if (!node?.unit_id) {
      continue;
    }
    if (Array.isArray(node.children) && node.children.length > 0) {
      collapsed.add(node.unit_id);
      node.children.forEach((child) => stack.push(child));
    }
  }
  state.orgUnits.collapsed = collapsed;
  renderOrgUnitTree();
};

const expandAllOrgUnits = () => {
  state.orgUnits.collapsed = new Set();
  renderOrgUnitTree();
};

const renderOrgUnitTree = () => {
  const tree = elements.orgUnitTree;
  if (!tree) {
    return;
  }
  tree.textContent = "";
  if (!state.orgUnits.tree.length) {
    tree.textContent = t("orgUnits.tree.empty");
    return;
  }
  const collapsedSet = state.orgUnits.collapsed instanceof Set ? state.orgUnits.collapsed : new Set();
  const selectUnit = (unitId) => {
    state.orgUnits.selectedId = unitId;
    renderOrgUnitTree();
    renderOrgUnitDetail();
  };
  const renderNode = (node, depth) => {
    const item = document.createElement("div");
    item.className = "org-unit-tree-item skill-tree-item";
    if (node.unit_id === state.orgUnits.selectedId) {
      item.classList.add("is-active");
    }
    const hasChildren = Array.isArray(node.children) && node.children.length > 0;
    const isCollapsed = hasChildren && collapsedSet.has(node.unit_id);
    if (hasChildren) {
      item.setAttribute("aria-expanded", String(!isCollapsed));
    }
    item.classList.add(hasChildren ? "is-dir" : "is-file");
    item.style.paddingLeft = `${8 + depth * 14}px`;
    item.title = node.path_name || node.name || node.unit_id || "";
    item.setAttribute("role", "button");
    item.tabIndex = 0;
    const toggle = document.createElement(hasChildren ? "button" : "span");
    if (hasChildren) {
      toggle.type = "button";
      toggle.className = "org-unit-tree-toggle";
      toggle.setAttribute("aria-label", t("orgUnits.tree.toggle"));
      toggle.addEventListener("click", (event) => {
        event.stopPropagation();
        if (collapsedSet.has(node.unit_id)) {
          collapsedSet.delete(node.unit_id);
        } else {
          collapsedSet.add(node.unit_id);
        }
        renderOrgUnitTree();
      });
    } else {
      toggle.className = "org-unit-tree-spacer";
    }
    if (hasChildren) {
      const caret = document.createElement("i");
      caret.className = collapsedSet.has(node.unit_id)
        ? "fa-solid fa-caret-right"
        : "fa-solid fa-caret-down";
      toggle.appendChild(caret);
    }
    const icon = document.createElement("i");
    icon.className = hasChildren ? "fa-solid fa-folder" : "fa-regular fa-file-lines";
    const name = document.createElement("span");
    name.className = "skill-tree-name";
    name.textContent = node.name || node.unit_id || "-";
    item.append(toggle, icon, name);
    item.addEventListener("click", () => selectUnit(node.unit_id));
    item.addEventListener("keydown", (event) => {
      if (event.key === "Enter" || event.key === " ") {
        event.preventDefault();
        selectUnit(node.unit_id);
      }
    });
    tree.appendChild(item);
    if (!isCollapsed) {
      (node.children || []).forEach((child) => renderNode(child, depth + 1));
    }
  };
  state.orgUnits.tree.forEach((node) => renderNode(node, 0));
};

const setDetailEnabled = (enabled) => {
  [
    elements.orgUnitFormName,
    elements.orgUnitFormParent,
    elements.orgUnitFormSortOrder,
    elements.orgUnitFormLeaders,
    elements.orgUnitSaveBtn,
    elements.orgUnitDeleteBtn,
    elements.orgUnitAddChildBtn,
  ].forEach((el) => {
    if (el) {
      el.disabled = !enabled;
    }
  });
};

const renderOrgUnitDetail = () => {
  const unit = resolveSelectedUnit();
  if (!unit) {
    elements.orgUnitDetailTitle.textContent = t("orgUnits.detail.empty");
    elements.orgUnitDetailMeta.textContent = "";
    elements.orgUnitFormName.value = "";
    elements.orgUnitFormSortOrder.value = "";
    elements.orgUnitFormLeaders.value = "";
    elements.orgUnitFormPath.textContent = "-";
    elements.orgUnitFormLevel.textContent = "-";
    syncSelectOptions(
      elements.orgUnitFormParent,
      buildUnitOptions(state.orgUnits.list, { includeRoot: true }),
      ""
    );
    setDetailEnabled(false);
    return;
  }
  elements.orgUnitDetailTitle.textContent = unit.name || unit.unit_id || "-";
  elements.orgUnitDetailMeta.textContent = unit.path_name
    ? `${unit.path_name} (${unit.unit_id})`
    : unit.unit_id;
  elements.orgUnitFormName.value = unit.name || "";
  elements.orgUnitFormSortOrder.value = Number.isFinite(unit.sort_order) ? unit.sort_order : "";
  elements.orgUnitFormLeaders.value = (unit.leader_ids || []).join(", ");
  elements.orgUnitFormPath.textContent = unit.path_name || "-";
  elements.orgUnitFormLevel.textContent = Number.isFinite(unit.level) ? String(unit.level) : "-";

  const descendants = computeDescendantIds(unit, state.orgUnits.list);
  descendants.add(unit.unit_id);
  const options = buildUnitOptions(state.orgUnits.list, {
    includeRoot: true,
    excludeIds: Array.from(descendants),
  });
  syncSelectOptions(elements.orgUnitFormParent, options, unit.parent_id || "");
  setDetailEnabled(true);
};

export const loadOrgUnits = async (options = {}) => {
  ensureOrgUnitsState();
  if (!ensureOrgUnitElements()) {
    return;
  }
  const wunderBase = getWunderBase();
  const endpoint = `${wunderBase}/admin/org_units`;
  if (!options.silent && elements.orgUnitTree) {
    elements.orgUnitTree.textContent = t("common.loading");
  }
  try {
    const response = await fetch(endpoint);
    if (!response.ok) {
      throw new Error(t("common.requestFailed", { status: response.status }));
    }
    const result = await response.json();
    const payload = result?.data || {};
    const items = Array.isArray(payload.items) ? payload.items : [];
    const tree = Array.isArray(payload.tree) ? payload.tree : [];
    state.orgUnits.list = items.map(normalizeOrgUnit);
    state.orgUnits.tree = normalizeTree(tree);
    state.orgUnits.loaded = true;
    state.orgUnits.lastUpdated = Date.now();
    if (!state.orgUnits.selectedId && state.orgUnits.list.length) {
      state.orgUnits.selectedId = state.orgUnits.list[0].unit_id;
    } else if (!state.orgUnits.list.find((unit) => unit.unit_id === state.orgUnits.selectedId)) {
      state.orgUnits.selectedId = "";
    }
    renderOrgUnitTree();
    renderOrgUnitDetail();
  } catch (error) {
    if (elements.orgUnitTree) {
      elements.orgUnitTree.textContent = t("common.loadFailedWithMessage", {
        message: error.message,
      });
    }
    if (!options.silent) {
      notify(t("orgUnits.toast.loadFailed", { message: error.message }), "error");
    }
    throw error;
  }
};

export const ensureOrgUnitsLoaded = async (options = {}) => {
  ensureOrgUnitsState();
  if (state.orgUnits.loaded && !options.force) {
    return state.orgUnits;
  }
  await loadOrgUnits({ silent: options.silent });
  return state.orgUnits;
};

export const getOrgUnitMap = () => buildUnitMap(state.orgUnits.list || []);

export const getOrgUnitOptions = (options = {}) => {
  const tree = state.orgUnits.tree || [];
  const flattened = tree.length ? flattenTree(tree, []) : [...(state.orgUnits.list || [])];
  return buildUnitOptions(flattened, options);
};

const submitOrgUnitUpdate = async () => {
  const unit = resolveSelectedUnit();
  if (!unit) {
    return;
  }
  const name = String(elements.orgUnitFormName.value || "").trim();
  if (!name) {
    notify(t("orgUnits.toast.nameRequired"), "warn");
    return;
  }
  const parentId = elements.orgUnitFormParent.value || "";
  const sortOrderValue = Number(elements.orgUnitFormSortOrder.value);
  const sortOrder = Number.isFinite(sortOrderValue) ? Math.max(0, Math.floor(sortOrderValue)) : null;
  const leaderIds = parseLeaderIds(elements.orgUnitFormLeaders.value);
  const payload = {
    name,
    parent_id: parentId,
    leader_ids: leaderIds,
  };
  if (sortOrder !== null) {
    payload.sort_order = sortOrder;
  }
  const wunderBase = getWunderBase();
  const endpoint = `${wunderBase}/admin/org_units/${encodeURIComponent(unit.unit_id)}`;
  try {
    const response = await fetch(endpoint, {
      method: "PATCH",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(payload),
    });
    if (!response.ok) {
      throw new Error(t("common.requestFailed", { status: response.status }));
    }
    notify(t("orgUnits.toast.updateSuccess"), "success");
    await loadOrgUnits({ silent: true });
  } catch (error) {
    notify(t("orgUnits.toast.updateFailed", { message: error.message }), "error");
  }
};

const submitOrgUnitDelete = async () => {
  const unit = resolveSelectedUnit();
  if (!unit?.unit_id) {
    return;
  }
  const confirmed = window.confirm(
    t("orgUnits.deleteConfirm", { name: unit.name || unit.unit_id })
  );
  if (!confirmed) {
    return;
  }
  const wunderBase = getWunderBase();
  const endpoint = `${wunderBase}/admin/org_units/${encodeURIComponent(unit.unit_id)}`;
  try {
    const response = await fetch(endpoint, { method: "DELETE" });
    if (!response.ok) {
      throw new Error(t("common.requestFailed", { status: response.status }));
    }
    notify(t("orgUnits.toast.deleteSuccess"), "success");
    state.orgUnits.selectedId = "";
    await loadOrgUnits({ silent: true });
  } catch (error) {
    notify(t("orgUnits.toast.deleteFailed", { message: error.message }), "error");
  }
};

const openCreateModal = (parentId = "") => {
  elements.orgUnitModalName.value = "";
  elements.orgUnitModalSortOrder.value = "";
  elements.orgUnitModalLeaders.value = "";
  const options = getOrgUnitOptions({ includeRoot: true });
  syncSelectOptions(elements.orgUnitModalParent, options, parentId);
  elements.orgUnitModalTitle.textContent = t("orgUnits.modal.title");
  openModal(elements.orgUnitModal);
};

const submitOrgUnitCreate = async () => {
  const name = String(elements.orgUnitModalName.value || "").trim();
  if (!name) {
    notify(t("orgUnits.toast.nameRequired"), "warn");
    return;
  }
  const parentId = elements.orgUnitModalParent.value || "";
  const sortOrderValue = Number(elements.orgUnitModalSortOrder.value);
  const sortOrder = Number.isFinite(sortOrderValue) ? Math.max(0, Math.floor(sortOrderValue)) : null;
  const leaderIds = parseLeaderIds(elements.orgUnitModalLeaders.value);
  const payload = {
    name,
    parent_id: parentId || null,
    leader_ids: leaderIds,
  };
  if (sortOrder !== null) {
    payload.sort_order = sortOrder;
  }
  const wunderBase = getWunderBase();
  const endpoint = `${wunderBase}/admin/org_units`;
  try {
    const response = await fetch(endpoint, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(payload),
    });
    if (!response.ok) {
      throw new Error(t("common.requestFailed", { status: response.status }));
    }
    notify(t("orgUnits.toast.createSuccess"), "success");
    closeModal(elements.orgUnitModal);
    await loadOrgUnits({ silent: true });
  } catch (error) {
    notify(t("orgUnits.toast.createFailed", { message: error.message }), "error");
  }
};

export const initOrgUnitsPanel = () => {
  ensureOrgUnitsState();
  if (!ensureOrgUnitElements()) {
    return;
  }
  elements.orgUnitRefreshBtn.addEventListener("click", async () => {
    try {
      await loadOrgUnits({ silent: true });
      notify(t("orgUnits.toast.loadSuccess"), "success");
    } catch (error) {
      appendLog(t("orgUnits.toast.loadFailed", { message: error.message }));
    }
  });
  elements.orgUnitExpandAllBtn.addEventListener("click", expandAllOrgUnits);
  elements.orgUnitCollapseAllBtn.addEventListener("click", collapseAllOrgUnits);
  elements.orgUnitCreateBtn.addEventListener("click", () => openCreateModal(""));
  elements.orgUnitAddChildBtn.addEventListener("click", () => {
    const selected = resolveSelectedUnit();
    openCreateModal(selected?.unit_id || "");
  });
  elements.orgUnitSaveBtn.addEventListener("click", submitOrgUnitUpdate);
  elements.orgUnitDeleteBtn.addEventListener("click", submitOrgUnitDelete);

  elements.orgUnitModalClose?.addEventListener("click", () => closeModal(elements.orgUnitModal));
  elements.orgUnitModalCancel?.addEventListener("click", () => closeModal(elements.orgUnitModal));
  elements.orgUnitModalSave?.addEventListener("click", submitOrgUnitCreate);
};
