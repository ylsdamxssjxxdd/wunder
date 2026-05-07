<template>
  <div class="companion-lazy-list">
    <div class="companion-lazy-list-content">
      <div
        v-for="item in pagedItems"
        :key="`${item.scope || 'private'}:${item.id}`"
        class="companion-lazy-list-item"
        :class="{ active: isSelected(item) }"
        role="button"
        tabindex="0"
        @click="selectItem(item)"
        @keydown.enter.prevent="selectItem(item)"
        @keydown.space.prevent="selectItem(item)"
      >
        <span class="companion-lazy-list-item-preview" aria-hidden="true">
          <CompanionSprite
            v-if="item.spritesheetDataUrl"
            class="companion-lazy-list-item-sprite"
            :source="item.spritesheetDataUrl"
            state="idle"
            :scale="previewScale"
            fit
            paused
          />
        </span>
        <span class="companion-lazy-list-item-main">
          <span class="companion-lazy-list-item-title">
            <span class="companion-lazy-list-item-name">{{ item.displayName }}</span>
            <span class="companion-lazy-list-item-scope">
              {{ (item.scope || 'private') === 'global' ? globalLabel : privateLabel }}
            </span>
          </span>
          <span class="companion-lazy-list-item-desc">
            {{ item.description || noDescriptionLabel }}
          </span>
        </span>
        <button
          v-if="(item.scope || 'private') === 'private' && showDelete"
          class="companion-lazy-list-item-remove"
          type="button"
          :title="deleteLabel"
          :aria-label="deleteLabel"
          @click.stop="removeItem(item)"
        >
          <i class="fa-solid fa-trash-can" aria-hidden="true"></i>
        </button>
      </div>
    </div>

    <div v-if="!items.length" class="companion-lazy-list-empty">
      {{ emptyLabel }}
    </div>

    <div v-if="totalPages > 1" class="companion-lazy-list-pager">
      <button
        class="companion-lazy-list-pager-btn"
        type="button"
        :disabled="currentPage <= 1"
        @click="goToPage(currentPage - 1)"
      >
        <i class="fa-solid fa-chevron-left" aria-hidden="true"></i>
      </button>
      <span class="companion-lazy-list-pager-info">
        {{ currentPage }} / {{ totalPages }}
      </span>
      <button
        class="companion-lazy-list-pager-btn"
        type="button"
        :disabled="currentPage >= totalPages"
        @click="goToPage(currentPage + 1)"
      >
        <i class="fa-solid fa-chevron-right" aria-hidden="true"></i>
      </button>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, ref, watch } from 'vue';
import CompanionSprite from '@/components/companions/CompanionSprite.vue';
import type { CompanionPackageRecord } from '@/stores/companions';

const PAGE_SIZE = 12;
const PREVIEW_SCALE = 0.28;

const props = withDefaults(
  defineProps<{
    items: CompanionPackageRecord[];
    selectedScope?: 'global' | 'private';
    selectedId?: string;
    globalLabel?: string;
    privateLabel?: string;
    noDescriptionLabel?: string;
    deleteLabel?: string;
    emptyLabel?: string;
    showDelete?: boolean;
    pageSize?: number;
    previewScale?: number;
  }>(),
  {
    selectedScope: 'private',
    selectedId: '',
    globalLabel: 'Global',
    privateLabel: 'Private',
    noDescriptionLabel: 'No description',
    deleteLabel: 'Delete',
    emptyLabel: 'No companions',
    showDelete: true,
    pageSize: PAGE_SIZE,
    previewScale: PREVIEW_SCALE
  }
);

const emit = defineEmits<{
  (event: 'select', item: CompanionPackageRecord): void;
  (event: 'remove', item: CompanionPackageRecord): void;
  (event: 'page-change', page: number): void;
}>();

const currentPage = ref(1);

const totalPages = computed(() => Math.ceil(props.items.length / props.pageSize));

const pagedItems = computed(() => {
  const start = (currentPage.value - 1) * props.pageSize;
  const end = start + props.pageSize;
  return props.items.slice(start, end);
});

const isSelected = (item: CompanionPackageRecord): boolean => {
  const scope = item.scope || 'private';
  return scope === props.selectedScope && item.id === props.selectedId;
};

