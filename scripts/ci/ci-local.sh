#!/bin/bash
# Local equivalent of the GitHub workflow (M1 plan §6.4). Runs every step, then prints a summary;
# exits 1 if any step failed. Skip steps with SC_CI_SKIP="linux-docker windows-check ..." (shown
# as SKIPPED, never as passed). Uses CARGO_TARGET_DIR as set (Skia-linking: target/skia, §0.2).
set -uo pipefail
cd "$(dirname "$0")/../.." || exit 1
SKIP=" ${SC_CI_SKIP:-} "
LOG_DIR=${SC_CI_LOG_DIR:-target/ci-local}
mkdir -p "$LOG_DIR"
results=()
failed=0

step() {
    local name=$1
    shift
    if [[ "$SKIP" == *" $name "* ]]; then
        results+=("SKIPPED  $name")
        return
    fi
    echo "=== ci-local: $name"
    local t0=$SECONDS
    if "$@" 2>&1 | tee "$LOG_DIR/$name.log"; then
        results+=("ok       $name ($((SECONDS - t0)) s)")
    else
        results+=("FAILED   $name ($((SECONDS - t0)) s, log $LOG_DIR/$name.log)")
        failed=1
    fi
}

ci_files() {
    local rc=0
    if command -v actionlint >/dev/null; then actionlint || rc=1; else echo "actionlint not installed: skipped"; fi
    if command -v shellcheck >/dev/null; then shellcheck -S warning scripts/ci/*.sh || rc=1; else echo "shellcheck not installed: skipped"; fi
    node --test scripts/ci/npm-licenses.test.mjs || rc=1
    return $rc
}

ui() {
    pnpm --dir ui install --frozen-lockfile && pnpm --dir ui check && pnpm --dir ui build && pnpm --dir ui test
}

step ci-files ci_files
step ui ui
step lint scripts/ci/lint.sh
step test cargo test --locked --workspace
step kill-synth cargo test --locked -p simple-capture-record --features test-hooks --test kill_synth
step deny scripts/ci/deny.sh
step licenses scripts/ci/licenses.sh
step notices scripts/ci/notices.sh
step netcheck scripts/ci/netcheck.sh
step windows-check scripts/ci/windows-check.sh
step linux-docker scripts/ci/linux-docker.sh

echo "=== ci-local summary"
printf '  %s\n' "${results[@]}"
exit $failed
