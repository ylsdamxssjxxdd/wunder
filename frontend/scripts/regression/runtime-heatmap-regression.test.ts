import test from 'node:test';
import assert from 'node:assert/strict';

import { normalizeRuntimeHeatmapItems } from '../../src/utils/runtimeHeatmap';

test('runtime heatmap merges MCP display aliases by runtime name', () => {
  const items = normalizeRuntimeHeatmapItems([
    {
      tool: 'extra_mcp@kb_query_alpha',
      runtime_name: 'extra_mcp@kb_query_alpha',
      total_calls: 2
    },
    {
      tool: '知识库检索（示例）',
      runtime_name: 'extra_mcp@kb_query_alpha',
      category: 'mcp',
      total_calls: 3
    },
    {
      display_name: '数据库查询（示例）',
      tool_name: 'extra_mcp@db_query_alpha',
      category: 'mcp',
      total_calls: 1
    }
  ]);

  assert.equal(items.length, 2);
  const kb = items.find((item) => item.runtimeName === 'extra_mcp@kb_query_alpha');
  assert.ok(kb);
  assert.equal(kb.tool, '知识库检索（示例）');
  assert.equal(kb.category, 'mcp');
  assert.equal(kb.total_calls, 5);
});

test('runtime heatmap keeps old display-only data compatible', () => {
  const items = normalizeRuntimeHeatmapItems([
    {
      tool: '知识库检索（示例）',
      total_calls: 4
    }
  ]);

  assert.deepEqual(items, [
    {
      tool: '知识库检索（示例）',
      runtimeName: '知识库检索（示例）',
      category: 'other',
      group: 'other',
      source: 'other',
      total_calls: 4
    }
  ]);
});
