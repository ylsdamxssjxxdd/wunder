<template>
  <Teleport to="body">
    <div
      v-if="visible"
      ref="menuElement"
      class="messenger-files-context-menu"
      :style="menuStyle"
      @contextmenu.prevent
    >
      <button class="messenger-files-menu-btn" type="button" @click="emit('open')">
        {{ t('messenger.files.menu.open') }}
      </button>
      <button class="messenger-files-menu-btn" type="button" @click="emit('copy-id')">
        {{ t('messenger.files.menu.copyId') }}
      </button>
      <button class="messenger-files-menu-btn" type="button" @click="emit('settings')">
        {{ t('messenger.files.menu.settings') }}
      </button>
    </div>
  </Teleport>
</template>

<script setup lang="ts">
import { computed, ref } from 'vue';

import { useI18n } from '@/i18n';

const props = defineProps<{
  visible: boolean;
  style?: Record<string, string>;
}>();

const emit = defineEmits<{
  open: [];
  'copy-id': [];
  settings: [];
}>();

const { t } = useI18n();
const menuElement = ref<HTMLElement | null>(null);

const menuStyle = computed(() => props.style || {});
const getMenuElement = (): HTMLElement | null => menuElement.value;

defineExpose({
  getMenuElement
});
</script>
