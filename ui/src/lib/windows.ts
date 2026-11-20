// Showing and hiding our own windows from the UI. Needs core:window:allow-show/allow-hide in the
// app's capability; failures are logged, never thrown, so a missing grant only loses the shortcut.

import { WebviewWindow, getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
import { api, inTauri } from './api.ts';
import type { Route } from './router.ts';
import type { Purpose } from './types.ts';
import { emitEvent } from './events.ts';

export async function showWindow(label: Route): Promise<boolean> {
  if (!inTauri()) {
    window.location.hash = `#/${label}`;
    return true;
  }
  try {
    const w = await WebviewWindow.getByLabel(label);
    if (!w) return false;
    await w.show();
    return true;
  } catch (err) {
    console.warn(`show ${label} failed`, err);
    return false;
  }
}

export async function hideSelf(): Promise<void> {
  if (!inTauri()) return;
  try {
    await getCurrentWebviewWindow().hide();
  } catch (err) {
    console.warn('hide failed', err);
  }
}

export const OVERLAY_CONTEXT_EVENT = 'overlay-context';
export interface OverlayContext { purpose: Purpose; display_id: string | null }

export function isOverlayContext(v: unknown): v is OverlayContext {
  if (typeof v !== 'object' || v === null) return false;
  const o = v as Record<string, unknown>;
  return (o.purpose === 'start' || o.purpose === 'switch') && (o.display_id === null || typeof o.display_id === 'string');
}

/** Tells the region and pick overlays which purpose and display the next selection is for. */
export async function announceOverlay(ctx: OverlayContext): Promise<void> {
  try {
    await emitEvent(OVERLAY_CONTEXT_EVENT, ctx);
  } catch (err) {
    console.warn('overlay-context emit failed', err);
  }
}

function mockNavigate(route: Route, purpose: Purpose, displayId: string | null) {
  if (inTauri()) return;
  const d = displayId ? `&display=${encodeURIComponent(displayId)}` : '';
  window.location.hash = `#/${route}?purpose=${purpose}${d}`;
}

export async function beginRegion(purpose: Purpose, displayId: string | null): Promise<void> {
  await announceOverlay({ purpose, display_id: displayId });
  await api.beginRegionSelect(purpose, displayId ?? undefined);
  mockNavigate('region', purpose, displayId);
}

export async function beginPick(purpose: Purpose): Promise<void> {
  await announceOverlay({ purpose, display_id: null });
  await api.beginPickMode(purpose);
  mockNavigate('pick', purpose, null);
}

export async function systemPicker(purpose: Purpose): Promise<void> {
  await api.openSystemPicker(purpose);
}
