<template>
  <el-dialog
    :model-value="visible"
    :title="t('userWorld.group.createTitle')"
    width="440px"
    class="messenger-dialog"
    append-to-body
    @update:model-value="handleVisibleChange"
  >
    <div class="messenger-group-create">
      <label class="messenger-group-create-field">
        <span>{{ t('userWorld.group.nameLabel') }}</span>
        <input
          v-model.trim="groupNameModel"
          type="text"
          :placeholder="t('userWorld.group.namePlaceholder')"
          autocomplete="off"
        />
      </label>
      <label class="messenger-group-create-field">
        <span>{{ t('userWorld.group.memberLabel') }}</span>
        <input
          v-model.trim="keywordModel"
          type="text"
          :placeholder="t('userWorld.group.memberPlaceholder')"
          autocomplete="off"
        />
      </label>
      <div class="messenger-group-create-list">
        <label
          v-for="contact in contacts"
          :key="`group-member-${contact.user_id}`"
          class="messenger-group-create-item"
        >
          <input v-model="memberIdsModel" type="checkbox" :value="String(contact.user_id || '')" />
          <span class="messenger-group-create-name">{{ contact.username || contact.user_id }}</span>
          <span class="messenger-group-create-unit">{{ resolveUnitLabel(contact.unit_id) }}</span>
        </label>
        <div v-if="!contacts.length" class="messenger-list-empty">
          {{ t('userWorld.group.memberEmpty') }}
        </div>
      </div>
    </div>
    <template #footer>
      <button class="messenger-inline-btn" type="button" :disabled="creating" @click="emit('update:visible', false)">
        {{ t('common.cancel') }}
      </button>
      <button class="messenger-inline-btn primary" type="button" :disabled="creating" @click="emit('submit')">
        {{ creating ? t('common.loading') : t('userWorld.group.createSubmit') }}
      </button>
    </template>
  </el-dialog>
</template>

<script setup lang="ts">
import { computed } from 'vue';

import { useI18n } from '@/i18n';

type GroupCreateContact = {
  user_id?: string | number;
  username?: string;
  unit_id?: string | number | null;
};

const props = defineProps<{
  visible: boolean;
  groupName: string;
  keyword: string;
  memberIds: string[];
  creating: boolean;
  contacts: GroupCreateContact[];
  resolveUnitLabel: (unitId: unknown) => string;
}>();

const emit = defineEmits<{
  'update:visible': [value: boolean];
  'update:groupName': [value: string];
  'update:keyword': [value: string];
  'update:memberIds': [value: string[]];
  submit: [];
}>();

const { t } = useI18n();

const groupNameModel = computed({
  get: () => props.groupName,
  set: (value: string) => emit('update:groupName', String(value || '').trim())
});

const keywordModel = computed({
  get: () => props.keyword,
  set: (value: string) => emit('update:keyword', String(value || '').trim())
});

const memberIdsModel = computed({
  get: () => props.memberIds,
  set: (value: string[]) =>
    emit(
      'update:memberIds',
      Array.from(new Set((value || []).map((item) => String(item || '').trim()).filter(Boolean)))
    )
});

const handleVisibleChange = (nextVisible: boolean) => {
  emit('update:visible', Boolean(nextVisible));
};

const resolveUnitLabel = (unitId: unknown): string => props.resolveUnitLabel(unitId);
</script>
