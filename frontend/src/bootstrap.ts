// Keep the entry free of application imports: even static imports execute before
// an awaited frame, pulling Vue, styles and route setup into the paint path.
const startApplication = async () => {
  const desktop = (window as Window & {
    wunderDesktop?: { reportRendererStage?: (stage: string, payload: object) => Promise<unknown> };
  }).wunderDesktop;
  if (desktop || window.__WUNDER_DESKTOP_RUNTIME__ || __WUNDER_DESKTOP_BUILD__) {
    document.documentElement.setAttribute('data-wunder-desktop-starting', '');
    // Paint timing can lag animation frames. Prefer the actual contentful paint
    // record, then start the quiet interval; hidden documents wait until visible.
    await new Promise<void>((resolve) => {
      if (typeof PerformanceObserver !== 'undefined' &&
          PerformanceObserver.supportedEntryTypes?.includes('paint')) {
        const observer = new PerformanceObserver((entries) => {
          if (entries.getEntries().some((entry) => entry.name === 'first-contentful-paint')) {
            observer.disconnect();
            resolve();
          }
        });
        observer.observe({ type: 'paint', buffered: true });
        return;
      }
      requestAnimationFrame(() => requestAnimationFrame(() => resolve()));
    });
    const deadline = performance.now() + 10;
    await new Promise<void>((resolve) => {
      const afterPaint = () => {
        const remaining = deadline - performance.now();
        if (remaining > 0) {
          window.setTimeout(afterPaint, Math.ceil(remaining));
        } else {
          resolve();
        }
      };
      window.setTimeout(afterPaint, 10);
    });
    performance.mark('desktop-post-first-frame');
    void desktop?.reportRendererStage?.('frontend-post-first-frame', { delay_ms: 10 })?.catch(() => {});
  }
  await import('./main');
};

void startApplication().catch((error: unknown) => {
  console.error('[desktop-startup] Application loading failed', error);
  const root = document.getElementById('wunder-desktop-startup-shell');
  if (!root) return;
  // Chunk/network failures occur before main.ts can install its error handlers.
  document.documentElement.setAttribute('data-wunder-desktop-starting', '');
  root.style.pointerEvents = 'auto';
  const hint = root.querySelector('.wunder-startup-shell__hint');
  if (hint) hint.textContent = 'Unable to load the application. Please reload.';
  root.querySelector('.wunder-startup-shell__spinner')?.remove();
  const retry = document.createElement('button');
  retry.type = 'button';
  retry.textContent = 'Reload';
  retry.addEventListener('click', () => window.location.reload(), { once: true });
  root.querySelector('.wunder-startup-shell__content')?.append(retry);
});
