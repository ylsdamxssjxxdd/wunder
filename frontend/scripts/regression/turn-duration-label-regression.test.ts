import test from 'node:test';
import assert from 'node:assert/strict';

import { effectScope, nextTick, ref } from 'vue';

import { useTurnDurationLabel } from '../../src/components/chat/useTurnDurationLabel';

// 假时钟 + 假定时器：三个坑（停在统计值上、统计帧滞后回退、重挂载清零）都只在
// 「同一统计值反复到达」时才露头，用真实定时器测不出来。
const realNow = Date.now;
let clock = 1_700_000_000_000;
const intervals = new Map<number, () => void>();
let nextTimerId = 1;

const tick = (ms: number) => {
  clock += ms;
  for (const advance of intervals.values()) advance();
};

test.before(() => {
  Date.now = () => clock;
  (globalThis as any).window = {
    setInterval: (fn: () => void) => {
      const id = nextTimerId++;
      intervals.set(id, fn);
      return id;
    },
    clearInterval: (id: number) => {
      intervals.delete(id);
    }
  };
});

test.afterEach(() => {
  intervals.clear();
});

test.after(() => {
  Date.now = realNow;
});

const liveMessage = (id: string, seconds: number) => ({
  role: 'assistant',
  root_turn_id: id,
  status: 'running',
  stats: { interaction_duration_s: seconds, interaction_duration_ms: seconds * 1000 }
});

test('keeps counting between statistics frames instead of pinning to the last one', async () => {
  const message = ref<Record<string, any>>(liveMessage('turn-tick', 40));
  const scope = effectScope();
  const label = scope.run(() => useTurnDurationLabel(message, ref(false)))!;
  assert.equal(label.value, '40s');
  // 同一个统计值再到达几次：不能把锚点重新压回 40s。
  message.value = liveMessage('turn-tick', 40);
  await nextTick();
  tick(1000);
  assert.equal(label.value, '41s');
  tick(2000);
  assert.equal(label.value, '43s');
  scope.stop();
});

test('only calibrates forward when a newer statistic arrives', async () => {
  const message = ref<Record<string, any>>(liveMessage('turn-forward', 10));
  const scope = effectScope();
  const label = scope.run(() => useTurnDurationLabel(message, ref(false)))!;
  tick(3000);
  assert.equal(label.value, '13s');
  // 服务端报 20s（比本地显示更靠前）：前移基准。
  message.value = liveMessage('turn-forward', 20);
  await nextTick();
  tick(250);
  assert.equal(label.value, '20s');
  tick(1000);
  assert.equal(label.value, '21s');
  // 服务端随后报 12s（滞后）：数字不许往回掉。
  message.value = liveMessage('turn-forward', 12);
  await nextTick();
  tick(1000);
  assert.equal(label.value, '22s');
  scope.stop();
});

test('resumes from the stored anchor after a remount', () => {
  const message = ref<Record<string, any>>(liveMessage('turn-remount', 30));
  const first = effectScope();
  const firstLabel = first.run(() => useTurnDurationLabel(message, ref(false)))!;
  tick(4000);
  assert.equal(firstLabel.value, '34s');
  first.stop();
  // 列表行重建会重新挂载组件：时间接着走，而不是回到 30s。
  const second = effectScope();
  const secondLabel = second.run(() => useTurnDurationLabel(message, ref(false)))!;
  assert.equal(secondLabel.value, '34s');
  tick(2000);
  assert.equal(secondLabel.value, '36s');
  second.stop();
});

test('settles on the authoritative value once the turn is no longer live', async () => {
  const message = ref<Record<string, any>>(liveMessage('turn-settle', 50));
  const scope = effectScope();
  const label = scope.run(() => useTurnDurationLabel(message, ref(false)))!;
  tick(2000);
  assert.equal(label.value, '52s');
  message.value = { ...liveMessage('turn-settle', 50), status: 'completed' };
  await nextTick();
  tick(5000);
  assert.equal(label.value, '50s');
  scope.stop();
});
