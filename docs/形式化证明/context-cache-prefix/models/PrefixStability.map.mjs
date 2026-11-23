// 上下文缓存前缀冻结：模型 ↔ 代码索引。
//
// 这个文件同时承担两件事：
// 1. 作为可导入的 ES module，导出模型动作/不变式到真实代码位置的映射
//    （scripts/check_context_cache_prefix_formal.py 只复制、不解析它）；
// 2. 提供 skills/statechart 的 from-spec 所需的 state()/step() 标签，
//    使真实跑 TLC 时无需再生成骨架映射。
//
// 代码位置一律使用仓库相对路径。

const invariants = [
  {
    name: "TypeOK",
    code: "crates/wunder-runtime/src/orchestrator/context.rs#stable_json_message_hash",
    note: "消息哈希的类型域：每条消息按规范化 JSON 取 16 位十六进制哈希",
  },
  {
    name: "AppendOnlyPrefix",
    code: "crates/wunder-runtime/src/orchestrator/context.rs#messages_extend_prefix",
    note: "本轮 messages 必须逐字重复上一轮已发送前缀；前缀分歧由 prefix_divergence_index 给出",
  },
  {
    name: "NoSilentRewrite",
    code: "crates/wunder-runtime/src/orchestrator/execute.rs#context_cache_prefix",
    note: "前缀被改写时必须发出 context_cache_prefix（append_only:false）事件，不允许静默改写",
  },
  {
    name: "CompactionOnce",
    code: "crates/wunder-runtime/src/orchestrator/memory.rs#maybe_compact_messages",
    note: "同一会话只压缩一次，避免同一前缀反复付全价",
  },
  {
    name: "ReplayKeepsSystem",
    code: "crates/wunder-runtime/src/orchestrator/context.rs#normalize_messages",
    note: "重放已发送前缀时，首条 system 消息保持原样，不被归一化重写",
  },
  {
    name: "TailPreserved",
    code: "crates/wunder-runtime/src/orchestrator/memory.rs#maybe_compact_messages",
    note: "压缩后保留 summary 与尾部窗口，不能把历史清成空序列",
  },
];

const actions = [
  {
    name: "Step",
    code: "crates/wunder-runtime/src/orchestrator/execute.rs#context_cache_prefix",
    note: "组装本轮 messages、比对上一轮前缀哈希并发送（等价于 Append / Retry）",
  },
  {
    name: "Compact",
    code: "crates/wunder-runtime/src/orchestrator/memory.rs#maybe_compact_messages",
    note: "摘要替换历史头部，改写已发送前缀",
  },
  {
    name: "Prune",
    code: "crates/wunder-runtime/src/services/image_context.rs#enforce_image_budget",
    note: "旧图片/工具结果就地换成占位符，长度不变、字节改变（等价于 RewriteAt）",
  },
  {
    name: "Restart",
    code: "crates/wunder-runtime/src/orchestrator/execute.rs#context_cache_prefix",
    note: "新一轮用户轮次或新会话：从存储重建上下文，开一个新缓存纪元",
  },
];

export const modelMap = {
  spec: "PrefixStability.tla",
  fixed: "PrefixStabilityFixed.tla（由 check_context_cache_prefix_formal.py 生成）",
  config: "PrefixStability.cfg",
  invariants,
  actions,
};

export default invariants;

// statechart 读取的字段：把内部变量说成读者能看懂的当前状态。
export const readerActions = [];

export function state(vars) {
  const sent = Array.isArray(vars?.sent) ? vars.sent.length : 0;
  const surface = Array.isArray(vars?.surface) ? vars.surface.length : 0;
  const marks = [];
  if (vars?.compacted) marks.push("已压缩");
  if (vars?.rewrote) marks.push("已改写");
  if (vars?.reset) marks.push("新缓存纪元");
  const suffix = marks.length ? `，${marks.join("、")}` : "";
  return `已发送 ${sent} 条，待发 ${surface} 条${suffix}`;
}

export function step(action) {
  const labels = {
    Step: "追加一条消息并发送",
    Compact: "压缩历史，摘要替换头部",
    Prune: "剪枝旧消息，换成占位符",
    Restart: "新轮次重建上下文",
  };
  return labels[action] ?? action;
}
