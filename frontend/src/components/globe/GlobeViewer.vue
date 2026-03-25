<template>
  <div ref="containerRef" class="globe-viewer" :class="{ 'globe-viewer--fill': props.fill }">
    <div
      class="globe-tooltip"
      :class="{ 'is-visible': tooltipVisible }"
      :style="tooltipStyle"
      v-show="tooltipVisible"
    >
      {{ tooltipText }}
    </div>
    <div class="globe-latlon">{{ latLonText }}</div>
    <div v-if="errorMessage" class="globe-viewer-error">
      {{ errorMessage }}
    </div>
  </div>
</template>

<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref } from 'vue';
import { useI18n } from '@/i18n';
import {
  AmbientLight,
  BufferGeometry,
  Color,
  DirectionalLight,
  Float32BufferAttribute,
  Group,
  LineBasicMaterial,
  LineSegments,
  Mesh,
  MeshStandardMaterial,
  PerspectiveCamera,
  Raycaster,
  Scene,
  SphereGeometry,
  SRGBColorSpace,
  Vector2,
  Vector3,
  WebGLRenderer
} from 'three';
import { OrbitControls } from 'three/examples/jsm/controls/OrbitControls.js';
import { feature } from 'topojson-client';
import countriesTopo from '@/assets/geo/countries-110m.json';
import countriesTopo50 from '@/assets/geo/countries-50m.json';
import chinaProvincesGeo from '@/assets/geo/china-provinces.json';
import chinaCityGeo from '@/assets/geo/china-citylevel.json';
import countryNamesZh from '@/assets/geo/country-names-zh.json';

const props = withDefaults(
  defineProps<{
    fill?: boolean;
  }>(),
  {
    fill: false
  }
);

const containerRef = ref<HTMLDivElement | null>(null);
const errorMessage = ref('');
const { t } = useI18n();

const RADIUS = 1;
const LINE_RADIUS = 1.012;
const HIGHLIGHT_RADIUS = 1.02;
const CAMERA_DISTANCE = 2.6;
const MAX_PIXEL_RATIO = 2;
const DETAIL_SWITCH_DISTANCE = 2.15;
const DETAIL_HYSTERESIS = 0.08;
const PROVINCE_SHOW_DISTANCE = 1.9;
const PROVINCE_HIDE_DISTANCE = 2.05;
const DEFAULT_CENTER = { lon: 104.0, lat: 35.0 };

type Bounds = {
  minLon: number;
  maxLon: number;
  minLat: number;
  maxLat: number;
};

type Ring = [number, number][];

type PolygonShape = {
  outer: Ring;
  holes: Ring[];
  bounds: Bounds;
  wrap: boolean;
};

type CountryShape = {
  name: string;
  polygons: PolygonShape[];
};

type HitShape = {
  name: string;
  polygons: PolygonShape[];
  key: string;
  kind: 'country' | 'province';
};

let renderer: WebGLRenderer | null = null;
let camera: PerspectiveCamera | null = null;
let scene: Scene | null = null;
let controls: OrbitControls | null = null;
let globeGroup: Group | null = null;
let globeMesh: Mesh | null = null;
let lineMesh: LineSegments | null = null;
let lineGeometry: BufferGeometry | null = null;
let provinceLineMesh: LineSegments | null = null;
let provinceLineGeometry: BufferGeometry | null = null;
let nineDashMesh: LineSegments | null = null;
let highlightMesh: LineSegments | null = null;
let globeMaterial: MeshStandardMaterial | null = null;
let lineMaterial: LineBasicMaterial | null = null;
let provinceLineMaterial: LineBasicMaterial | null = null;
let nineDashMaterial: LineBasicMaterial | null = null;
let highlightMaterial: LineBasicMaterial | null = null;
let animationId: number | null = null;
let resizeObserver: ResizeObserver | null = null;
let themeObserver: MutationObserver | null = null;
let pointerTarget: HTMLCanvasElement | null = null;
let activeHighlight: HitShape | null = null;

let cachedBoundaryPositionsLow: Float32Array | null = null;
let cachedBoundaryPositionsMid: Float32Array | null = null;
let cachedCountryShapes: CountryShape[] | null = null;
let cachedCountryPolygonGroups: Map<string, PolygonShape[]> | null = null;
let cachedProvincePositions: Float32Array | null = null;
let cachedProvinceShapes: CountryShape[] | null = null;
let currentDetailLevel: 'low' | 'mid' = 'low';
let provincesVisible = false;

