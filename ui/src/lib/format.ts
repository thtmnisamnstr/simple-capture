export function formatElapsed(ns: number): string {
  const total = Math.max(0, Math.floor(ns / 1e9));
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = total % 60;
  const mm = String(m).padStart(h ? 2 : 1, '0');
  const ss = String(s).padStart(2, '0');
  return h ? `${h}:${mm}:${ss}` : `${mm}:${ss}`;
}

export function formatBytes(n: number): string {
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  let v = n;
  let i = 0;
  while (v >= 1000 && i < units.length - 1) (v /= 1000), i++;
  return `${v >= 10 || i === 0 ? Math.round(v) : v.toFixed(1)} ${units[i]}`;
}

/** The app's own `sc://<host>` scheme; Windows (WebView2) serves it as `http://sc.localhost/<host>`. */
export function scUrl(host: string, ua: string = typeof navigator === 'undefined' ? '' : navigator.userAgent): string {
  return /Windows|Android/.test(ua) ? `http://sc.localhost/${host}` : `sc://${host}`;
}

export function basename(path: string): string {
  const parts = path.split(/[\\/]/).filter(Boolean);
  return parts[parts.length - 1] ?? path;
}

export type HostOs = 'macos' | 'windows' | 'linux';

export function hostOs(ua: string = typeof navigator === 'undefined' ? '' : navigator.userAgent): HostOs {
  if (/Windows/i.test(ua)) return 'windows';
  if (/Mac OS X|Macintosh/i.test(ua)) return 'macos';
  return 'linux';
}

/** A ⌘ shortcut as this platform writes it: unchanged on macOS, with Ctrl elsewhere. */
export function kbd(mac: string, os: HostOs = hostOs()): string {
  return os === 'macos' ? mac : mac.replace('⇧⌘', 'Ctrl+Shift+').replace('⌘', 'Ctrl+');
}

export function revealLabel(os: HostOs = hostOs()): string {
  return os === 'macos' ? 'Show in Finder' : os === 'windows' ? 'Show in Explorer' : 'Show in Files';
}
