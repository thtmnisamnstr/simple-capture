// Runtime guards for DTOs arriving from the app (events and command replies).

import type {
  ExportProgressDto,
  ExportStateDto,
  HealthDto,
  NoticeDto,
  PermissionStatus,
  PermissionsDto,
  PickedDto,
  RecordingState,
  RectPt,
  RegionSelectedDto,
  SourceSpecDto,
} from './types.ts';

type Obj = Record<string, unknown>;

const isObj = (v: unknown): v is Obj => typeof v === 'object' && v !== null && !Array.isArray(v);
const isNum = (v: unknown): v is number => typeof v === 'number' && Number.isFinite(v);
const isStr = (v: unknown): v is string => typeof v === 'string';
const isBool = (v: unknown): v is boolean => typeof v === 'boolean';

const PERMISSION_STATUSES = ['granted', 'denied', 'not_determined', 'restricted', 'unknown'];

export const isPermissionStatus = (v: unknown): v is PermissionStatus =>
  isStr(v) && PERMISSION_STATUSES.includes(v);

export const isPermissions = (v: unknown): v is PermissionsDto =>
  isObj(v) && isPermissionStatus(v.screen) && isPermissionStatus(v.camera) && isPermissionStatus(v.mic);

export const isRectPt = (v: unknown): v is RectPt =>
  isObj(v) && isNum(v.x) && isNum(v.y) && isNum(v.w) && isNum(v.h) && v.w >= 0 && v.h >= 0;

export function isSourceSpec(v: unknown): v is SourceSpecDto {
  if (!isObj(v)) return false;
  switch (v.kind) {
    case 'display': return isStr(v.display_id);
    case 'window': return isNum(v.window_id);
    case 'application': return isStr(v.display_id) && isNum(v.pid) && (v.bundle_id === null || isStr(v.bundle_id));
    case 'region': return isStr(v.display_id) && isRectPt(v.region_pt);
    case 'picked': return isStr(v.pick_id) && (v.region_pt === null || isRectPt(v.region_pt));
    default: return false;
  }
}

const isPurpose = (v: unknown) => v === 'start' || v === 'switch';
const isVia = (v: unknown) => v === 'own_chooser' || v === 'system_picker' || v === 'portal';

export const isPicked = (v: unknown): v is PickedDto =>
  isObj(v) && isPurpose(v.purpose) && isSourceSpec(v.source) && isVia(v.via);

export const isRegionSelected = (v: unknown): v is RegionSelectedDto =>
  isObj(v) && isPurpose(v.purpose) && isStr(v.display_id) && isRectPt(v.region_pt);

export function isRecordingState(v: unknown): v is RecordingState {
  if (!isObj(v)) return false;
  switch (v.state) {
    case 'idle':
    case 'preparing':
    case 'stopping':
      return true;
    case 'recording':
    case 'switching':
      return isNum(v.elapsed_ns) && v.elapsed_ns >= 0;
    case 'finalized': return isStr(v.project_dir);
    case 'failed': return isStr(v.error);
    default: return false;
  }
}

export const isHealth = (v: unknown): v is HealthDto =>
  isObj(v) &&
  isNum(v.rec_ns) &&
  isNum(v.free_disk_bytes) &&
  Array.isArray(v.tracks) &&
  v.tracks.every((t) => isObj(t) && isStr(t.track_id) && isNum(t.frames) && isNum(t.dropped) && isNum(t.fps));

export function isNotice(v: unknown): v is NoticeDto {
  if (!isObj(v)) return false;
  switch (v.kind) {
    case 'switch_failed': return isStr(v.error) && isBool(v.restored);
    case 'source_lost': return isStr(v.segment);
    case 'device_lost': return (v.device === 'camera' || v.device === 'mic') && isStr(v.name);
    case 'disk_low': return isNum(v.bytes_left);
    case 'audio_gap': return (v.track === 'mic' || v.track === 'system_audio') && isNum(v.dur_ns);
    case 'frames_dropping': return isStr(v.track) && isNum(v.pct);
    case 'stopped_for_sleep':
    case 'stopped_for_disk':
      return true;
    case 'hotkey_unavailable': return isStr(v.error);
    default: return false;
  }
}

export const isExportProgress = (v: unknown): v is ExportProgressDto =>
  isObj(v) &&
  isStr(v.id) &&
  isNum(v.frames_done) &&
  isNum(v.frames_total) &&
  isNum(v.fps) &&
  (v.eta_s === null || isNum(v.eta_s));

export function isExportState(v: unknown): v is ExportStateDto {
  if (!isObj(v) || !isStr(v.id)) return false;
  switch (v.state) {
    case 'preparing':
    case 'rendering':
    case 'finalizing':
    case 'canceled':
      return true;
    case 'done': return isStr(v.path);
    case 'failed': return isStr(v.error);
    default: return false;
  }
}
