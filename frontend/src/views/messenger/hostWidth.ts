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
  let observedHostElement: HTMLElement | null = null;
  let observedContainerElement: HTMLElement | null = null;

  function cancelScheduledMeasure() {
    if (resizeFrameId === null || typeof window === 'undefined') return;
    window.cancelAnimationFrame(resizeFrameId);
    resizeFrameId = null;
  }

  function resolveElementWidth(element: HTMLElement | null): number {
    if (!element) return 0;
    const rectWidth = Math.round(element.getBoundingClientRect().width);
    if (Number.isFinite(rectWidth) && rectWidth > 0) {
      return rectWidth;
    }
    const clientWidth = Math.round(element.clientWidth || 0);
    return clientWidth > 0 ? clientWidth : 0;
  }

  function resolveTopLevelShellWidth(hostElement: HTMLElement): number {
    let current = hostElement.parentElement;
    let resolved = 0;
    while (current) {
      if (
        current.id === 'app' ||
        current.classList.contains('app-shell') ||
        current.classList.contains('app-shell-content')
      ) {
        resolved = Math.max(resolved, resolveElementWidth(current));
      }
      current = current.parentElement;
    }
    return resolved;
  }

  function resolveMeasuredWidth(): number {
    const hostElement = hostRootRef.value;
    if (hostElement) {
      const hostElementWidth = resolveElementWidth(hostElement);
      const containerWidth = resolveElementWidth(hostElement.parentElement);
      const topLevelShellWidth = resolveTopLevelShellWidth(hostElement);

      // Use the narrowest resolved width across Messenger and its host containers.
      // This keeps layout breakpoints aligned with the real rendered space when
      // outer shells and inner panes do not resize in lockstep.
      const constrainedWidths = [hostElementWidth, containerWidth, topLevelShellWidth].filter(
        (width) => Number.isFinite(width) && width > 0
      );
      if (constrainedWidths.length > 0) {
        return Math.min(...constrainedWidths);
      }
    }
    if (typeof document !== 'undefined') {
      const documentWidth = Math.round(document.documentElement?.clientWidth || 0);
      if (documentWidth > 0) {
        return documentWidth;
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

  function syncResizeObserverTargets() {
    if (!resizeObserver) return;
    const hostElement = hostRootRef.value;
    const containerElement = hostElement?.parentElement ?? null;

    if (observedHostElement && observedHostElement !== hostElement) {
      resizeObserver.unobserve(observedHostElement);
    }
    if (
      observedContainerElement &&
      observedContainerElement !== containerElement &&
      observedContainerElement !== hostElement
    ) {
      resizeObserver.unobserve(observedContainerElement);
    }

    if (hostElement && observedHostElement !== hostElement) {
      resizeObserver.observe(hostElement);
    }
    if (
      containerElement &&
      containerElement !== hostElement &&
      observedContainerElement !== containerElement
    ) {
      resizeObserver.observe(containerElement);
    }

    observedHostElement = hostElement;
    observedContainerElement = containerElement;
  }

  onMounted(() => {
    disposed = false;
    if (typeof ResizeObserver !== 'undefined') {
      // Use the rendered host width instead of window width so embedded layouts
      // can switch to compact mode even when the outer page viewport is large.
      resizeObserver = new ResizeObserver(() => {
        refreshHostWidth();
      });
      syncResizeObserverTargets();
    }
    nextTick(() => {
      refreshHostWidth();
    });
  });

  watch(hostRootRef, () => {
    syncResizeObserverTargets();
    refreshHostWidth();
  });

  onBeforeUnmount(() => {
    disposed = true;
    cancelScheduledMeasure();
    if (resizeObserver) {
      resizeObserver.disconnect();
      resizeObserver = null;
    }
    observedHostElement = null;
    observedContainerElement = null;
  });

  return {
    hostWidth,
    hostRootRef,
    refreshHostWidth
  };
}
