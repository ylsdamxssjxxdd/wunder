<template>
  <el-dialog
    :model-value="visible"
    class="messenger-dialog messenger-world-history-dialog"
    :title="t('messenger.world.history')"
    width="860px"
    append-to-body
    @update:model-value="handleDialogVisibleChange"
  >
    <div class="messenger-world-history-dialog">
      <div class="messenger-world-history-filter-row">
        <label class="messenger-world-history-search">
          <i class="fa-solid fa-magnifying-glass" aria-hidden="true"></i>
          <input
            v-model.trim="keywordModel"
            type="text"
            :placeholder="t('messenger.world.historySearch')"
            autocomplete="off"
            spellcheck="false"
          />
        </label>
        <el-date-picker
          v-model="dateRangeModel"
          type="daterange"
          unlink-panels
          value-format="x"
          :range-separator="t('messenger.world.historyDateRangeSeparator')"
          :start-placeholder="t('messenger.world.historyDateStart')"
          :end-placeholder="t('messenger.world.historyDateEnd')"
          class="messenger-world-history-date"
        />
      </div>

      <div class="messenger-world-history-tabs">
        <button
          v-for="tab in tabOptions"
          :key="tab.key"
          class="messenger-world-history-tab"
          :class="{ active: activeTab === tab.key }"
          type="button"
          @click="emit('update:activeTab', tab.key)"
        >
          {{ tab.label }}
        </button>
      </div>

      <div class="messenger-world-history-dialog-list">
        <button
          v-for="entry in records"
          :key="entry.key"
          class="messenger-world-history-record"
          type="button"
          :title="entry.rawContent"
          @click="emit('locate', entry)"
        >
          <div class="messenger-world-history-record-meta">
            <span class="messenger-world-history-record-sender">{{ entry.sender }}</span>
            <span class="messenger-world-history-record-time">{{ formatTime(entry.createdAt) }}</span>
          </div>
          <div class="messenger-world-history-record-content">
            <i class="fa-solid" :class="entry.icon" aria-hidden="true"></i>
            <span class="messenger-world-history-record-text">{{ entry.preview }}</span>
          </div>
        </button>
        <div v-if="!records.length" class="messenger-world-history-empty">
          {{ t('messenger.world.historyEmpty') }}
        </div>
      </div>
    </div>
  </el-dialog>
</template>

<script setup lang="ts">
import { computed } from 'vue';

import { useI18n } from '@/i18n';

type WorldHistoryCategory = 'all' | 'media' | 'document' | 'other_file';

type MessengerWorldHistoryTabOption = {
  key: WorldHistoryCategory;
  label: string;
};

type MessengerWorldHistoryRecord = {
  key: string;
  messageId: number;
  sender: string;
  createdAt: number;
  preview: string;
  rawContent: string;
  category: Exclude<WorldHistoryCategory, 'all'> | 'text';
  icon: string;
};

const props = defineProps<{
  visible: boolean;
  keyword: string;
  dateRange: [string, string] | [];
  activeTab: WorldHistoryCategory;
  tabOptions: MessengerWorldHistoryTabOption[];
  records: MessengerWorldHistoryRecord[];
  formatTime: (value: unknown) => string;
}>();

const emit = defineEmits<{
  'update:visible': [value: boolean];
  'update:keyword': [value: string];
  'update:dateRange': [value: [string, string] | []];
  'update:activeTab': [value: WorldHistoryCategory];
  locate: [entry: MessengerWorldHistoryRecord];
}>();

const { t } = useI18n();

const keywordModel = computed({
  get: () => props.keyword,
  set: (value: string) => {
    emit('update:keyword', String(value || '').trim());
  }
});

const dateRangeModel = computed({
  get: () => props.dateRange,
  set: (value: [string, string] | string[] | null) => {
    if (Array.isArray(value) && value.length === 2) {
      emit('update:dateRange', [String(value[0] || ''), String(value[1] || '')]);
      return;
    }
    emit('update:dateRange', []);
  }
});

const handleDialogVisibleChange = (nextVisible: boolean) => {
  emit('update:visible', Boolean(nextVisible));
};

const formatTime = (value: unknown): string => props.formatTime(value);
</script>
