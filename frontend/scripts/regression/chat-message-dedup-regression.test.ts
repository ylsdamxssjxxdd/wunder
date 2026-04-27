import test from 'node:test';
import assert from 'node:assert/strict';

import { dedupeAssistantMessages } from '../../src/stores/chatMessageDedup';

test('assistant dedupe merges duplicate stream failure messages sharing request id', () => {
  const messages = [
    { role: 'user', content: '请把攻略发给我', created_at: '2026-04-04T08:03:00.000Z' },
    {
      role: 'assistant',
      content:
        '模型调用失败: LLM stream request failed: 429 Too Many Requests {"request_id":"2d24ef97-354b-43da-b165-71f0abb304dd"}',
      created_at: '2026-04-04T08:03:02.000Z',
      stream_round: 12
    },
    {
      role: 'assistant',
      content:
        '模型请求失败: 模型调用失败: LLM stream request failed: 429 Too Many Requests {"request_id":"2d24ef97-354b-43da-b165-71f0abb304dd"}',
      created_at: '2026-04-04T08:03:06.000Z',
      stream_round: null
    }
  ];

  const deduped = dedupeAssistantMessages(messages);
  assert.equal(deduped.length, 2);
  assert.equal(deduped[1].role, 'assistant');
  assert.match(String(deduped[1].content), /request failed|请求失败|调用失败/i);
});

test('assistant dedupe keeps distinct assistant messages when request ids differ', () => {
  const messages = [
    { role: 'user', content: 'hello', created_at: '2026-04-04T08:03:00.000Z' },
    {
      role: 'assistant',
      content:
        '模型调用失败: LLM stream request failed: 429 Too Many Requests {"request_id":"11111111-1111-1111-1111-111111111111"}',
      created_at: '2026-04-04T08:03:02.000Z'
    },
    {
      role: 'assistant',
      content:
        '模型调用失败: LLM stream request failed: 429 Too Many Requests {"request_id":"22222222-2222-2222-2222-222222222222"}',
      created_at: '2026-04-04T08:03:08.000Z'
    }
  ];

  const deduped = dedupeAssistantMessages(messages);
  assert.equal(deduped.length, 3);
});

test('assistant dedupe collapses manual stop abort bubble with server session-cancelled bubble', () => {
  const messages = [
    { role: 'user', content: '继续执行', created_at: '2026-04-27T10:00:00.000Z' },
    {
      role: 'assistant',
      content: '已中止',
      created_at: '2026-04-27T10:00:05.000Z',
      stream_round: 18,
      workflowItems: [{ status: 'failed', title: '已中止', detail: '请求已中止' }]
    },
    {
      role: 'assistant',
      content: '会话已取消',
      created_at: '2026-04-27T10:00:11.000Z',
      workflowItems: [{ status: 'failed', title: '请求失败', detail: '会话已取消' }]
    }
  ];

  const deduped = dedupeAssistantMessages(messages);
  assert.equal(deduped.length, 2);
  assert.equal(deduped[1]?.role, 'assistant');
  assert.equal(deduped[1]?.content, '已中止');
});

test('assistant dedupe collapses stop bubble even when both terminal notices carry workflow traces', () => {
  const messages = [
    { role: 'user', content: '继续执行', created_at: '2026-04-27T07:43:41.000Z' },
    {
      role: 'assistant',
      content: '已中止',
      created_at: '2026-04-27T07:43:41.658Z',
      stream_event_id: 6,
      stream_round: 1,
      workflowItems: [
        { status: 'completed', title: '事件：received', detail: '{}' },
        { status: 'completed', title: '事件：progress', detail: '{}' },
        { eventType: 'llm_request', status: 'completed', title: '模型请求体', detail: '{}' },
        {
          eventType: 'llm_stream_retry',
          status: 'pending',
          title: '模型连接重试',
          detail: '{"attempt":1}'
        },
        { status: 'failed', title: '已中止', detail: '请求已中止' }
      ]
    },
    {
      role: 'assistant',
      content: '会话已取消',
      created_at: '2026-04-27T07:43:50.177Z',
      stream_round: 2,
      workflowItems: [
        { status: 'completed', title: '事件：received', detail: '{}' },
        { status: 'completed', title: '事件：progress', detail: '{}' },
        { eventType: 'llm_request', status: 'completed', title: '模型请求体', detail: '{}' },
        {
          eventType: 'llm_stream_retry',
          status: 'pending',
          title: '模型连接重试',
          detail: '{"attempt":1}'
        },
        { eventType: 'error', status: 'failed', title: '请求失败', detail: '会话已取消' }
      ]
    }
  ];

  const deduped = dedupeAssistantMessages(messages);
  assert.equal(deduped.length, 2);
  assert.equal(deduped[1]?.role, 'assistant');
  assert.equal(deduped[1]?.content, '已中止');
  assert.equal(deduped[1]?.workflowItems?.length, 5);
});
