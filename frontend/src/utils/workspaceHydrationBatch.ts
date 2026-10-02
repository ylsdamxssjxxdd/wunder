/** Coalesce render notifications without losing later rows. Full scans dominate. */
export const createWorkspaceHydrationBatch = () => {
  const keys = new Set<string>();
  let fullScan = false;
  return {
    add(messageKeys?: string[]) {
      if (!messageKeys?.length) fullScan = true;
      if (fullScan) { keys.clear(); return; }
      for (const key of messageKeys) {
        if (key.trim()) keys.add(key.trim());
        // Bound batches under heavy virtualization; a full scan is DOM-bounded.
        if (keys.size > 128) { fullScan = true; keys.clear(); break; }
      }
    },
    take(): { messageKeys?: string[] } {
      const batch = fullScan ? {} : { messageKeys: [...keys] };
      keys.clear();
      fullScan = false;
      return batch;
    },
    clear() { keys.clear(); fullScan = false; }
  };
};
