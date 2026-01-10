import { t } from "./i18n.js?v=20260110-07";

// A2UI 消息类型顺序，用于定位消息主体。
const A2UI_MESSAGE_KEYS = [
  "beginRendering",
  "surfaceUpdate",
  "dataModelUpdate",
  "deleteSurface",
];

// 内存态：记录 surface 与组件数据模型，便于增量更新。
const a2uiState = {
  surfaces: new Map(),
};

// A2UI 图标名到 Font Awesome 图标类的映射，提升调试面板可读性。
const A2UI_ICON_MAP = {
  accountCircle: "fa-user-circle",
  add: "fa-plus",
  arrowBack: "fa-arrow-left",
  arrowForward: "fa-arrow-right",
  attachFile: "fa-paperclip",
  calendarToday: "fa-calendar-day",
  call: "fa-phone",
  camera: "fa-camera",
  check: "fa-check",
  close: "fa-xmark",
  delete: "fa-trash",
  download: "fa-download",
  edit: "fa-pen-to-square",
  event: "fa-calendar-check",
  error: "fa-circle-exclamation",
  favorite: "fa-heart",
  favoriteOff: "fa-heart-crack",
  folder: "fa-folder",
  help: "fa-circle-question",
  home: "fa-house",
  info: "fa-circle-info",
  locationOn: "fa-location-dot",
  lock: "fa-lock",
  lockOpen: "fa-lock-open",
  mail: "fa-envelope",
  menu: "fa-bars",
  moreVert: "fa-ellipsis-vertical",
  moreHoriz: "fa-ellipsis",
  notificationsOff: "fa-bell-slash",
  notifications: "fa-bell",
  payment: "fa-credit-card",
  person: "fa-user",
  phone: "fa-phone",
  photo: "fa-image",
  print: "fa-print",
  refresh: "fa-rotate",
  search: "fa-magnifying-glass",
  send: "fa-paper-plane",
  settings: "fa-gear",
  share: "fa-share-nodes",
  shoppingCart: "fa-cart-shopping",
  star: "fa-star",
  starHalf: "fa-star-half-stroke",
  starOff: "fa-star",
  upload: "fa-upload",
  visibility: "fa-eye",
  visibilityOff: "fa-eye-slash",
  warning: "fa-triangle-exclamation",
};

// 将a2ui 消息转为统一数组结构。
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

// 获取消息类型与对应payload。
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
    const content = document.createElement("div");
    content.className = "a2ui-surface-content";
    panel.appendChild(title);
    panel.appendChild(content);
    container.appendChild(panel);
    surface = {
      id: surfaceId,
      rootId: "",
      styles: {},
      data: {},
      components: new Map(),
      container: panel,
      contentRoot: content,
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

// 将valueMap 结构还原为对象。
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

// 将contents 转换为数据模型。
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

// 解析绑定值，优先使用 path，其次literal 值。
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
  if ("literal" in value) {
    return value.literal;
  }
  return "";
};

// 将字符串转换为kebab-case，便于CSS 类名复用。
const toKebabCase = (value) =>
  String(value || "")
    .replace(/([a-z])([A-Z])/g, "$1-$2")
    .replace(/[_\s]+/g, "-")
    .toLowerCase();

// 根据组件布局配置同步 flex 对齐与分布，提升布局一致性。
const applyFlexLayout = (element, props) => {
  if (!element || !props || typeof props !== "object") {
    return;
  }
  const alignmentMap = {
    start: "flex-start",
    center: "center",
    end: "flex-end",
    stretch: "stretch",
  };
  const distributionMap = {
    start: "flex-start",
    center: "center",
    end: "flex-end",
    spaceBetween: "space-between",
    spaceAround: "space-around",
    spaceEvenly: "space-evenly",
  };
  const alignment = alignmentMap[props.alignment];
  const distribution = distributionMap[props.distribution];
  if (alignment) {
    element.style.alignItems = alignment;
  }
  if (distribution) {
    element.style.justifyContent = distribution;
  }
};

// 解析图标名称并返回对应的 Font Awesome 类名。
const resolveIconClass = (name) => {
  const key = String(name || "").trim();
  if (!key) {
    return "";
  }
  return A2UI_ICON_MAP[key] || "";
};

// 渲染文本组件，支持usageHint。
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
  if (type === "Row") {
    element.className = "a2ui-row";
  } else if (type === "Column") {
    element.className = "a2ui-column";
  } else {
    element.className = "a2ui-list";
    const direction = String(props.direction || "vertical");
    element.classList.add(`a2ui-list--${direction === "horizontal" ? "horizontal" : "vertical"}`);
  }
  applyFlexLayout(element, props);
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

