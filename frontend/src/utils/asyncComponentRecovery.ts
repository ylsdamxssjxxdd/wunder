import { defineAsyncComponent } from 'vue';

const ASYNC_COMPONENT_RELOAD_KEY = '__wunder_async_component_reload__';

const normalizeErrorMessage = (error: unknown) => String((error as { message?: unknown })?.message || error || '').trim();

const isDynamicImportFetchError = (error: unknown) => {
  const message = normalizeErrorMessage(error).toLowerCase();
  if (!message) return false;
  return (
    message.includes('failed to fetch dynamically imported module') ||
    message.includes('importing a module script failed') ||
    message.includes('chunkloaderror') ||
    message.includes('loading chunk') ||
    message.includes('unable to preload css')
  );
};

const reloadOnceForAsyncComponentFailure = () => {
  if (typeof window === 'undefined') {
    return;
  }
  try {
    if (window.sessionStorage.getItem(ASYNC_COMPONENT_RELOAD_KEY) === '1') {
      return;
    }
    window.sessionStorage.setItem(ASYNC_COMPONENT_RELOAD_KEY, '1');
  } catch {
    return;
  }
  window.location.reload();
};

export const clearAsyncComponentReloadMarker = () => {
  if (typeof window === 'undefined') {
    return;
  }
  try {
    window.sessionStorage.removeItem(ASYNC_COMPONENT_RELOAD_KEY);
  } catch {
    // ignore storage failures
  }
};

export const defineRecoverableAsyncComponent = <T extends object>(loader: () => Promise<T>) =>
  defineAsyncComponent({
    loader,
    suspensible: false,
    onError(error, retry, fail, attempts) {
      if (!isDynamicImportFetchError(error)) {
        fail();
        return;
      }
      if (attempts <= 1) {
        retry();
        return;
      }
      reloadOnceForAsyncComponentFailure();
      fail();
    }
  });
