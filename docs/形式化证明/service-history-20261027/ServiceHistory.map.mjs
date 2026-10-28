export const readerActions = ["Emit", "MetadataRead", "Commit", "ReadCount"];
export function state(value) {
  return [value.events.some(size => size > 2) ? "单条事件超预算" : "诊断在预算内", value.copiedBytes ? "元信息复制正文" : "元信息不复制正文", value.scannedRows ? "统计扫描历史" : "统计不扫描历史", value.indexed.length ? "已有投影" : "空投影"].join("，");
}
export function step(action) {
  return { Emit: "收到模型或工具事件", MetadataRead: "运行时读取元信息", Commit: "原子写入或重放投影", ReadCount: "舰桥读取条目统计" }[action] || action;
}
export function cause(value, facts, invariant) {
  return { BoundedDiagnostics: "限制条数未限制单条正文", MetadataNoBodyCopies: "元信息读取复制诊断正文", ConstantCountRead: "热查询重新扫描历史条目", ExactProjectionCount: "投影计数与成功插入数量不一致" }[invariant];
}
export const legend = { works: "正常", wrong: "修复前风险", fixed: "修复后不可达" };