const selectItem = (item: CompanionPackageRecord) => {
  emit('select', item);
};

const removeItem = (item: CompanionPackageRecord) => {
  emit('remove', item);
};

const goToPage = (page: number) => {
  const newPage = Math.max(1, Math.min(totalPages.value, page));
  if (newPage !== currentPage.value) {
    currentPage.value = newPage;
    emit('page-change', newPage);
  }
};

watch(
  () => props.items,
  () => {
    currentPage.value = 1;
  }
);

watch(
  () => props.pageSize,
  () => {
    currentPage.value = 1;
  }
);
</script>

<style scoped>
.companion-lazy-list {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.companion-lazy-list-content {
  display: flex;
  flex-direction: column;
  gap: 6px;
  max-height: min(320px, 36vh);
  overflow-y: auto;
  overflow-x: hidden;
  padding-right: 4px;
}

.companion-lazy-list-item {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 8px 10px;
  border: 1px solid rgba(148, 163, 184, 0.18);
  border-radius: 12px;
  background: #fff;
  cursor: pointer;
  transition: background-color 0.15s ease, border-color 0.15s ease;
}

.companion-lazy-list-item:hover {
  background: rgba(0, 0, 0, 0.02);
}

.companion-lazy-list-item.active {
  border-color: rgba(var(--ui-accent-rgb, 246, 177, 76), 0.42);
  background: rgba(var(--ui-accent-rgb, 246, 177, 76), 0.08);
}

.companion-lazy-list-item:focus-visible {
  outline: none;
  border-color: rgba(var(--ui-accent-rgb, 59, 130, 246), 0.42);
  box-shadow: 0 0 0 2px rgba(var(--ui-accent-rgb, 59, 130, 246), 0.14);
}

.companion-lazy-list-item-preview {
  flex-shrink: 0;
  width: 54px;
  height: 58px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  overflow: hidden;
  border-radius: 10px;
  background: #f8fafc;
}

.companion-lazy-list-item-sprite {
  flex: 0 0 auto;
}

.companion-lazy-list-item-main {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.companion-lazy-list-item-title {
  display: flex;
  align-items: baseline;
  gap: 6px;
}

.companion-lazy-list-item-name {
  font-size: 13px;
  font-weight: 600;
  color: #1f2937;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.companion-lazy-list-item-scope {
  font-size: 10px;
  color: #6b7280;
  flex-shrink: 0;
  padding: 1px 5px;
  border-radius: 4px;
  background: rgba(0, 0, 0, 0.04);
}

.companion-lazy-list-item-desc {
  font-size: 11px;
  color: #6b7280;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.companion-lazy-list-item-remove {
  flex-shrink: 0;
  width: 28px;
  height: 28px;
  border: 0;
  border-radius: 6px;
  background: transparent;
  color: #9ca3af;
  cursor: pointer;
  display: flex;
  align-items: center;
  justify-content: center;
  transition: color 0.15s ease, background-color 0.15s ease;
}

.companion-lazy-list-item-remove:hover {
  color: #ef4444;
  background: rgba(239, 68, 68, 0.08);
}

.companion-lazy-list-empty {
  padding: 24px 16px;
  text-align: center;
  font-size: 13px;
  color: #6b7280;
}

.companion-lazy-list-pager {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 12px;
  padding-top: 8px;
  border-top: 1px solid rgba(148, 163, 184, 0.12);
}

.companion-lazy-list-pager-btn {
  width: 28px;
  height: 28px;
  border: 1px solid rgba(148, 163, 184, 0.22);
  border-radius: 6px;
  background: #fff;
  color: #475569;
  cursor: pointer;
  display: flex;
  align-items: center;
  justify-content: center;
  transition: background-color 0.15s ease, border-color 0.15s ease;
}

.companion-lazy-list-pager-btn:hover:not(:disabled) {
  background: rgba(0, 0, 0, 0.02);
  border-color: rgba(148, 163, 184, 0.32);
}

.companion-lazy-list-pager-btn:disabled {
  opacity: 0.4;
  cursor: not-allowed;
}

.companion-lazy-list-pager-info {
  font-size: 12px;
  color: #64748b;
  min-width: 48px;
  text-align: center;
}
</style>
