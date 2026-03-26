<template>
  <aside
    class="messenger-right-dock"
    :class="{
      'messenger-right-dock--collapsed': collapsed,
      'messenger-right-dock--edge-active': edgeActive
    }"
    @pointerdown.right.stop="swallowRightDockRightPointer"
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
        <div
          class="messenger-right-section-title"
          @contextmenu.prevent.stop="openSandboxContainerMenu($event)"
          @mousedown.right.prevent.stop="openSandboxContainerMenu($event)"
        >
          <i class="fa-solid fa-box-archive" aria-hidden="true"></i>
          <span>{{ t('messenger.right.sandbox') }}</span>
        </div>
        <div v-if="showAgentPanels" class="messenger-workspace-scope chat-shell">
          <WorkspacePanel :agent-id="agentIdForApi" :container-id="containerId" />
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
        <div class="messenger-right-section-title">
          <span class="messenger-right-section-title-main">
            <i class="fa-solid fa-puzzle-piece" aria-hidden="true"></i>
            <span>{{ t('toolManager.system.skills') }}</span>
          </span>
          <button
            class="messenger-inline-btn messenger-inline-btn--compact messenger-skill-upload-btn"
            type="button"
            :disabled="skillsUploading"
            :title="t('userTools.skills.action.upload')"
            :aria-label="t('userTools.skills.action.upload')"
            @click="openSkillArchivePicker"
          >
            <i class="fa-solid fa-upload" aria-hidden="true"></i>
          </button>
          <input
            ref="skillArchiveInputRef"
            type="file"
            accept=".zip,.skill"
            hidden
            @change="handleSkillArchiveInputChange"
          />
        </div>
        <div class="messenger-skill-drop-hint" :class="{ 'is-active': skillDropActive, 'is-uploading': skillsUploading }">
          <i
            class="fa-solid"
            :class="skillsUploading ? 'fa-spinner fa-spin' : skillDropActive ? 'fa-file-zipper' : 'fa-cloud-arrow-up'"
            aria-hidden="true"
          ></i>
          <span>
            {{
              skillsUploading
                ? t('common.loading')
                : skillDropActive
                  ? t('userTools.skills.action.upload')
                  : t('userTools.skills.upload.zipOnly')
            }}
          </span>
        </div>
        <div v-if="skillsLoading && !enabledSkills.length && !disabledSkills.length" class="messenger-list-empty">
          {{ t('chat.ability.loading') }}
        </div>
        <div v-else class="messenger-skill-groups">
          <div class="messenger-skill-group">
            <div class="messenger-skill-group-header">
              <span>{{ t('common.enabled') }}</span>
              <span class="messenger-skill-group-count">{{ enabledSkills.length }}</span>
            </div>
            <div v-if="!enabledSkills.length" class="messenger-list-empty">{{ t('chat.ability.emptySkills') }}</div>
            <div v-else class="messenger-skill-list">
              <div
                v-for="item in enabledSkills"
                :key="`enabled-${item.name}`"
                class="messenger-skill-item is-enabled"
              >
                <div class="messenger-skill-item-title-row">
                  <div class="messenger-skill-item-title" :title="item.name">{{ item.name }}</div>
                  <span class="messenger-skill-status-tag">{{ t('common.enabled') }}</span>
                </div>
                <div class="messenger-skill-item-desc">
                  {{ item.description || t('chat.ability.noDesc') }}
                </div>
              </div>
            </div>
          </div>

          <div class="messenger-skill-group">
            <div class="messenger-skill-group-header">
              <span>{{ t('common.disabled') }}</span>
              <span class="messenger-skill-group-count">{{ disabledSkills.length }}</span>
            </div>
            <div v-if="!disabledSkills.length" class="messenger-list-empty">{{ t('chat.ability.emptySkills') }}</div>
            <div v-else class="messenger-skill-list">
              <div
                v-for="item in disabledSkills"
                :key="`disabled-${item.name}`"
                class="messenger-skill-item is-disabled"
              >
                <div class="messenger-skill-item-title-row">
                  <div class="messenger-skill-item-title" :title="item.name">{{ item.name }}</div>
                  <span class="messenger-skill-status-tag">{{ t('common.disabled') }}</span>
                </div>
                <div class="messenger-skill-item-desc">
                  {{ item.description || t('chat.ability.noDesc') }}
                </div>
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>
  </aside>

  <Teleport to="body">
    <div
      v-if="sandboxContextMenu.visible"
      ref="sandboxMenuRef"
      class="messenger-files-context-menu"
      :style="sandboxContextMenuStyle"
      @contextmenu.prevent
    >
      <button class="messenger-files-menu-btn" type="button" @click="handleSandboxMenuOpenContainer">
        {{ t('messenger.files.menu.open') }}
      </button>
      <button class="messenger-files-menu-btn" type="button" @click="handleSandboxMenuCopyId">
        {{ t('messenger.files.menu.copyId') }}
      </button>
      <button class="messenger-files-menu-btn" type="button" @click="handleSandboxMenuOpenSettings">
        {{ t('messenger.files.menu.settings') }}
      </button>
    </div>
  </Teleport>
