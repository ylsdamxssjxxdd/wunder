import { t } from "./i18n.js?v=20261015-06";
import { mountWunderModelViewer } from "../shared/wunder-model-viewer.js?v=20261018-07";

let viewer = null;

const labels = () => Object.fromEntries(
  ["modelLabel", "loading", "loadFailed", "unavailable", "retry", "reset", "rotate", "pause"]
    .map((key) => [key, t(`wunderModel.${key}`)])
);

export function toggleWunderModelPanel(active) {
  if (!active) {
    viewer?.dispose();
    viewer = null;
    return;
  }
  const host = document.getElementById("wunderModelMount");
  if (host && !viewer) {
    viewer = mountWunderModelViewer(host, {
      assetBase: new URL("../third/", import.meta.url).href,
      labels: labels(),
    });
  }
}

export function initWunderModelPanel() {
  window.addEventListener("wunder:language-changed", () => viewer?.updateLabels(labels()));
  window.addEventListener("pagehide", () => toggleWunderModelPanel(false));
}
