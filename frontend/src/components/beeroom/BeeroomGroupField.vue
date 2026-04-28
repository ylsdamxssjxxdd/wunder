<template>
  <div class="beeroom-group-field">
    <div v-if="allowCreate" class="beeroom-group-field__mode-row">
      <button
        class="beeroom-group-field__mode-btn"
        :class="{ active: localDraft.mode === 'existing' }"
        type="button"
        :disabled="disabled"
        @click="setMode('existing')"
      >
        {{ t('messenger.agentGroup.modeExisting') }}
      </button>
      <button
        class="beeroom-group-field__mode-btn"
        :class="{ active: localDraft.mode === 'new' }"
        type="button"
        :disabled="disabled"
        @click="setMode('new')"
      >
        {{ t('messenger.agentGroup.modeNew') }}
      </button>
    </div>

    <template v-if="localDraft.mode === 'existing' || !allowCreate">
      <el-select
        v-model="localDraft.hive_id"
        clearable
        filterable
        class="beeroom-group-field__select"
        :disabled="disabled"
        :placeholder="t('messenger.agentGroup.placeholder')"
        @change="emitChange"
      >
        <el-option :label="t('messenger.agentGroup.noneOption')" :value="UNGROUPED_GROUP_VALUE" />
        <el-option
          v-for="group in normalizedGroups"
          :key="group.group_id"
          :label="group.name || group.group_id"
          :value="group.group_id"
        />
      </el-select>
    </template>

    <template v-else>
      <el-input
        v-model="localDraft.hive_name"
        class="beeroom-group-field__input"
        :disabled="disabled"
        :placeholder="t('messenger.agentGroup.newNamePlaceholder')"
        @input="emitChange"
      />
      <el-input
        v-model="localDraft.hive_description"
        class="beeroom-group-field__input"
        type="textarea"
        :rows="3"
        :disabled="disabled"
        :placeholder="t('messenger.agentGroup.newDescriptionPlaceholder')"
        @input="emitChange"
      />
      <div class="beeroom-group-field__hint">
        {{ t('messenger.agentGroup.newHint') }}
      </div>
    </template>
  </div>
</template>

<script setup lang="ts">
import { computed, reactive, watch } from 'vue';
import { useI18n } from '@/i18n';

import type { BeeroomGroupDraft, BeeroomGroupOption } from '@/utils/beeroomGroupDraft';
import { createBeeroomGroupDraft, normalizeBeeroomGroupDraft } from '@/utils/beeroomGroupDraft';

const props = withDefaults(
  defineProps<{
    modelValue?: Partial<BeeroomGroupDraft> | null;
    groups?: BeeroomGroupOption[];
    defaultGroupId?: string;
    allowCreate?: boolean;
    disabled?: boolean;
  }>(),
  {
    modelValue: null,
    groups: () => [],
    defaultGroupId: '',
    allowCreate: true,
    disabled: false
  }
);

const emit = defineEmits<{
  (event: 'update:modelValue', value: BeeroomGroupDraft): void;
}>();

const { t } = useI18n();
const UNGROUPED_GROUP_VALUE = '__ungrouped__';

function normalizeSelectableGroupId(value: unknown) {
  const cleaned = String(value || '').trim();
  return cleaned ? cleaned : UNGROUPED_GROUP_VALUE;
}

function normalizePayloadGroupId(value: unknown) {
  const cleaned = String(value || '').trim();
  return cleaned === UNGROUPED_GROUP_VALUE ? '' : cleaned;
}

const normalizedGroups = computed(() =>
  (Array.isArray(props.groups) ? props.groups : [])
    .map((group) => ({
      group_id: String(group?.group_id || '').trim(),
      name: String(group?.name || group?.group_id || '').trim(),
      description: String(group?.description || '').trim(),
      is_default: Boolean(group?.is_default)
    }))
    .filter((group) => group.group_id.length > 0)
);

const localDraft = reactive<BeeroomGroupDraft>(createBeeroomGroupDraft(props.defaultGroupId));

const resolveSelectedGroupMeta = (value: unknown) => {
  const groupId = normalizePayloadGroupId(value);
  if (!groupId) {
    return { hive_name: '', hive_description: '' };
  }
  const matched = normalizedGroups.value.find((group) => group.group_id === groupId);
  return {
    hive_name: String(matched?.name || '').trim(),
    hive_description: String(matched?.description || '').trim()
  };
};

