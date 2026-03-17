import { nextTick, onBeforeUnmount, onMounted, ref, watch, type Ref } from 'vue';

type HostWidthState = {
  hostWidth: Ref<number>;
  hostRootRef: Ref<HTMLElement | null>;
  refreshHostWidth: () => void;
};

export function useMessengerHostWidth(fallbackWidth = 1440): HostWidthState {
  const hostWidth = ref(fallbackWidth);
  const hostRootRef = ref<HTMLElement | null>(null);

  let disposed = false;
  let resizeObserver: ResizeObserver | null = null;
  let resizeFrameId: number | null = null;

  function cancelScheduledMeasure() {
    if (resizeFrameId === null || typeof window === 'undefined') return;
    window.cancelAnimationFrame(resizeFrameId);
    resizeFrameId = null;
  }

  function resolveMeasuredWidth(): number {
    const element = hostRootRef.value;
    if (element) {
      const rectWidth = Math.round(element.getBoundingClientRect().width);
      if (Number.isFinite(rectWidth) && rectWidth > 0) {
        return rectWidth;
      }
      const clientWidth = Math.round(element.clientWidth || 0);
      if (clientWidth > 0) {
        return clientWidth;
      }
    }
    if (typeof window !== 'undefined') {
      const viewportWidth = Math.round(window.innerWidth || 0);
      if (viewportWidth > 0) {
        return viewportWidth;
      }
    }
    return fallbackWidth;
  }

  function measureHostWidth() {
    if (disposed) return;
    hostWidth.value = resolveMeasuredWidth();
  }

  function refreshHostWidth() {
    if (disposed) return;
    cancelScheduledMeasure();
    if (typeof window === 'undefined') {
      measureHostWidth();
      return;
    }
    resizeFrameId = window.requestAnimationFrame(() => {
      resizeFrameId = null;
      measureHostWidth();
    });
  }

  function attachResizeObserver(element: HTMLElement | null) {
    if (!resizeObserver || !element) return;
    resizeObserver.observe(element);
  }

  function detachResizeObserver(element: HTMLElement | null) {
    if (!resizeObserver || !element) return;
    resizeObserver.unobserve(element);
  }

  onMounted(() => {
    disposed = false;
    if (typeof ResizeObserver !== 'undefined') {
      // Use the rendered host width instead of window width so embedded layouts
      // can switch to compact mode even when the outer page viewport is large.
      resizeObserver = new ResizeObserver(() => {
        refreshHostWidth();
      });
      attachResizeObserver(hostRootRef.value);
    }
    nextTick(() => {
      refreshHostWidth();
    });
  });

  watch(hostRootRef, (current, previous) => {
    if (previous) {
      detachResizeObserver(previous);
    }
    if (current) {
      attachResizeObserver(current);
      refreshHostWidth();
    }
  });

  onBeforeUnmount(() => {
    disposed = true;
    cancelScheduledMeasure();
    if (resizeObserver) {
      resizeObserver.disconnect();
      resizeObserver = null;
    }
  });

  return {
    hostWidth,
    hostRootRef,
    refreshHostWidth
  };
}