const raycaster = new Raycaster();
const pointer = new Vector2();
const intersectionPoint = new Vector3();
let pendingPointerEvent: PointerEvent | null = null;
let pointerRafId: number | null = null;
let lastPointerEvent: PointerEvent | null = null;
let isInteracting = false;

const tooltipText = ref('');
const tooltipVisible = ref(false);
const tooltipStyle = ref<Record<string, string>>({ left: '0px', top: '0px' });
const latLonText = ref('--, --');
const CHINA_COUNTRY_NAME = '中国';

const toRadians = (value: number): number => (value * Math.PI) / 180;

const toCartesian = (lon: number, lat: number, radius: number) => {
  const latRad = toRadians(lat);
  const lonRad = toRadians(lon);
  const cosLat = Math.cos(latRad);
  return {
    x: radius * cosLat * Math.sin(lonRad),
    y: radius * Math.sin(latRad),
    z: radius * cosLat * Math.cos(lonRad)
  };
};

const addRingSegments = (ring: number[][], output: number[], radius: number = LINE_RADIUS) => {
  if (!Array.isArray(ring) || ring.length < 2) return;
  for (let i = 0; i < ring.length; i += 1) {
    const current = ring[i];
    const next = ring[(i + 1) % ring.length];
    if (!Array.isArray(current) || !Array.isArray(next)) continue;
    if (current.length < 2 || next.length < 2) continue;
    const start = toCartesian(current[0], current[1], radius);
    const end = toCartesian(next[0], next[1], radius);
    output.push(start.x, start.y, start.z, end.x, end.y, end.z);
  }
};

const buildBoundaryPositions = (topology: any): Float32Array => {
  const output: number[] = [];
  const features = feature(topology, topology.objects.countries) as any;
  const list = Array.isArray(features?.features) ? features.features : [];
  list.forEach((item) => {
    const geometry = item?.geometry;
    if (!geometry) return;
    if (geometry.type === 'Polygon') {
      (geometry.coordinates || []).forEach((ring: number[][]) => {
        addRingSegments(ring, output);
      });
    } else if (geometry.type === 'MultiPolygon') {
      (geometry.coordinates || []).forEach((polygon: number[][][]) => {
        (polygon || []).forEach((ring: number[][]) => {
          addRingSegments(ring, output);
        });
      });
    }
  });
  return new Float32Array(output);
};

const getBoundaryPositions = (detail: 'low' | 'mid'): Float32Array => {
  if (detail === 'mid') {
    if (!cachedBoundaryPositionsMid) {
      cachedBoundaryPositionsMid = buildBoundaryPositions(countriesTopo50 as any);
    }
    return cachedBoundaryPositionsMid;
  }
  if (!cachedBoundaryPositionsLow) {
    cachedBoundaryPositionsLow = buildBoundaryPositions(countriesTopo as any);
  }
  return cachedBoundaryPositionsLow;
};

const normalizeName = (value: unknown): string => {
  const text = String(value || '').trim();
  return text || 'Unknown';
};

const normalizeCountryName = (value: unknown, numericId?: unknown): string => {
  const rawNumeric = String(numericId || '').trim();
  const numeric = /^\d+$/.test(rawNumeric) ? rawNumeric.padStart(3, '0') : '';
  const mapped = numeric ? (countryNamesZh as Record<string, string>)[numeric] : '';
  const text = mapped || normalizeName(value);
  if (text.includes('台湾')) {
    return CHINA_COUNTRY_NAME;
  }
  const lowerText = text.toLowerCase();
  if (lowerText === 'taiwan' || lowerText.includes('taiwan province')) {
    return CHINA_COUNTRY_NAME;
  }
  return text;
};

const normalizeProvinceName = (value: unknown): string => {
  const text = normalizeName(value);
  if (text.includes('台湾')) {
    return '台湾省';
  }
  return text;
};

const computeBounds = (ring: Ring): Bounds => {
  let minLon = Infinity;
  let maxLon = -Infinity;
  let minLat = Infinity;
  let maxLat = -Infinity;
  ring.forEach(([lon, lat]) => {
    minLon = Math.min(minLon, lon);
    maxLon = Math.max(maxLon, lon);
    minLat = Math.min(minLat, lat);
    maxLat = Math.max(maxLat, lat);
  });
  return { minLon, maxLon, minLat, maxLat };
};

const shouldWrap = (ring: Ring): boolean => {
  if (!ring.length) return false;
  const bounds = computeBounds(ring);
  return bounds.maxLon - bounds.minLon > 180;
};

