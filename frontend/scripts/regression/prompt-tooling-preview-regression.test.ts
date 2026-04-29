import test from 'node:test';
import assert from 'node:assert/strict';

import { resolveAbilityVisual } from '../../src/utils/abilityVisuals';
import { resolveAgentOverviewAbilityCounts } from '../../src/views/messenger/agentOverviewAbilities';
import {
  extractPromptToolingPreview,
  inferPromptToolingAbilityMeta
} from '../../src/utils/promptToolingPreview';

test('prompt tooling preview infers skill meta for skill-like tools', () => {
  assert.deepEqual(
    inferPromptToolingAbilityMeta({
      name: 'Skill Builder',
      description: 'Create reusable workflow templates for prompts.'
    }),
    {
      kind: 'skill',
      group: 'skills',
      source: 'skills'
    }
  );
});

test('prompt tooling preview keeps MCP and knowledge tones aligned with shared ability visuals', () => {
  const preview = extractPromptToolingPreview({
    tooling_preview: {
      selected_tool_names: ['Skill Builder', 'github@get_issue', 'Policy Knowledge Search'],
      llm_tools: [
        {
          type: 'function',
          function: {
            name: 'skill_creator',
            description: 'Create reusable workflow templates for prompts.'
          }
        },
        {
          type: 'function',
          function: {
            name: 'github_get_issue',
            description: 'GitHub MCP endpoint for issue lookup.'
          }
        },
        {
          type: 'function',
          function: {
            name: 'policy_knowledge_search',
            description: 'Search policy knowledge base documents.'
          }
        }
      ],
      llm_tool_name_map: {
        skill_creator: 'Skill Builder',
        github_get_issue: 'github@get_issue',
        policy_knowledge_search: 'Policy Knowledge Search'
      }
    }
  });

  const byName = new Map(preview.items.map((item) => [item.name, item]));
  const skillItem = byName.get('Skill Builder');
  const mcpItem = byName.get('github@get_issue');
  const knowledgeItem = byName.get('Policy Knowledge Search');

  assert.ok(skillItem);
  assert.ok(mcpItem);
  assert.ok(knowledgeItem);

  assert.deepEqual(
    skillItem && {
      kind: skillItem.kind,
      group: skillItem.group,
      source: skillItem.source
    },
    {
      kind: 'skill',
      group: 'skills',
      source: 'skills'
    }
  );
  assert.equal(resolveAbilityVisual(skillItem || {}).tone, 'skill');
  assert.equal(resolveAbilityVisual(skillItem || {}).icon, 'fa-book');

  assert.deepEqual(
    mcpItem && {
      kind: mcpItem.kind,
      group: mcpItem.group,
      source: mcpItem.source
    },
    {
      kind: 'tool',
      group: 'mcp',
      source: 'mcp'
    }
  );
  assert.equal(resolveAbilityVisual(mcpItem || {}).tone, 'mcp');
  assert.equal(resolveAbilityVisual(mcpItem || {}).icon, 'fa-plug');

  assert.deepEqual(
    knowledgeItem && {
      kind: knowledgeItem.kind,
      group: knowledgeItem.group,
      source: knowledgeItem.source
    },
    {
      kind: 'tool',
      group: 'knowledge',
      source: 'knowledge'
    }
  );
  assert.equal(resolveAbilityVisual(knowledgeItem || {}).tone, 'knowledge');
  assert.equal(resolveAbilityVisual(knowledgeItem || {}).icon, 'fa-database');
});

test('prompt tooling preview tolerates display-only llm tool name map entries', () => {
  const preview = extractPromptToolingPreview({
    tooling_preview: {
      selected_tool_names: ['extra_mcp@kb_query_product_docs'],
      llm_tools: [
        {
          type: 'function',
          function: {
            name: 'tool_2d9e84',
            description: 'Search product knowledge base.'
          }
        }
      ],
      llm_tool_name_map: {
        tool_2d9e84: '知识库检索（产品文档）',
        extra_mcp@kb_query_product_docs: '知识库检索（产品文档）'
      }
    }
  });

  assert.ok(
    preview.items.some(
      (item) =>
        item.name === '知识库检索（产品文档）' &&
        item.protocolName === 'tool_2d9e84'
    )
  );
});

test('agent overview counts only selected structured skills and MCP items', () => {
  assert.deepEqual(
    resolveAgentOverviewAbilityCounts({
      declared_tool_names: ['read_file', 'write_file', 'github@get_issue', 'search_web'],
      declared_skill_names: ['planner'],
      ability_items: [
        {
          runtime_name: 'planner',
          name: 'planner',
          kind: 'skill',
          group: 'skills',
          source: 'skill',
          selected: true
        },
        {
          runtime_name: 'github@get_issue',
          name: 'github@get_issue',
          kind: 'tool',
          group: 'mcp',
          source: 'mcp',
          selected: true
        },
        {
          runtime_name: 'read_file',
          name: 'read_file',
          kind: 'tool',
          group: 'builtin',
          source: 'builtin',
          selected: true
        }
      ]
    }),
    {
      skillCount: 1,
      mcpCount: 1
    }
  );
});

test('agent overview does not infer MCP count from declared tool names without explicit MCP data', () => {
  assert.deepEqual(
    resolveAgentOverviewAbilityCounts({
      declared_tool_names: ['read_file', 'write_file', 'search_web'],
      declared_skill_names: ['planner']
    }),
    {
      skillCount: 1,
      mcpCount: 0
    }
  );
});
