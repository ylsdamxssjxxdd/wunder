import test from 'node:test';
import assert from 'node:assert/strict';

import {
  compareChatRuntimeRenderShadow
} from '../../src/realtime/chat/chatRuntimeRenderShadow';
import type { ChatRuntimeRenderableMessage } from '../../src/realtime/chat/chatRuntimeRenderAdapter';

const renderable = (
  key: string,
  message: Record<string, unknown>,
  sourceIndex = 0
): ChatRuntimeRenderableMessage => ({
  key,
  sourceIndex,
  message
});

test('chat runtime render shadow accepts matching render sources', () => {
  const legacy = [
    renderable('runtime:user:user-1', {
      message_id: 'user-1',
      role: 'user',
      content: 'hello'
    }),
    renderable('runtime:assistant:assistant-1', {
      message_id: 'assistant-1',
      role: 'assistant',
      content: 'hi',
      stream_incomplete: false
    })
  ];
  const projection = [
    renderable('runtime:user:user-1', {
      __runtime_message_id: 'user-1',
      role: 'user',
      content: 'hello'
    }),
    renderable('runtime:assistant:assistant-1', {
      __runtime_message_id: 'assistant-1',
      role: 'assistant',
      content: 'hi',
      stream_incomplete: false
    })
  ];

  const report = compareChatRuntimeRenderShadow({
    sessionId: 'session-1',
    legacy,
    projection
  });

  assert.equal(report.ok, true);
  assert.equal(report.issues.length, 0);
  assert.equal(report.matchedCount, 2);
});

test('chat runtime render shadow reports key drift for matched render sources', () => {
  const report = compareChatRuntimeRenderShadow({
    sessionId: 'session-1',
    legacy: [
      renderable('assistant-1:0', {
        message_id: 'assistant-1',
        role: 'assistant',
        content: 'hi'
      })
    ],
    projection: [
      renderable('runtime:assistant:assistant-1', {
        __runtime_message_id: 'assistant-1',
        role: 'assistant',
        content: 'hi'
      })
    ]
  });

  assert.equal(report.ok, false);
  assert.ok(report.issues.some((issue) => issue.code === 'render_key_drift'));
});

test('chat runtime render shadow reports missing renderable messages', () => {
  const report = compareChatRuntimeRenderShadow({
    sessionId: 'session-1',
    legacy: [
      renderable('user-1:0', {
        message_id: 'user-1',
        role: 'user',
        content: 'hello'
      })
    ],
    projection: [
      renderable('runtime:assistant:assistant-1', {
        __runtime_message_id: 'assistant-1',
        role: 'assistant',
        content: 'hi'
      })
    ]
  });

  assert.equal(report.ok, false);
  assert.ok(report.issues.some((issue) => issue.code === 'render_missing_projection_message'));
  assert.ok(report.issues.some((issue) => issue.code === 'render_missing_legacy_message'));
});

test('chat runtime render shadow reports order drift', () => {
  const user = renderable('user-1:0', {
    message_id: 'user-1',
    role: 'user',
    content: 'hello'
  });
  const assistant = renderable('assistant-1:1', {
    message_id: 'assistant-1',
    role: 'assistant',
    content: 'hi'
  });

  const report = compareChatRuntimeRenderShadow({
    sessionId: 'session-1',
    legacy: [user, assistant],
    projection: [assistant, user]
  });

  assert.equal(report.ok, false);
  assert.ok(report.issues.some((issue) => issue.code === 'render_order_drift'));
});

test('chat runtime render shadow reports content, reasoning and streaming drift', () => {
  const report = compareChatRuntimeRenderShadow({
    sessionId: 'session-1',
    legacy: [
      renderable('assistant-1:0', {
        message_id: 'assistant-1',
        role: 'assistant',
        content: 'old',
        reasoning: '',
        stream_incomplete: false
      })
    ],
    projection: [
      renderable('runtime:assistant:assistant-1', {
        __runtime_message_id: 'assistant-1',
        role: 'assistant',
        content: 'new',
        reasoning: 'thinking',
        stream_incomplete: true
      })
    ]
  });

  assert.equal(report.ok, false);
  assert.ok(report.issues.some((issue) => issue.code === 'render_content_drift'));
  assert.ok(report.issues.some((issue) => issue.code === 'render_reasoning_drift'));
  assert.ok(report.issues.some((issue) => issue.code === 'render_streaming_flag_drift'));
});

test('chat runtime render shadow reports workflow and subagent drift', () => {
  const report = compareChatRuntimeRenderShadow({
    sessionId: 'session-1',
    legacy: [
      renderable('assistant-1:0', {
        message_id: 'assistant-1',
        role: 'assistant',
        content: 'done',
        workflowItems: [
          {
            eventType: 'tool_call',
            status: 'loading',
            toolCallId: 'call-1',
            toolName: 'lookup'
          }
        ],
        subagents: [
          {
            key: 'child-run-1',
            run_id: 'child-run-1',
            status: 'running',
            terminal: false
          }
        ]
      })
    ],
    projection: [
      renderable('runtime:assistant:assistant-1', {
        __runtime_message_id: 'assistant-1',
        role: 'assistant',
        content: 'done',
        workflowItems: [
          {
            eventType: 'tool_result',
            status: 'completed',
            toolCallId: 'call-1',
            toolName: 'lookup'
          }
        ],
        subagents: [
          {
            key: 'child-run-1',
            run_id: 'child-run-1',
            status: 'completed',
            terminal: true
          }
        ]
      })
    ]
  });

  assert.equal(report.ok, false);
  assert.ok(report.issues.some((issue) => issue.code === 'render_workflow_drift'));
  assert.ok(report.issues.some((issue) => issue.code === 'render_subagent_drift'));
});
