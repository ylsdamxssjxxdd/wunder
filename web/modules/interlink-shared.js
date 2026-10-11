// 舰桥「互通舰队」共享层：管理端请求、契约状态、格式化、分页器、抽屉、二次确认。
//
// 契约见 docs/云端本地互通方案.md §3.2 管理面 / §5.2 / §9.2 / §9.4：
//   GET   /wunder/admin/interlink/fleet?offset=&limit=&client=&status=&user_id=
//   GET   /wunder/admin/interlink/channels?offset=&limit=&user_id=&device_id=
//   GET   /wunder/admin/interlink/commands?offset=&limit=&user_id=&device_id=&kind=&status=&direction=
//   GET   /wunder/admin/interlink/audit?offset=&limit=&user_id=&device_id=&action=&since=&until=&format=csv
//   GET   /wunder/admin/interlink/runtime（只读快照：live_channels / rtt_p50_ms / rtt_p95_ms /
//         channels / commands / watched_threads / blob_cache_bytes / open_streams / alerts）
//   PATCH /wunder/admin/interlink/devices/{id}/policy
//   POST  /wunder/admin/interlink/devices/{id}/rotate_secret
//   DELETE /wunder/admin/cloud/devices/{id}（复用既有吊销端点）
//
// 鉴权：所有请求走 app.js 的全局 fetch 包装（自动补 Authorization / X-API-Key /
// X-Wunder-Language），后端由 /wunder/admin/* 守卫强制管理员身份；前端确认不能
// 替代后端鉴权，这里只负责把后端拒绝的原因显式呈现。
//
// 契约三态（unknown / ready / unavailable）：端点 404 或 403/401 时置
// unavailable 并短路后续管理端请求，避免在同一会话里反复打不可用端点。
// 例外：/runtime 是新增只读端点，404 只表示「契约未就绪」，用 adminGet 的
// missingOk 选项走 EndpointMissingError，不锁死整条管理端契约。

import { getWunderBase } from "./api.js";
import { openImpactConfirmModal } from "./preset-agents.js?v=20261007-01";

// 后端 CSV 导出是「单有界页」，上限由响应头 x-wunder-audit-csv-max-rows 回传，
// 这里同步一份用于界面声明（与服务端 CSV_PAGE_MAX 一致）。
export const CSV_MAX_ROWS_FALLBACK = 2000;

export const NODE_TYPES = ["desktop", "cli", "web"];
export const NODE_STATUSES = ["online", "busy", "away", "reconnecting", "offline"];
export const COMMAND_STATUSES = [
  "issued",
  "queued",
  "acked",
  "running",
  "succeeded",
  "failed",
  "canceled",
  "timeout",
];
export const APPROVAL_STATES = ["none", "pending", "approved", "rejected", "expired"];
export const DIRECTIONS = ["c2l", "l2c"];
export const AUDIT_ACTIONS = [
  "channel.open",
  "channel.close",
  "channel.rejected",
  "command.issue",
  "command.ack",
  "command.finish",
  "approval.decide",
  "file.read",
  "file.write",
  "shadow.sync",
  "policy.update",
  "secret.rotate",
  "secret.issue",
  // 治理告警：alerts.rs 的三类触发都写这一行（见 ALERT_TRIGGERS）
  "alert.raised",
];
// alert.raised 行的 detail.trigger 取值（crates/wunder-runtime/src/services/interlink/alerts.rs）
export const ALERT_TRIGGERS = ["l3_execution", "rejection_storm", "secret_stale_version"];
// §7.1 冻结命令目录（与 crates/wunder-core/src/interlink.rs 一致）
export const COMMAND_KINDS = [
  "node.summary",
  "shadow.refresh",
  "workspace.list",
  "workspace.read",
  "workspace.search",
  "workspace.stat",
  "threads.list",
  "threads.get",
  "thread.create",
  "thread.message",
  "thread.cancel",
  "thread.answer",
  "workspace.write",
  "workspace.mkdir",
  "workspace.move",
  "workspace.copy",
  "workspace.delete",
  "tool.exec",
  "agent.spawn",
];
// §9.2 服务端认识的能力集（与 api/interlink_ws::known_capabilities 一致）
export const KNOWN_CAPS = [
  "query.basic",
  "workspace.read.binary",
  "shadow:full",
  "shadow:minimal",
  "thread.drive",
  "workspace.write",
  "tool.exec",
  "agent.spawn",
];
export const DEFAULT_DEVICE_CAPS = ["shadow:minimal", "query.basic", "thread.drive"];

export const PAGE_SIZES = [25, 50, 100, 200];

const TYPE_ICONS = {
  desktop: "fa-solid fa-desktop",
  cli: "fa-solid fa-terminal",
  web: "fa-solid fa-globe",
  server: "fa-solid fa-server",
};

