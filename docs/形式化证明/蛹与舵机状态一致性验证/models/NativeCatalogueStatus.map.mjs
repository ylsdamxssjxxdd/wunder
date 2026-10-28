export const readerActions = ['Send'];

export function state(values) {
  const local = values.local === 'hot' ? '本地任务执行中' : '本地任务已完成';
  const catalogue = values.catalogue === 'hot' ? '目录记录执行中' : '目录记录已完成';
  return `${local}<br/>${catalogue}，${values.shown ? '已弹出完成提示' : '未弹完成提示'}`;
}

export function step(action) {
  return {
    Send: '用户发送消息',
    OldCatalogueDone: '旧目录快照返回完成',
    RealTerminal: '真实执行终态到达',
    BackgroundHot: '后台线程开始',
    BackgroundDone: '后台线程完成',
  }[action] ?? action;
}

export function cause() {
  return '目录读取开始早于本地提交，旧完成快照覆盖本地热状态';
}

export const legend = {
  works: '规则成立',
  wrong: '状态失配',
  fixed: '修复后旧错误不可达或新增正确状态',
};
