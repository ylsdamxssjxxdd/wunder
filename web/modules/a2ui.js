import { t } from "./i18n.js";

// A2UI 消息类型顺序，用于定位消息主体。
const A2UI_MESSAGE_KEYS = [
  "beginRendering",
  "surfaceUpdate",
  "dataModelUpdate",
  "deleteSurface",
];

// 内存态：记录 surface 与组件/数据模型，便于增量更新。
const a2uiState = {
  surfaces: new Map(),
};

// 将 a2ui 消息转为统一数组结构。
const normalizeMessages = (raw) => {
  if (!raw) {
    return [];
  }
  if (typeof raw === "string") {
    try {
      const parsed = JSON.parse(raw);
      return Array.isArray(parsed) ? parsed : [parsed];
    } catch (error) {
      return [];
    }
  }
  if (Array.isArray(raw)) {
    return raw;
  }
  if (typeof raw === "object") {
    return [raw];
  }
  return [];
};

// 获取消息类型与对应 payload。
const resolveMessage = (message) => {
  if (!message || typeof message !== "object") {
    return { type: "", payload: null };
  }
  for (const key of A2UI_MESSAGE_KEYS) {
    if (message[key]) {
      return { type: key, payload: message[key] };
    }
  }
  return { type: "", payload: null };
};

// 确保 surface 容器存在。
const ensureSurface = (container, surfaceId) => {
  if (!container || !surfaceId) {
    return null;
  }
  let surface = a2uiState.surfaces.get(surfaceId);
  if (!surface) {
    const panel = document.createElement("div");
    panel.className = "a2ui-surface";
    panel.dataset.surfaceId = surfaceId;
    const title = document.createElement("div");
    title.className = "a2ui-surface-title";
    title.textContent = `${t("debug.a2ui.surface")}: ${surfaceId}`;
    panel.appendChild(title);
    container.appendChild(panel);
    surface = {
      id: surfaceId,
      rootId: "",
      styles: {},
      data: {},
      components: new Map(),
      container: panel,
      contentRoot: panel,
    };
    a2uiState.surfaces.set(surfaceId, surface);
  }
  return surface;
};

// 清理指定 surface。
const removeSurface = (surfaceId) => {
  const surface = a2uiState.surfaces.get(surfaceId);
  if (!surface) {
    return;
  }
  if (surface.container?.parentNode) {
    surface.container.parentNode.removeChild(surface.container);
  }
  a2uiState.surfaces.delete(surfaceId);
};

// 解析路径为数组。
const splitPath = (path) => {
  if (!path) {
    return [];
  }
  return String(path)
    .replace(/^\/+/, "")
    .split("/")
    .map((segment) => segment.trim())
    .filter(Boolean);
};

// 读取路径值，支持绝对路径与相对路径。
const resolvePathValue = (root, path) => {
  if (!path || path === "/") {
    return root;
  }
  const segments = splitPath(path);
  let current = root;
  for (const segment of segments) {
    if (!current || typeof current !== "object") {
      return undefined;
    }
    current = current[segment];
  }
  return current;
};

// 写入路径值，必要时创建中间对象。
const setPathValue = (root, path, value) => {
  const segments = splitPath(path);
  if (!segments.length) {
    return value;
  }
  let current = root;
  for (let index = 0; index < segments.length; index += 1) {
    const key = segments[index];
    if (index === segments.length - 1) {
      current[key] = value;
      break;
    }
    if (!current[key] || typeof current[key] !== "object") {
      current[key] = {};
    }
    current = current[key];
  }
  return root;
};

// 将 valueMap 结构还原为对象。
const buildValueFromEntry = (entry) => {
  if (!entry || typeof entry !== "object") {
    return null;
  }
  if ("valueString" in entry) {
    return entry.valueString;
  }
  if ("valueNumber" in entry) {
    return entry.valueNumber;
  }
  if ("valueBoolean" in entry) {
    return entry.valueBoolean;
  }
  if ("valueMap" in entry) {
    return buildMapFromEntries(entry.valueMap);
  }
  return null;
};

// 解析 dataModelUpdate.contents 为对象。
const buildMapFromEntries = (entries) => {
  if (!Array.isArray(entries)) {
    return {};
  }
  const output = {};
  entries.forEach((item) => {
    if (!item || typeof item !== "object") {
      return;
    }
    const key = String(item.key || "");
    if (!key) {
      return;
    }
    output[key] = buildValueFromEntry(item);
  });
  return output;
};

