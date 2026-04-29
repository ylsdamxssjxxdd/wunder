<template>
  <div v-if="items.length || fallbackText" class="prompt-tooling-preview">
    <div v-if="items.length" class="prompt-tooling-preview-list" role="list">
      <div
        v-for="item in items"
        :key="item.key"
        class="prompt-tooling-preview-item"
        :class="`prompt-tooling-preview-item--${resolveItemTone(item)}`"
        :title="buildItemTitle(item)"
        role="listitem"
      >
        <AbilityIconBadge
          :name="item.name"
          :description="item.description"
          :hint="item.protocolName"
          :kind="item.kind"
          :group="item.group"
          :source="item.source"
          size="xs"
        />
        <div class="prompt-tooling-preview-copy">
          <span class="prompt-tooling-preview-name">{{ item.name }}</span>
          <span
            v-if="showProtocolName(item)"
            class="prompt-tooling-preview-protocol"
          >
            {{ item.protocolName }}
          </span>
        </div>
      </div>
    </div>
    <details v-if="items.length && fallbackText" class="prompt-tooling-preview-raw">
      <summary>JSON</summary>
      <pre class="prompt-tooling-preview-fallback">{{ fallbackText }}</pre>
    </details>
    <pre v-else-if="fallbackText" class="prompt-tooling-preview-fallback">{{ fallbackText }}</pre>
  </div>
</template>

<script setup lang="ts">
import AbilityIconBadge from '@/components/common/AbilityIconBadge.vue';
import { resolveAbilityVisual, type AbilityVisualTone } from '@/utils/abilityVisuals';
import type { PromptToolingPreviewItem } from '@/utils/promptToolingPreview';

withDefaults(
  defineProps<{
    items?: PromptToolingPreviewItem[];
    fallbackText?: string;
  }>(),
  {
    items: () => [],
    fallbackText: ''
  }
);

const isInternalProtocolName = (value: string): boolean =>
  /^tool_[a-z0-9]+$/i.test(value) || value.includes('@');

const showProtocolName = (item: PromptToolingPreviewItem): boolean =>
  Boolean(
    item.protocolName &&
      item.protocolName !== item.name &&
      !isInternalProtocolName(item.protocolName)
  );

const resolveItemTone = (item: PromptToolingPreviewItem): AbilityVisualTone =>
  resolveAbilityVisual({
    name: item.name,
    description: item.description,
    hint: item.protocolName,
    kind: item.kind,
    group: item.group,
    source: item.source
  }).tone;

const buildItemTitle = (item: PromptToolingPreviewItem): string =>
  [item.name, showProtocolName(item) ? item.protocolName : '', item.description]
    .filter(Boolean)
    .join('\n');
</script>

<style scoped>
.prompt-tooling-preview {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.prompt-tooling-preview-list {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(220px, 1fr));
  gap: 10px;
}

.prompt-tooling-preview-item {
  min-width: 0;
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 10px 12px;
  border-radius: 12px;
  border: 1px solid rgba(148, 163, 184, 0.24);
  background: rgba(148, 163, 184, 0.08);
}

.prompt-tooling-preview-item--skill {
  border-color: rgba(245, 158, 11, 0.24);
  background: rgba(245, 158, 11, 0.08);
}

.prompt-tooling-preview-item--mcp {
  border-color: rgba(14, 165, 233, 0.24);
  background: rgba(14, 165, 233, 0.08);
}

.prompt-tooling-preview-item--knowledge {
  border-color: rgba(16, 185, 129, 0.24);
  background: rgba(16, 185, 129, 0.08);
}

.prompt-tooling-preview-item--shared {
  border-color: rgba(139, 92, 246, 0.24);
  background: rgba(139, 92, 246, 0.08);
}

.prompt-tooling-preview-item--automation {
  border-color: rgba(99, 102, 241, 0.24);
  background: rgba(99, 102, 241, 0.08);
}

.prompt-tooling-preview-item--search {
  border-color: rgba(34, 197, 94, 0.24);
  background: rgba(34, 197, 94, 0.08);
}

.prompt-tooling-preview-item--file {
  border-color: rgba(59, 130, 246, 0.24);
  background: rgba(59, 130, 246, 0.08);
}

.prompt-tooling-preview-item--terminal {
  border-color: rgba(51, 65, 85, 0.24);
  background: rgba(51, 65, 85, 0.08);
}

.prompt-tooling-preview-copy {
  min-width: 0;
  flex: 1 1 auto;
  display: flex;
  align-items: center;
  gap: 8px;
}

.prompt-tooling-preview-name {
  min-width: 0;
  flex: 1 1 auto;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  color: var(--app-text-color, var(--chat-text, #f1f5f9));
  font-size: 13px;
  font-weight: 700;
  line-height: 1.3;
}

.prompt-tooling-preview-protocol {
  max-width: 48%;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  border-radius: 999px;
  border: 1px solid rgba(148, 163, 184, 0.24);
  padding: 2px 8px;
  color: var(--app-text-color-secondary, var(--chat-muted, rgba(226, 232, 240, 0.78)));
  font-size: 11px;
  line-height: 1.2;
  font-family: 'Cascadia Mono', 'SFMono-Regular', Consolas, monospace;
}

.prompt-tooling-preview-fallback {
  margin: 0;
  color: inherit;
  font-size: 12px;
  line-height: 1.55;
  white-space: pre-wrap;
  word-break: break-word;
}

.prompt-tooling-preview-raw {
  border-top: 1px solid rgba(148, 163, 184, 0.18);
  padding-top: 10px;
}

.prompt-tooling-preview-raw summary {
  cursor: pointer;
  user-select: none;
  color: var(--app-text-color-secondary, var(--chat-muted, rgba(226, 232, 240, 0.78)));
  font-size: 11px;
  line-height: 1.2;
}

.prompt-tooling-preview-raw[open] summary {
  margin-bottom: 10px;
}
</style>
