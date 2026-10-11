// 远程会话视图的帧契约回归：断言按**消费方读取路径**写，形状取自互通回环用例
// （docs §7.4 冻结的上行帧形状），不是这里想象的形状。
// 模型链路会经 i18n 读浏览器存储，因此先补最小存根再动态载入被测模块。
import test from 'node:test';
import assert from 'node:assert/strict';
import type { InterlinkRemoteFrame } from '@/api/interlink';

const storageStub = () => {
  const holder = globalThis as { localStorage?: unknown };
  if (!holder.localStorage) {
    holder.localStorage = { getItem: () => null, setItem: () => {}, removeItem: () => {} };
  }
};

const loadModel = async () => {
  storageStub();
  const module = await import('@/views/messenger/interlink/remoteSessionModel');
  return {
    applyRemoteFrame: module.applyRemoteFrame,
    createRemoteSessionState: module.createRemoteSessionState
  };
};

const asFrame = (value: Record<string, unknown>): InterlinkRemoteFrame =>
  value as unknown as InterlinkRemoteFrame;

const snapshotFrame = (seq: number): InterlinkRemoteFrame =>
  asFrame({
    v: 1,
    type: 'snapshot',
    thread_id: 't_remote',
    seq,
    payload: {
      items: [{ id: 'i_1', role: 'assistant', content: '第一句' }],
      cursor: seq,
      item_total: 1
    }
  });

const deltaFrame = (seq: number, text: string): InterlinkRemoteFrame =>
  asFrame({
    v: 1,
    type: 'delta',
    thread_id: 't_remote',
    seq,
    payload: { event: 'item.added', data: { id: 'i_2', role: 'assistant', content: text } }
  });

// 服务端在命令 ack 与终态时推的控制帧：没有 payload，也没有 seq。
const commandFrame = (status: string): InterlinkRemoteFrame =>
  asFrame({ v: 1, type: 'command', kind: 'thread.message', status, command_id: 'cmd_1' });

test('baseline snapshot replaces the rendered list', async () => {
  const { applyRemoteFrame, createRemoteSessionState } = await loadModel();
  const state = createRemoteSessionState();
  assert.equal(applyRemoteFrame(state, snapshotFrame(7)), true);
  assert.equal(state.gotSnapshot, true);
  assert.deepEqual(
    state.messages.map((item) => item.text),
    ['第一句']
  );
  assert.equal(state.lastSeq, 7);
});

test('a change frame after the baseline renders its payload.data record', async () => {
  const { applyRemoteFrame, createRemoteSessionState } = await loadModel();
  const state = createRemoteSessionState();
  applyRemoteFrame(state, snapshotFrame(7));
  assert.equal(applyRemoteFrame(state, deltaFrame(8, '第二句')), true);
  assert.deepEqual(
    state.messages.map((item) => item.text),
    ['第一句', '第二句']
  );
});

test('a repeated sequence is dropped without touching the list', async () => {
  const { applyRemoteFrame, createRemoteSessionState } = await loadModel();
  const state = createRemoteSessionState();
  applyRemoteFrame(state, snapshotFrame(7));
  assert.equal(applyRemoteFrame(state, deltaFrame(7, '重帧')), false);
  assert.equal(state.messages.length, 1);
});

test('the command lifecycle notice renders nothing and never occupies the sequence', async () => {
  const { applyRemoteFrame, createRemoteSessionState } = await loadModel();
  const state = createRemoteSessionState();
  applyRemoteFrame(state, snapshotFrame(7));
  const bubbles = state.messages.length;
  const lastSeq = state.lastSeq;
  const dropped = state.droppedFrames;
  assert.equal(applyRemoteFrame(state, commandFrame('acked')), false);
  assert.equal(applyRemoteFrame(state, commandFrame('succeeded')), false);
  assert.equal(state.messages.length, bubbles, 'the notice adds no bubble');
  assert.equal(state.lastSeq, lastSeq, 'the notice must not advance lastSeq');
  assert.equal(state.droppedFrames, dropped, 'the notice is not a dropped frame');
  // 控制帧排在增量之前时，增量仍要落地（幂等序号不能被子帧占用）。
  assert.equal(applyRemoteFrame(state, deltaFrame(8, '第二句')), true);
  assert.equal(state.messages.length, bubbles + 1);
});