const TEXT = {
  "zh-CN": {
    loading: "加载中…",
    retry: "重试",
    empty: "无数据",
    requestFailed: "请求失败（HTTP {status}）",
    unknownError: "未知错误",
    contractUnavailable: "管理端契约不可用：{reason}。已短路后续互通管理请求，点刷新可重试。",
    disabledServer: "服务端未启用互通（interlink.enabled=false）。",
    forbidden: "需要管理员权限（/wunder/admin/* 守卫拒绝）。",
    // 分页
    pageInfo: "共 {total} 条 · 第 {page}/{pages} 页",
    pageSize: "每页",
    prev: "上一页",
    next: "下一页",
    scannedNote: "统计口径：本页 {scanned} 台（服务端总数 {total} 台），非全表汇总。",
    csvNote: "CSV 为单有界页导出，最多 {max} 条。",
    csvExport: "导出 CSV",
    csvDone: "已导出 {rows} 条审计记录。",
    csvFailed: "CSV 导出失败：{message}",
    // 状态
    statusOnline: "在线",
    statusBusy: "忙碌",
    statusAway: "离开",
    statusReconnecting: "重连中",
    statusOffline: "离线",
    statusUnknown: "未知",
    never: "从未活跃",
    // 总览
    statOnlineTotal: "在线 / 总数",
    statLiveTunnels: "活跃隧道",
    statByClient: "按客户端",
    statRtt: "RTT p50 / p95",
    statReconnectTop: "重连 TopN",
    heatmapTitle: "24h 在线率（按小时，取自隧道通道记录）",
    heatmapHint: "色深＝该小时内有隧道的设备占比",
    qualityTitle: "隧道质量",
    topNone: "无重连记录",
    noSample: "无样本",
    // 互通告警与运行时（只读面板）
    alertsTitle: "互通告警与运行时",
    alertsTip: "只读治理面：告警计数与运行时快照来自 /admin/interlink/runtime，告警行来自审计端点。",
    endpointMissing: "契约未就绪：{path} 尚不可用（后端未实现或已过时）。",
    runtimeStateLoading: "运行时快照读取中…",
    runtimeStateFailed: "运行时快照读取失败：{message}",
    alertCountersTitle: "告警计数",
    alertCountersHint: "累计值，进程重启后归零",
    alertRuntimeTitle: "运行时指标",
    alertListTitle: "最近告警",
    alertListHint: "默认筛选 action=alert.raised，逐页读取",
    statRaised: "已产生",
    statDropped: "已丢弃",
    statDelivered: "已投递",
    statWebhookFailures: "webhook 失败",
    statQueueCapacity: "队列容量",
    statTracked: "检测态设备",
    statPump: "告警泵",
    pumpRunning: "运行中",
    pumpStopped: "未运行",
    pumpStoppedWarn: "告警泵未运行：新告警只计入「已丢弃」，不会投递（本地形态不起泵）。",
    webhookFailureWarn: "webhook 失败 {count} 次：投递链路不通，告警只落审计。",
    metricLiveChannels: "在线隧道",
    metricChannels: "通道记录",
    metricRtt: "RTT p50 / p95",
    metricBlobCache: "Blob 缓存",
    metricOpenStreams: "打开的流",
    metricWatchedThreads: "被监听线程",
    metricCommandStats: "在途 / 排队命令",
    alertColTrigger: "触发",
    alertColDevice: "设备",
    alertColKind: "类型 / 等级",
    alertColLevel: "等级",
    alertColResult: "结果",
    alertColCount: "计数",
    alertColSeq: "审计序号",
    alertColActor: "主体",
    alertColCommand: "命令",
    alertTriggerUnknown: "未归类",
    alertTriggerL3Execution: "L3 执行",
    alertTriggerRejectionStorm: "拒绝风暴",
    alertTriggerSecretStale: "旧密钥握手",
    alertDrawerTitle: "告警详情",
    alertDrawerHint: "告警行只含标识符（§9.3 红线），正文不进审计。",
    alertFilterThisDevice: "只看该设备的告警",
    alertFilterDevice: "设备",
    alertFilterAction: "审计动作",
    alertNoDevice: "该告警未带设备标识。",
    // 表格
    colDevice: "设备",
    colClient: "客户端",
    colUser: "账号",
    colStatus: "状态",
    colTunnel: "隧道",
    colShadow: "影子 rev",
    colRtt: "RTT",
    colResumed: "重连",
    colSeen: "最近活跃",
    colInterlink: "互通",
    colActions: "操作",
    detail: "详情",
    colChannel: "通道",
    colProtocol: "协议",
    colCaps: "能力",
    colConnected: "建链时间",
    colLastBeat: "最近心跳",
    colClosedReason: "关闭原因",
    colLive: "在线",
    colTime: "时间",
    colKind: "类型",
    colDirection: "方向",
    colCommandStatus: "状态",
    colApproval: "审批",
    colRoute: "链路",
    colDigest: "参数摘要",
    colDuration: "时长",
    colError: "错误",
    colSeq: "序号",
    colAction: "动作",
    colActor: "主体",
    colCommand: "命令",
    colApprovalId: "审批单",
    colDetailDigest: "详情摘要",
    liveYes: "在线",
    liveNo: "已关闭",
    tunnelUp: "隧道在线",
    tunnelDown: "隧道断开",
    interlinkOn: "已允许",
    interlinkOff: "已暂停",
    revokedBadge: "已吊销",
    filterUser: "账号",
    filterUserHint: "用户 ID",
    filterDevice: "设备",
    filterDeviceHint: "device_id",
    filterClient: "客户端",
    filterStatus: "状态",
    filterAll: "全部",
    filterKind: "命令类型",
    filterDirection: "方向",
    filterApproval: "审批状态",
    filterAction: "审计动作",
    filterSince: "起始",
    filterUntil: "截止",
    filterReset: "重置",
    apply: "查询",
    // 抽屉
    drawerNodeTitle: "节点详情",
    drawerCommandTitle: "命令生命周期",
    close: "关闭",
    shadowSection: "影子摘要",
    shadowRevision: "revision",
    shadowNever: "未同步",
    shadowOwnerOnly: "影子正文仅归属账号可读（§9.3 隐私红线），治理面只显示版本与时间。",
    lastTunnel: "最近建链",
    secretVersion: "密钥版本",
    rotatedAt: "最近轮换",
    osArch: "系统 / 架构 / 版本",
    capsSection: "生效能力",
    capsNone: "无（已被策略收敛到空集）",
    policySection: "策略 overrides",
    policyNone: "无收敛（沿用节点声明 ∩ 默认许可）",
    disabledKinds: "禁用类型",
    disabledCaps: "禁用能力",
    forceApproval: "强制审批",
    shadowMode: "影子模式",
    commandsSection: "最近命令（该节点为目标的 20 条）",
    channelsSection: "最近隧道开合（20 条）",
    opsSection: "策略操作",
    opsHint: "每项操作都会写审计，并在生效时立即关闭该节点的在线隧道。",
    opsDenied: "该操作被后端拒绝：{reason}",
    opsOk: "已生效：{detail}",
    opsPending: "提交中…",
    opPause: "暂停互通",
    opResume: "恢复互通",
    opPauseDone: "互通已暂停",
    opResumeDone: "互通已恢复",
    opAlreadyPaused: "互通已处于暂停状态",
    opAlreadyActive: "互通已处于允许状态",
    opRevokeKind: "禁用类型",
    opEnableKind: "解禁",
    opForceApproval: "强制审批",
    opUnforceApproval: "取消强制审批",
    opShadowMinimal: "强制 minimal 影子",
    opShadowFull: "解除 minimal",
    opDropCap: "移除能力",
    opRestoreCaps: "恢复默认能力集",
    opRotateSecret: "强制轮换密钥",
    opRevokeDevice: "吊销设备",
    revokedDeviceBlocked: "设备已吊销，策略与密钥操作不可用。",
    confirmPauseTitle: "暂停该节点的互通",
    confirmPauseSummary:
      "服务端将拒绝新建隧道，并立即关闭该节点当前的在线隧道；蜂巢侧的远端入口随之消失。云模型调用不受影响（§13.5.16）。",
    confirmResumeTitle: "恢复该节点的互通",
    confirmResumeSummary:
      "互通开关置为允许；节点需自行重连后隧道才恢复（服务端不主动外连）。",
    confirmRotateTitle: "强制轮换节点密钥",
    confirmRotateSummary:
      "secret_version +1 并只保存新指纹；旧密钥进入 24h 双密钥宽限，当前隧道会被立即关闭。密钥本体不入库也不返回（§9.1）。",
    confirmRevokeTitle: "吊销设备",
    confirmRevokeSummary:
      "两步执行：先暂停互通（立刻关闭隧道），再调用既有云端吊销端点。此后该设备的全部互通端点返回 401（§13.5.18）。",
    confirmRevokeAck: "我确认吊销该设备，其后续互通请求将被拒绝。",
    confirmApplyTitle: "下发策略变更",
    confirmApplySummary: "变更写入设备策略并立即关闭在线隧道，操作进入审计。",
    confirmOk: "确认下发",
    confirmDangerOk: "确认执行",
    detailDevice: "设备 {device}",
    detailUser: "账号 {user}",
    detailShadow: "影子 rev {revision}",
    step1: "第 1 步",
    step2: "第 2 步",
    stepFailed: "第 {step} 步失败：{message}",
    timelineIssued: "已发起",
    timelineAcked: "已确认",
    timelineFinished: "已结束",
    timelineApproval: "审批 {state}",
    timelineStatus: "状态 {status}",
    timelineNoAck: "未确认（无隧道或已超时）",
    timelineNoFinish: "未结束",
    ackLatency: "确认延迟",
    totalDuration: "总时长",
    digestFields: "摘要字段",
    digestHash: "参数哈希",
    noBodyHint: "摘要不含正文（§9.3）",
  },
  "en-US": {
    loading: "Loading…",
    retry: "Retry",
    empty: "No data",
    requestFailed: "Request failed (HTTP {status})",
    unknownError: "unknown error",
    contractUnavailable: "Admin contract unavailable: {reason}. Interlink admin requests are short-circuited; use refresh to retry.",
    disabledServer: "Interlink is disabled on this server (interlink.enabled=false).",
    forbidden: "Administrator rights required (the /wunder/admin/* guard refused the call).",
    pageInfo: "{total} rows · page {page}/{pages}",
    pageSize: "Page size",
    prev: "Previous",
    next: "Next",
    scannedNote: "Scope: this page only ({scanned} of {total} devices server-side), not a full-table rollup.",
    csvNote: "CSV export is a single bounded page: at most {max} rows.",
    csvExport: "Export CSV",
    csvDone: "Exported {rows} audit rows.",
    csvFailed: "CSV export failed: {message}",
    statusOnline: "Online",
    statusBusy: "Busy",
    statusAway: "Away",
    statusReconnecting: "Reconnecting",
    statusOffline: "Offline",
    statusUnknown: "Unknown",
    never: "never",
    statOnlineTotal: "Online / total",
    statLiveTunnels: "Live tunnels",
    statByClient: "By client",
    statRtt: "RTT p50 / p95",
    statReconnectTop: "Reconnect TopN",
    heatmapTitle: "24h online rate by hour (from tunnel channel records)",
    heatmapHint: "Darker = larger share of tunnel-bearing devices online in that hour",
    qualityTitle: "Tunnel quality",
    topNone: "no reconnect records",
    noSample: "no sample",
    alertsTitle: "Interlink alerts & runtime",
    alertsTip:
      "Read-only governance: alert counters and the runtime snapshot come from /admin/interlink/runtime, alert rows from the audit endpoint.",
    endpointMissing: "Contract not ready: {path} is not available yet (backend not implemented or stale).",
    runtimeStateLoading: "Reading the runtime snapshot…",
    runtimeStateFailed: "Runtime snapshot failed: {message}",
    alertCountersTitle: "Alert counters",
    alertCountersHint: "Process-lifetime totals, reset on restart",
    alertRuntimeTitle: "Runtime metrics",
    alertListTitle: "Recent alerts",
    alertListHint: "Filtered to action=alert.raised by default, one page per request",
    statRaised: "Raised",
    statDropped: "Dropped",
    statDelivered: "Delivered",
    statWebhookFailures: "Webhook failures",
    statQueueCapacity: "Queue capacity",
    statTracked: "Tracked devices",
    statPump: "Alert pump",
    pumpRunning: "running",
    pumpStopped: "stopped",
    pumpStoppedWarn:
      "Alert pump is not running: new alerts only count towards Dropped and are never delivered (local forms do not spawn the pump).",
    webhookFailureWarn: "Webhook failed {count} times: delivery is unreachable, alerts only land in the audit trail.",
    metricLiveChannels: "Live tunnels",
    metricChannels: "Channel records",
    metricRtt: "RTT p50 / p95",
    metricBlobCache: "Blob cache",
    metricOpenStreams: "Open streams",
    metricWatchedThreads: "Watched threads",
    metricCommandStats: "In flight / queued commands",
    alertColTrigger: "Trigger",
    alertColDevice: "Device",
    alertColKind: "Kind / level",
    alertColLevel: "Level",
    alertColResult: "Result",
    alertColCount: "Count",
    alertColSeq: "Audit seq",
    alertColActor: "Actor",
    alertColCommand: "Command",
    alertTriggerUnknown: "unclassified",
    alertTriggerL3Execution: "L3 execution",
    alertTriggerRejectionStorm: "Rejection storm",
    alertTriggerSecretStale: "Stale secret handshake",
    alertDrawerTitle: "Alert detail",
    alertDrawerHint: "An alert row carries identifiers only (§9.3 red line); payloads never reach the audit trail.",
    alertFilterThisDevice: "Show only this device",
    alertFilterDevice: "Device",
    alertFilterAction: "Audit action",
    alertNoDevice: "This alert carries no device identifier.",
    colDevice: "Device",
    colClient: "Client",
    colUser: "Account",
    colStatus: "Status",
    colTunnel: "Tunnel",
    colShadow: "Shadow rev",
    colRtt: "RTT",
    colResumed: "Resumes",
    colSeen: "Last seen",
    colInterlink: "Interlink",
    colActions: "Actions",
    detail: "Detail",
    colChannel: "Channel",
    colProtocol: "Proto",
    colCaps: "Caps",
    colConnected: "Opened",
    colLastBeat: "Last beat",
    colClosedReason: "Closed reason",
    colLive: "Live",
    colTime: "Time",
    colKind: "Kind",
    colDirection: "Direction",
    colCommandStatus: "Status",
    colApproval: "Approval",
    colRoute: "Route",
    colDigest: "Args digest",
    colDuration: "Duration",
    colError: "Error",
    colSeq: "Seq",
    colAction: "Action",
    colActor: "Actor",
    colCommand: "Command",
    colApprovalId: "Approval",
    colDetailDigest: "Detail digest",
    liveYes: "live",
    liveNo: "closed",
    tunnelUp: "tunnel up",
    tunnelDown: "tunnel down",
    interlinkOn: "allowed",
    interlinkOff: "paused",
    revokedBadge: "revoked",
    filterUser: "Account",
    filterUserHint: "user id",
    filterDevice: "Device",
    filterDeviceHint: "device_id",
    filterClient: "Client",
    filterStatus: "Status",
    filterAll: "All",
    filterKind: "Kind",
    filterDirection: "Direction",
    filterApproval: "Approval",
    filterAction: "Action",
    filterSince: "Since",
    filterUntil: "Until",
    filterReset: "Reset",
    apply: "Query",
    drawerNodeTitle: "Node detail",
    drawerCommandTitle: "Command lifecycle",
    close: "Close",
    shadowSection: "Shadow summary",
    shadowRevision: "revision",
    shadowNever: "never synced",
    shadowOwnerOnly:
      "Shadow bodies are readable only by the owning account (§9.3 privacy red line); governance shows revision and time.",
    lastTunnel: "Last tunnel up",
    secretVersion: "Secret version",
    rotatedAt: "Last rotation",
    osArch: "OS / arch / version",
    capsSection: "Effective capabilities",
    capsNone: "none (converged to an empty set)",
    policySection: "Policy overrides",
    policyNone: "no convergence (declared ∩ default grant)",
    disabledKinds: "Disabled kinds",
    disabledCaps: "Disabled caps",
    forceApproval: "Forced approval",
    shadowMode: "Shadow mode",
    commandsSection: "Recent commands (20 targeting this node)",
    channelsSection: "Recent channel open/close (20)",
    opsSection: "Policy operations",
    opsHint: "Every operation is audited and closes the live tunnel when it takes effect.",
    opsDenied: "Backend denied this operation: {reason}",
    opsOk: "Applied: {detail}",
    opsPending: "Submitting…",
    opPause: "Pause interlink",
    opResume: "Resume interlink",
    opPauseDone: "interlink paused",
    opResumeDone: "interlink resumed",
    opAlreadyPaused: "interlink is already paused",
    opAlreadyActive: "interlink is already allowed",
    opRevokeKind: "Disable kind",
    opEnableKind: "re-enable",
    opForceApproval: "Force approval",
    opUnforceApproval: "unforce",
    opShadowMinimal: "Force minimal shadow",
    opShadowFull: "Clear minimal shadow",
    opDropCap: "Drop capability",
    opRestoreCaps: "Restore default caps",
    opRotateSecret: "Rotate secret",
    opRevokeDevice: "Revoke device",
    revokedDeviceBlocked: "Device is revoked; policy and secret operations are unavailable.",
    confirmPauseTitle: "Pause interlink for this node",
    confirmPauseSummary:
      "The server rejects new tunnels and closes the node's live tunnel immediately; hive remote entries disappear with it. Cloud model calls are unaffected (§13.5.16).",
    confirmResumeTitle: "Resume interlink for this node",
    confirmResumeSummary:
      "The kill switch is set back to allowed; the tunnel comes back when the node reconnects (the server never dials out).",
    confirmRotateTitle: "Force node secret rotation",
    confirmRotateSummary:
      "secret_version advances by one and only the new fingerprint is stored; the previous key stays in a 24h dual-key grace window and the live tunnel is closed. The secret itself is never stored or returned (§9.1).",
    confirmRevokeTitle: "Revoke device",
    confirmRevokeSummary:
      "Two steps: pause interlink first (closing the live tunnel), then call the existing cloud revoke endpoint. Afterwards every interlink endpoint answers 401 for this device (§13.5.18).",
    confirmRevokeAck: "I confirm revoking this device; its later interlink requests will be denied.",
    confirmApplyTitle: "Apply policy change",
    confirmApplySummary:
      "The change is written to the device policy, closes the live tunnel immediately and lands in the audit trail.",
    confirmOk: "Apply",
    confirmDangerOk: "Confirm",
    detailDevice: "Device {device}",
    detailUser: "Account {user}",
    detailShadow: "shadow rev {revision}",
    step1: "step 1",
    step2: "step 2",
    stepFailed: "{step} failed: {message}",
    timelineIssued: "issued",
    timelineAcked: "acked",
    timelineFinished: "finished",
    timelineApproval: "approval {state}",
    timelineStatus: "status {status}",
    timelineNoAck: "not acked (no tunnel or timed out)",
    timelineNoFinish: "not finished",
    ackLatency: "ack latency",
    totalDuration: "total",
    digestFields: "digest fields",
    digestHash: "args hash",
    noBodyHint: "the digest carries no payload (§9.3)",
  },
};