const applyWrap = (ring: Ring): Ring =>
  ring.map(([lon, lat]) => [lon < 0 ? lon + 360 : lon, lat]);

const buildCountryShapes = (): CountryShape[] => {
  if (cachedCountryShapes) return cachedCountryShapes;
  const topology = countriesTopo as any;
  const features = feature(topology, topology.objects.countries) as any;
  const list = Array.isArray(features?.features) ? features.features : [];
  const shapes: CountryShape[] = [];
  list.forEach((item) => {
    const geometry = item?.geometry;
    if (!geometry) return;
    const name = normalizeCountryName(item?.properties?.name || item?.id, item?.id);
    const rawPolygons =
      geometry.type === 'Polygon' ? [geometry.coordinates] : geometry.coordinates || [];
    const polygons: PolygonShape[] = [];
    rawPolygons.forEach((polygon: number[][][]) => {
      if (!Array.isArray(polygon) || polygon.length === 0) return;
      const outerRing = polygon[0] as Ring;
      if (!outerRing || outerRing.length < 3) return;
      const wrap = shouldWrap(outerRing);
      const normalizedOuter = wrap ? applyWrap(outerRing) : outerRing;
      const holes = (polygon.slice(1) || []).map((ring) => (wrap ? applyWrap(ring as Ring) : (ring as Ring)));
      const bounds = computeBounds(normalizedOuter);
      polygons.push({
        outer: normalizedOuter,
        holes,
        bounds,
        wrap
      });
    });
    if (polygons.length) {
      shapes.push({ name, polygons });
    }
  });
  cachedCountryShapes = shapes;
  return shapes;
};

const buildCountryPolygonGroups = (): Map<string, PolygonShape[]> => {
  if (cachedCountryPolygonGroups) return cachedCountryPolygonGroups;
  const groups = new Map<string, PolygonShape[]>();
  buildCountryShapes().forEach((country) => {
    const current = groups.get(country.name);
    if (current) {
      current.push(...country.polygons);
      return;
    }
    groups.set(country.name, [...country.polygons]);
  });
  cachedCountryPolygonGroups = groups;
  return groups;
};

const buildProvincePositions = (): Float32Array => {
  if (cachedProvincePositions) return cachedProvincePositions;
  const output: number[] = [];
  const features = Array.isArray((chinaProvincesGeo as any)?.features)
    ? (chinaProvincesGeo as any).features
    : [];
  features.forEach((item: any) => {
    const geometry = item?.geometry;
    if (!geometry) return;
    const polygons =
      geometry.type === 'Polygon' ? [geometry.coordinates] : geometry.coordinates || [];
    polygons.forEach((polygon: number[][][]) => {
      (polygon || []).forEach((ring: number[][]) => {
        addRingSegments(ring, output, LINE_RADIUS + 0.002);
      });
    });
  });
  cachedProvincePositions = new Float32Array(output);
  return cachedProvincePositions;
};

const buildProvinceShapes = (): CountryShape[] => {
  if (cachedProvinceShapes) return cachedProvinceShapes;
  const features = Array.isArray((chinaProvincesGeo as any)?.features)
    ? (chinaProvincesGeo as any).features
    : [];
  const shapes: CountryShape[] = [];
  features.forEach((item: any) => {
    const geometry = item?.geometry;
    if (!geometry) return;
    const name = normalizeProvinceName(item?.properties?.name || item?.id);
    const rawPolygons =
      geometry.type === 'Polygon' ? [geometry.coordinates] : geometry.coordinates || [];
    const polygons: PolygonShape[] = [];
    rawPolygons.forEach((polygon: number[][][]) => {
      if (!Array.isArray(polygon) || polygon.length === 0) return;
      const outerRing = polygon[0] as Ring;
      if (!outerRing || outerRing.length < 3) return;
      const wrap = shouldWrap(outerRing);
      const normalizedOuter = wrap ? applyWrap(outerRing) : outerRing;
      const holes = (polygon.slice(1) || []).map((ring) => (wrap ? applyWrap(ring as Ring) : (ring as Ring)));
      const bounds = computeBounds(normalizedOuter);
      polygons.push({
        outer: normalizedOuter,
        holes,
        bounds,
        wrap
      });
    });
    if (polygons.length) {
      shapes.push({ name, polygons });
    }
  });
  cachedProvinceShapes = shapes;
  return shapes;
};

