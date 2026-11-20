// Typed event listening/emitting: Tauri events in the app, the in-process bus elsewhere.

import { emit, listen } from '@tauri-apps/api/event';
import { inTauri } from './api.ts';
import { busEmit, busListen } from './bus.ts';

export type Unlisten = () => void;

export function onEvent<T>(name: string, guard: (v: unknown) => v is T, handler: (payload: T) => void): Unlisten {
  const accept = (payload: unknown) => {
    if (guard(payload)) handler(payload);
    else console.warn(`ignored malformed ${name} event`, payload);
  };
  if (!inTauri()) return busListen(name, accept);
  let stop: Unlisten | null = null;
  let cancelled = false;
  listen(name, (e) => accept(e.payload))
    .then((un) => (cancelled ? un() : (stop = un)))
    .catch((err) => console.error(`listen ${name} failed`, err));
  return () => {
    cancelled = true;
    stop?.();
  };
}

export async function emitEvent(name: string, payload: unknown): Promise<void> {
  if (!inTauri()) {
    busEmit(name, payload);
    return;
  }
  await emit(name, payload);
}