// ---------------------------------------------------------------------------
// 语言与文案
// ---------------------------------------------------------------------------

const LANG_STORAGE_KEY = "wunder_app_config";

export const resolveLanguage = () => {
  try {
    const fromDom = String(document?.documentElement?.lang || "").trim();
    if (fromDom) {
      return fromDom;
    }
    const raw = localStorage.getItem(LANG_STORAGE_KEY);
    if (raw) {
      const parsed = JSON.parse(raw);
      const stored = String(parsed?.language || "").trim();
      if (stored) {
        return stored;
      }
    }
  } catch (error) {
    // 忽略：退回默认语言
  }
  return "zh-CN";
};

export const t = (key, vars) => {
  const table = TEXT[resolveLanguage()] || TEXT["zh-CN"];
  let text = table[key] ?? TEXT["zh-CN"][key] ?? key;
  if (vars && typeof vars === "object") {
    Object.entries(vars).forEach(([name, value]) => {
      text = text.split(`{${name}}`).join(String(value));
    });
  }
  return text;
};

export const statusLabel = (status) =>
  t(
    {
      online: "statusOnline",
      busy: "statusBusy",
      away: "statusAway",
      reconnecting: "statusReconnecting",
      offline: "statusOffline",
    }[status] || "statusUnknown"
  );

