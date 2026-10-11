// 调试面板右侧日志区已重构为线程轨迹视图；appendLog 退化为控制台兜底，
// 保留各模块既有调用。需要用户可见反馈时使用各面板的 status/notify 通道。
export const appendLog = (text, options = {}) => {
  const detail = options.detail;
  const line = `[${new Date().toLocaleTimeString()}] ${text}`;
  if (detail !== undefined && detail !== null && detail !== text) {
    console.log(line, detail);
    return;
  }
  console.log(line);
};
