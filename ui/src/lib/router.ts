// Hash routes: one Vite build, one route per window label (plan §5.2).

export const ROUTES = [
  'panel',
  'onboarding',
  'bar',
  'switcher',
  'region',
  'pick',
  'saved',
  'stage',
  'editor',
  'library',
  'diagnostics',
  'about',
] as const;
export type Route = (typeof ROUTES)[number];

export function routeFromHash(hash: string): Route {
  const name = hash.replace(/^#\/?/, '').split(/[/?]/)[0];
  return (ROUTES as readonly string[]).includes(name) ? (name as Route) : 'panel';
}

export function hashParams(hash: string): URLSearchParams {
  const i = hash.indexOf('?');
  return new URLSearchParams(i < 0 ? '' : hash.slice(i + 1));
}