// ---------------------------------------------------------------------------
// 契约状态（unknown / ready / unavailable）
// ---------------------------------------------------------------------------

export const contract = {
  status: "unknown",
  reason: "",
  markUnavailable(reason) {
    this.status = "unavailable";
    this.reason = reason || "";
  },
  markReady() {
    this.status = "ready";
    this.reason = "";
  },
  reset() {
    this.status = "unknown";
    this.reason = "";
  },
  isBlocked() {
    return this.status === "unavailable";
  },
};

// ---------------------------------------------------------------------------
// DOM 小工具
// ---------------------------------------------------------------------------

export const el = (tag, className, text) => {
  const node = document.createElement(tag);
  if (className) {
    node.className = className;
  }
  if (text !== undefined && text !== null) {
    node.textContent = String(text);
  }
  return node;
};

export const clearNode = (node) => {
  if (node) {
    node.textContent = "";
  }
};

export const badge = (text, variant, title = "") => {
  const node = el("span", `monitor-status${variant ? ` ${variant}` : ""}`, text);
  if (title) {
    node.title = title;
  }
  return node;
};

export const statusBadge = (status) =>
  badge(statusLabel(status), `interlink-${NODE_STATUSES.includes(status) ? status : "away"}`, status || "");

export const triggerLabel = (trigger) =>
  t(
    {
      l3_execution: "alertTriggerL3Execution",
      rejection_storm: "alertTriggerRejectionStorm",
      secret_stale_version: "alertTriggerSecretStale",
    }[trigger] || "alertTriggerUnknown"
  );