// 将 contents 转换为数据模型。
const buildDataModel = (contents) => {
  if (!contents) {
    return {};
  }
  if (Array.isArray(contents)) {
    return buildMapFromEntries(contents);
  }
  if (typeof contents === "object") {
    return contents;
  }
  return {};
};

// 解析绑定值，优先使用 path，其次 literal 值。
const resolveBoundValue = (value, context) => {
  if (!value || typeof value !== "object") {
    return value;
  }
  const path = typeof value.path === "string" ? value.path : "";
  if (path) {
    if (path.startsWith("/")) {
      return resolvePathValue(context.data, path);
    }
    if (context.dataContext && typeof context.dataContext === "object") {
      const resolved = resolvePathValue(context.dataContext, path);
      if (resolved !== undefined) {
        return resolved;
      }
    }
    return resolvePathValue(context.data, path);
  }
  if ("literalString" in value) {
    return value.literalString;
  }
  if ("literalNumber" in value) {
    return value.literalNumber;
  }
  if ("literalBoolean" in value) {
    return value.literalBoolean;
  }
  if ("literalArray" in value) {
    return Array.isArray(value.literalArray) ? value.literalArray : [];
  }
  return "";
};

// 渲染文本组件，支持 usageHint。
const renderText = (props, context) => {
  const raw = resolveBoundValue(props.text, context);
  const text = raw === undefined || raw === null ? "" : String(raw);
  const usageHint = String(props.usageHint || "").toLowerCase();
  const element = document.createElement("div");
  element.className = "a2ui-text";
  if (usageHint) {
    element.classList.add(`a2ui-text--${usageHint}`);
  }
  element.textContent = text;
  return element;
};

// 渲染容器类组件：Row / Column / List。
const renderContainer = (type, props, context, renderChildById) => {
  const element = document.createElement("div");
  element.className = type === "Row" ? "a2ui-row" : "a2ui-column";
  const children = props.children || {};
  if (Array.isArray(children.explicitList)) {
    children.explicitList.forEach((childId) => {
      const child = renderChildById(String(childId), context);
      if (child) {
        element.appendChild(child);
      }
    });
  } else if (children.template && children.template.componentId) {
    const binding = children.template.dataBinding || "";
    const templateId = String(children.template.componentId);
    const dataList = resolveBoundValue({ path: binding }, context);
    if (Array.isArray(dataList)) {
      dataList.forEach((item) => {
        const child = renderChildById(templateId, {
          ...context,
          dataContext: item,
        });
        if (child) {
          element.appendChild(child);
        }
      });
    } else if (dataList && typeof dataList === "object") {
      Object.keys(dataList).forEach((key) => {
        const child = renderChildById(templateId, {
          ...context,
          dataContext: dataList[key],
        });
        if (child) {
          element.appendChild(child);
        }
      });
    }
  }
  return element;
};

// 渲染按钮组件，显示 action 名称。
const renderButton = (props, context, renderChildById) => {
  const button = document.createElement("button");
  button.className = "a2ui-button";
  const label = resolveBoundValue(props.label || props.text, context);
  if (props.child) {
    const child = renderChildById(String(props.child), context);
    if (child) {
      button.appendChild(child);
    }
  } else {
    button.textContent = label === undefined || label === null ? t("debug.a2ui.action") : String(label);
  }
  if (props.action?.name) {
    button.dataset.action = String(props.action.name);
    button.title = `${t("debug.a2ui.action")}: ${props.action.name}`;
  }
  return button;
};

// 渲染卡片组件。
const renderCard = (props, context, renderChildById) => {
  const wrapper = document.createElement("div");
  wrapper.className = "a2ui-card";
  if (props.child) {
    const child = renderChildById(String(props.child), context);
    if (child) {
      wrapper.appendChild(child);
    }
  }
  return wrapper;
};

// 渲染图片组件。
const renderImage = (props, context) => {
  const url = resolveBoundValue(props.url, context);
  const img = document.createElement("img");
  img.className = "a2ui-image";
  img.alt = "";
  if (url) {
    img.src = String(url);
  }
  return img;
};

// 渲染分割线组件。
const renderDivider = () => {
  const divider = document.createElement("hr");
  divider.className = "a2ui-divider";
  return divider;
};

// 渲染图标组件（调试环境仅展示名字）。
const renderIcon = (props) => {
  const name = resolveBoundValue(props.name || props.icon, { data: {} });
  const icon = document.createElement("span");
  icon.className = "a2ui-icon";
  icon.textContent = name ? String(name) : "icon";
  return icon;
};

