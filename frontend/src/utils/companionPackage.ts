import { strFromU8, strToU8, unzipSync, zipSync } from 'three/examples/jsm/libs/fflate.module.js';

export type CompanionPackageManifest = {
  id: string;
  displayName: string;
  description: string;
  spritesheetPath: string;
};

export type CompanionImportResult = CompanionPackageManifest & {
  spritesheetDataUrl: string;
  spritesheetMime: string;
};

const MAX_COMPANION_PACKAGE_BYTES = 24 * 1024 * 1024;
const MAX_COMPANION_SPRITESHEET_BYTES = 18 * 1024 * 1024;
const MANIFEST_PATH = 'pet.json';
const FALLBACK_MIME = 'application/octet-stream';

const MIME_BY_EXTENSION: Record<string, string> = {
  webp: 'image/webp',
  png: 'image/png',
  gif: 'image/gif',
  jpg: 'image/jpeg',
  jpeg: 'image/jpeg'
};

const normalizeZipPath = (value: unknown): string =>
  String(value || '')
    .trim()
    .replace(/\\/g, '/')
    .replace(/^\/+/, '');

const sanitizeText = (value: unknown, maxLength: number): string =>
  String(value || '')
    .trim()
    .replace(/\s+/g, ' ')
    .slice(0, maxLength);

const sanitizePackageId = (value: unknown): string =>
  sanitizeText(value, 80)
    .toLowerCase()
    .replace(/[^a-z0-9_-]+/g, '-')
    .replace(/^-+|-+$/g, '');

const resolveMimeFromPath = (path: string): string => {
  const extension = path.split('.').pop()?.toLowerCase() || '';
  return MIME_BY_EXTENSION[extension] || FALLBACK_MIME;
};

const bytesToDataUrl = (bytes: Uint8Array, mime: string): string => {
  let binary = '';
  const chunkSize = 0x8000;
  for (let index = 0; index < bytes.length; index += chunkSize) {
    binary += String.fromCharCode(...bytes.subarray(index, index + chunkSize));
  }
  return `data:${mime};base64,${btoa(binary)}`;
};

const dataUrlToBytes = (dataUrl: string): Uint8Array => {
  const commaIndex = dataUrl.indexOf(',');
  if (!dataUrl.startsWith('data:') || commaIndex < 0) {
    throw new Error('invalid data url');
  }
  const header = dataUrl.slice(0, commaIndex).toLowerCase();
  const body = dataUrl.slice(commaIndex + 1);
  if (!header.includes(';base64')) {
    return strToU8(decodeURIComponent(body));
  }
  const binary = atob(body);
  const output = new Uint8Array(binary.length);
  for (let index = 0; index < binary.length; index += 1) {
    output[index] = binary.charCodeAt(index);
  }
  return output;
};

const findZipEntry = (
  entries: Record<string, Uint8Array>,
  path: string
): Uint8Array | null => {
  const normalized = normalizeZipPath(path).toLowerCase();
  const matchedKey = Object.keys(entries).find((key) => normalizeZipPath(key).toLowerCase() === normalized);
  return matchedKey ? entries[matchedKey] || null : null;
};

const parseManifest = (bytes: Uint8Array): CompanionPackageManifest => {
  const raw = JSON.parse(strFromU8(bytes));
  const id = sanitizePackageId(raw?.id);
  const displayName = sanitizeText(raw?.displayName || raw?.name || id, 80);
  const description = sanitizeText(raw?.description, 240);
  const spritesheetPath = normalizeZipPath(raw?.spritesheetPath);
  if (!id) {
    throw new Error('missing companion id');
  }
  if (!displayName) {
    throw new Error('missing companion display name');
  }
  if (!spritesheetPath || spritesheetPath.includes('..')) {
    throw new Error('invalid companion spritesheet path');
  }
  return {
    id,
    displayName,
    description,
    spritesheetPath
  };
};

export const parseCompanionPackageFile = async (file: File): Promise<CompanionImportResult> => {
  if (file.size > MAX_COMPANION_PACKAGE_BYTES) {
    throw new Error('companion package is too large');
  }
  const archiveBytes = new Uint8Array(await file.arrayBuffer());
  const entries = unzipSync(archiveBytes);
  const manifestBytes = findZipEntry(entries, MANIFEST_PATH);
  if (!manifestBytes) {
    throw new Error('pet.json not found');
  }
  const manifest = parseManifest(manifestBytes);
  const spritesheetBytes = findZipEntry(entries, manifest.spritesheetPath);
  if (!spritesheetBytes) {
    throw new Error('spritesheet not found');
  }
  if (spritesheetBytes.length > MAX_COMPANION_SPRITESHEET_BYTES) {
    throw new Error('spritesheet is too large');
  }
  const spritesheetMime = resolveMimeFromPath(manifest.spritesheetPath);
  return {
    ...manifest,
    spritesheetMime,
    spritesheetDataUrl: bytesToDataUrl(spritesheetBytes, spritesheetMime)
  };
};

export const buildCompanionPackageBlob = (manifest: CompanionPackageManifest, spritesheetDataUrl: string): Blob => {
  const normalizedManifest = parseManifest(
    strToU8(
      JSON.stringify({
        id: manifest.id,
        displayName: manifest.displayName,
        description: manifest.description,
        spritesheetPath: manifest.spritesheetPath
      })
    )
  );
  const spritesheetBytes = dataUrlToBytes(spritesheetDataUrl);
  const archiveBytes = zipSync(
    {
      [MANIFEST_PATH]: strToU8(`${JSON.stringify(normalizedManifest, null, 2)}\n`),
      [normalizedManifest.spritesheetPath]: spritesheetBytes
    },
    { level: 6 }
  );
  const archiveBuffer = new ArrayBuffer(archiveBytes.byteLength);
  new Uint8Array(archiveBuffer).set(archiveBytes);
  return new Blob([archiveBuffer], { type: 'application/zip' });
};

export const buildCompanionPackageFilename = (id: string): string => {
  const normalized = sanitizePackageId(id) || 'companion';
  return `${normalized}.zip`;
};