export const chip = (text, options = {}) => {
  const node = el("span", options.denied ? "interlink-chip is-denied" : "interlink-chip", text);
  if (options.title) {
    node.title = options.title;
  }
  if (typeof options.onRemove === "function") {
    const button = el("button", "", "×");
    button.type = "button";
    button.title = options.removeTitle || "";
    button.addEventListener("click", (event) => {
      event.stopPropagation();
      options.onRemove();
    });
    node.appendChild(button);
  }
  return node;
};

export const typeIcon = (type) => TYPE_ICONS[type] || "fa-solid fa-circle-nodes";

// ---------------------------------------------------------------------------
// 时间格式化（契约时间戳是秒；对毫秒量级做防御性兼容）
// ---------------------------------------------------------------------------

export const toEpochMs = (value) => {
  const ts = Number(value);
  if (!Number.isFinite(ts) || ts <= 0) {
    return 0;
  }
  return ts > 1e12 ? ts : ts * 1000;
};

export const formatClock = (value) => {
  const ms = toEpochMs(value);
  if (!ms) {
    return "-";
  }
  try {
    return new Date(ms).toLocaleString();
  } catch (error) {
    return "-";
  }
};

export const formatRelative = (value) => {
  const ms = toEpochMs(value);
  if (!ms) {
    return t("never");
  }
  const diffSeconds = Math.round((Date.now() - ms) / 1000);
  if (!Number.isFinite(diffSeconds)) {
    return t("never");
  }
  try {
    const formatter = new Intl.RelativeTimeFormat(resolveLanguage(), { numeric: "auto" });
    if (Math.abs(diffSeconds) < 60) {
      return formatter.format(-diffSeconds, "second");
    }
    const minutes = Math.round(diffSeconds / 60);
    if (Math.abs(minutes) < 60) {
      return formatter.format(-minutes, "minute");
    }
    const hours = Math.round(minutes / 60);
    if (Math.abs(hours) < 48) {
      return formatter.format(-hours, "hour");
    }
    return formatter.format(-Math.round(hours / 24), "day");
  } catch (error) {
    return formatClock(ms);
  }
};

