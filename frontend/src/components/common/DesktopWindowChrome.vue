<template>
  <header
    class="desktop-window-chrome"
    data-tauri-drag-region
    @dblclick="handleToggleMaximize"
    @mousedown="handleDragRegionMouseDown"
  >
    <div class="desktop-window-title" data-tauri-drag-region>
      <span class="desktop-window-title-text">{{ titleText }}</span>
    </div>
    <div class="desktop-window-controls" data-tauri-drag-region="false">
      <button
        class="desktop-window-btn"
        type="button"
        :title="t('desktop.action.minimize')"
        :aria-label="t('desktop.action.minimize')"
        @click.stop="handleMinimize"
      >
        <i class="fa-solid fa-minus" aria-hidden="true"></i>
      </button>
      <button
        class="desktop-window-btn"
        type="button"
        :title="windowMaximized ? t('desktop.action.restore') : t('desktop.action.maximize')"
        :aria-label="windowMaximized ? t('desktop.action.restore') : t('desktop.action.maximize')"
        @click.stop="handleToggleMaximize"
      >
        <i
          class="fa-regular"
          :class="windowMaximized ? 'fa-window-restore' : 'fa-square'"
          aria-hidden="true"
        ></i>
      </button>
      <button
        class="desktop-window-btn desktop-window-btn--close"
        type="button"
        :title="t('desktop.action.close')"
        :aria-label="t('desktop.action.close')"
        @click.stop="handleClose"
      >
        <i class="fa-solid fa-xmark" aria-hidden="true"></i>
      </button>
    </div>
  </header>
</template>

<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref } from 'vue';

import { useI18n } from '@/i18n';

type DesktopWindowBridge = {
  minimizeWindow?: () => Promise<void> | void;
  toggleMaximizeWindow?: () => Promise<void> | void;
  closeWindow?: () => Promise<void> | void;
  isWindowMaximized?: () => Promise<boolean> | boolean;
  startWindowDrag?: () => Promise<void> | void;
};

const { t } = useI18n();
const windowMaximized = ref(false);
const titleText = 'Wunder Desktop';

const getDesktopBridge = (): DesktopWindowBridge | null => {
  if (typeof window === 'undefined') return null;
  const candidate = (window as Window & { wunderDesktop?: DesktopWindowBridge }).wunderDesktop;
  return candidate && typeof candidate === 'object' ? candidate : null;
};

const refreshMaximizedState = async () => {
  const bridge = getDesktopBridge();
  if (!bridge || typeof bridge.isWindowMaximized !== 'function') {
    windowMaximized.value = false;
    return;
  }
  try {
    windowMaximized.value = Boolean(await bridge.isWindowMaximized());
  } catch {
    // Ignore non-critical sync failures.
  }
};

const handleMinimize = async () => {
  const bridge = getDesktopBridge();
  if (!bridge || typeof bridge.minimizeWindow !== 'function') return;
  try {
    await bridge.minimizeWindow();
  } catch {
    // Ignore non-critical minimize failures.
  }
};

const handleToggleMaximize = async () => {
  const bridge = getDesktopBridge();
  if (!bridge || typeof bridge.toggleMaximizeWindow !== 'function') return;
  try {
    await bridge.toggleMaximizeWindow();
  } catch {
    // Ignore non-critical maximize failures.
  } finally {
    await refreshMaximizedState();
  }
};

const handleClose = async () => {
  const bridge = getDesktopBridge();
  if (!bridge || typeof bridge.closeWindow !== 'function') return;
  try {
    await bridge.closeWindow();
  } catch {
    // Ignore non-critical close failures.
  }
};

const handleDragRegionMouseDown = async (event: MouseEvent) => {
  if (event.button !== 0) return;
  const target = event.target as HTMLElement | null;
  if (target?.closest('[data-tauri-drag-region=\"false\"]')) return;
  const bridge = getDesktopBridge();
  if (!bridge || typeof bridge.startWindowDrag !== 'function') return;
  try {
    await bridge.startWindowDrag();
  } catch {
    // Ignore non-critical drag failures.
  }
};

const handleWindowResize = () => {
  void refreshMaximizedState();
};

onMounted(async () => {
  await refreshMaximizedState();
  window.addEventListener('resize', handleWindowResize);
  window.addEventListener('focus', handleWindowResize);
});

onBeforeUnmount(() => {
  window.removeEventListener('resize', handleWindowResize);
  window.removeEventListener('focus', handleWindowResize);
});
</script>

<style scoped>
.desktop-window-chrome {
  --desktop-window-chrome-height: 36px;
  position: fixed;
  inset: 0 0 auto;
  height: var(--desktop-window-chrome-height);
  display: flex;
  align-items: center;
  justify-content: space-between;
  background: rgba(255, 255, 255, 0.9);
  border-bottom: 1px solid rgba(var(--ui-accent-rgb), 0.18);
  z-index: 3000;
  user-select: none;
  -webkit-app-region: drag;
}

.desktop-window-title {
  flex: 1;
  min-width: 0;
  padding-left: 12px;
  display: inline-flex;
  align-items: center;
  height: 100%;
}

.desktop-window-title-text {
  font-size: 12px;
  font-weight: 600;
  color: #4b5563;
  letter-spacing: 0.02em;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.desktop-window-controls {
  display: inline-flex;
  align-items: stretch;
  height: 100%;
  -webkit-app-region: no-drag;
}

.desktop-window-btn {
  width: 44px;
  border: none;
  background: transparent;
  color: #5f6b7a;
  cursor: pointer;
  transition: background-color 0.15s ease, color 0.15s ease;
  -webkit-app-region: no-drag;
}

.desktop-window-btn:hover {
  background: rgba(var(--ui-accent-rgb), 0.15);
  color: var(--ui-accent-deep);
}

.desktop-window-btn--close:hover {
  background: #d9534f;
  color: #ffffff;
}
</style>