const addDashedRingSegments = (
  ring: Ring,
  output: number[],
  radius: number,
  dashStride = 5,
  dashOn = 2
) => {
  if (!Array.isArray(ring) || ring.length < 2) return;
  for (let i = 0; i < ring.length; i += 1) {
    if (i % dashStride >= dashOn) continue;
    const current = ring[i];
    const next = ring[(i + 1) % ring.length];
    if (!Array.isArray(current) || !Array.isArray(next)) continue;
    if (current.length < 2 || next.length < 2) continue;
    const start = toCartesian(current[0], current[1], radius);
    const end = toCartesian(next[0], next[1], radius);
    output.push(start.x, start.y, start.z, end.x, end.y, end.z);
  }
};

const buildNineDashPositions = (): Float32Array => {
  const output: number[] = [];
  const features = Array.isArray((chinaCityGeo as any)?.features)
    ? (chinaCityGeo as any).features
    : [];
  const sansha = features.find(
    (item: any) => String(item?.properties?.name || '').trim() === '三沙市'
  );
  if (!sansha?.geometry) {
    return new Float32Array(output);
  }
  const geometry = sansha.geometry;
  const polygons =
    geometry.type === 'Polygon' ? [geometry.coordinates] : geometry.coordinates || [];
  polygons.forEach((polygon: number[][][]) => {
    (polygon || []).forEach((ring: number[][]) => {
      addDashedRingSegments(ring as Ring, output, LINE_RADIUS + 0.004, 4, 3);
    });
  });
  return new Float32Array(output);
};

const pointInRing = (lon: number, lat: number, ring: Ring): boolean => {
  let inside = false;
  for (let i = 0, j = ring.length - 1; i < ring.length; j = i++) {
    const [xi, yi] = ring[i];
    const [xj, yj] = ring[j];
    const intersect = yi > lat !== yj > lat && lon < ((xj - xi) * (lat - yi)) / (yj - yi) + xi;
    if (intersect) inside = !inside;
  }
  return inside;
};

const pointInPolygon = (lon: number, lat: number, polygon: PolygonShape): boolean => {
  if (!pointInRing(lon, lat, polygon.outer)) return false;
  for (const hole of polygon.holes) {
    if (pointInRing(lon, lat, hole)) return false;
  }
  return true;
};

const normalizeLon = (lon: number): number => {
  let value = lon;
  if (value < -180) value += 360;
  if (value > 180) value -= 360;
  return value;
};

const CHINA_BOUNDS: Bounds = {
  minLon: 73,
  maxLon: 135,
  minLat: 3,
  maxLat: 54
};

const resolveCountryHit = (lon: number, lat: number): HitShape | null => {
  const shapes = buildCountryShapes();
  const polygonGroups = buildCountryPolygonGroups();
  const normalizedLon = normalizeLon(lon);
  for (const country of shapes) {
    for (const polygon of country.polygons) {
      const testLon = polygon.wrap ? (normalizedLon < 0 ? normalizedLon + 360 : normalizedLon) : normalizedLon;
      if (testLon < polygon.bounds.minLon || testLon > polygon.bounds.maxLon) continue;
      if (lat < polygon.bounds.minLat || lat > polygon.bounds.maxLat) continue;
      if (pointInPolygon(testLon, lat, polygon)) {
        return {
          name: country.name,
          polygons: polygonGroups.get(country.name) || [polygon],
          key: `country:${country.name}`,
          kind: 'country'
        };
      }
    }
  }
  return null;
};

const resolveProvinceHit = (lon: number, lat: number): HitShape | null => {
  if (lon < CHINA_BOUNDS.minLon || lon > CHINA_BOUNDS.maxLon) return null;
  if (lat < CHINA_BOUNDS.minLat || lat > CHINA_BOUNDS.maxLat) return null;
  const shapes = buildProvinceShapes();
  const normalizedLon = normalizeLon(lon);
  for (const province of shapes) {
    for (const polygon of province.polygons) {
      const testLon = polygon.wrap ? (normalizedLon < 0 ? normalizedLon + 360 : normalizedLon) : normalizedLon;
      if (testLon < polygon.bounds.minLon || testLon > polygon.bounds.maxLon) continue;
      if (lat < polygon.bounds.minLat || lat > polygon.bounds.maxLat) continue;
      if (pointInPolygon(testLon, lat, polygon)) {
        return {
          name: province.name,
          polygons: province.polygons,
          key: `province:${province.name}`,
          kind: 'province'
        };
      }
    }
  }
  return null;
};