export const formatMs = (value) => {
  const num = Number(value);
  if (!Number.isFinite(num)) {
    return "-";
  }
  if (num < 1000) {
    return `${Math.round(num)} ms`;
  }
  const seconds = num / 1000;
  if (seconds < 120) {
    return `${seconds.toFixed(seconds < 10 ? 2 : 1)} s`;
  }
  return `${Math.round(seconds / 60)} min`;
};

export const formatRtt = (value) => {
  const num = Number(value);
  return Number.isFinite(num) ? `${Math.round(num)} ms` : "-";
};

// datetime-local <-> epoch seconds（审计时间窗）
export const readLocalTime = (input) => {
  const raw = String(input?.value || "").trim();
  if (!raw) {
    return null;
  }
  const parsed = Date.parse(raw);
  return Number.isFinite(parsed) ? parsed / 1000 : null;
};

export const toCsv = (value) => {
  if (value === null || value === undefined) {
    return "";
  }
  if (typeof value === "object") {
    try {
      return JSON.stringify(value);
    } catch (error) {
      return String(value);
    }
  }
  return String(value);
};

// ---------------------------------------------------------------------------
// 请求
// ---------------------------------------------------------------------------

export const adminBase = () => `${getWunderBase()}/admin/interlink`;

export const buildQuery = (fields) => {
  const params = new URLSearchParams();
  Object.entries(fields || {}).forEach(([key, value]) => {
    const text = value === null || value === undefined ? "" : String(value).trim();
    if (text) {
      params.set(key, text);
    }
  });
  return params.toString();
};

export const extractResponseMessage = async (response, fallback) => {
  try {
    const payload = await response.json();
    return (
      payload?.error?.message ||
      payload?.error?.detail ||
      payload?.message ||
      payload?.detail?.message ||
      fallback
    );
  } catch (error) {
    return fallback;
  }
};

/// 契约不可用时抛 ContractError，调用方只渲染一次原因，不再重试。
export class ContractError extends Error {
  constructor(reason) {
    super(reason);
    this.name = "ContractError";
    this.contractBlocked = true;
  }
}

/// 单个端点尚未落地（404）：只影响调用它的那块界面，不锁死整条管理端契约。
export class EndpointMissingError extends Error {
  constructor(path) {
    super(t("endpointMissing", { path }));
    this.name = "EndpointMissingError";
    this.endpointMissing = true;
    this.path = path;
  }
}

const blockForStatus = async (response) => {
  const fallback = t("requestFailed", { status: response.status });
  const message = await extractResponseMessage(response, fallback);
  if (response.status === 404) {
    contract.markUnavailable(t("disabledServer"));
  } else if (response.status === 401 || response.status === 403) {
    contract.markUnavailable(t("forbidden"));
  } else if (response.status >= 500) {
    // 5xx 是瞬时故障，不锁死契约，只把消息抛给调用方。
    return new Error(message);
  } else {
    return new Error(message);
  }
  return new Error(`${contract.reason || message}`);
};

