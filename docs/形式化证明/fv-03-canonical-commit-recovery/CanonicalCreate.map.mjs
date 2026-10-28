const CATALOG = { absent: '无目录行', present: '目录行已发布' };

const FILE = {
  absent: 'rollout 文件不存在',
  head: 'rollout 首条为 SessionMeta',
  headless: 'rollout 首条是普通记录（无头历史）',
};

const PC = {
  idle: '无进行中的 create',
  db: '目录行已提交、头未写',
  file: '头已写、目录未发布',
  done: 'create 已完成',
};

export const readerActions = [];

export function state(values) {
  const bits = [
    CATALOG[values.catalog] ?? values.catalog,
    FILE[values.file] ?? values.file,
    PC[values.pc] ?? values.pc,
  ];
  if (values.orphan) bits.push('存在谱系无引用的孤儿文件');
  if (values.appended) bits.push('已发生后续 append');
  if (values.rejected) bits.push('坏状态的 append 已被拒绝');
  return bits.join('<br/>');
}

export function step(action, before, after) {
  const base = String(action).split('(')[0];
  if (base === 'CreateCatalog') return '在 Immediate 事务中提交 canonical_threads 目录行';
  if (base === 'CreateHead') return '写入 rollout 的 SessionMeta 头并 flush';
  if (base === 'Crash') {
    if (before.pc === 'db') return '进程崩溃：目录行已提交但 rollout 文件从未创建';
    if (before.pc === 'file') return '进程崩溃：带头的文件成为无引用孤儿，目录行从未发布';
    return '进程崩溃';
  }
  if (base === 'Append') {
    if (after.file === 'headless' && before.file !== 'headless') {
      return '崩溃后第一次 append：writer 惰性创建文件，从 ordinal 0 写出无头 rollout';
    }
    if (after.rejected && !before.rejected) return 'append 检测到目录行引用的 rollout 不存在，拒绝写入';
    return '正常 append 到已有头的 rollout';
  }
  return base;
}

export function cause(vars, facts, invariant) {
  if (String(invariant).includes('Headless')) {
    return '缺失文件被 writer 惰性创建，且 SessionMeta 校验只在首条本身是 SessionMeta 时执行，普通记录从 ordinal 0 接续成无头历史';
  }
  return 'create 先提交目录行再写文件，进程在两者之间崩溃留下有目录行、无文件的线程，且重名检查使重试永远失败';
}

export const legend = {
  works: '规则成立',
  wrong: '状态失配',
  fixed: '修复后旧错误不可达或新增正确状态',
};
