import test from 'node:test';
import assert from 'node:assert/strict';

import type { InterlinkNode } from '../../src/api/interlink';
import {
  applyWorkspaceOrder,
  moveWorkspaceRow
} from '../../src/views/messenger/interlink/interlinkWorkspaceOrder';
import type { InterlinkWorkspaceRow } from '../../src/views/messenger/interlink/interlinkNodeModel';

const asNode = (nodeId: string): InterlinkNode =>
  ({
    node_id: nodeId,
    node_type: 'desktop',
    user_id: 'u1',
    label: nodeId,
    status: 'online',
    last_seen_at: 0,
    capabilities: [],
    shadow_revision: 1,
    connected: true,
    meta: {}
  }) as unknown as InterlinkNode;

const row = (key: string, workspaceId = ''): InterlinkWorkspaceRow => ({
  key,
  deviceId: key.replace(/^device:|@.*$/g, ''),
  workspaceId,
  label: key,
  node: asNode(key.replace(/^device:|@.*$/g, ''))
});

test('applyWorkspaceOrder reorders, appends new rows and drops stale keys', () => {
  const rows = [row('device:a'), row('device:b@ws-1', 'ws-1'), row('device:b@ws-2', 'ws-2')];
  // 保存的顺序在前，没记过的行按自然顺序排在末尾。
  assert.deepEqual(
    applyWorkspaceOrder(rows, ['device:b@ws-2', 'device:a']).map((item) => item.key),
    ['device:b@ws-2', 'device:a', 'device:b@ws-1']
  );
  // 已消失的 key（设备下线/工作区删除）被忽略，不产生幽灵行。
  assert.deepEqual(
    applyWorkspaceOrder(rows, ['device:ghost', 'device:b@ws-1']).map((item) => item.key),
    ['device:b@ws-1', 'device:a', 'device:b@ws-2']
  );
  // 空 saved = 自然顺序原样返回。
  assert.deepEqual(
    applyWorkspaceOrder(rows, []).map((item) => item.key),
    rows.map((item) => item.key)
  );
  assert.deepEqual(applyWorkspaceOrder([], ['device:a']), []);
});

test('moveWorkspaceRow lands the dragged row on the target slot', () => {
  const keys = ['cloud', 'device:a', 'device:b', 'device:c'];
  // 向下拖：落到目标行原槽位，目标行及其后整体后移。
  assert.deepEqual(moveWorkspaceRow(keys, 'cloud', 'device:b'), [
    'device:a',
    'device:b',
    'cloud',
    'device:c'
  ]);
  // 向上拖同语义。
  assert.deepEqual(moveWorkspaceRow(keys, 'device:c', 'device:a'), [
    'cloud',
    'device:c',
    'device:a',
    'device:b'
  ]);
  assert.equal(moveWorkspaceRow(keys, 'device:a', 'device:a'), null, 'same row is a no-op');
  assert.equal(moveWorkspaceRow(keys, 'device:ghost', 'device:a'), null, 'unknown drag key');
  assert.equal(moveWorkspaceRow(keys, 'device:a', 'device:ghost'), null, 'unknown target key');
});
