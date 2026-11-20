// In-process event bus standing in for Tauri events outside the app (mock backend, Browser pane).

type Listener = (payload: unknown) => void;

const listeners = new Map<string, Set<Listener>>();

export function busListen(name: string, fn: Listener): () => void {
  let set = listeners.get(name);
  if (!set) listeners.set(name, (set = new Set()));
  set.add(fn);
  return () => set.delete(fn);
}

export function busEmit(name: string, payload: unknown): void {
  for (const fn of [...(listeners.get(name) ?? [])]) fn(payload);
}
