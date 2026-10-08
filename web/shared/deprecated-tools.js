// 蜂群编排（蜂群工具、蜂房、编排态、team run、蜂群包）已全链路移除，舰桥不再暴露任何蜂群入口。
//
// 【待后端移除后一并删除该过滤】
// 后端 /admin/tools、/tools 等接口可能仍下发历史蜂群工具（agent_swarm / swarm_control / 智能体蜂群）。
// 在服务端彻底下线这些工具之前，前端在数据入口处集中做一次过滤，避免它们再次出现在
// 工具可见性树、内置工具面板、工具详情弹窗、工具选择器与监控统计口径中。
// 后端确认不再下发后，删除本文件及其全部 import 调用点即可。

// 精确匹配：工具名（name/runtime_name/tool_name 等）
const REMOVED_SWARM_TOOL_NAMES = new Set(["agent_swarm", "swarm_control"]);
// 精确匹配：显示名/别名（display_name/title/label 等）
const REMOVED_SWARM_TOOL_ALIASES = new Set(["智能体蜂群"]);

// 参与精确匹配的字段，按后端可能使用的命名逐项覆盖
const TOOL_IDENTITY_FIELDS = [
  "name",
  "runtimeName",
  "runtime_name",
  "toolName",
  "tool_name",
  "tool",
  "displayName",
  "display_name",
  "title",
  "label",
  "alias",
  "aliases",
];

const normalizeToken = (value) => String(value ?? "").trim().toLowerCase();

const isRemovedToken = (value) => {
  const token = normalizeToken(value);
  if (!token) {
    return false;
  }
  return REMOVED_SWARM_TOOL_NAMES.has(token) || REMOVED_SWARM_TOOL_ALIASES.has(token);
};

// 判断单个工具（字符串或对象）是否为已移除的蜂群工具
export const isRemovedSwarmTool = (input) => {
  if (typeof input === "string") {
    return isRemovedToken(input);
  }
  if (!input || typeof input !== "object") {
    return false;
  }
  return TOOL_IDENTITY_FIELDS.some((field) => {
    const value = input[field];
    if (Array.isArray(value)) {
      return value.some((item) => isRemovedToken(item));
    }
    return isRemovedToken(value);
  });
};

// 过滤工具清单，保持入参顺序；非数组入参返回空数组
export const filterRemovedSwarmTools = (list) =>
  (Array.isArray(list) ? list : []).filter((item) => !isRemovedSwarmTool(item));

// 过滤可见性规则等以 name 为键的规则清单
export const filterRemovedSwarmToolRules = (rules) =>
  (Array.isArray(rules) ? rules : []).filter((rule) => !isRemovedSwarmTool(rule?.name ?? rule));
