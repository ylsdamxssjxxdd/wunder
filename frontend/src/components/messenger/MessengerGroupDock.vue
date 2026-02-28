<template>
  <aside class="messenger-right-dock" :class="{ 'messenger-right-dock--collapsed': collapsed }">
    <button
      class="messenger-right-dock-toggle"
      type="button"
      :title="collapsed ? t('common.expand') : t('common.collapse')"
      :aria-label="collapsed ? t('common.expand') : t('common.collapse')"
      @click="$emit('toggle-collapse')"
    >
      <i class="fa-solid" :class="collapsed ? 'fa-chevron-left' : 'fa-chevron-right'" aria-hidden="true"></i>
    </button>
    <div v-if="!collapsed" class="messenger-right-content messenger-right-content--stack messenger-right-content--group">
      <div class="messenger-right-panel messenger-right-panel--group-announcement">
        <div class="messenger-right-section-title">
          <i class="fa-solid fa-bullhorn" aria-hidden="true"></i>
          <span>{{ t('messenger.groupDock.announcement') }}</span>
        </div>

        <div v-if="loading" class="messenger-list-empty">{{ t('common.loading') }}</div>
        <div v-else-if="errorText" class="messenger-group-dock-error">{{ errorText }}</div>
        <div v-else class="messenger-group-announcement-body">
          <p v-if="!isEditing" class="messenger-group-announcement-text">
            {{ detail?.announcement || t('messenger.groupDock.emptyAnnouncement') }}
          </p>
          <textarea
            v-else
            v-model="draftAnnouncement"
            class="messenger-group-announcement-input"
            :placeholder="t('messenger.groupDock.announcementPlaceholder')"
            maxlength="4000"
            spellcheck="false"
          ></textarea>

          <div class="messenger-group-announcement-meta">
            <span>
              {{
                detail?.announcement_updated_at
                  ? t('messenger.groupDock.updatedAt', { time: formatTime(detail.announcement_updated_at) })
                  : t('messenger.groupDock.neverUpdated')
              }}
            </span>
          </div>

          <div class="messenger-group-announcement-actions">
            <button
              v-if="!isEditing"
              class="messenger-inline-btn"
              type="button"
              :disabled="!detail"
              @click="startEdit"
            >
              {{ t('messenger.groupDock.editAnnouncement') }}
            </button>
            <template v-else>
              <button class="messenger-inline-btn" type="button" :disabled="saving" @click="cancelEdit">
                {{ t('common.cancel') }}
              </button>
              <button class="messenger-inline-btn primary" type="button" :disabled="saving" @click="submitEdit">
                {{ saving ? t('common.loading') : t('common.save') }}
              </button>
            </template>
          </div>
        </div>
      </div>

      <div class="messenger-right-panel messenger-right-panel--group-members">
        <div class="messenger-right-section-title">
          <i class="fa-solid fa-users" aria-hidden="true"></i>
          <span>{{ t('messenger.groupDock.members', { count: detail?.member_count || 0 }) }}</span>
        </div>
        <div v-if="loading" class="messenger-list-empty">{{ t('common.loading') }}</div>
        <div v-else-if="!members.length" class="messenger-list-empty">{{ t('messenger.groupDock.emptyMembers') }}</div>
        <div v-else class="messenger-group-member-list">
          <div v-for="member in members" :key="member.user_id" class="messenger-group-member-item">
            <div class="messenger-group-member-avatar">{{ avatarLabel(member.username || member.user_id) }}</div>
            <div class="messenger-group-member-main">
              <div class="messenger-group-member-row">
                <span class="messenger-group-member-name">{{ member.username || member.user_id }}</span>
                <span v-if="member.is_owner" class="messenger-kind-tag">{{ t('messenger.groupDock.owner') }}</span>
              </div>
              <div class="messenger-group-member-meta">
                <span>{{ member.user_id }}</span>
                <span>{{ member.unit_id || t('messenger.groupDock.noUnit') }}</span>
                <span>{{ member.status || '-' }}</span>
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>
  </aside>
</template>

<script setup lang="ts">
import { computed, ref, watch } from 'vue';

import { ElMessage } from 'element-plus';

