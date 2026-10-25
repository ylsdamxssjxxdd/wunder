<template>
  <div ref="hostRef" class="model-preview">
    <div v-if="status === 'loading'" class="model-preview-state">
      <span class="model-preview-spinner" aria-hidden="true"></span>
      <span>{{ progressText }}</span>
    </div>
    <div v-else-if="status === 'error'" class="model-preview-state is-error">
      <i class="fa-regular fa-file" aria-hidden="true"></i>
      <span>{{ errorText }}</span>
    </div>

    <div v-if="status === 'ready'" class="model-preview-toolbar">
      <span class="model-preview-tip">{{ t('workspace.preview.model.hint') }}</span>
      <div class="model-preview-actions">
        <button
          type="button"
          class="model-preview-btn"
          :title="t('workspace.preview.model.reset')"
          :aria-label="t('workspace.preview.model.reset')"
          @click="resetView"
        >
          <i class="fa-solid fa-house" aria-hidden="true"></i>
        </button>
        <button
          type="button"
          class="model-preview-btn"
          :class="{ 'is-active': autoRotate }"
          :title="t('workspace.preview.model.autoRotate')"
          :aria-label="t('workspace.preview.model.autoRotate')"
          @click="toggleAutoRotate"
        >
          <i class="fa-solid fa-rotate" aria-hidden="true"></i>
        </button>
        <button
          v-if="hasAnimation"
          type="button"
          class="model-preview-btn"
          :class="{ 'is-active': animationPlaying }"
          :title="t(animationPlaying ? 'workspace.preview.model.animationPause' : 'workspace.preview.model.animationPlay')"
          :aria-label="t(animationPlaying ? 'workspace.preview.model.animationPause' : 'workspace.preview.model.animationPlay')"
          @click="toggleAnimation"
        >
          <i :class="animationPlaying ? 'fa-solid fa-pause' : 'fa-solid fa-play'" aria-hidden="true"></i>
        </button>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue';

import { useI18n } from '@/i18n';

const props = withDefaults(
  defineProps<{
    src: string;
    active?: boolean;
  }>(),
  { active: true }
);

const { t } = useI18n();

const hostRef = ref<HTMLDivElement | null>(null);
const status = ref<'idle' | 'loading' | 'ready' | 'error'>('idle');
const errorText = ref('');
const progress = ref(0);
const hasAnimation = ref(false);
const animationPlaying = ref(false);
const autoRotate = ref(false);

const progressText = computed(() =>
  progress.value > 0
    ? `${t('workspace.preview.loading')} ${Math.round(progress.value * 100)}%`
    : t('workspace.preview.loading')
);

type ViewerHandle = {
  resetView: () => void;
  setAutoRotate: (value: boolean) => void;
  setAnimationPlaying: (value: boolean) => void;
  dispose: () => void;
};

let viewer: ViewerHandle | null = null;
let loadSerial = 0;

const teardown = () => {
  loadSerial += 1;
  const current = viewer;
  viewer = null;
  status.value = 'idle';
  errorText.value = '';
  progress.value = 0;
  hasAnimation.value = false;
  animationPlaying.value = false;
  autoRotate.value = false;
  current?.dispose();
};

