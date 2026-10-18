/**
 * Bounded lazy read of a projection body.
 *
 * Thread-view rows carry a ~2 KB preview plus a `BodyRef`. Reading the durable
 * body upfront would blow the page budget, so full text arrives on demand.
 * The read is shared, keyed and bounded: one flight per (body, revision), a
 * small pool of concurrent readers, a head byte cap, and cancellation grouped
 * by the session that requested it.
 */
export interface ThreadBodyHydrationRequest {
  sessionId: string;
  bodyId: string;
  revision: number;
  /** Identity of this (row, role, body) read; duplicates never enter the queue. */
  token: string;
}

const MAX_CONCURRENT = 3;
const MAX_QUEUED = 64;
const MAX_BODY_BYTES = 2 * 1024 * 1024;

interface Queued {
  request: ThreadBodyHydrationRequest;
  settle: (content: string | null) => void;
}

const queue: Queued[] = [];
const tokens = new Set<string>();
let active = 0;

function drain(): void {
  while (active < MAX_CONCURRENT && queue.length > 0) {
    const item = queue.shift()!;
    tokens.delete(item.request.token);
    active += 1;
    void read(item.request)
      .then(item.settle, () => item.settle(null))
      .catch(() => item.settle(null))
      .finally(() => {
        active -= 1;
        drain();
      });
  }
}

/** Paged read of one body. `null` means "not available" and must be ignored. */
export function enqueueThreadBodyHydration(
  request: ThreadBodyHydrationRequest
): Promise<string | null> {
  if (tokens.has(request.token)) return Promise.resolve(null);
  // Newer content wins: drop the oldest waiting read rather than the newcomer.
  while (queue.length >= MAX_QUEUED) {
    const dropped = queue.shift();
    if (dropped) dropped.settle(null);
  }
  return new Promise<string | null>((resolve) => {
    tokens.add(request.token);
    queue.push({ request, settle: resolve });
    drain();
  });
}

/** Drop every queued read for a session; the batch leaves as soon as it drains. */
export function cancelThreadBodyHydration(sessionId: string): void {
  let index = queue.findIndex((item) => item.request.sessionId === sessionId);
  while (index !== -1) {
    const [item] = queue.splice(index, 1);
    tokens.delete(item.request.token);
    item.settle(null);
    index = queue.findIndex((item) => item.request.sessionId === sessionId);
  }
}

export function resetThreadBodyHydration(): void {
  const waiting = queue.splice(0, queue.length);
  tokens.clear();
  for (const item of waiting) item.settle(null);
}

async function read(request: ThreadBodyHydrationRequest): Promise<string | null> {
  // Loaded on demand: a static import would drag the HTTP/i18n graph into every
  // consumer of the thread-view store, including its headless regression tests.
  const { getThreadViewBody } = await import('@/api/chat');
  const parts: string[] = [];
  let bytes = 0;
  let cursor = 0;
  for (;;) {
    const response = await getThreadViewBody(
      request.sessionId,
      request.bodyId,
      { revision: request.revision, cursor },
      { timeout: 15000 }
    );
    const page = response?.data?.data;
    if (!page || !Array.isArray(page.chunks)) return null;
    for (const chunk of page.chunks) {
      if (typeof chunk !== 'string') return null;
      bytes += chunk.length;
      if (bytes > MAX_BODY_BYTES) return null;
      parts.push(chunk);
    }
    if (!page.has_more) break;
    const next = Number(page.next_cursor);
    if (!Number.isSafeInteger(next) || next <= cursor) return null;
    cursor = next;
  }
  return parts.join('');
}