// 渲染按钮组件，显示action 名称。
const renderButton = (props, context, renderChildById) => {
  const button = document.createElement("button");
  button.className = "a2ui-button";
  button.type = "button";
  const isPrimary = props.primary !== false;
  button.classList.add(isPrimary ? "a2ui-button--primary" : "a2ui-button--secondary");
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
    if (Array.isArray(props.action.context)) {
      const actionContext = {};
      props.action.context.forEach((item) => {
        if (!item || !item.key) {
          return;
        }
        actionContext[item.key] = resolveBoundValue(item.value, context);
      });
      if (Object.keys(actionContext).length) {
        button.dataset.actionContext = JSON.stringify(actionContext);
      }
    }
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
  img.loading = "lazy";
  if (props.usageHint) {
    img.classList.add(`a2ui-image--${toKebabCase(props.usageHint)}`);
  }
  if (props.fit) {
    img.style.objectFit = String(props.fit);
  }
  if (url) {
    img.src = String(url);
  }
  return img;
};

// 渲染分割线组件。
const renderDivider = (props = {}) => {
  const divider = document.createElement("hr");
  divider.className = "a2ui-divider";
  if (props.axis === "vertical") {
    divider.classList.add("a2ui-divider--vertical");
  }
  return divider;
};

// 渲染图标组件（调试环境仅展示名字）。
const renderIcon = (props, context) => {
  const name = resolveBoundValue(props.name || props.icon, context);
  const icon = document.createElement("span");
  icon.className = "a2ui-icon";
  const iconName = name ? String(name) : "";
  const iconClass = resolveIconClass(iconName);
  if (iconClass) {
    const iconElement = document.createElement("i");
    iconElement.className = `fa-solid ${iconClass}`;
    icon.appendChild(iconElement);
    icon.dataset.iconName = iconName;
  } else {
    icon.textContent = iconName || "icon";
    icon.classList.add("a2ui-icon--text");
  }
  if (props.size) {
    icon.style.setProperty("--a2ui-icon-size", `${props.size}px`);
  }
  if (props.color) {
    icon.style.setProperty("--a2ui-icon-color", String(props.color));
  }
  return icon;
};

// 渲染视频组件，提供基础播放控件。
const renderVideo = (props, context) => {
  const url = resolveBoundValue(props.url, context);
  const video = document.createElement("video");
  video.className = "a2ui-video";
  video.controls = true;
  video.preload = "metadata";
  if (url) {
    video.src = String(url);
  }
  return video;
};

// 渲染音频播放器组件，可选加入描述文本。
const renderAudioPlayer = (props, context) => {
  const wrapper = document.createElement("div");
  wrapper.className = "a2ui-audio";
  const url = resolveBoundValue(props.url, context);
  const audio = document.createElement("audio");
  audio.className = "a2ui-audio-player";
  audio.controls = true;
  audio.preload = "metadata";
  if (url) {
    audio.src = String(url);
  }
  wrapper.appendChild(audio);
  const description = resolveBoundValue(props.description, context);
  if (description !== undefined && description !== null && String(description).trim() !== "") {
    const desc = document.createElement("div");
    desc.className = "a2ui-audio-description";
    desc.textContent = String(description);
    wrapper.appendChild(desc);
  }
  return wrapper;
};

// 渲染复选框，调试场景下只读展示。
const renderCheckbox = (props, context) => {
  const wrapper = document.createElement("label");
  wrapper.className = "a2ui-checkbox";
  const input = document.createElement("input");
  input.type = "checkbox";
  input.disabled = true;
  const value = resolveBoundValue(props.value, context);
  input.checked = Boolean(value);
  const label = document.createElement("span");
  label.className = "a2ui-checkbox-label";
  const labelText = resolveBoundValue(props.label, context);
  label.textContent = labelText === undefined || labelText === null ? "" : String(labelText);
  wrapper.appendChild(input);
  wrapper.appendChild(label);
  return wrapper;
};

