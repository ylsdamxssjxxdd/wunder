<template>
  <aside
    class="messenger-right-dock"
    :class="{
      'messenger-right-dock--collapsed': collapsed,
      'messenger-right-dock--edge-active': edgeActive
    }"
  >
    <div class="messenger-right-dock-toggle-hitbox" aria-hidden="true"></div>
    <button
      class="messenger-right-dock-toggle"
      type="button"
      :title="collapsed ? t('common.expand') : t('common.collapse')"
      :aria-label="collapsed ? t('common.expand') : t('common.collapse')"
      @click="$emit('toggle-collapse')"
    >
      <i class="fa-solid" :class="collapsed ? 'fa-chevron-left' : 'fa-chevron-right'" aria-hidden="true"></i>
    </button>
    <div v-if="!collapsed" class="messenger-right-content messenger-right-content--stack">
      <div class="messenger-right-panel messenger-right-panel--sandbox">
        <div v-if="showAgentPanels" class="messenger-workspace-scope chat-shell">
          <WorkspacePanel ref="workspacePanelRef" :agent-id="agentIdForApi" :container-id="containerId" />
        </div>
        <div v-else class="messenger-list-empty">{{ t('messenger.settings.agentOnly') }}</div>
      </div>

      <div
        class="messenger-right-panel messenger-right-panel--skills"
        :class="{ 'is-drop-active': skillDropActive }"
        @dragenter.prevent="handleSkillDragEnter"
        @dragover.prevent="handleSkillDragOver"
        @dragleave.prevent="handleSkillDragLeave"
        @drop.prevent="handleSkillDrop"
      >
        <div class="messenger-right-section-title messenger-right-section-title--with-actions">
          <span class="messenger-right-section-title-main">
            <i class="fa-solid fa-book" aria-hidden="true"></i>
            <span>技能 skill</span>
          </span>
        </div>
        <div v-if="skillsLoading && !enabledSkills.length && !disabledSkills.length" class="messenger-list-empty">
          {{ t('chat.ability.loading') }}
        </div>
        <div v-else-if="!enabledSkills.length && !disabledSkills.length" class="messenger-list-empty">
          {{ t('chat.ability.emptySkills') }}
        </div>
        <div v-else class="messenger-skill-groups" @wheel.capture="handleSkillGroupsWheel">
          <div class="messenger-skill-list">
            <el-tooltip
              v-for="item in enabledSkills"
              :key="`enabled-${item.name}`"
              placement="left-start"
              :disabled="true"
              popper-class="ability-card-popper"
            >
              <template #content>
                <AbilityTooltipCard
                  :name="item.name"
                  :description="item.description"
                  kind="skill"
                  group="skills"
                  source="skill"
                  :chips="[t('toolManager.system.skills'), t('common.enabled')]"
                />
              </template>
              <div
                class="messenger-skill-item"
                :class="item.enabled ? 'is-enabled' : 'is-disabled'"
                role="button"
                tabindex="0"
                @click="openSkillDetail(item.name)"
                @keydown.enter.prevent="openSkillDetail(item.name)"
                @keydown.space.prevent="openSkillDetail(item.name)"
              >
                <div class="ability-entry">
                  <AbilityIconBadge
                    :name="item.name"
                    :description="item.description"
                    kind="skill"
                    group="skills"
                    source="skill"
                    size="sm"
                  />
                  <div class="messenger-skill-item-copy ability-entry__copy">
                    <div class="messenger-skill-item-title-row">
                      <div class="messenger-skill-item-title" :title="item.name">{{ item.name }}</div>
                    </div>
                  </div>
                </div>
              </div>
            </el-tooltip>
            <div v-if="enabledSkills.length && disabledSkills.length" class="messenger-skill-divider" aria-hidden="true"></div>
            <el-tooltip
              v-for="item in disabledSkills"
              :key="`disabled-${item.name}`"
              placement="left-start"
              :disabled="true"
              popper-class="ability-card-popper"
            >
              <template #content>
                <AbilityTooltipCard
                  :name="item.name"
                  :description="item.description"
                  kind="skill"
                  group="skills"
                  source="skill"
                  :chips="[t('toolManager.system.skills'), t('common.disabled')]"
                />
              </template>
              <div
                class="messenger-skill-item"
                :class="item.enabled ? 'is-enabled' : 'is-disabled'"
                role="button"
                tabindex="0"
                @click="openSkillDetail(item.name)"
                @keydown.enter.prevent="openSkillDetail(item.name)"
                @keydown.space.prevent="openSkillDetail(item.name)"
              >
                <div class="ability-entry">
                  <AbilityIconBadge
                    :name="item.name"
                    :description="item.description"
                    kind="skill"
                    group="skills"
                    source="skill"
                    size="sm"
                  />
                  <div class="messenger-skill-item-copy ability-entry__copy">
                    <div class="messenger-skill-item-title-row">
                      <div class="messenger-skill-item-title" :title="item.name">{{ item.name }}</div>
                    </div>
                  </div>
                </div>
              </div>
            </el-tooltip>
          </div>
        </div>
      </div>
    </div>
  </aside>

