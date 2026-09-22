<template>
  <section data-testid="developer-tool-preview">
    <MessageToolWorkflow :items="items" :visible="true" state-key="developer-tool-preview" />
  </section>
</template>

<script setup lang="ts">
import MessageToolWorkflow from '@/components/chat/MessageToolWorkflow.vue';

// Exercise the real workflow parser and card with a bounded server preview.
const data = {
  dry_run: true,
  changed_files: 1,
  added: 0,
  updated: 0,
  deleted: 1,
  moved: 0,
  hunks_applied: 1,
  added_lines: 0,
  deleted_lines: 10000,
  diff_lines_omitted: 9920,
  files: [{
    action: 'delete', path: 'file.txt', hunks: 1,
    deleted_lines: 10000, added_lines: 0, diff_lines_omitted: 9920,
    diff_blocks: [{
      header: 'deleted file',
      lines: Array.from({ length: 80 }, (_, index) => ({
        kind: 'delete', old_line: index + 1, new_line: null, text: `line ${index + 1}`
      }))
    }]
  }]
};
const items = [{
  id: 'preview-call', toolCallId: 'preview', eventType: 'tool_call',
  toolName: 'apply_patch', status: 'completed',
  detail: JSON.stringify({ args: { input: '*** Begin Patch\n*** Delete File: file.txt\n*** End Patch', dry_run: true } })
}, {
  id: 'preview-result', toolCallId: 'preview', eventType: 'tool_result',
  toolName: 'apply_patch', status: 'completed',
  detail: JSON.stringify({ tool: 'apply_patch', ok: true, data })
}];
</script>