// 渲染文本输入组件，便于可视化数据绑定结果。
const renderTextField = (props, context) => {
  const wrapper = document.createElement("div");
  wrapper.className = "a2ui-field";
  const labelText = resolveBoundValue(props.label, context);
  if (labelText !== undefined && labelText !== null && String(labelText).trim() !== "") {
    const label = document.createElement("label");
    label.className = "a2ui-field-label";
    label.textContent = String(labelText);
    wrapper.appendChild(label);
  }
  const value = resolveBoundValue(props.text, context);
  const fieldType = String(props.textFieldType || props.type || "shortText");
  if (fieldType === "longText") {
    const textarea = document.createElement("textarea");
    textarea.className = "a2ui-textarea";
    textarea.readOnly = true;
    textarea.rows = 4;
    textarea.value = value === undefined || value === null ? "" : String(value);
    wrapper.appendChild(textarea);
  } else {
    const input = document.createElement("input");
    input.className = "a2ui-input";
    input.readOnly = true;
    if (fieldType === "number") {
      input.type = "number";
    } else if (fieldType === "obscured") {
      input.type = "password";
    } else if (fieldType === "date") {
      input.type = "date";
    } else {
      input.type = "text";
    }
    input.value = value === undefined || value === null ? "" : String(value);
    wrapper.appendChild(input);
  }
  return wrapper;
};

// 将ISO 时间转换为input 组件可读格式，避免时区与秒数干扰。
const normalizeDateTimeValue = (raw, inputType) => {
  if (!raw) {
    return "";
  }
  const text = String(raw);
  if (inputType === "time") {
    const timeMatch = text.match(/(\d{2}:\d{2})(:\d{2})?/);
    return timeMatch ? timeMatch[1] : text;
  }
  if (inputType === "date") {
    const dateMatch = text.match(/(\d{4}-\d{2}-\d{2})/);
    return dateMatch ? dateMatch[1] : text;
  }
  if (/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}$/.test(text)) {
    return text;
  }
  const parsed = new Date(text);
  if (Number.isNaN(parsed.getTime())) {
    return text;
  }
  const pad = (value) => String(value).padStart(2, "0");
  return `${parsed.getFullYear()}-${pad(parsed.getMonth() + 1)}-${pad(parsed.getDate())}T${pad(
    parsed.getHours()
  )}:${pad(parsed.getMinutes())}`;
};

// 渲染日期/时间输入组件，以只读样式呈现数值。
const renderDateTimeInput = (props, context) => {
  const wrapper = document.createElement("div");
  wrapper.className = "a2ui-field";
  const input = document.createElement("input");
  input.className = "a2ui-input";
  input.readOnly = true;
  const enableDate = Boolean(props.enableDate);
  const enableTime = Boolean(props.enableTime);
  if (enableDate && enableTime) {
    input.type = "datetime-local";
  } else if (enableDate) {
    input.type = "date";
  } else if (enableTime) {
    input.type = "time";
  } else {
    input.type = "text";
  }
  const value = resolveBoundValue(props.value, context);
  input.value = normalizeDateTimeValue(value, input.type);
  wrapper.appendChild(input);
  return wrapper;
};

// 渲染多选组件，使用 disabled select 呈现选项状态。
const renderMultipleChoice = (props, context) => {
  const wrapper = document.createElement("div");
  wrapper.className = "a2ui-field";
  const select = document.createElement("select");
  select.className = "a2ui-select";
  select.disabled = true;
  const selections = resolveBoundValue(props.selections, context);
  const selectionList = Array.isArray(selections)
    ? selections.map((item) => String(item))
    : selections !== undefined && selections !== null
      ? [String(selections)]
      : [];
  const maxAllowed = Number.isFinite(props.maxAllowedSelections) ? props.maxAllowedSelections : 0;
  if (maxAllowed > 1 || selectionList.length > 1) {
    select.multiple = true;
  }
  const options = Array.isArray(props.options) ? props.options : [];
  options.forEach((option) => {
    const optionLabel = resolveBoundValue(option.label, context);
    const optionValue = option.value ?? "";
    const optionElement = document.createElement("option");
    optionElement.value = String(optionValue);
    optionElement.textContent =
      optionLabel === undefined || optionLabel === null ? String(optionValue) : String(optionLabel);
    if (selectionList.includes(optionElement.value)) {
      optionElement.selected = true;
    }
    select.appendChild(optionElement);
  });
  wrapper.appendChild(select);
  return wrapper;
};

