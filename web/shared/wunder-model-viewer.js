// Shared, on-demand GLB viewer used by the bridge and hive.
const MAX_MODEL_BYTES = 64 * 1024 * 1024;

export function mountWunderModelViewer(host, { assetBase = "/third/", labels }) {
  const root = document.createElement("div");
  root.className = "wunder-model-viewer";
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
  let disposed = false;
  let renderer, controls, scene, camera, model;
  let observer;
  let frame = 0;
  let loadAbort;
  let rendering = false;
  let homePosition;
  let fitCamera;
  let coordinates;
  let attempt = 0;

  const assetUrl = (path) => new URL(path, new URL(assetBase, window.location.href)).href;
  const render = (time = 0) => {
    frame = 0;
    if (disposed || !renderer || document.hidden) return;
    rendering = true;
    controls?.update();
    rendering = false;
    frame = requestAnimationFrame(render);
    updateCoordinates();
    renderer.render(scene, camera);
  };
  const invalidate = () => {
    if (!rendering && !frame && !disposed && renderer && !document.hidden) frame = requestAnimationFrame(render);
  };
  const disposeModel = (object) => {
    const geometries = new Set();
    const materials = new Set();
    const textures = new Set();
    object?.traverse((node) => {
      if (node.geometry) geometries.add(node.geometry);
      for (const material of Array.isArray(node.material) ? node.material : [node.material]) {
        if (!material) continue;
        materials.add(material);
        for (const value of Object.values(material)) if (value?.isTexture) textures.add(value);
      }
    });
    for (const geometry of geometries) geometry.dispose();
    for (const material of materials) material.dispose();
    for (const texture of textures) {
      texture.dispose();
      texture.source?.data?.close?.();
    }
  };
  const releaseScene = () => {
    cancelAnimationFrame(frame);
    frame = 0;
    observer?.disconnect();
    observer = null;
    controls?.dispose();
    controls = null;
    disposeModel(model);
    model = null;
    homePosition = null;
    fitCamera = null;
    renderer?.dispose();
    renderer?.forceContextLoss();
    renderer = null;
    scene = null;
    camera = null;
    stage.replaceChildren();
  };
  const reset = () => {
    if (!camera || !controls || !homePosition) return;
    // OrbitControls advances its azimuth when update() runs with autoRotate.
    // Keep the reset frame at the exact requested underside coordinate, then
    // resume automatic rotation on the next animation frame.
    controls.autoRotate = false;
    controls.target.set(0, 0, 0);
    camera.position.copy(homePosition);
    controls.update();
    controls.autoRotate = true;
    invalidate();
  };
  const setStatus = (key, canRetry = false) => {
    status.hidden = false;
    statusText.textContent = labels[key];
    retryButton.hidden = !canRetry;
  };
  const updateLabels = (next) => {
    labels = next;
    stage.setAttribute("aria-label", labels.modelLabel);
    retryButton.textContent = labels.retry;
    if (!status.hidden) statusText.textContent = labels[status.dataset.state || "loading"];
  };
  const updateCoordinates = () => {
    if (!coordinates || !camera) return;
    const position = camera.position;
    coordinates.textContent = `X ${position.x.toFixed(3)} · Y ${position.y.toFixed(3)} · Z ${position.z.toFixed(3)}`;
  };
  coordinates = root.querySelector("[data-model-coordinates]");
  updateLabels(labels);

  const load = async () => {
    const currentAttempt = ++attempt;
    root.dataset.state = "loading";
    loadAbort?.abort();
    releaseScene();
    updateLabels(labels);
    status.dataset.state = "loading";
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
      renderer = new THREE.WebGLRenderer({ antialias: true, alpha: true, powerPreference: "low-power" });
      renderer.setPixelRatio(Math.min(window.devicePixelRatio || 1, 1.5));
      renderer.outputColorSpace = THREE.SRGBColorSpace;
      renderer.toneMapping = THREE.ACESFilmicToneMapping;
      renderer.toneMappingExposure = 0.9;
      stage.append(renderer.domElement);
      scene = new THREE.Scene();
      camera = new THREE.PerspectiveCamera(36, 1, 0.01, 100);
      controls = new OrbitControls(camera, renderer.domElement);
      controls.enableDamping = false;
      controls.enablePan = true;
      controls.autoRotate = true;
      // OrbitControls speed 2 is one full orbit in about 30 seconds. The
      // bridge view is intentionally slow and instrument-like: about 12.5
      // minutes per orbit at 60 FPS.
      controls.autoRotateSpeed = 0.08;
      controls.addEventListener("change", invalidate);
      controls.addEventListener("change", updateCoordinates);
      // Lift the underside-facing surfaces so the reference palette remains
      // visible from the requested bottom view instead of collapsing to navy.
      scene.add(new THREE.HemisphereLight(0xe5efff, 0x858b95, 2.1));
      const keyLight = new THREE.DirectionalLight(0xffffff, 2.7);
      keyLight.position.set(3, 4, 5);
      scene.add(keyLight);
      const rimLight = new THREE.DirectionalLight(0xa6c9ff, 1.6);
      rimLight.position.set(-4, 2, -3);
      scene.add(rimLight);
      const resize = () => {
        if (!renderer) return;
        const { width, height } = stage.getBoundingClientRect();
        if (width < 1 || height < 1) return;
        renderer.setSize(width, height);
        camera.aspect = width / height;
        camera.updateProjectionMatrix();
        fitCamera?.();
        invalidate();
      };
      resize();
      observer = new ResizeObserver(resize);
      observer.observe(stage);
      renderer.domElement.addEventListener("webglcontextlost", (event) => {
        event.preventDefault();
        if (disposed || event.target !== renderer?.domElement) return;
        cancelAnimationFrame(frame);
        frame = 0;
        status.dataset.state = "unavailable";
        setStatus("unavailable", true);
      });
      const response = await fetch(assetUrl("models/wunder.glb?v=20261018-11"), { signal: controller.signal });
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
      model = new THREE.Group();
      model.add(gltf.scene);
      // Precise bounds avoid inflated boxes from the source's rotated submeshes.
      const bounds = new THREE.Box3().setFromObject(model, true);
      const size = bounds.getSize(new THREE.Vector3());
      const extent = Math.max(size.x, size.y, size.z);
      if (!Number.isFinite(extent) || extent <= 0) throw new Error("empty-model");
      const center = bounds.getCenter(new THREE.Vector3());
      gltf.scene.position.sub(center);
      model.scale.setScalar(4 / extent);
      scene.add(model);
      const centeredBounds = new THREE.Box3().setFromObject(model, true);
      const sphere = centeredBounds.getBoundingSphere(new THREE.Sphere());
      // Start from the reference underside view: the keel runs from lower-left
      // toward the bridge at upper-right, matching the requested entrance view.
      const referencePosition = new THREE.Vector3(0.02, -4.88, 0.01);
      const direction = referencePosition.clone().normalize();
      camera.position.copy(direction);
      camera.lookAt(0, 0, 0);
      const viewRight = new THREE.Vector3(1, 0, 0).applyQuaternion(camera.quaternion);
      const viewUp = new THREE.Vector3(0, 1, 0).applyQuaternion(camera.quaternion);
      const verticalTan = Math.tan(camera.fov * Math.PI / 360);
      // A bounded radial profile fits the entire turn without clipping the wings.
      // Traverse vertices only once; resizing uses at most 64 profile slices.
      const profile = Array.from({ length: 64 }, () => ({ radius: 0, minY: Infinity, maxY: -Infinity }));
      const height = Math.max(centeredBounds.max.y - centeredBounds.min.y, 0.001);
      const vertex = new THREE.Vector3();
      model.traverse((node) => {
        const positions = node.geometry?.attributes.position;
        if (!positions || !node.isMesh) return;
        for (let index = 0; index < positions.count; index++) {
          vertex.fromBufferAttribute(positions, index).applyMatrix4(node.matrixWorld);
          const slice = profile[Math.min(63, Math.max(0, Math.floor((vertex.y - centeredBounds.min.y) / height * 64)))];
          slice.radius = Math.max(slice.radius, Math.hypot(vertex.x, vertex.z));
          slice.minY = Math.min(slice.minY, vertex.y);
          slice.maxY = Math.max(slice.maxY, vertex.y);
        }
      });
      fitCamera = () => {
        const previousDistance = homePosition?.length();
        const planes = [
          direction.clone().addScaledVector(viewRight, 1 / (verticalTan * camera.aspect)),
          direction.clone().addScaledVector(viewRight, -1 / (verticalTan * camera.aspect)),
          direction.clone().addScaledVector(viewUp, 1 / verticalTan),
          direction.clone().addScaledVector(viewUp, -1 / verticalTan),
        ];
        let distance = 0;
        for (const plane of planes) {
          const radialFactor = Math.hypot(plane.x, plane.z);
          for (const slice of profile) {
            if (!Number.isFinite(slice.minY)) continue;
            distance = Math.max(distance, radialFactor * slice.radius + plane.y * (plane.y > 0 ? slice.maxY : slice.minY));
          }
        }
        // Keep the requested underside reference distance so the first frame
        // opens at the known bottom coordinate (X 0.02, Y -4.88, Z 0.01).
        homePosition = referencePosition.clone();
        controls.maxDistance = homePosition.length() * 4;
        if (previousDistance) {
          camera.position.sub(controls.target).multiplyScalar(homePosition.length() / previousDistance).add(controls.target);
          controls.update();
        }
      };
      fitCamera();
      controls.minDistance = sphere.radius * 0.5;
      reset();
      updateCoordinates();
      status.hidden = true;
      root.dataset.state = "ready";
      invalidate();
    } catch {
      if (disposed || currentAttempt !== attempt) return;
      const key = renderer ? "loadFailed" : "unavailable";
      releaseScene();
      status.dataset.state = key;
      root.dataset.state = "error";
      setStatus(key, true);
    } finally {
      clearTimeout(timeout);
    }
  };
  const onVisibility = () => {
    cancelAnimationFrame(frame);
    frame = 0;
    if (!document.hidden) invalidate();
  };
  const onKeyDown = (event) => {
    if (event.key.toLowerCase() === "r") {
      event.preventDefault();
      reset();
    }
  };
  retryButton.addEventListener("click", load);
  stage.addEventListener("keydown", onKeyDown);
  document.addEventListener("visibilitychange", onVisibility);
  void load();
  return {
    updateLabels,
    dispose() {
      if (disposed) return;
      disposed = true;
      ++attempt;
      loadAbort?.abort();
      document.removeEventListener("visibilitychange", onVisibility);
      releaseScene();
      root.remove();
    },
  };
}
