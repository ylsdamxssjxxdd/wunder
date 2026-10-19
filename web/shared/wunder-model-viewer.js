const MAX_MODEL_BYTES = 64 * 1024 * 1024;
const AUTO_ROTATE_SPEED = 0.08;
const COORDINATE_INTERVAL_MS = 120;
const DEFAULT_MODEL = {
  id: "wunder",
  url: "models/wunder.glb?v=20261018-11",
  view: [0.02, -4.88, 0.01],
};

/**
 * Mount the bridge's static Wunder model. The viewer owns one WebGL context for
 * its lifetime; changing panels pauses frames without throwing the decoded GLB
 * and GPU resources away. That keeps navigation immediate while producing no
 * background rendering work.
 */
export function mountWunderModelViewer(host, { assetBase = "/third/", labels, model: modelEntry } = {}) {
  const entry = modelEntry || DEFAULT_MODEL;
  const root = document.createElement("div");
  root.className = "wunder-model-viewer";
  root.dataset.model = entry.id || "wunder";
  root.innerHTML = `
    <div class="wunder-model-stage" tabindex="0" role="img"></div>
    <div class="wunder-model-status" role="status" aria-live="polite">
      <span></span><button type="button" hidden></button>
    </div>
    <div class="wunder-model-toolbar">
      <output class="wunder-model-coordinates" data-model-coordinates aria-live="polite"></output>
    </div>
    <div class="wunder-model-brand">AAA wunder 心舰</div>
  `;
  host.replaceChildren(root);

  const stage = root.querySelector(".wunder-model-stage");
  const status = root.querySelector(".wunder-model-status");
  const statusText = status.querySelector("span");
  const retryButton = status.querySelector("button");
  const coordinates = root.querySelector("[data-model-coordinates]");
  const assetUrl = (path) => new URL(path, new URL(assetBase, window.location.href)).href;

  let disposed = false;
  let active = true;
  let renderer = null;
  let controls = null;
  let scene = null;
  let camera = null;
  let model = null;
  let observer = null;
  let frame = 0;
  let rendering = false;
  let lastFrameAt = 0;
  let lastCoordinateAt = 0;
  let coordinateText = "";
  let attempt = 0;
  let loadAbort = null;
  let homePosition = null;

  const cancelRender = () => {
    if (frame) cancelAnimationFrame(frame);
    frame = 0;
  };
  const canRender = () => active && !disposed && !document.hidden && renderer && scene && camera;
  const updateCoordinates = (force = false) => {
    if (!coordinates || !camera) return;
    const now = performance.now();
    if (!force && now - lastCoordinateAt < COORDINATE_INTERVAL_MS) return;
    const position = camera.position;
    const nextText = `X ${position.x.toFixed(3)} · Y ${position.y.toFixed(3)} · Z ${position.z.toFixed(3)}`;
    lastCoordinateAt = now;
    if (nextText === coordinateText) return;
    coordinateText = nextText;
    coordinates.textContent = nextText;
  };
  const render = (time) => {
    frame = 0;
    if (!canRender()) return;
    // OrbitControls treats an omitted delta as a 60 Hz frame. Supplying the
    // real interval keeps the established 0.08 turntable speed identical on
    // 60, 120 and 144 Hz displays.
    const delta = lastFrameAt ? Math.min((time - lastFrameAt) / 1000, 0.05) : 1 / 60;
    lastFrameAt = time;
    rendering = true;
    controls?.update(delta);
    rendering = false;
    renderer.render(scene, camera);
    updateCoordinates();
    if (canRender() && !frame) frame = requestAnimationFrame(render);
  };
  const invalidate = () => {
    if (!rendering && canRender() && !frame) frame = requestAnimationFrame(render);
  };
  const setStatus = (key, canRetry = false) => {
    status.hidden = false;
    status.dataset.state = key;
    statusText.textContent = labels[key] || "";
    retryButton.hidden = !canRetry;
  };
  const updateLabels = (next) => {
    labels = next || labels;
    stage.setAttribute("aria-label", labels.modelLabel || "");
    retryButton.textContent = labels.retry || "";
    if (!status.hidden) statusText.textContent = labels[status.dataset.state || "loading"] || "";
  };
  const disposeMaterial = (material) => {
    for (const value of Object.values(material)) {
      if (!value?.isTexture) continue;
      value.dispose();
      value.source?.data?.close?.();
    }
    material.dispose();
  };
  const disposeModel = (object) => {
    const geometries = new Set();
    const materials = new Set();
    object?.traverse((node) => {
      if (node.geometry) geometries.add(node.geometry);
      for (const material of Array.isArray(node.material) ? node.material : [node.material]) {
        if (material) materials.add(material);
      }
    });
    for (const geometry of geometries) geometry.dispose();
    for (const material of materials) disposeMaterial(material);
  };
  const releaseScene = () => {
    cancelRender();
    observer?.disconnect();
    observer = null;
    controls?.dispose();
    controls = null;
    disposeModel(model);
    model = null;
    homePosition = null;
    renderer?.dispose();
    renderer?.forceContextLoss();
    renderer = null;
    scene = null;
    camera = null;
    stage.replaceChildren();
  };
  const reset = () => {
    if (!camera || !controls || !homePosition) return;
    controls.autoRotate = false;
    controls.target.set(0, 0, 0);
    camera.position.copy(homePosition);
    controls.update();
    controls.autoRotate = true;
    updateCoordinates(true);
    invalidate();
  };
  const resize = () => {
    if (!renderer || !camera) return;
    const { width, height } = stage.getBoundingClientRect();
    if (width < 1 || height < 1) return;
    renderer.setSize(width, height, false);
    camera.aspect = width / height;
    camera.updateProjectionMatrix();
    invalidate();
  };
  const load = async () => {
    const currentAttempt = ++attempt;
    loadAbort?.abort();
    releaseScene();
    root.dataset.state = "loading";
    setStatus("loading");
    const controller = new AbortController();
    loadAbort = controller;
    const timeout = setTimeout(() => controller.abort(), 60000);
    try {
      const [THREE, { OrbitControls }, { GLTFLoader }] = await Promise.all([
        import(/* @vite-ignore */ assetUrl("three/three.module.min.js")),
        import(/* @vite-ignore */ assetUrl("three/OrbitControls.js")),
        import(/* @vite-ignore */ assetUrl("three/GLTFLoader.js")),
      ]);
      if (disposed || currentAttempt !== attempt) return;

      const nextRenderer = new THREE.WebGLRenderer({ antialias: true, alpha: true, powerPreference: "low-power" });
      // The bridge is a dense operations surface; one device pixel per CSS
      // pixel keeps the moving model from competing with navigation rendering.
      nextRenderer.setPixelRatio(1);
      nextRenderer.outputColorSpace = THREE.SRGBColorSpace;
      nextRenderer.toneMapping = THREE.ACESFilmicToneMapping;
      nextRenderer.toneMappingExposure = entry.exposure ?? 0.9;
      renderer = nextRenderer;
      stage.append(nextRenderer.domElement);
      scene = new THREE.Scene();
      camera = new THREE.PerspectiveCamera(36, 1, 0.01, 100);
      controls = new OrbitControls(camera, nextRenderer.domElement);
      controls.enableDamping = false;
      controls.enablePan = true;
      controls.autoRotate = true;
      controls.autoRotateSpeed = AUTO_ROTATE_SPEED;
      controls.addEventListener("change", invalidate);
      scene.add(new THREE.HemisphereLight(0xe5efff, 0x858b95, 2.1));
      const keyLight = new THREE.DirectionalLight(0xffffff, 2.7);
      keyLight.position.set(3, 4, 5);
      scene.add(keyLight);
      const rimLight = new THREE.DirectionalLight(0xa6c9ff, 1.6);
      rimLight.position.set(-4, 2, -3);
      scene.add(rimLight);
      observer = new ResizeObserver(resize);
      observer.observe(stage);
      nextRenderer.domElement.addEventListener("webglcontextlost", (event) => {
        event.preventDefault();
        if (disposed || event.target !== renderer?.domElement) return;
        cancelRender();
        root.dataset.state = "error";
        setStatus("unavailable", true);
      });
      resize();

      const response = await fetch(assetUrl(entry.url), { signal: controller.signal });
      if (!response.ok) throw new Error("model-request-failed");
      if (Number(response.headers.get("content-length")) > MAX_MODEL_BYTES) throw new Error("model-too-large");
      const bytes = await response.arrayBuffer();
      if (bytes.byteLength > MAX_MODEL_BYTES) throw new Error("model-too-large");
      if (disposed || currentAttempt !== attempt) return;
      const gltf = await new GLTFLoader().parseAsync(bytes, assetUrl("models/"));
      if (disposed || currentAttempt !== attempt) {
        disposeModel(gltf.scene);
        return;
      }
      const sourceBounds = new THREE.Box3().setFromObject(gltf.scene, true);
      const sourceSize = sourceBounds.getSize(new THREE.Vector3());
      const extent = Math.max(sourceSize.x, sourceSize.y, sourceSize.z);
      if (!Number.isFinite(extent) || extent <= 0) throw new Error("empty-model");
      gltf.scene.position.sub(sourceBounds.getCenter(new THREE.Vector3()));
      model = new THREE.Group();
      model.scale.setScalar(4 / extent);
      model.add(gltf.scene);
      scene.add(model);
      const bounds = new THREE.Box3().setFromObject(model, true);
      const sphere = bounds.getBoundingSphere(new THREE.Sphere());
      if (!Number.isFinite(sphere.radius) || sphere.radius <= 0) throw new Error("empty-model");
      homePosition = new THREE.Vector3().fromArray(entry.view || DEFAULT_MODEL.view);
      camera.position.copy(homePosition);
      controls.target.set(0, 0, 0);
      controls.minDistance = Math.max(sphere.radius * 0.5, 0.01);
      controls.maxDistance = Math.max(homePosition.length() * 4, controls.minDistance * 2);
      reset();
      status.hidden = true;
      root.dataset.state = "ready";
      resize();
      invalidate();
    } catch {
      if (disposed || currentAttempt !== attempt) return;
      const key = renderer ? "loadFailed" : "unavailable";
      releaseScene();
      root.dataset.state = "error";
      setStatus(key, true);
    } finally {
      clearTimeout(timeout);
    }
  };
  const setActive = (nextActive) => {
    const next = Boolean(nextActive);
    if (active === next || disposed) return;
    active = next;
    root.dataset.paused = next ? "false" : "true";
    if (!active) {
      controls && (controls.enabled = false);
      cancelRender();
      lastFrameAt = 0;
      return;
    }
    if (controls) controls.enabled = true;
    resize();
    invalidate();
  };
  const onVisibility = () => {
    if (document.hidden) {
      cancelRender();
      lastFrameAt = 0;
    }
    else invalidate();
  };
  const onKeyDown = (event) => {
    if (event.key.toLowerCase() !== "r" || !active) return;
    event.preventDefault();
    reset();
  };

  retryButton.addEventListener("click", load);
  stage.addEventListener("keydown", onKeyDown);
  document.addEventListener("visibilitychange", onVisibility);
  updateLabels(labels);
  void load();

  return {
    updateLabels,
    setActive,
    get model() { return entry.id || "wunder"; },
    get state() { return root.dataset.state || ""; },
    dispose() {
      if (disposed) return;
      disposed = true;
      ++attempt;
      loadAbort?.abort();
      document.removeEventListener("visibilitychange", onVisibility);
      cancelRender();
      releaseScene();
      root.remove();
    },
  };
}
