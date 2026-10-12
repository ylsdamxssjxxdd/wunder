// W2-F: thread snapshot parse/normalization runs off the main thread. The
// worker only flattens embedded item rows and splits the payload into bounded,
// serializable shard plans; applying those shards to the durable state stays on
// the main thread so the reducer keeps its single source of truth.
import {
  planChatThreadSnapshot,
  type ChatThreadSnapshot,
  type ChatThreadSnapshotPlanOptions
} from '@/realtime/chat/chatThreadState';

self.onmessage = (event: MessageEvent) => {
  const data = event?.data as
    | { id?: number; snapshot?: ChatThreadSnapshot; options?: ChatThreadSnapshotPlanOptions }
    | undefined;
  const id = data?.id;
  try {
    const plan = planChatThreadSnapshot(data?.snapshot, data?.options ?? {});
    self.postMessage({ id, plan });
  } catch {
    self.postMessage({ id, error: true });
  }
};