const readCssVar = (name: string): string => {
  if (typeof document === 'undefined') return '';
  return getComputedStyle(document.documentElement).getPropertyValue(name).trim();
};

const normalizeColor = (value: string, fallback: string): string => {
  const trimmed = String(value || '').trim();
  if (!trimmed) return fallback;
  return trimmed;
};

const resolveThemeColors = () => {
  const mode = document.documentElement.getAttribute('data-user-theme') || 'light';
  const accent = normalizeColor(readCssVar('--ui-accent'), mode === 'dark' ? '#5eead4' : '#0ea5e9');
  const accentSoft = normalizeColor(readCssVar('--ui-accent-soft-3'), mode === 'dark' ? '#0f172a' : '#e2e8f0');
  const accentDeep = normalizeColor(readCssVar('--ui-accent-deep'), mode === 'dark' ? '#f97316' : '#ea580c');
  return {
    globe: '#000000',
    line: accent,
    province: accentSoft,
    nineDash: accentDeep,
    highlight: accentDeep
  };
};

const applyTheme = () => {
  if (!globeMaterial || !lineMaterial) return;
  const colors = resolveThemeColors();
  globeMaterial.color.set(colors.globe);
  lineMaterial.color.set(colors.line);
  if (provinceLineMaterial) {
    provinceLineMaterial.color.set(colors.province);
  }
  if (nineDashMaterial) {
    nineDashMaterial.color.set(colors.nineDash);
  }
  if (highlightMaterial) {
    highlightMaterial.color.set(colors.highlight);
  }
};

const toLocalPoint = (point: Vector3): Vector3 => {
  if (!globeGroup) return point.clone();
  globeGroup.updateMatrixWorld(true);
  return globeGroup.worldToLocal(point.clone());
};

const setInitialView = (lon: number, lat: number) => {
  if (!camera || !controls) return;
  const position = toCartesian(lon, lat, CAMERA_DISTANCE);
  camera.position.set(position.x, position.y, position.z);
  const north = toCartesian(lon, lat + 1, CAMERA_DISTANCE);
  const up = new Vector3(north.x - position.x, north.y - position.y, north.z - position.z).normalize();
  camera.up.copy(up);
  controls.target.set(0, 0, 0);
  camera.lookAt(0, 0, 0);
  controls.update();
};

const cartesianToLatLon = (point: Vector3) => {
  const length = point.length();
  if (!length) return { lon: 0, lat: 0 };
  const lat = Math.asin(point.y / length) * (180 / Math.PI);
  let lon = Math.atan2(point.x, point.z) * (180 / Math.PI);
  if (lon < -180) lon += 360;
  if (lon > 180) lon -= 360;
  return { lon, lat };
};

const updateBoundaryDetail = (detail: 'low' | 'mid') => {
  if (!lineMesh) return;
  const positions = getBoundaryPositions(detail);
  const nextGeometry = new BufferGeometry();
  nextGeometry.setAttribute('position', new Float32BufferAttribute(positions, 3));
  nextGeometry.computeBoundingSphere();
  lineMesh.geometry.dispose();
  lineMesh.geometry = nextGeometry;
  lineGeometry = nextGeometry;
  currentDetailLevel = detail;
};

const resolveDetailLevel = (distance: number): 'low' | 'mid' => {
  if (currentDetailLevel === 'low') {
    return distance < DETAIL_SWITCH_DISTANCE ? 'mid' : 'low';
  }
  return distance > DETAIL_SWITCH_DISTANCE + DETAIL_HYSTERESIS ? 'low' : 'mid';
};

const resolveViewCenter = (): { lon: number; lat: number } => {
  if (!camera) return { lon: 0, lat: 0 };
  const direction = camera.position.clone().normalize().multiplyScalar(RADIUS);
  const local = toLocalPoint(direction);
  return cartesianToLatLon(local);
};

const updateProvinceVisibility = (distance: number) => {
  if (!provinceLineMesh) return;
  const center = resolveViewCenter();
  const withinChina =
    center.lon >= CHINA_BOUNDS.minLon &&
    center.lon <= CHINA_BOUNDS.maxLon &&
    center.lat >= CHINA_BOUNDS.minLat &&
    center.lat <= CHINA_BOUNDS.maxLat;
  if (!provincesVisible) {
    if (withinChina && distance < PROVINCE_SHOW_DISTANCE) {
      provincesVisible = true;
      provinceLineMesh.visible = true;
    }
    return;
  }
  if (!withinChina || distance > PROVINCE_HIDE_DISTANCE) {
    provincesVisible = false;
    provinceLineMesh.visible = false;
    if (activeHighlight?.kind === 'province') {
      updateHighlight(null);
    }
  }
};

