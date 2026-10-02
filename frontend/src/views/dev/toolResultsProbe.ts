import { createApp, h } from 'vue';
import { createPinia } from 'pinia';
import MessageToolWorkflow from '@/components/chat/MessageToolWorkflow.vue';
import type { WorkflowItem } from '@/components/chat/toolWorkflowRunModel';

// A UI fixture mounts the production workflow component with retained backend
// payloads. It tests expansion, rather than only the standalone formatters.
export const installToolResultsProbe = () => {
  const target = document.createElement('div');
  target.id = 'tool-results-probe';
  target.style.cssText = 'position:fixed;inset:20px;z-index:999;background:#f8fafc;padding:20px;overflow:auto';
  document.body.append(target);
  const fixtures = [
    { tool: 'read_file', args: { path: 'sample.txt' }, data: { content: '>>> sample.txt\nretained file content' } },
    { tool: 'write_file', args: { path: 'written.txt', content: 'written sample content' }, data: { path: 'written.txt' } },
    { tool: 'execute_command', args: { command: 'echo sample' }, data: { results: [{ command: 'echo sample', stdout: 'sample output', returncode: 0 }] } },
    { tool: 'apply_patch', args: {}, data: { changed_files: 1, files: [{ path: 'changed.txt', action: 'update', diff_blocks: [{ lines: [{ kind: 'delete', text: 'before' }, { kind: 'add', text: 'after' }] }] }] } },
    { tool: 'custom_tool', args: {}, data: { summary: 'Useful custom result', query_handle: 'hidden-handle', cursor: 987 } },
    { tool: 'write_file', args: { path: 'failed.txt', content: 'must not show as written' }, data: {}, failed: true },
    { tool: 'write_file', args: { path: 'pending.txt', content: 'must not show pending content' }, data: {}, pending: true }
  ];
  const items: WorkflowItem[] = fixtures.flatMap((fixture, index) => {
    const common = { toolName: fixture.tool, toolCallId: `fixture-call-${index}`, timestamp: index + 1 };
    const call = { ...common, id: `call-${index}`, eventType: 'tool_call', status: fixture.pending ? 'running' : 'completed',
      detail: JSON.stringify({ tool: fixture.tool, args: fixture.args }) };
    return fixture.pending ? [call] : [call, { ...common, id: `result-${index}`, eventType: 'tool_result',
      status: fixture.failed ? 'failed' : 'completed', detail: JSON.stringify({ tool: fixture.tool,
        ok: !fixture.failed, error: fixture.failed ? 'Write denied' : undefined, data: fixture.data }) }];
  });
  // Compaction events do not require tool fields in the durable protocol.
  for (const [index, status] of ['running', 'completed', 'failed'].entries()) {
    items.push({ id: `compaction-${index}`, eventType: 'compaction', title: 'tool_call', status,
      detail: JSON.stringify({ trigger_mode: 'manual', status, summary: 'Compaction result',
        summary_model_output: 'Retained compaction summary', context_tokens_before: 12000,
        context_tokens_after: 3000 }) });
  }
  for (const [tool, data] of [
    ['ptc', { path: 'fixture.py', returncode: 0, stdout: 'Computed fixture result', stderr: '' }],
    ['memory_manager', { count: 1, items: [{ title: 'Fixture preference', content: 'Retained memory detail' }] }]
  ] as const) {
    items.push({ id: tool, toolName: tool, toolCallId: tool, eventType: 'tool_result', status: 'completed',
      detail: JSON.stringify({ tool, ok: true, data: { status: 'completed', summary: '已完成', data } }) });
  }
  const app = createApp({ render: () => h(MessageToolWorkflow, { items, visible: true, loading: false, stateKey: 'tool-results-fixture' }) });
  app.use(createPinia());
  app.mount(target);
  return () => { app.unmount(); target.remove(); };
};