</template>

<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import { ElMessage } from 'element-plus';

import { WorkspacePanel } from '@/components/messenger/lazyDockPanels';
import { useI18n } from '@/i18n';
import { copyText } from '@/utils/clipboard';

type SkillItem = {
  name: string;
  description: string;
  enabled: boolean;
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
  (event: 'open-container', containerId: number): void;
  (event: 'open-container-settings', containerId: number): void;
}>();

const { t } = useI18n();
const sandboxMenuRef = ref<HTMLElement | null>(null);
const skillArchiveInputRef = ref<HTMLInputElement | null>(null);
const skillDropDepth = ref(0);
const skillDropActive = ref(false);
const sandboxContextMenu = ref({
  visible: false,
  x: 0,
  y: 0
});
const sandboxContextMenuStyle = computed(() => ({
  left: `${sandboxContextMenu.value.x}px`,
  top: `${sandboxContextMenu.value.y}px`
}));

const closeSandboxContextMenu = () => {
  sandboxContextMenu.value.visible = false;
};

const swallowRightDockRightPointer = () => {
  // Stop bubbling right-click pointer events so overlay auto-hide logic does not collapse the dock.
};

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

const openSkillArchivePicker = () => {
  if (props.skillsUploading || !skillArchiveInputRef.value) return;
  skillArchiveInputRef.value.value = '';
  skillArchiveInputRef.value.click();
};

const handleSkillArchiveInputChange = (event: Event) => {
  const input = event.target as HTMLInputElement | null;
  const file = input?.files?.[0];
  emitSkillArchive(file);
  if (input) {
    input.value = '';
  }
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

const openSandboxContainerMenu = async (event: MouseEvent) => {
  const target = event.target as HTMLElement | null;
  if (target?.closest('.workspace-context-menu, .workspace-dialog')) {
    return;
  }
  const targetRect = target?.getBoundingClientRect();
  const initialX =
    Number.isFinite(event.clientX) && event.clientX > 0
      ? event.clientX
      : Math.round((targetRect?.left || 0) + (targetRect?.width || 0) / 2);
  const initialY =
    Number.isFinite(event.clientY) && event.clientY > 0
      ? event.clientY
      : Math.round((targetRect?.top || 0) + (targetRect?.height || 0) / 2);
  sandboxContextMenu.value.visible = true;
  sandboxContextMenu.value.x = initialX;
  sandboxContextMenu.value.y = initialY;
  await nextTick();
  const menuRect = sandboxMenuRef.value?.getBoundingClientRect();
  if (!menuRect) return;
  const maxLeft = Math.max(8, window.innerWidth - menuRect.width - 8);
  const maxTop = Math.max(8, window.innerHeight - menuRect.height - 8);
  sandboxContextMenu.value.x = Math.min(Math.max(8, sandboxContextMenu.value.x), maxLeft);
  sandboxContextMenu.value.y = Math.min(Math.max(8, sandboxContextMenu.value.y), maxTop);
};

const normalizeContainerId = (value: unknown): number => {
  const parsed = Number.parseInt(String(value ?? 1), 10);
  if (!Number.isFinite(parsed)) return 1;
  return Math.min(10, Math.max(1, parsed));
};

const handleSandboxMenuOpenContainer = () => {
  const targetId = normalizeContainerId(props.containerId);
  closeSandboxContextMenu();
  emit('open-container', targetId);
};

const handleSandboxMenuCopyId = async () => {
  const targetId = normalizeContainerId(props.containerId);
  closeSandboxContextMenu();
  const copied = await copyText(String(targetId));
  if (copied) {
    ElMessage.success(t('messenger.files.copyIdSuccess', { id: targetId }));
  } else {
    ElMessage.warning(t('messenger.files.copyIdFailed'));
  }
};

const handleSandboxMenuOpenSettings = () => {
  const targetId = normalizeContainerId(props.containerId);
  closeSandboxContextMenu();
  emit('open-container-settings', targetId);
};

const closeSandboxMenuWhenOutside = (event: Event) => {
  if (!sandboxContextMenu.value.visible) {
    return;
  }
  const target = event.target as Node | null;
  if (!target || !sandboxMenuRef.value?.contains(target)) {
    closeSandboxContextMenu();
  }
};

onMounted(() => {
  window.addEventListener('pointerdown', closeSandboxMenuWhenOutside);
  window.addEventListener('resize', closeSandboxContextMenu);
  document.addEventListener('scroll', closeSandboxContextMenu, true);
});

onBeforeUnmount(() => {
  window.removeEventListener('pointerdown', closeSandboxMenuWhenOutside);
  window.removeEventListener('resize', closeSandboxContextMenu);
  document.removeEventListener('scroll', closeSandboxContextMenu, true);
});

watch(
  () => props.collapsed,
  () => {
    closeSandboxContextMenu();
    skillDropDepth.value = 0;
    skillDropActive.value = false;
  }
);
</script>
