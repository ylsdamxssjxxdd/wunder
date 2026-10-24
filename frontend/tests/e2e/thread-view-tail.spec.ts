import { expect, test } from '@playwright/test';

test('thread view binds the first reasoning tail before its durable assistant row', async ({ page }) => {
  await page.goto('/__e2e/composer-b4');
  const result = await page.evaluate(async () => {
    const view = await import('/src/realtime/chat/threadViewStore.ts');
    const key = 'fixture-thread-tail';
    view.resetThreadViews();
    const row = (revision: number, reasoning = '') => ({
      root_turn_id: 'turn-1', execution_turn_id: 'turn-1', user_round: 1,
      client_message_id: 'client-1', revision, status: 'running',
      user: { item_id: 'turn-1:user', content: 'fixture request' },
      assistant: { item_id: reasoning ? 'turn-1:text-1' : '', content: '', reasoning,
        content_offset: 0, reasoning_offset: [...reasoning].reduce((n, ch) => n + ch.length, 0) },
      workflow_count: 0, reasoning_count: reasoning ? 1 : 0,
      reasoning_item_id: reasoning ? 'turn-1:text-1' : '', failed_count: 0
    });
    view.beginThreadView(key);
    if (!view.commitThreadView(key, {
      session_id: key, epoch: 1, view_seq: 1, projection_state: 'ready',
      thread_empty: false, latest_round: 1, chat: [row(1)], trajectory: [], tiles: [],
      tile_level: 0, next_before: null, has_more: false
    })) throw new Error('fixture page was rejected');
    view.applyThreadViewTail(key, { item_id: 'turn-1:text-1', field: 'reasoning', offset: 0, text: 'first thought' });
    view.applyThreadViewTail(key, { item_id: 'turn-1:text-1', field: 'reasoning', offset: 13, text: ' and more' });
    const live = view.threadViewSlots(key)?.[0]?.assistant;
    if (!live) throw new Error('missing live assistant slot');
    const committed = view.commitThreadView(key, {
      session_id: key, epoch: 1, view_seq: 2, projection_state: 'ready',
      thread_empty: false, latest_round: 1, chat: [row(2, 'first thought and more')],
      trajectory: [], tiles: [], tile_level: 0, next_before: null, has_more: false
    });
    const settled = view.threadViewSlots(key)?.[0]?.assistant;
    return {
      liveReasoning: live.reasoning,
      liveProcess: live.process_activity?.kind,
      committed,
      rowCount: view.threadViewSlots(key)?.length,
      settledReasoning: settled?.reasoning,
      settledItem: settled?.item_id
    };
  });
  expect(result).toEqual({
    liveReasoning: 'first thought and more',
    liveProcess: 'thinking',
    committed: true,
    rowCount: 1,
    settledReasoning: 'first thought and more',
    settledItem: 'turn-1:text-1'
  });
});