// 渲染未知组件，避免渲染失败。
const renderUnknown = (type) => {
  const element = document.createElement("div");
  element.className = "a2ui-unknown";
  element.textContent = `${t("debug.a2ui.unknown")}: ${type}`;
  return element;
};

// 根据组件类型渲染 DOM。
const renderComponent = (component, context, renderChildById) => {
  const componentDef = component.component || {};
  const types = Object.keys(componentDef);
  const type = types.length ? types[0] : "";
  const props = type ? componentDef[type] || {} : {};
  let element = null;
  switch (type) {
    case "Text":
      element = renderText(props, context);
      break;
    case "Row":
    case "Column":
    case "List":
      element = renderContainer(type, props, context, renderChildById);
      break;
    case "Button":
      element = renderButton(props, context, renderChildById);
      break;
    case "Card":
      element = renderCard(props, context, renderChildById);
      break;
    case "Image":
      element = renderImage(props, context);
      break;
    case "Divider":
      element = renderDivider();
      break;
    case "Icon":
      element = renderIcon(props);
      break;
    default:
      element = renderUnknown(type);
      break;
  }
  if (element && Number.isFinite(component.weight)) {
    element.style.flexGrow = String(component.weight);
  }
  if (element && component.id) {
    element.dataset.componentId = component.id;
  }
  return element;
};

// 按 rootId 渲染整个 surface。
const renderSurface = (surface) => {
  if (!surface || !surface.rootId) {
    return;
  }
  const contentRoot = surface.container;
  if (!contentRoot) {
    return;
  }
  // 清空旧渲染，保留标题。
  while (contentRoot.children.length > 1) {
    contentRoot.removeChild(contentRoot.lastChild);
  }
  const renderChildById = (componentId, context) => {
    const component = surface.components.get(componentId);
    if (!component) {
      return null;
    }
    return renderComponent(component, context, renderChildById);
  };
  const rootComponent = renderChildById(surface.rootId, {
    data: surface.data,
    dataContext: null,
  });
  if (rootComponent) {
    contentRoot.appendChild(rootComponent);
  } else {
    const placeholder = document.createElement("div");
    placeholder.className = "a2ui-empty";
    placeholder.textContent = t("debug.a2ui.empty");
    contentRoot.appendChild(placeholder);
  }
};

// 更新数据模型并触发渲染。
const applyDataModelUpdate = (surface, update) => {
  const path = typeof update.path === "string" ? update.path : "";
  const data = buildDataModel(update.contents);
  if (!path || path === "/") {
    surface.data = data;
  } else {
    surface.data = setPathValue(surface.data || {}, path, data);
  }
};

// 应用 beginRendering 样式。
const applySurfaceStyles = (surface, styles) => {
  if (!styles || typeof styles !== "object") {
    return;
  }
  surface.styles = { ...styles };
  if (styles.font && surface.container) {
    surface.container.style.fontFamily = String(styles.font);
  }
  if (styles.primaryColor && surface.container) {
    surface.container.style.setProperty("--a2ui-primary-color", String(styles.primaryColor));
  }
};

// 对外：清空 A2UI 渲染状态。
export const resetA2uiState = (container) => {
  a2uiState.surfaces.clear();
  if (container) {
    container.innerHTML = "";
  }
};

// 对外：应用 A2UI 消息列表并渲染。
export const applyA2uiMessages = (container, payload) => {
  if (!container) {
    return;
  }
  const uid = String(payload?.uid || "").trim();
  const messages = normalizeMessages(payload?.messages || payload?.a2ui || payload);
  if (!messages.length) {
    return;
  }
  messages.forEach((message) => {
    const { type, payload: data } = resolveMessage(message);
    if (!type || !data || typeof data !== "object") {
      return;
    }
    const surfaceId = String(data.surfaceId || uid || "").trim();
    if (!surfaceId) {
      return;
    }
    if (type === "deleteSurface") {
      removeSurface(surfaceId);
      return;
    }
    const surface = ensureSurface(container, surfaceId);
    if (!surface) {
      return;
    }
    if (type === "surfaceUpdate") {
      (data.components || []).forEach((component) => {
        if (!component || !component.id) {
          return;
        }
        surface.components.set(component.id, {
          id: component.id,
          component: component.component || {},
          weight: component.weight,
        });
      });
    } else if (type === "dataModelUpdate") {
      applyDataModelUpdate(surface, data);
    } else if (type === "beginRendering") {
      surface.rootId = String(data.root || "");
      applySurfaceStyles(surface, data.styles);
    }
    renderSurface(surface);
  });
};