const showTooltip = (text: string, x: number, y: number) => {
  tooltipText.value = text;
  tooltipStyle.value = { left: `${x}px`, top: `${y}px` };
  tooltipVisible.value = true;
};

const hideTooltip = () => {
  tooltipVisible.value = false;
};

const formatCoord = (value: number, posLabel: string, negLabel: string): string => {
  const abs = Math.abs(value).toFixed(2);
  const label = value >= 0 ? posLabel : negLabel;
  return `${label} ${abs}°`;
};

const updatePointerHover = (event: PointerEvent) => {
  if (!renderer || !camera || !globeMesh) return;
  const rect = renderer.domElement.getBoundingClientRect();
  if (!rect.width || !rect.height) return;
  const x = ((event.clientX - rect.left) / rect.width) * 2 - 1;
  const y = -((event.clientY - rect.top) / rect.height) * 2 + 1;
  pointer.set(x, y);
  raycaster.setFromCamera(pointer, camera);
  const intersects = raycaster.intersectObject(globeMesh, false);
  if (!intersects.length) {
    hideTooltip();
    latLonText.value = '--, --';
    updateHighlight(null);
    return;
  }
  intersectionPoint.copy(intersects[0].point);
  const localPoint = toLocalPoint(intersectionPoint);
  const { lon, lat } = cartesianToLatLon(localPoint);
  latLonText.value = `${formatCoord(lat, 'N', 'S')}, ${formatCoord(lon, 'E', 'W')}`;
  const provinceHit = provincesVisible ? resolveProvinceHit(lon, lat) : null;
  const countryHit = provinceHit ? null : resolveCountryHit(lon, lat);
  const hit = provinceHit || countryHit;
  if (!hit) {
    hideTooltip();
    updateHighlight(null);
    return;
  }
  const localX = event.clientX - rect.left;
  const localY = event.clientY - rect.top;
  showTooltip(hit.name, localX, localY);
  updateHighlight(hit);
};

const handlePointerMove = (event: PointerEvent) => {
  lastPointerEvent = event;
  if (isInteracting) return;
  pendingPointerEvent = event;
  if (pointerRafId) return;
  pointerRafId = requestAnimationFrame(() => {
    pointerRafId = null;
    if (pendingPointerEvent) {
      updatePointerHover(pendingPointerEvent);
    }
  });
};

const handlePointerLeave = () => {
  pendingPointerEvent = null;
  lastPointerEvent = null;
  hideTooltip();
  latLonText.value = '--, --';
  updateHighlight(null);
};

const handleControlStart = () => {
  isInteracting = true;
  pendingPointerEvent = null;
  hideTooltip();
  latLonText.value = '--, --';
  updateHighlight(null);
};

const handleControlEnd = () => {
  isInteracting = false;
  if (lastPointerEvent) {
    updatePointerHover(lastPointerEvent);
  }
};

const updateHighlight = (hit: HitShape | null) => {
  if (!highlightMesh) return;
  if (!hit) {
    highlightMesh.visible = false;
    activeHighlight = null;
    return;
  }
  if (activeHighlight?.key === hit.key) {
    return;
  }
  const positions: number[] = [];
  hit.polygons.forEach((polygon) => {
    addRingSegments(polygon.outer, positions, HIGHLIGHT_RADIUS);
  });
  if (!positions.length) {
    highlightMesh.visible = false;
    activeHighlight = null;
    return;
  }
  const nextGeometry = new BufferGeometry();
  nextGeometry.setAttribute('position', new Float32BufferAttribute(positions, 3));
  nextGeometry.computeBoundingSphere();
  highlightMesh.geometry.dispose();
  highlightMesh.geometry = nextGeometry;
  highlightMesh.visible = true;
  activeHighlight = hit;
};

