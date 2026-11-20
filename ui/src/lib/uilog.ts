// Forwards CSP violations and uncaught errors to the app log (`ui-log` event), so a run of the
// app shows them without a web inspector. `window.__scLog` lets the debug driver report back.

import { emitEvent } from './events.ts';

declare global {
  interface Window {
    __scLog?: (line: string) => void;
  }
}

export function uiLogLine(route: string, kind: string, detail: string): string {
  return `[${route}] ${kind}: ${detail}`;
}

export function installUiLog(route: () => string): void {
  const send = (kind: string, detail: string) => {
    emitEvent('ui-log', uiLogLine(route(), kind, detail)).catch(() => undefined);
  };
  window.__scLog = (line: string) => send('log', line);
  document.addEventListener('securitypolicyviolation', (e) =>
    send('csp', `${e.effectiveDirective} blocked ${e.blockedURI || 'inline'}`),
  );
  window.addEventListener('error', (e) => send('error', e.message));
  window.addEventListener('unhandledrejection', (e) => send('rejection', String(e.reason)));
}
