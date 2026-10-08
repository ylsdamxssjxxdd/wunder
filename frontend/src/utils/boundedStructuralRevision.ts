const STRING_SAMPLE_CHARS = 64;
const MAX_DEPTH = 4;
const COLLECTION_SAMPLE_COUNT = 2;

const objectIdentity = new WeakMap<object, number>();
let objectIdentityClock = 0;

const resolveObjectIdentity = (value: object): number => {
  const existing = objectIdentity.get(value);
  if (existing) return existing;
  const next = ++objectIdentityClock;
  objectIdentity.set(value, next);
  return next;
};

const mixHash = (hash: number, value: number): number => {
  let next = (hash ^ value) >>> 0;
  next = Math.imul(next, 0x01000193) >>> 0;
  return next;
};

const mixString = (hash: number, value: string): number => {
  let next = mixHash(hash, value.length);
  const headEnd = Math.min(value.length, STRING_SAMPLE_CHARS);
  for (let index = 0; index < headEnd; index += 1) {
    next = mixHash(next, value.charCodeAt(index));
  }
  const tailStart = Math.max(headEnd, value.length - STRING_SAMPLE_CHARS);
  for (let index = tailStart; index < value.length; index += 1) {
    next = mixHash(next, value.charCodeAt(index));
  }
  return next;
};

const hashValue = (
  value: unknown,
  hash: number,
  depth: number,
  seen: WeakSet<object>
): number => {
  if (value === null) return mixHash(hash, 0x11);
  if (value === undefined) return mixHash(hash, 0x12);
  if (typeof value === 'string') return mixString(mixHash(hash, 0x21), value);
  if (typeof value === 'number') return mixString(mixHash(hash, 0x22), String(value));
  if (typeof value === 'boolean') return mixHash(hash, value ? 0x23 : 0x24);
  if (typeof value === 'bigint') return mixString(mixHash(hash, 0x25), String(value));
  if (typeof value !== 'object') return mixString(mixHash(hash, 0x26), typeof value);

  const objectValue = value as object;
  let next = mixHash(hash, resolveObjectIdentity(objectValue));
  if (seen.has(objectValue) || depth >= MAX_DEPTH) return next;
  seen.add(objectValue);

  if (Array.isArray(value)) {
    next = mixHash(next, 0x31);
    next = mixHash(next, value.length);
    const headEnd = Math.min(value.length, COLLECTION_SAMPLE_COUNT);
    for (let index = 0; index < headEnd; index += 1) {
      next = hashValue(value[index], next, depth + 1, seen);
    }
    const tailStart = Math.max(headEnd, value.length - COLLECTION_SAMPLE_COUNT);
    for (let index = tailStart; index < value.length; index += 1) {
      next = hashValue(value[index], next, depth + 1, seen);
    }
    return next;
  }

  next = mixHash(next, 0x32);
  const entries = Object.entries(value as Record<string, unknown>);
  next = mixHash(next, entries.length);
  for (const [key, item] of entries) {
    next = mixString(next, key);
    next = hashValue(item, next, depth + 1, seen);
  }
  return next;
};

// Large tool payloads are sampled at the string edges, while object/array shape
// and identities remain part of the revision. Canonical runtime updates also
// carry sequence fields, so middle-only payload changes still invalidate safely.
export const buildBoundedStructuralRevision = (value: unknown): string =>
  hashValue(value, 0x811c9dc5, 0, new WeakSet()).toString(16).padStart(8, '0');