const setupScene = () => {
  const container = containerRef.value;
  if (!container) return;

  try {
    scene = new Scene();
    camera = new PerspectiveCamera(45, 1, 0.1, 20);
    camera.position.set(0, 0, CAMERA_DISTANCE);

    renderer = new WebGLRenderer({ antialias: true, alpha: true });
    renderer.outputColorSpace = SRGBColorSpace;
    renderer.setPixelRatio(Math.min(window.devicePixelRatio || 1, MAX_PIXEL_RATIO));
    container.appendChild(renderer.domElement);

    const globeGeometry = new SphereGeometry(RADIUS, 64, 64);
    globeMaterial = new MeshStandardMaterial({
      color: new Color('#000000'),
      roughness: 0.85,
      metalness: 0.05
    });
    globeMesh = new Mesh(globeGeometry, globeMaterial);

    const boundaryPositions = getBoundaryPositions(currentDetailLevel);
    lineGeometry = new BufferGeometry();
    lineGeometry.setAttribute('position', new Float32BufferAttribute(boundaryPositions, 3));
    lineMaterial = new LineBasicMaterial({ color: new Color('#0ea5e9'), transparent: true, opacity: 0.7 });
    lineMesh = new LineSegments(lineGeometry, lineMaterial);

    const provincePositions = buildProvincePositions();
    provinceLineGeometry = new BufferGeometry();
    provinceLineGeometry.setAttribute('position', new Float32BufferAttribute(provincePositions, 3));
    provinceLineMaterial = new LineBasicMaterial({ color: new Color('#38bdf8'), transparent: true, opacity: 0.55 });
    provinceLineMesh = new LineSegments(provinceLineGeometry, provinceLineMaterial);
    provinceLineMesh.visible = false;

    const nineDashPositions = buildNineDashPositions();
    const nineDashGeometry = new BufferGeometry();
    nineDashGeometry.setAttribute('position', new Float32BufferAttribute(nineDashPositions, 3));
    nineDashGeometry.computeBoundingSphere();
    nineDashMaterial = new LineBasicMaterial({ color: new Color('#ea580c'), transparent: true, opacity: 1 });
    nineDashMaterial.depthWrite = false;
    nineDashMesh = new LineSegments(nineDashGeometry, nineDashMaterial);
    nineDashMesh.renderOrder = 2;

    const highlightGeometry = new BufferGeometry();
    highlightMaterial = new LineBasicMaterial({ color: new Color('#ea580c'), transparent: true, opacity: 0.95 });
    highlightMesh = new LineSegments(highlightGeometry, highlightMaterial);
    highlightMesh.visible = false;
    highlightMesh.renderOrder = 3;

    globeGroup = new Group();
    globeGroup.add(globeMesh);
    globeGroup.add(lineMesh);
    globeGroup.add(nineDashMesh);
    globeGroup.add(provinceLineMesh);
    globeGroup.add(highlightMesh);

    scene.add(globeGroup);
    scene.add(new AmbientLight(0xffffff, 0.85));
    const directional = new DirectionalLight(0xffffff, 0.6);
    directional.position.set(4, 2, 3);
    scene.add(directional);

    controls = new OrbitControls(camera, renderer.domElement);
    controls.enablePan = false;
    controls.enableDamping = true;
    controls.dampingFactor = 0.12;
    controls.rotateSpeed = 0.35;
    controls.zoomSpeed = 0.8;
    controls.autoRotate = false;
    controls.autoRotateSpeed = 0.0;
    controls.minDistance = 1.6;
    controls.maxDistance = 4.2;
    controls.addEventListener('start', handleControlStart);
    controls.addEventListener('end', handleControlEnd);

    applyTheme();
    startRenderLoop();
    observeResize(container);
    observeTheme();
    setInitialView(DEFAULT_CENTER.lon, DEFAULT_CENTER.lat);
    pointerTarget = renderer.domElement;
    pointerTarget.addEventListener('pointermove', handlePointerMove, { passive: true });
    pointerTarget.addEventListener('pointerleave', handlePointerLeave, { passive: true });
  } catch (error) {
    errorMessage.value = t('userWorld.helperApps.globe.webglError');
  }
};

const updateSize = () => {
  if (!containerRef.value || !renderer || !camera) return;
  const { width, height } = containerRef.value.getBoundingClientRect();
  if (width <= 0 || height <= 0) return;
  camera.aspect = width / height;
  camera.updateProjectionMatrix();
  renderer.setSize(width, height, false);
};

const observeResize = (container: HTMLDivElement) => {
  resizeObserver?.disconnect();
  resizeObserver = new ResizeObserver(() => updateSize());
  resizeObserver.observe(container);
  updateSize();
};

const observeTheme = () => {
  if (typeof MutationObserver === 'undefined') return;
  themeObserver?.disconnect();
  themeObserver = new MutationObserver(() => applyTheme());
  themeObserver.observe(document.documentElement, {
    attributes: true,
    attributeFilter: ['data-user-theme', 'data-user-accent']
  });
};

