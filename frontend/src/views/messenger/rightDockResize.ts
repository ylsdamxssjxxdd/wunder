import { computed, onBeforeUnmount, onMounted, ref, watch, type ComputedRef, type Ref } from 'vue';

const DEFAULT_RIGHT_DOCK_WIDTH = 284;
const MIN_RIGHT_DOCK_WIDTH = 248;
const MAX_RIGHT_DOCK_WIDTH = 520;
const RIGHT_DOCK_LEFT_RAIL_WIDTH = 56;
const RIGHT_DOCK_MIDDLE_PANE_WIDTH = 220;
const RIGHT_DOCK_MIN_CHAT_WIDTH = 460;
const RIGHT_DOCK_OVERLAY_GUTTER = 20;

type BooleanLikeRef = Readonly<Ref<boolean>> | ComputedRef<boolean>;

type UseMessengerRightDockResizeOptions = {
  hostWidth: Ref<number>;
  isOverlay: BooleanLikeRef;
  isMiddlePaneOverlay: BooleanLikeRef;
  navigationPaneCollapsed: BooleanLikeRef;
  collapsed: BooleanLikeRef;
  storageKey: string;
};

export function useMessengerRightDockResize(options: UseMessengerRightDockResizeOptions) {
  const rightDockWidth = ref(DEFAULT_RIGHT_DOCK_WIDTH);
  const isRightDockResizing = ref(false);

  let activeResizePointerId: number | null = null;
  let dragStartClientX = 0;
  let dragStartWidth = DEFAULT_RIGHT_DOCK_WIDTH;

  const resolveWidthBounds = () => {
    const hostWidth = Math.max(0, Math.round(options.hostWidth.value || 0));
    if (options.isOverlay.value) {
      const overlayMaxWidth = Math.min(
        MAX_RIGHT_DOCK_WIDTH,
        Math.max(MIN_RIGHT_DOCK_WIDTH, hostWidth - RIGHT_DOCK_LEFT_RAIL_WIDTH - RIGHT_DOCK_OVERLAY_GUTTER)
      );
      return {
        min: MIN_RIGHT_DOCK_WIDTH,
        max: overlayMaxWidth
      };
    }

    // Keep the center chat area above its minimum readable width when the dock is resized.
    const occupiedLeftWidth =
      RIGHT_DOCK_LEFT_RAIL_WIDTH +
      (!options.isMiddlePaneOverlay.value && !options.navigationPaneCollapsed.value
        ? RIGHT_DOCK_MIDDLE_PANE_WIDTH
        : 0);
    const layoutMaxWidth = Math.min(
      MAX_RIGHT_DOCK_WIDTH,
      Math.max(MIN_RIGHT_DOCK_WIDTH, hostWidth - occupiedLeftWidth - RIGHT_DOCK_MIN_CHAT_WIDTH)
    );
    return {
      min: MIN_RIGHT_DOCK_WIDTH,
      max: layoutMaxWidth
    };
  };

  const clampRightDockWidth = (value: number) => {
    const bounds = resolveWidthBounds();
    const normalized = Math.round(Number.isFinite(value) ? value : DEFAULT_RIGHT_DOCK_WIDTH);
    return Math.max(bounds.min, Math.min(bounds.max, normalized));
  };

  const resolvedRightDockWidth = computed(() => clampRightDockWidth(rightDockWidth.value));
  const rightDockStyle = computed(() => ({
    '--messenger-right-dock-width': `${resolvedRightDockWidth.value}px`
  }));
  const rightDockResizable = computed(() => !options.isOverlay.value && !options.collapsed.value);

  const persistRightDockWidth = () => {
    if (typeof window === 'undefined') return;
    try {
      window.localStorage.setItem(options.storageKey, String(resolvedRightDockWidth.value));
    } catch {
      // Ignore localStorage write failures and keep the session-local width.
    }
  };

  const applyRightDockWidth = (value: number, options: { persist?: boolean } = {}) => {
    const nextWidth = clampRightDockWidth(value);
    if (nextWidth !== rightDockWidth.value) {
      rightDockWidth.value = nextWidth;
    }
    if (options.persist) {
      persistRightDockWidth();
    }
  };

  const resetRightDockWidth = () => {
    applyRightDockWidth(DEFAULT_RIGHT_DOCK_WIDTH, { persist: true });
  };

  const nudgeRightDockWidth = (delta: number) => {
    applyRightDockWidth((rightDockWidth.value || resolvedRightDockWidth.value || DEFAULT_RIGHT_DOCK_WIDTH) + delta, {
      persist: true
    });
  };

  const stopRightDockResize = () => {
    activeResizePointerId = null;
    if (!isRightDockResizing.value) return;
    isRightDockResizing.value = false;
    persistRightDockWidth();
  };

  const handleRightDockResizePointerMove = (event: PointerEvent) => {
    if (activeResizePointerId === null || event.pointerId !== activeResizePointerId) return;
    applyRightDockWidth(dragStartWidth + (dragStartClientX - event.clientX));
  };

  const handleRightDockResizePointerUp = (event: PointerEvent) => {
    if (activeResizePointerId === null || event.pointerId !== activeResizePointerId) return;
    stopRightDockResize();
  };

  const startRightDockResize = (event: PointerEvent) => {
    if (event.button !== 0 || !rightDockResizable.value) return;
    activeResizePointerId = event.pointerId;
    dragStartClientX = event.clientX;
    dragStartWidth = resolvedRightDockWidth.value;
    isRightDockResizing.value = true;
    const target = event.currentTarget as HTMLElement | null;
    target?.setPointerCapture?.(event.pointerId);
    event.preventDefault();
  };

  onMounted(() => {
    if (typeof window !== 'undefined') {
      const storedWidth = Number.parseInt(String(window.localStorage.getItem(options.storageKey) || ''), 10);
      if (Number.isFinite(storedWidth) && storedWidth > 0) {
        rightDockWidth.value = clampRightDockWidth(storedWidth);
      }
      window.addEventListener('pointermove', handleRightDockResizePointerMove);
      window.addEventListener('pointerup', handleRightDockResizePointerUp);
      window.addEventListener('pointercancel', handleRightDockResizePointerUp);
    }
  });

  onBeforeUnmount(() => {
    if (typeof window !== 'undefined') {
      window.removeEventListener('pointermove', handleRightDockResizePointerMove);
      window.removeEventListener('pointerup', handleRightDockResizePointerUp);
      window.removeEventListener('pointercancel', handleRightDockResizePointerUp);
    }
    stopRightDockResize();
  });

  watch(
    () =>
      [
        options.hostWidth.value,
        options.isOverlay.value,
        options.isMiddlePaneOverlay.value,
        options.navigationPaneCollapsed.value
      ] as const,
    () => {
      const clamped = clampRightDockWidth(rightDockWidth.value);
      if (clamped !== rightDockWidth.value) {
        rightDockWidth.value = clamped;
      }
    }
  );

  watch(
    () => [options.collapsed.value, options.isOverlay.value] as const,
    ([collapsed, overlay]) => {
      if (collapsed || overlay) {
        stopRightDockResize();
      }
    }
  );

  return {
    isRightDockResizing,
    rightDockResizable,
    rightDockStyle,
    resetRightDockWidth,
    nudgeRightDockWidth,
    startRightDockResize
  };
}
