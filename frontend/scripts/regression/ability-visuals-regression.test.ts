import test from 'node:test';
import assert from 'node:assert/strict';

import {
  resolveAbilityVisual,
  resolveToolIconClass
} from '../../src/utils/abilityVisuals';

test('tool icon inference ignores generic search wording in names and descriptions', () => {
  assert.equal(
    resolveToolIconClass({
      name: 'generic ability',
      description: 'description mentions \u68c0\u7d22 wording'
    }),
    'fa-toolbox'
  );

  assert.equal(
    resolveToolIconClass({
      name: 'generic query wording',
      description: 'search and retrieve are regular wording here'
    }),
    'fa-toolbox'
  );
});

test('tool icon inference keeps explicit search tool identifiers', () => {
  assert.equal(resolveToolIconClass({ name: 'web_search' }), 'fa-magnifying-glass');
  assert.equal(resolveToolIconClass({ name: 'search_content' }), 'fa-magnifying-glass');
});

test('ability visuals keep skill and knowledge defaults when descriptions mention retrieval', () => {
  assert.deepEqual(
    resolveAbilityVisual({
      kind: 'skill',
      group: 'skill',
      name: 'generic skill',
      description: 'description mentions \u68c0\u7d22 wording'
    }),
    {
      icon: 'fa-book',
      tone: 'skill'
    }
  );

  assert.deepEqual(
    resolveAbilityVisual({
      group: 'knowledge',
      source: 'user_knowledge',
      name: 'generic knowledge',
      description: 'description mentions \u68c0\u7d22 wording'
    }),
    {
      icon: 'fa-database',
      tone: 'knowledge'
    }
  );
});

test('ability visuals keep MCP sources on plug icon over skill-like wording', () => {
  assert.deepEqual(
    resolveAbilityVisual({
      group: 'mcp',
      source: 'mcp',
      name: 'template_read',
      description: 'Read presentation template metadata for generated pages.'
    }),
    {
      icon: 'fa-plug',
      tone: 'mcp'
    }
  );

  assert.equal(
    resolveToolIconClass({
      group: 'user-mcp',
      source: 'user_mcp',
      name: 'prompt_template_query',
      description: 'Query knowledge documents through a prompt template.'
    }),
    'fa-plug'
  );
});