</template>

<script setup lang="ts">
import { ref, watch } from 'vue';
import { ElMessage } from 'element-plus';

import AbilityIconBadge from '@/components/common/AbilityIconBadge.vue';
import AbilityTooltipCard from '@/components/common/AbilityTooltipCard.vue';
import { WorkspacePanel } from '@/components/messenger/lazyDockPanels';
import { useI18n } from '@/i18n';

type SkillItem = {
  name: string;
  description: string;
  enabled: boolean;
};

type WorkspacePanelViewRef = {
  refreshView?: (options?: { background?: boolean }) => Promise<boolean>;
};

const props = defineProps<{
  collapsed: boolean;
  edgeActive: boolean;
  showAgentPanels: boolean;
  agentIdForApi: string;
  containerId: number;
  skillsLoading: boolean;
  skillsUploading: boolean;
  enabledSkills: SkillItem[];
  disabledSkills: SkillItem[];
}>();

const emit = defineEmits<{
  (event: 'toggle-collapse'): void;
  (event: 'upload-skill-archive', file: File): void;
  (event: 'open-skill-detail', skillName: string): void;
  (event: 'open-container', containerId: number): void;
  (event: 'open-container-settings', containerId: number): void;
}>();

const { t } = useI18n();
const workspacePanelRef = ref<WorkspacePanelViewRef | null>(null);
const skillDropDepth = ref(0);
const skillDropActive = ref(false);

const isSkillArchiveFilename = (name: string): boolean => {
  const lower = String(name || '').trim().toLowerCase();
  return lower.endsWith('.zip') || lower.endsWith('.skill');
};

const emitSkillArchive = (file: File | null | undefined) => {
  if (!file || props.skillsUploading) return;
  if (!isSkillArchiveFilename(file.name)) {
    ElMessage.warning(t('userTools.skills.upload.zipOnly'));
    return;
  }
  emit('upload-skill-archive', file);
};

const openSkillDetail = (name: unknown) => {
  const normalized = String(name || '').trim();
  if (!normalized) return;
  emit('open-skill-detail', normalized);
};

const hasFilePayload = (event: DragEvent): boolean => {
  const transfer = event.dataTransfer;
  if (!transfer) return false;
  if (transfer.files && transfer.files.length > 0) return true;
  const types = Array.from(transfer.types || []);
  return types.includes('Files');
};

const handleSkillDragEnter = (event: DragEvent) => {
  if (props.skillsUploading || !hasFilePayload(event)) return;
  skillDropDepth.value += 1;
  skillDropActive.value = true;
};

const handleSkillDragOver = (event: DragEvent) => {
  if (props.skillsUploading || !hasFilePayload(event)) return;
  event.preventDefault();
  skillDropActive.value = true;
};

const handleSkillDragLeave = () => {
  skillDropDepth.value = Math.max(0, skillDropDepth.value - 1);
  if (!skillDropDepth.value) {
    skillDropActive.value = false;
  }
};

const handleSkillDrop = (event: DragEvent) => {
  skillDropDepth.value = 0;
  skillDropActive.value = false;
  if (props.skillsUploading) return;
  const file = event.dataTransfer?.files?.[0];
  emitSkillArchive(file);
};

const handleSkillGroupsWheel = (event: WheelEvent) => {
  const container = event.currentTarget as HTMLElement | null;
  if (!container) return;
  const deltaY = Number(event.deltaY || 0);
  if (!Number.isFinite(deltaY) || deltaY === 0) return;
  if (event.cancelable) {
    event.preventDefault();
  }
  event.stopPropagation();
  const maxScrollTop = Math.max(0, container.scrollHeight - container.clientHeight);
  const nextScrollTop = container.scrollTop + deltaY;
  container.scrollTop = Math.min(maxScrollTop, Math.max(0, nextScrollTop));
};

watch(
  () => props.collapsed,
  () => {
    skillDropDepth.value = 0;
    skillDropActive.value = false;
  }
);

const refreshWorkspace = async (options: { background?: boolean } = {}) => {
  if (!workspacePanelRef.value?.refreshView) {
    return false;
  }
  return workspacePanelRef.value.refreshView({
    background: options.background !== false
  });
};

defineExpose({
  refreshWorkspace
});
</script>
