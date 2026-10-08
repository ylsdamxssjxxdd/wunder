import { createHash } from 'node:crypto';

const hash = (value: string) => createHash('sha256').update(value).digest('hex');
const structure = new Set(['kind', 'status', 'role', 'visibility', 'change_type', 'event',
  'event_type', 'record_type', 'field', 'trigger_kind', 'trigger_mode', 'type']);
const allowedValue = /^[a-z_]{1,64}$/;
// Raw values stay in the test process. Identical strings keep identical fingerprints.
export function redactEvidence(value: any, key = ''): any {
  if (key === 'evidence_mode' && ['mock-service', 'scripted-events', 'real-service'].includes(value)) return value;
  if (/token$|secret|password|authorization|cookie|api_key/i.test(key)) return '[redacted]';
  if (typeof value === 'string') return structure.has(key) && allowedValue.test(value)
    ? value : { sha256: hash(value), length: value.length };
  if (Array.isArray(value)) return value.map(item => redactEvidence(item));
  if (value && typeof value === 'object') return Object.fromEntries(
    Object.entries(value).map(([name, item]) => [name, redactEvidence(item, name)]));
  return value;
}
