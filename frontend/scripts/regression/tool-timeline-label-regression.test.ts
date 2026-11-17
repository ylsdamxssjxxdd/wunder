// AI生成 · 工作过程行首工具名：两字一格，命令类一律「执行」。
import test from 'node:test';
import assert from 'node:assert/strict';

import { toolRowLabel } from '../../src/components/chat/toolTimelineLabel';

test('文件类工具收成两字', () => {
  assert.equal(toolRowLabel('读取文件'), '读取');
  assert.equal(toolRowLabel('read_file'), '读取');
  assert.equal(toolRowLabel('编辑文件'), '编辑');
  assert.equal(toolRowLabel('apply_patch'), '编辑');
  assert.equal(toolRowLabel('列出文件'), '列出');
  assert.equal(toolRowLabel('搜索内容'), '搜索');
  assert.equal(toolRowLabel('网页抓取'), '网页');
  assert.equal(toolRowLabel('web_fetch'), '网页');
  assert.equal(toolRowLabel('browser'), '浏览');
});

test('命令类工具无论哪个拼写都是「执行」', () => {
  for (const name of ['执行命令', '运行命令', 'execute_command', 'exec_command', 'run_command',
    'shell', '命令会话', 'command_session', 'write_stdin', 'write_command_stdin']) {
    assert.equal(toolRowLabel(name), '执行', name);
  }
});

test('未收录的名字原样保留，空名退回「工具」', () => {
  assert.equal(toolRowLabel('自定义工具'), '自定义工具');
  assert.equal(toolRowLabel(''), '工具');
  assert.equal(toolRowLabel(' Read_File '), '读取');
});
