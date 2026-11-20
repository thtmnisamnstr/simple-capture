#!/bin/bash
# npm license gate for ui/ (docs/LICENSES.md §2.3): production packages strictly, every package
# (dev tools included) with the named exceptions in npm-licenses.mjs. Needs `pnpm --dir ui install`.
set -euo pipefail
cd "$(dirname "$0")/../.."
[ -d ui/node_modules ] || { echo "licenses.sh: run pnpm --dir ui install --frozen-lockfile first" >&2; exit 1; }
node --test scripts/ci/npm-licenses.test.mjs >/dev/null
pnpm --dir ui licenses list --prod --json | node scripts/ci/npm-licenses.mjs --prod
pnpm --dir ui licenses list --json | node scripts/ci/npm-licenses.mjs --all