const buildViewer = async (
  serial: number,
  src: string,
  host: HTMLElement
): Promise<void> => {
  const THREE = await import('three');
  const [{ GLTFLoader }, { OrbitControls }, { RoomEnvironment }, { DRACOLoader }] = await Promise.all([
    import('three/addons/loaders/GLTFLoader.js'),
    import('three/addons/controls/OrbitControls.js'),
    import('three/addons/environments/RoomEnvironment.js'),
    import('three/addons/loaders/DRACOLoader.js')
  ]);
  if (serial !== loadSerial) return;

  // A forced context loss cannot be undone on the same element, so each load
  // gets its own canvas; dialogs may keep this component mounted across opens.
  const canvas = document.createElement('canvas');
  canvas.className = 'model-preview-canvas';
  canvas.setAttribute('aria-hidden', 'true');
  host.prepend(canvas);

  const renderer = new THREE.WebGLRenderer({
    canvas,
    antialias: true,
    alpha: true,
    powerPreference: 'high-performance'
  });
  renderer.setPixelRatio(Math.min(window.devicePixelRatio || 1, 2));
  renderer.outputColorSpace = THREE.SRGBColorSpace;
  renderer.toneMapping = THREE.ACESFilmicToneMapping;

  const scene = new THREE.Scene();
  const disposeObject = (object: any) => {
    object.traverse((child: any) => {
      child.geometry?.dispose();
      const materials = Array.isArray(child.material) ? child.material : child.material ? [child.material] : [];
      materials.forEach((material: any) => {
        Object.values(material).forEach((value: any) => {
          if (value && value.isTexture) value.dispose();
        });
        material.dispose();
      });
    });
  };

  const pmrem = new THREE.PMREMGenerator(renderer);
  const roomScene = new RoomEnvironment(renderer);
  const environment = pmrem.fromScene(roomScene, 0.04).texture;
  disposeObject(roomScene);
  pmrem.dispose();
  scene.environment = environment;

  const camera = new THREE.PerspectiveCamera(42, 1, 0.01, 1000);
  const controls = new OrbitControls(camera, canvas);
  controls.enableDamping = true;
  controls.dampingFactor = 0.08;
  controls.rotateSpeed = 0.9;
  controls.zoomSpeed = 0.9;
  controls.panSpeed = 0.8;
  controls.autoRotateSpeed = 2.2;

  const dracoLoader = new DRACOLoader();
  dracoLoader.setDecoderPath(`${(import.meta.env.BASE_URL || '/').replace(/\/+$/, '/')}three/draco/`);
  const loader = new GLTFLoader();
  loader.setDRACOLoader(dracoLoader);

  let disposed = false;
  let frameHandle = 0;
  let resizeObserver: ResizeObserver | null = null;
  let root: any = null;
  let mixer: any = null;
  let defaultPosition: any = null;
  let defaultTarget: any = null;
  const clock = new THREE.Clock();

  const disposeResources = () => {
    if (disposed) return;
    disposed = true;
    cancelAnimationFrame(frameHandle);
    resizeObserver?.disconnect();
    controls.dispose();
    if (mixer) {
      mixer.stopAllAction();
      if (root) mixer.uncacheRoot(root);
    }
    if (root) {
      scene.remove(root);
      disposeObject(root);
    }
    scene.environment = null;
    environment.dispose();
    renderer.dispose();
    renderer.forceContextLoss();
    canvas.remove();
  };

  const renderFrame = () => {
    if (disposed) return;
    frameHandle = requestAnimationFrame(renderFrame);
    controls.update();
    const delta = clock.getDelta();
    if (mixer && animationPlaying.value) mixer.update(delta);
    renderer.render(scene, camera);
  };

  const resize = () => {
    if (disposed) return;
    const width = host.clientWidth;
    const height = host.clientHeight;
    if (!width || !height) return;
    renderer.setPixelRatio(Math.min(window.devicePixelRatio || 1, 2));
    renderer.setSize(width, height, false);
    camera.aspect = width / height;
    camera.updateProjectionMatrix();
  };

  const fitCamera = (object: any) => {
    const box = new THREE.Box3().setFromObject(object);
    if (box.isEmpty()) return;
    const size = box.getSize(new THREE.Vector3());
    const center = box.getCenter(new THREE.Vector3());
    const radius = Math.max(size.length() / 2, 1e-4);
    const fovRadians = (camera.fov * Math.PI) / 180;
    const distance = (radius / Math.sin(fovRadians / 2)) * 1.15;
    camera.position.copy(center).addScaledVector(new THREE.Vector3(0.68, 0.46, 1).normalize(), distance);
    camera.near = Math.max(radius * 0.001, 0.0001);
    camera.far = distance * 4 + radius * 2;
    camera.updateProjectionMatrix();
    controls.target.copy(center);
    controls.minDistance = radius * 0.08;
    controls.maxDistance = distance * 4;
    controls.update();
    defaultPosition = camera.position.clone();
    defaultTarget = controls.target.clone();
  };

  try {
    const gltf = await loader.loadAsync(src, (event: any) => {
      if (serial !== loadSerial || disposed) return;
      if (event?.total > 0) progress.value = Math.min(event.loaded / event.total, 1);
    });
    if (serial !== loadSerial || disposed) {
      disposeObject(gltf.scene);
      disposeResources();
      return;
    }

    root = gltf.scene;
    scene.add(root);

    if (gltf.animations?.length) {
      mixer = new THREE.AnimationMixer(root);
      gltf.animations.forEach((clip: any) => mixer.clipAction(clip).play());
      animationPlaying.value = true;
      hasAnimation.value = true;
    }

    fitCamera(root);
    resize();
    resizeObserver = new ResizeObserver(resize);
    resizeObserver.observe(host);

    viewer = {
      resetView: () => {
        if (!defaultPosition || !defaultTarget) return;
        camera.position.copy(defaultPosition);
        controls.target.copy(defaultTarget);
        controls.update();
      },
      setAutoRotate: (value: boolean) => {
        controls.autoRotate = value;
      },
      setAnimationPlaying: (value: boolean) => {
        if (mixer) mixer.timeScale = value ? 1 : 0;
      },
      dispose: disposeResources
    };

    progress.value = 0;
    status.value = 'ready';
    renderFrame();
  } catch (error) {
    disposeResources();
    throw error;
  } finally {
    dracoLoader.dispose();
  }
};