const startRenderLoop = () => {
  if (!renderer || !scene || !camera) return;
  const render = () => {
    animationId = requestAnimationFrame(render);
    controls?.update();
    const distance = camera.position.length();
    const nextDetail = resolveDetailLevel(distance);
    if (nextDetail !== currentDetailLevel) {
      updateBoundaryDetail(nextDetail);
    }
    updateProvinceVisibility(distance);
    renderer?.render(scene, camera);
  };
  render();
};

const cleanup = () => {
  if (animationId) {
    cancelAnimationFrame(animationId);
    animationId = null;
  }
  if (pointerRafId) {
    cancelAnimationFrame(pointerRafId);
    pointerRafId = null;
  }
  if (pointerTarget) {
    pointerTarget.removeEventListener('pointermove', handlePointerMove);
    pointerTarget.removeEventListener('pointerleave', handlePointerLeave);
    pointerTarget = null;
  }
  pendingPointerEvent = null;
  resizeObserver?.disconnect();
  resizeObserver = null;
  themeObserver?.disconnect();
  themeObserver = null;
  if (controls) {
    controls.removeEventListener('start', handleControlStart);
    controls.removeEventListener('end', handleControlEnd);
    controls.dispose();
  }
  controls = null;
  if (globeMesh) {
    globeMesh.geometry.dispose();
  }
  if (lineMesh) {
    lineMesh.geometry.dispose();
  }
  if (provinceLineMesh) {
    provinceLineMesh.geometry.dispose();
  }
  if (nineDashMesh) {
    nineDashMesh.geometry.dispose();
  }
  if (highlightMesh) {
    highlightMesh.geometry.dispose();
  }
  globeMaterial?.dispose();
  lineMaterial?.dispose();
  provinceLineMaterial?.dispose();
  nineDashMaterial?.dispose();
  highlightMaterial?.dispose();
  if (renderer) {
    renderer.dispose();
    if (renderer.domElement && renderer.domElement.parentNode) {
      renderer.domElement.parentNode.removeChild(renderer.domElement);
    }
  }
  renderer = null;
  scene = null;
  camera = null;
  globeGroup = null;
  globeMesh = null;
  lineMesh = null;
  lineGeometry = null;
  provinceLineMesh = null;
  provinceLineGeometry = null;
  nineDashMesh = null;
  highlightMesh = null;
  globeMaterial = null;
  lineMaterial = null;
  provinceLineMaterial = null;
  nineDashMaterial = null;
  highlightMaterial = null;
  activeHighlight = null;
};

onMounted(() => {
  setupScene();
});

onBeforeUnmount(() => {
  cleanup();
});
</script>

<style scoped>
.globe-viewer {
  width: 100%;
  height: clamp(260px, 48vh, 430px);
  border-radius: 16px;
  border: 1px solid rgba(var(--ui-accent-rgb), 0.26);
  background:
    radial-gradient(circle at 20% 20%, rgba(var(--ui-accent-rgb), 0.24), transparent 45%),
    radial-gradient(circle at 80% 75%, rgba(var(--ui-accent-rgb), 0.14), transparent 55%),
    var(--ui-accent-soft-2, #f6f7f9);
  overflow: hidden;
  position: relative;
}

.globe-viewer--fill {
  height: 100%;
  flex: 1;
  min-height: 0;
}

.globe-viewer :deep(canvas) {
  width: 100% !important;
  height: 100% !important;
  display: block;
}

.globe-tooltip {
  position: absolute;
  top: 0;
  left: 0;
  padding: 6px 10px;
  border-radius: 8px;
  background: rgba(15, 23, 42, 0.86);
  color: #f8fafc;
  font-size: 12px;
  letter-spacing: 0.2px;
  pointer-events: none;
  opacity: 0;
  transform: translate(12px, 12px);
  transition: opacity 0.12s ease;
  z-index: 2;
  white-space: nowrap;
}

.globe-tooltip.is-visible {
  opacity: 1;
}

.globe-latlon {
  position: absolute;
  right: 10px;
  bottom: 10px;
  padding: 6px 10px;
  border-radius: 8px;
  background: rgba(15, 23, 42, 0.72);
  color: #f8fafc;
  font-size: 12px;
  letter-spacing: 0.2px;
  z-index: 2;
  pointer-events: none;
}

.globe-viewer-error {
  position: absolute;
  inset: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--hula-text, #0f172a);
  font-size: 13px;
  background: rgba(15, 23, 42, 0.08);
  text-align: center;
  padding: 12px;
}
</style>
