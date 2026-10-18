export type WunderModelLabels = {
  modelLabel: string;
  loading: string;
  loadFailed: string;
  unavailable: string;
  retry: string;
  reset: string;
  rotate: string;
  pause: string;
};

export function mountWunderModelViewer(
  host: HTMLElement,
  options: { assetBase?: string; labels: WunderModelLabels }
): { updateLabels(labels: WunderModelLabels): void; dispose(): void };