const loadModel = async () => {
  teardown();
  if (!props.active) return;
  const src = String(props.src || '').trim();
  const host = hostRef.value;
  if (!src || !host) return;

  const serial = loadSerial;
  status.value = 'loading';
  try {
    await buildViewer(serial, src, host);
  } catch {
    if (serial !== loadSerial) return;
    status.value = 'error';
    errorText.value = t('workspace.preview.model.error');
  }
};

const resetView = () => viewer?.resetView();

const toggleAutoRotate = () => {
  autoRotate.value = !autoRotate.value;
  viewer?.setAutoRotate(autoRotate.value);
};

const toggleAnimation = () => {
  animationPlaying.value = !animationPlaying.value;
  viewer?.setAnimationPlaying(animationPlaying.value);
};

watch(
  () => props.src,
  () => {
    void loadModel();
  }
);

watch(
  () => props.active,
  (active) => {
    if (active) {
      void loadModel();
      return;
    }
    teardown();
  }
);

onMounted(() => {
  if (props.active) void loadModel();
});

onBeforeUnmount(teardown);
</script>

<style scoped>
.model-preview {
  position: relative;
  width: 100%;
  height: 100%;
  min-height: 0;
  overflow: hidden;
  background: radial-gradient(circle at 50% 32%, #ffffff 0%, #eef2f7 58%, #e3e9f0 100%);
}

.model-preview :deep(.model-preview-canvas) {
  display: block;
  width: 100%;
  height: 100%;
  touch-action: none;
  outline: none;
}

.model-preview-state {
  position: absolute;
  inset: 0;
  display: grid;
  place-content: center;
  justify-items: center;
  gap: 10px;
  padding: 16px;
  color: #64748b;
  font-size: 13px;
  text-align: center;
  pointer-events: none;
}

.model-preview-state.is-error {
  color: #b91c1c;
}

.model-preview-spinner {
  width: 26px;
  height: 26px;
  border-radius: 50%;
  border: 2px solid rgba(100, 116, 139, 0.22);
  border-top-color: rgb(var(--ui-accent-rgb, 37 99 235));
  animation: model-preview-spin 0.8s linear infinite;
}

.model-preview-toolbar {
  position: absolute;
  right: 12px;
  bottom: 12px;
  left: 12px;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
  pointer-events: none;
}

.model-preview-tip {
  padding: 4px 10px;
  border-radius: 999px;
  background: rgba(255, 255, 255, 0.72);
  color: #64748b;
  font-size: 12px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  backdrop-filter: blur(6px);
}

.model-preview-actions {
  display: flex;
  flex: 0 0 auto;
  gap: 6px;
  pointer-events: auto;
}

.model-preview-btn {
  display: grid;
  place-items: center;
  width: 32px;
  height: 32px;
  padding: 0;
  border: 1px solid rgba(148, 163, 184, 0.35);
  border-radius: 10px;
  background: rgba(255, 255, 255, 0.85);
  color: #475569;
  cursor: pointer;
  transition: background-color 0.16s ease, color 0.16s ease, border-color 0.16s ease;
}

.model-preview-btn:hover {
  background: #ffffff;
  color: #0f172a;
}

.model-preview-btn.is-active {
  border-color: rgba(var(--ui-accent-rgb, 37 99 235), 0.55);
  background: rgba(var(--ui-accent-rgb, 37 99 235), 0.1);
  color: rgb(var(--ui-accent-rgb, 37 99 235));
}

@keyframes model-preview-spin {
  to {
    transform: rotate(360deg);
  }
}
</style>
