// npm license gate (docs/LICENSES.md §1, §2.3). No dependencies.
//
//   pnpm --dir ui licenses list --prod --json | node scripts/ci/npm-licenses.mjs --prod
//   pnpm --dir ui licenses list --json        | node scripts/ci/npm-licenses.mjs --all
//
// --prod: every production package (shipped in the UI bundle) must satisfy ALLOWED.
// --all:  also dev tools; DEV_EXCEPTIONS names build tools that are never shipped. This catches
//         devDependencies that Vite compiles into the bundle anyway (svelte and its runtime).
import { readFileSync } from 'node:fs';
import { pathToFileURL } from 'node:url';

export const ALLOWED = new Set([
  'MIT', 'Apache-2.0', 'Apache-2.0 WITH LLVM-exception', 'BSD-2-Clause', 'BSD-3-Clause',
  'ISC', 'Zlib', 'Unicode-3.0', 'CC0-1.0', '0BSD',
]);

// Build-time only (Vite's CSS engine), never in ui/dist.
export const DEV_EXCEPTIONS = [{ prefix: 'lightningcss', license: 'MPL-2.0' }];

// Minimal SPDX expression check: AND, OR, WITH, parentheses.
export function satisfies(expr, allowed = ALLOWED) {
  const tokens = String(expr).replace(/[()]/g, ' $& ').trim().split(/\s+/).filter(Boolean);
  let i = 0;
  const peek = () => tokens[i];
  function primary() {
    const t = tokens[i++];
    if (t === undefined) throw new Error(`bad SPDX expression: ${expr}`);
    if (t === '(') {
      const v = or();
      if (tokens[i++] !== ')') throw new Error(`bad SPDX expression: ${expr}`);
      return v;
    }
    if (peek() === 'WITH') {
      i++;
      const exc = tokens[i++];
      return allowed.has(`${t} WITH ${exc}`);
    }
    return allowed.has(t.replace(/\+$/, ''));
  }
  function and() {
    let v = primary();
    while (peek() === 'AND') { i++; v = primary() && v; }
    return v;
  }
  function or() {
    let v = and();
    while (peek() === 'OR') { i++; v = and() || v; }
    return v;
  }
  const v = or();
  if (i !== tokens.length) throw new Error(`bad SPDX expression: ${expr}`);
  return v;
}

// pnpm's JSON: { "<license>": [ { name, versions, license, ... } ] }
export function check(report, mode) {
  const bad = [];
  let count = 0;
  for (const [groupLicense, pkgs] of Object.entries(report)) {
    for (const pkg of pkgs) {
      count++;
      const license = pkg.license ?? groupLicense;
      let ok;
      try { ok = satisfies(license); } catch { ok = false; }
      if (!ok && mode === 'all') {
        ok = DEV_EXCEPTIONS.some((e) => pkg.name.startsWith(e.prefix) && e.license === license);
      }
      if (!ok) bad.push(`${pkg.name}@${(pkg.versions ?? []).join(',')}: ${license}`);
    }
  }
  return { count, bad };
}

if (import.meta.url === pathToFileURL(process.argv[1]).href) {
  const mode = process.argv.includes('--all') ? 'all' : 'prod';
  const input = readFileSync(0, 'utf8').trim();
  const report = input ? JSON.parse(input) : {};
  const { count, bad } = check(report, mode);
  if (bad.length) {
    console.error(`npm-licenses (${mode}): ${bad.length} package(s) outside the allow list:`);
    for (const b of bad) console.error(`  ${b}`);
    process.exit(1);
  }
  console.log(`npm-licenses (${mode}): ${count} package(s) ok`);
}
