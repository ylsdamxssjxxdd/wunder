// AI生成 · 工作过程中“执行命令”条目的开合回归：
// 运行中自动展开、结束后自动收起，用户点过则以用户为准。
import test from 'node:test';
import assert from 'node:assert/strict';

import {
  autoOpenCommandKeys,
  resolveEntryOpen,
  type CommandFoldEntry
} from '../../src/components/chat/timelineCommandFold';

const entry = (
  key: string,
  commandInteraction: string | null,
  status: string
): CommandFoldEntry => ({ key, commandInteraction, status });

test('只有仍在跑的执行命令自带宽开的资格', () => {
  const keys = autoOpenCommandKeys([
    entry('run-1', 'execute', 'loading'),
    entry('run-2', 'execute', 'completed'),
    // 命令会话（写入 stdin / 等待）与读取文件都不自动展开
    entry('run-3', 'write', 'loading'),
    entry('run-4', 'poll', 'loading'),
    entry('run-5', null, 'loading')
  ]);
  assert.deepEqual([...keys], ['run-1']);
});

test('命令结束就不再自动展开', () => {
  const running = autoOpenCommandKeys([entry('run-1', 'execute', 'loading')]);
  assert.equal(resolveEntryOpen({ key: 'run-1', manual: [], open: [], auto: running }), true);
  const settled = autoOpenCommandKeys([entry('run-1', 'execute', 'completed')]);
  assert.equal(resolveEntryOpen({ key: 'run-1', manual: [], open: ['run-1'], auto: settled }), false);
});

test('用户点过的条目一切以用户为准，自动开合不再改写', () => {
  // 用户把运行中的命令收起来了：不会因为仍在跑而被重新铺开。
  const running = autoOpenCommandKeys([entry('run-1', 'execute', 'loading')]);
  assert.equal(
    resolveEntryOpen({ key: 'run-1', manual: ['run-1'], open: [], auto: running }),
    false
  );
  // 用户点开了已结束的命令：结算帧也不会把它收回去。
  const settled = autoOpenCommandKeys([entry('run-1', 'execute', 'completed')]);
  assert.equal(
    resolveEntryOpen({ key: 'run-1', manual: ['run-1'], open: ['run-1'], auto: settled }),
    true
  );
});

test('普通条目的展开仍然只来自用户自己', () => {
  const auto = autoOpenCommandKeys([entry('read-1', null, 'completed')]);
  assert.equal(resolveEntryOpen({ key: 'read-1', manual: [], open: [], auto }), false);
  assert.equal(resolveEntryOpen({ key: 'read-1', manual: ['read-1'], open: ['read-1'], auto }), true);
});
