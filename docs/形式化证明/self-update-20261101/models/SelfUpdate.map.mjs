export const readerActions = [
  "Check",
  "Download",
  "ApplyDispatch",
  "RestartRestore",
  "RestartClear",
];

export function state(value) {
  const phase = {
    idle: "空闲，可检查更新",
    checking: "正在检查更新",
    up_to_date: "已是最新版本",
    available: "发现新版本",
    downloading: "正在下载更新包",
    staged: "已下载待应用",
    applying: `正在应用更新（${{
      desktop: "蛹本体",
      cli: "舵机",
      supplement: "补充包",
    }[value.applying] || value.applying}）`,
    applied: "应用完成",
    failed: "失败",
  }[value.phase] || value.phase;
  const pkg = value.staged === "pkg"
    ? `暂存包（${value.staged_target}${value.staged_verified ? "，已校验" : "，未校验"}）`
    : "无暂存包";
  const exe = value.exe_state === "absent"
    ? "程序本体已摘下"
    : value.exe_new === "ready"
      ? `新版已复制，运行版本 ${value.exe_version}`
      : `运行版本 ${value.exe_version}`;
  return `${phase}，${pkg}，${exe}`;
}

export function step(action) {
  return {
    Check: "点击检查更新",
    OfferNewer: "源上有更新的版本",
    OfferNone: "源上没有更新",
    OfferStaged: "已下载的包不落后，回到待应用",
    Download: "点击下载更新",
    DownloadVerify: "下载完成并校验清单哈希",
    DownloadFail: "下载失败，保留原有暂存包",
    ApplyDispatch: "点击应用更新",
    RotateStage: "复制更新包并再次校验",
    RotateBackup: "旧版本改名备份",
    RotateActivate: "新版本就位",
    ConsumeInPlace: "舵机就地替换完成",
    ApplyImport: "补充包导入完成",
    RestartRestore: "带着待应用包重启",
    RestartClear: "重启并清空暂存",
    KillAfterBackup: "轮换中途进程被杀",
  }[action] || action;
}

export function cause(value, facts, invariant) {
  return {
    StagedAlwaysVerified: "下载完成没有做清单哈希校验，未校验的包直接进入暂存区",
  }[invariant];
}

export const legend = {
  works: "正常更新",
  wrong: "未校验风险",
  fixed: "修复后不可达",
};
