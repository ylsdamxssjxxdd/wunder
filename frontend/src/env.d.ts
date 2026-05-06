/// <reference types="vite/client" />

declare const __WUNDER_APP_VERSION__: string;

declare module 'three/examples/jsm/libs/fflate.module.js' {
  export function strToU8(str: string, latin1?: boolean): Uint8Array;
  export function strFromU8(data: Uint8Array, latin1?: boolean): string;
  export function zipSync(data: Record<string, Uint8Array>, opts?: Record<string, unknown>): Uint8Array;
  export function unzipSync(data: Uint8Array, opts?: Record<string, unknown>): Record<string, Uint8Array>;
}