// 渲染滑块组件，展示当前数值。
const renderSlider = (props, context) => {
  const wrapper = document.createElement("div");
  wrapper.className = "a2ui-slider";
  const input = document.createElement("input");
  input.className = "a2ui-slider-input";
  input.type = "range";
  input.disabled = true;
  const minValue = Number.isFinite(props.minValue) ? props.minValue : 0;
  const maxValue = Number.isFinite(props.maxValue) ? props.maxValue : 100;
  input.min = String(minValue);
  input.max = String(maxValue);
  const value = resolveBoundValue(props.value, context);
  const normalizedValue = Number.isFinite(Number(value)) ? Number(value) : minValue;
  input.value = String(normalizedValue);
  const valueLabel = document.createElement("span");
  valueLabel.className = "a2ui-slider-value";
  valueLabel.textContent = input.value;
  wrapper.appendChild(input);
  wrapper.appendChild(valueLabel);
  return wrapper;
};

// 渲染 Tab 组件，以简易切换实现基础体验。
const renderTabs = (props, context, renderChildById) => {
  const wrapper = document.createElement("div");
  wrapper.className = "a2ui-tabs";
  const header = document.createElement("div");
  header.className = "a2ui-tabs-header";
  const content = document.createElement("div");
  content.className = "a2ui-tabs-content";
  const tabItems = Array.isArray(props.tabItems) ? props.tabItems : [];
  const panels = [];
  const buttons = [];
  tabItems.forEach((item, index) => {
    const button = document.createElement("button");
    button.type = "button";
    button.className = "a2ui-tab-button";
    const title = resolveBoundValue(item.title, context);
    button.textContent = title === undefined || title === null ? `Tab ${index + 1}` : String(title);
    if (index === 0) {
      button.classList.add("is-active");
    }
    header.appendChild(button);
    buttons.push(button);
    const panel = document.createElement("div");
    panel.className = "a2ui-tab-panel";
    if (index !== 0) {
      panel.style.display = "none";
    }
    const child = item.child ? renderChildById(String(item.child), context) : null;
    if (child) {
      panel.appendChild(child);
    }
    content.appendChild(panel);
    panels.push(panel);
    button.addEventListener("click", () => {
      buttons.forEach((btn, btnIndex) => {
        btn.classList.toggle("is-active", btnIndex === index);
      });
      panels.forEach((panelItem, panelIndex) => {
        panelItem.style.display = panelIndex === index ? "" : "none";
      });
    });
  });
  wrapper.appendChild(header);
  wrapper.appendChild(content);
  return wrapper;
};

// 渲染 Modal 组件，使用details/summary 轻量展开。
const renderModal = (props, context, renderChildById) => {
  const wrapper = document.createElement("details");
  wrapper.className = "a2ui-modal";
  const summary = document.createElement("summary");
  summary.className = "a2ui-modal-summary";
  const entry = props.entryPointChild ? renderChildById(String(props.entryPointChild), context) : null;
  if (entry) {
    summary.appendChild(entry);
  } else {
    summary.textContent = t("debug.a2ui.action");
  }
  const content = document.createElement("div");
  content.className = "a2ui-modal-content";
  const modalChild = props.contentChild ? renderChildById(String(props.contentChild), context) : null;
  if (modalChild) {
    content.appendChild(modalChild);
  }
  wrapper.appendChild(summary);
  wrapper.appendChild(content);
  return wrapper;
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
    case "Video":
      element = renderVideo(props, context);
      break;
    case "AudioPlayer":
      element = renderAudioPlayer(props, context);
      break;
    case "Tabs":
      element = renderTabs(props, context, renderChildById);
      break;
    case "Modal":
      element = renderModal(props, context, renderChildById);
      break;
    case "Divider":
      element = renderDivider(props);
      break;
    case "Icon":
      element = renderIcon(props, context);
      break;
    case "CheckBox":
    case "Checkbox":
      element = renderCheckbox(props, context);
      break;
    case "TextField":
      element = renderTextField(props, context);
      break;
    case "DateTimeInput":
      element = renderDateTimeInput(props, context);
      break;
    case "MultipleChoice":
      element = renderMultipleChoice(props, context);
      break;
    case "Slider":
      element = renderSlider(props, context);
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

// 按rootId 渲染整个 surface。
const renderSurface = (surface) => {
  if (!surface || !surface.rootId) {
    return;
  }
  const contentRoot = surface.contentRoot || surface.container;
  if (!contentRoot) {
    return;
  }
  // 清空旧渲染，保留标题。
  if (contentRoot === surface.container) {
    while (contentRoot.children.length > 1) {
      contentRoot.removeChild(contentRoot.lastChild);
    }
  } else {
    contentRoot.textContent = "";
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

// 对外：清空A2UI 渲染状态。
export const resetA2uiState = (container) => {
  a2uiState.surfaces.clear();
  if (container) {
    container.innerHTML = "";
  }
};

// 对外：应用A2UI 消息列表并渲染。
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


