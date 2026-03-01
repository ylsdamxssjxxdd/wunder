<template>
  <header
    class="desktop-window-chrome"
    data-tauri-drag-region
    @dblclick="handleToggleMaximize"
    @mousedown="handleDragRegionMouseDown"
  >
    <div class="desktop-window-title" data-tauri-drag-region>
      <img
        class="desktop-window-logo"
        :src="logoSrc"
        alt=""
        aria-hidden="true"
        @error="handleLogoError"
      />
      <span class="desktop-window-title-text">{{ titleText }}</span>
    </div>
    <div class="desktop-window-runtime" data-tauri-drag-region>
      <span class="desktop-window-runtime-label">{{ t('desktop.chrome.runtimeLabel') }}</span>
      <span class="desktop-window-runtime-value">{{ runtimeModeText }}</span>
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
import { computed, onBeforeUnmount, onMounted, ref } from 'vue';

import { isDesktopRemoteAuthMode, onDesktopRemoteModeChange } from '@/config/desktop';
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
const logoSrc = ref('/desktop-icon.png');
const runtimeMode = ref<'local' | 'hybrid'>('local');
const runtimeModeText = computed(() =>
  runtimeMode.value === 'hybrid'
    ? t('desktop.chrome.runtimeHybrid')
    : t('desktop.chrome.runtimeLocal')
);
let stopRuntimeModeListener: (() => void) | null = null;

const refreshRuntimeMode = () => {
  runtimeMode.value = isDesktopRemoteAuthMode() ? 'hybrid' : 'local';
};

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

const handleLogoError = () => {
  if (logoSrc.value === '/favicon.svg') {
    return;
  }
  logoSrc.value = '/favicon.svg';
};

const handleWindowResize = () => {
  refreshRuntimeMode();
  void refreshMaximizedState();
};

onMounted(async () => {
  refreshRuntimeMode();
  stopRuntimeModeListener = onDesktopRemoteModeChange((detail) => {
    runtimeMode.value = detail.remoteAuthMode ? 'hybrid' : 'local';
  });
  await refreshMaximizedState();
  window.addEventListener('resize', handleWindowResize);
  window.addEventListener('focus', handleWindowResize);
});

onBeforeUnmount(() => {
  if (stopRuntimeModeListener) {
    stopRuntimeModeListener();
    stopRuntimeModeListener = null;
  }
  window.removeEventListener('resize', handleWindowResize);
  window.removeEventListener('focus', handleWindowResize);
});
</script>

<style scoped>
.desktop-window-chrome {
  --desktop-window-chrome-height: 36px;
  --desktop-window-chrome-bg: linear-gradient(
    180deg,
    rgba(255, 255, 255, 0.98),
    rgba(248, 250, 252, 0.95)
  );
  --desktop-window-chrome-border: rgba(148, 163, 184, 0.3);
  --desktop-window-chrome-title: #344255;
  --desktop-window-runtime-label: #7a8594;
  --desktop-window-runtime-value: #3d4f67;
  --desktop-window-btn-color: #5f6b7a;
  --desktop-window-btn-hover-bg: rgba(var(--ui-accent-rgb), 0.16);
  --desktop-window-btn-hover-color: var(--ui-accent-deep);
  position: fixed;
  inset: 0 0 auto;
  height: var(--desktop-window-chrome-height);
  display: flex;
  align-items: center;
  justify-content: space-between;
  background: var(--desktop-window-chrome-bg);
  border-bottom: 1px solid var(--desktop-window-chrome-border);
  box-shadow: none;
  z-index: 3000;
  user-select: none;
  -webkit-app-region: drag;
}

:global(:root[data-user-theme='dark']) .desktop-window-chrome {
  --desktop-window-chrome-bg: linear-gradient(
    180deg,
    rgba(37, 46, 58, 0.96),
    rgba(30, 38, 50, 0.93)
  );
  --desktop-window-chrome-border: rgba(111, 124, 145, 0.5);
  --desktop-window-chrome-title: #edf2ff;
  --desktop-window-runtime-label: rgba(230, 237, 247, 0.78);
  --desktop-window-runtime-value: #f3f6ff;
  --desktop-window-btn-color: #d9e0ec;
  --desktop-window-btn-hover-bg: rgba(var(--ui-accent-rgb), 0.28);
  --desktop-window-btn-hover-color: #ffffff;
}

.desktop-window-title {
  flex: 1;
  min-width: 0;
  padding-left: 12px;
  display: inline-flex;
  align-items: center;
  gap: 8px;
  height: 100%;
}

.desktop-window-logo {
  width: 18px;
  height: 18px;
  border-radius: 4px;
  flex: 0 0 auto;
}

.desktop-window-title-text {
  font-size: 12px;
  font-weight: 600;
  color: var(--desktop-window-chrome-title);
  letter-spacing: 0.02em;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.desktop-window-runtime {
  position: absolute;
  left: 50%;
  top: 50%;
  transform: translate(-50%, -50%);
  display: inline-flex;
  align-items: center;
  gap: 6px;
  max-width: 50vw;
  font-size: 11px;
  line-height: 1;
  white-space: nowrap;
  color: var(--desktop-window-runtime-label);
  pointer-events: none;
}

.desktop-window-runtime-label {
  opacity: 0.9;
}

.desktop-window-runtime-value {
  font-weight: 600;
  color: var(--desktop-window-runtime-value);
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
  color: var(--desktop-window-btn-color);
  cursor: pointer;
  transition: background-color 0.15s ease, color 0.15s ease;
  -webkit-app-region: no-drag;
}

.desktop-window-btn:hover {
  background: var(--desktop-window-btn-hover-bg);
  color: var(--desktop-window-btn-hover-color);
}

.desktop-window-btn--close:hover {
  background: #d9534f;
  color: #ffffff;
}

@media (max-width: 760px) {
  .desktop-window-runtime {
    display: none;
  }
}
</style>