const syncLocalDraft = (value: Partial<BeeroomGroupDraft> | null | undefined) => {
  const next = normalizeBeeroomGroupDraft(value, props.defaultGroupId);
  localDraft.mode = props.allowCreate ? next.mode : 'existing';
  localDraft.hive_id =
    localDraft.mode === 'existing' ? normalizeSelectableGroupId(next.hive_id) : next.hive_id;
  const selectedMeta =
    localDraft.mode === 'existing' ? resolveSelectedGroupMeta(next.hive_id) : { hive_name: '', hive_description: '' };
  localDraft.hive_name = next.hive_name || selectedMeta.hive_name;
  localDraft.hive_description = next.hive_description || selectedMeta.hive_description;
};

const emitChange = () => {
  const next = normalizeBeeroomGroupDraft(localDraft, props.defaultGroupId);
  const nextHiveId =
    next.mode === 'existing' ? normalizePayloadGroupId(next.hive_id) : next.hive_id;
  if (next.mode === 'existing') {
    const selectedMeta = resolveSelectedGroupMeta(nextHiveId);
    emit('update:modelValue', {
      mode: 'existing',
      hive_id: nextHiveId,
      hive_name: next.hive_name || selectedMeta.hive_name,
      hive_description: next.hive_description || selectedMeta.hive_description
    });
    return;
  }
  emit('update:modelValue', {
    ...next,
    hive_id: nextHiveId
  });
};

const setMode = (mode: 'existing' | 'new') => {
  if (props.disabled || !props.allowCreate) return;
  localDraft.mode = mode;
  if (mode === 'existing') {
    if (!localDraft.hive_id) {
      localDraft.hive_id = String(props.defaultGroupId || '').trim();
    }
    const selectedMeta = resolveSelectedGroupMeta(localDraft.hive_id);
    localDraft.hive_name = selectedMeta.hive_name;
    localDraft.hive_description = selectedMeta.hive_description;
  } else {
    localDraft.hive_id = '';
  }
  emitChange();
};

watch(
  () => props.modelValue,
  (value) => {
    syncLocalDraft(value);
  },
  { immediate: true, deep: true }
);

watch(
  () => props.defaultGroupId,
  (value) => {
    if (localDraft.mode === 'existing' && !String(localDraft.hive_id || '').trim()) {
      localDraft.hive_id = String(value || '').trim();
      emitChange();
    }
  }
);

watch(
  () => props.allowCreate,
  (allowCreate) => {
    if (allowCreate || localDraft.mode === 'existing') return;
    localDraft.mode = 'existing';
    const selectedMeta = resolveSelectedGroupMeta(localDraft.hive_id);
    localDraft.hive_name = selectedMeta.hive_name;
    localDraft.hive_description = selectedMeta.hive_description;
    emitChange();
  }
);

watch(
  normalizedGroups,
  () => {
    if (localDraft.mode !== 'existing') {
      return;
    }
    const selectedMeta = resolveSelectedGroupMeta(localDraft.hive_id);
    if (
      selectedMeta.hive_name === localDraft.hive_name &&
      selectedMeta.hive_description === localDraft.hive_description
    ) {
      return;
    }
    localDraft.hive_name = selectedMeta.hive_name;
    localDraft.hive_description = selectedMeta.hive_description;
    emitChange();
  },
  { deep: true }
);
</script>

<style scoped>
.beeroom-group-field {
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.beeroom-group-field__mode-row {
  display: inline-flex;
  gap: 8px;
  flex-wrap: wrap;
}

.beeroom-group-field__mode-btn {
  min-height: 34px;
  padding: 0 12px;
  border: 1px solid var(--hula-border);
  border-radius: 999px;
  background: var(--hula-main-bg);
  color: var(--hula-text-color);
  cursor: pointer;
}

.beeroom-group-field__mode-btn.active {
  border-color: var(--hula-accent);
  background: var(--hula-accent-soft);
  color: var(--hula-accent);
}

.beeroom-group-field__select,
.beeroom-group-field__input {
  width: 100%;
}

.beeroom-group-field__select :deep(.el-select__wrapper),
.beeroom-group-field__select :deep(.el-select__selection),
.beeroom-group-field__select :deep(.el-select__selected-item),
.beeroom-group-field__select :deep(.el-select__placeholder),
.beeroom-group-field__select :deep(.el-select__caret) {
  cursor: pointer;
}

.beeroom-group-field__select :deep(.el-select__input) {
  cursor: text;
}

.beeroom-group-field__hint {
  color: var(--hula-muted);
  font-size: 12px;
  line-height: 1.5;
}
</style>