import { getUserWorldGroupDetail, updateUserWorldGroupAnnouncement } from '@/api/userWorld';
import { useI18n } from '@/i18n';
import { showApiError } from '@/utils/apiError';

type GroupMember = {
  user_id: string;
  username: string;
  status: string;
  unit_id?: string | null;
  is_owner: boolean;
};

type GroupDetail = {
  group_id: string;
  conversation_id: string;
  group_name: string;
  owner_user_id: string;
  announcement?: string | null;
  announcement_updated_at?: number | null;
  member_count: number;
  updated_at: number;
  last_message_at: number;
  last_message_id?: number | null;
  last_message_preview?: string | null;
  members: GroupMember[];
};

const props = defineProps<{
  collapsed: boolean;
  groupId: string;
}>();

defineEmits<{
  (event: 'toggle-collapse'): void;
}>();

const { t } = useI18n();

const loading = ref(false);
const saving = ref(false);
const detail = ref<GroupDetail | null>(null);
const errorText = ref('');
const isEditing = ref(false);
const draftAnnouncement = ref('');
let requestToken = 0;

const members = computed(() => (Array.isArray(detail.value?.members) ? detail.value?.members || [] : []));

const normalizeTimestamp = (value: unknown): number => {
  const numeric = Number(value);
  if (Number.isFinite(numeric) && numeric > 0) {
    return numeric < 1_000_000_000_000 ? Math.floor(numeric * 1000) : Math.floor(numeric);
  }
  return 0;
};

const formatTime = (value: unknown): string => {
  const ts = normalizeTimestamp(value);
  if (!ts) return '--';
  const date = new Date(ts);
  const year = date.getFullYear();
  const month = String(date.getMonth() + 1).padStart(2, '0');
  const day = String(date.getDate()).padStart(2, '0');
  const hour = String(date.getHours()).padStart(2, '0');
  const minute = String(date.getMinutes()).padStart(2, '0');
  return `${year}-${month}-${day} ${hour}:${minute}`;
};

const avatarLabel = (value: unknown): string => {
  const text = String(value || '').trim();
  if (!text) return '#';
  if (/[a-zA-Z]/.test(text)) {
    return text.slice(0, 2).toUpperCase();
  }
  return text.slice(0, 2);
};

const applyDetail = (source: GroupDetail | null) => {
  detail.value = source;
  draftAnnouncement.value = String(source?.announcement || '');
};

const fetchGroupDetail = async (groupId: string) => {
  const cleaned = String(groupId || '').trim();
  if (!cleaned) {
    applyDetail(null);
    errorText.value = '';
    loading.value = false;
    return;
  }
  const currentToken = ++requestToken;
  loading.value = true;
  errorText.value = '';
  try {
    const response = await getUserWorldGroupDetail(cleaned);
    if (currentToken !== requestToken) return;
    const data = (response.data?.data || null) as GroupDetail | null;
    applyDetail(data);
  } catch (error) {
    if (currentToken !== requestToken) return;
    applyDetail(null);
    errorText.value = t('messenger.groupDock.loadFailed');
  } finally {
    if (currentToken === requestToken) {
      loading.value = false;
      isEditing.value = false;
    }
  }
};

const startEdit = () => {
  if (!detail.value) return;
  isEditing.value = true;
  draftAnnouncement.value = String(detail.value.announcement || '');
};

const cancelEdit = () => {
  isEditing.value = false;
  draftAnnouncement.value = String(detail.value?.announcement || '');
};

const submitEdit = async () => {
  const cleanedGroupId = String(props.groupId || '').trim();
  if (!cleanedGroupId || !detail.value) return;
  saving.value = true;
  try {
    const announcement = String(draftAnnouncement.value || '').trim();
    const response = await updateUserWorldGroupAnnouncement(cleanedGroupId, {
      announcement: announcement || null
    });
    const data = (response.data?.data || null) as GroupDetail | null;
    applyDetail(data);
    isEditing.value = false;
    ElMessage.success(t('messenger.groupDock.updateSuccess'));
  } catch (error) {
    showApiError(error, t('messenger.groupDock.updateFailed'));
  } finally {
    saving.value = false;
  }
};

watch(
  () => props.groupId,
  (groupId) => {
    fetchGroupDetail(groupId);
  },
  { immediate: true }
);
</script>
