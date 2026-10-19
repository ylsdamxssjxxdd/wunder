import { t } from "./i18n.js?v=20261015-06";
import { mountWunderModelViewer } from "../shared/wunder-model-viewer.js?v=20261019-35";

const WUNDER_MODEL = {
  id: "wunder",
  url: "models/wunder.glb?v=20261018-11",
  view: [0.02, -4.88, 0.01],
};

let viewer = null;
let mountTimer = 0;

const labels = () => Object.fromEntries(
  ["modelLabel", "loading", "loadFailed", "unavailable", "retry"]
    .map((key) => [key, t(`wunderModel.${key}`)])
);

export function toggleWunderModelPanel(active) {
  if (!active) {
    if (mountTimer) {
      window.clearTimeout(mountTimer);
      mountTimer = 0;
    }
    if (viewer?.state === "loading") {
      viewer.dispose();
      viewer = null;
      return;
    }
    viewer?.setActive(false);
    return;
  }
  const host = document.getElementById("wunderModelMount");
  if (!host) return;
  if (viewer) {
    viewer.setActive(true);
    return;
  }
  // Let the bridge finish its first layout and input binding before decoding a
  // 3D asset. A user who immediately opens an administration page cancels this
  // work entirely, keeping the first sidebar interaction responsive.
  if (mountTimer) return;
  mountTimer = window.setTimeout(() => {
    mountTimer = 0;
    if (viewer || !document.getElementById("wunderModelMount")) return;
    viewer = mountWunderModelViewer(host, {
      assetBase: new URL("../third/", import.meta.url).href,
      labels: labels(),
      model: WUNDER_MODEL,
    });
  }, 800);
}

export function initWunderModelPanel() {
  window.addEventListener("wunder:language-changed", () => viewer?.updateLabels(labels()));
  window.addEventListener("pagehide", () => toggleWunderModelPanel(false));
}