/**
 * GET 管理端 JSON，返回 `{ data }` 里的载荷。
 * 404 / 401 / 403 会把契约置为 unavailable 并短路后续请求。
 * `missingOk` 用于新增只读端点：404 抛 EndpointMissingError（界面显示「契约未就绪」），
 * 不影响契约状态，其它管理端请求继续可用。
 */
export const adminGet = async (path, query, options = {}) => {
  if (contract.isBlocked()) {
    throw new ContractError(contract.reason);
  }
  const suffix = query ? `?${query}` : "";
  const response = await fetch(`${adminBase()}${path}${suffix}`, {
    method: "GET",
    credentials: "same-origin",
  });
  if (!response.ok) {
    if (options.missingOk && response.status === 404) {
      throw new EndpointMissingError(`${adminBase()}${path}`);
    }
    const error = await blockForStatus(response);
    if (contract.isBlocked()) {
      throw new ContractError(error.message);
    }
    throw error;
  }
  contract.markReady();
  const payload = await response.json().catch(() => null);
  return payload?.data ?? {};
};

/**
 * PATCH / POST 管理端 JSON。返回 `{ ok, status, data, message }`，
 * 由调用方把拒绝原因显式渲染（不抛异常，便于展示后端文案）。
 */
export const adminSend = async (path, method, body) => {
  const response = await fetch(`${adminBase()}${path}`, {
    method,
    credentials: "same-origin",
    headers: body === undefined ? {} : { "Content-Type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const payload = await response.json().catch(() => null);
  const data = payload?.data ?? null;
  if (!response.ok) {
    const message = await Promise.resolve(
      data ? JSON.stringify(data) : extractResponseMessage(response, t("requestFailed", { status: response.status }))
    );
    return { ok: false, status: response.status, data, message: String(message) };
  }
  contract.markReady();
  return { ok: true, status: response.status, data, message: "" };
};

/** 既有云端吊销端点（DELETE /wunder/admin/cloud/devices/{id}）。 */
export const revokeCloudDevice = async (deviceId) => {
  const response = await fetch(
    `${getWunderBase()}/admin/cloud/devices/${encodeURIComponent(deviceId)}`,
    { method: "DELETE", credentials: "same-origin" }
  );
  if (!response.ok) {
    return {
      ok: false,
      status: response.status,
      message: await extractResponseMessage(response, t("requestFailed", { status: response.status })),
    };
  }
  return { ok: true, status: response.status, message: "" };
};

/** CSV 导出：需要携带鉴权头，因此用 fetch + blob 下载，而不是裸链接。 */
export const downloadCsv = async (query, signal) => {
  const response = await fetch(`${adminBase()}/audit?${query}`, {
    method: "GET",
    credentials: "same-origin",
    signal,
  });
  if (!response.ok) {
    return {
      ok: false,
      message: await extractResponseMessage(response, t("requestFailed", { status: response.status })),
    };
  }
  const maxHeader = Number(response.headers.get("x-wunder-audit-csv-max-rows") || CSV_MAX_ROWS_FALLBACK);
  const rowsHeader = Number(response.headers.get("x-wunder-audit-csv-rows") || 0);
  const disposition = response.headers.get("content-disposition") || "";
  const nameMatch = /filename="?([^";]+)"?/i.exec(disposition);
  const blob = await response.blob();
  const url = URL.createObjectURL(blob);
  const link = document.createElement("a");
  link.href = url;
  link.download = nameMatch ? nameMatch[1] : "interlink-audit.csv";
  document.body.appendChild(link);
  link.click();
  link.remove();
  setTimeout(() => URL.revokeObjectURL(url), 4000);
  return { ok: true, rows: rowsHeader, max: Number.isFinite(maxHeader) ? maxHeader : CSV_MAX_ROWS_FALLBACK };
};

// ---------------------------------------------------------------------------
// 分页器（每页条数 + 上/下一页；始终 offset/limit 有界）
// ---------------------------------------------------------------------------

export const createPager = (options) => {
  const root = options.root;
  clearNode(root);
  root.classList.add("interlink-pager");

  const info = el("div", "monitor-pagination-info", "");
  const controls = el("div", "interlink-pager-controls");

  const sizeLabel = el("span", "interlink-flag", t("pageSize"));
  const sizeSelect = el("select");
  sizeSelect.setAttribute("aria-label", t("pageSize"));
  PAGE_SIZES.forEach((size) => {
    const option = el("option", "", String(size));
    option.value = String(size);
    sizeSelect.appendChild(option);
  });
  sizeSelect.value = String(options.pageSize || 50);
  const sizeBox = el("span", "interlink-pager-size");
  sizeBox.appendChild(sizeLabel);
  sizeBox.appendChild(sizeSelect);

  const prev = el("button", "secondary", t("prev"));
  prev.type = "button";
  const next = el("button", "secondary", t("next"));
  next.type = "button";
  controls.appendChild(sizeBox);
  controls.appendChild(prev);
  controls.appendChild(next);
  root.appendChild(info);
  root.appendChild(controls);

  const state = { page: options.page || 1, pageSize: Number(sizeSelect.value), total: 0 };

  sizeSelect.addEventListener("change", () => {
    state.pageSize = Number(sizeSelect.value) || 50;
    state.page = 1;
    options.onChange?.({ page: state.page, pageSize: state.pageSize });
  });
  prev.addEventListener("click", () => {
    if (state.page <= 1) {
      return;
    }
    state.page -= 1;
    options.onChange?.({ page: state.page, pageSize: state.pageSize });
  });
  next.addEventListener("click", () => {
    state.page += 1;
    options.onChange?.({ page: state.page, pageSize: state.pageSize });
  });

  return {
    state,
    setMeta(meta) {
      state.page = Number(meta?.page) || state.page;
      state.pageSize = Number(meta?.pageSize) || state.pageSize;
      state.total = Number(meta?.total) || 0;
      sizeSelect.value = String(state.pageSize);
    },
    render(busy) {
      const pages = Math.max(1, Math.ceil(state.total / state.pageSize));
      state.page = Math.min(Math.max(1, state.page), pages);
      info.textContent = t("pageInfo", { total: state.total, page: state.page, pages });
      prev.disabled = Boolean(busy) || state.page <= 1;
      next.disabled = Boolean(busy) || state.page >= pages;
      sizeSelect.disabled = Boolean(busy);
      root.style.display = state.total > 0 ? "flex" : "flex";
    },
    offset() {
      return (Math.max(1, state.page) - 1) * state.pageSize;
    },
    destroy() {
      const clone = root.cloneNode(false);
      root.replaceWith(clone);
    },
  };
};

// ---------------------------------------------------------------------------
// 二次确认（复用预设面板的影响面预览模态：与云端接入页同款）
// ---------------------------------------------------------------------------

export const confirmAction = ({ title, summary, details, hint, ackLabel, danger, confirmLabel, onConfirm }) =>
  openImpactConfirmModal({
    title,
    summary,
    details: details || [],
    hint: hint || "",
    ackLabel: ackLabel || "",
    confirmLabel: confirmLabel || t("confirmOk"),
    danger: danger === true,
    onConfirm,
  });

// ---------------------------------------------------------------------------
// 抽屉（面板内部右滑层；面板隐藏时由宿主强制关闭）
// ---------------------------------------------------------------------------

export const drawer = {
  node: null,
  body: null,
  titleNode: null,
  subNode: null,
  bound: false,
  onClose: null,

  init(rootId) {
    const node = document.getElementById(rootId);
    if (!node) {
      return null;
    }
    this.node = node;
    this.titleNode = node.querySelector("[data-drawer-title]");
    this.subNode = node.querySelector("[data-drawer-sub]");
    this.body = node.querySelector("[data-drawer-body]");
    if (!this.bound) {
      node.querySelector("[data-drawer-close]")?.addEventListener("click", () => this.close());
      document.addEventListener("keydown", (event) => {
        if (event.key === "Escape" && this.isOpen()) {
          this.close();
        }
      });
      this.bound = true;
    }
    node.hidden = false;
    return node;
  },

  isOpen() {
    return Boolean(this.node?.classList.contains("is-open"));
  },

  open(title, sub, content, onClose) {
    if (!this.node) {
      return;
    }
    this.titleNode.textContent = title;
    this.subNode.textContent = sub || "";
    clearNode(this.body);
    if (typeof content === "function") {
      content(this.body);
    } else if (content) {
      this.body.appendChild(content);
    }
    this.onClose = onClose || null;
    this.node.hidden = false;
    requestAnimationFrame(() => this.node.classList.add("is-open"));
  },

  close() {
    if (!this.node) {
      return;
    }
    this.node.classList.remove("is-open");
    const pending = this.onClose;
    this.onClose = null;
    if (pending) {
      try {
        pending();
      } catch (error) {
        // 抽屉关闭回调不影响主流程
      }
    }
  },

  setTitleSub(title, sub) {
    if (this.titleNode) {
      this.titleNode.textContent = title;
    }
    if (this.subNode) {
      this.subNode.textContent = sub || "";
    }
  },
};

// ---------------------------------------------------------------------------
// 归一化：后端字段可能缺省，统一成可渲染的安全形状
// ---------------------------------------------------------------------------

export const normalizeCapabilities = (value) =>
  Array.isArray(value) ? value.map((item) => String(item || "").trim()).filter(Boolean) : [];

export const normalizePolicy = (value) => ({
  disabled_kinds: normalizeCapabilities(value?.disabled_kinds),
  disabled_caps: normalizeCapabilities(value?.disabled_caps),
  force_approval_kinds: normalizeCapabilities(value?.force_approval_kinds),
  shadow_mode: String(value?.shadow_mode || "").trim(),
});

export const pickFields = (digest, limit = 6) => {
  const fields = digest?.fields;
  if (!fields || typeof fields !== "object" || Array.isArray(fields)) {
    return [];
  }
  return Object.entries(fields)
    .slice(0, limit)
    .map(([key, value]) => [key, describeDigestValue(value)]);
};

/// 摘要值渲染：短字符串原样，其余只显示类型/长度/哈希前缀（永不含正文）。
export const describeDigestValue = (value) => {
  if (value === null || value === undefined) {
    return "-";
  }
  if (typeof value === "object") {
    if (value.t) {
      const parts = [String(value.t)];
      if (value.n !== undefined) {
        parts.push(`len=${value.n}`);
      }
      if (value.h) {
        parts.push(`sha=${value.h}`);
      }
      return parts.join(" ");
    }
    return "{…}";
  }
  return String(value);
};

export { TYPE_ICONS };
