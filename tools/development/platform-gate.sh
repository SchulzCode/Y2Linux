#!/bin/sh
# Every check a platform commit needs, in one run; prints "GATE OK" only when all pass:
# cargo fmt, clippy (no warning), the armv7 type check, the code-quality ratchet over the platform and
# Y2Reborn (tools/development/code-quality.py), cargo test (no failure) and the byte comparison of
# every golden case's stdout and files against a baseline (tools/development/platform-raw-goldens.py).
#
#   tools/development/platform-gate.sh                         # gate a change (semantic, the default)
#   tools/development/platform-gate.sh <baseline dir>          # gate a change byte for byte (critical code)
#   tools/development/platform-gate.sh --baseline <new dir>    # record the byte baseline from the current tree
#
# The default (PLATFORM-RUST-IDIOMATIC-PLAN.md §2): the golden suite (parsed JSON; text exactly), the /data
# record compatibility check against the last release (tools/development/record-compat.py) and Reborn's
# contract test (tools/development/reborn-contract.py, then `cargo test` in ../Y2Reborn) replace the byte
# comparison. A deliberate output change re-records its goldens (Y2_RECORD_GOLDENS=1) and is reviewed with
# tools/development/golden-diff.py before the commit.
#
# Record the baseline once from the commit you start from (outputs of HEAD), then gate every change against
# it. A deliberate change of recorded calls or files (re-recorded with Y2_RECORD_GOLDENS=1) moves the
# baseline for exactly those cases: copy them from <baseline>.now/ into the baseline and note why.
# SPDX-License-Identifier: GPL-2.0-only
set -u
cd "$(dirname "$0")/../.." || exit 1
project=$PWD
if [ "${1:-}" = "--baseline" ]; then
    [ -n "${2:-}" ] && [ ! -e "$2" ] || { echo "usage: $0 --baseline <new directory>" >&2; exit 2; }
    (cd platform && cargo build --offline --locked -q) || exit 1
    # The suites run with raw outputs kept: their own shape assertions fail by design (stderr).
    python3 tools/development/platform-raw-goldens.py platform/target/debug/y2-platform "$2" >/dev/null 2>&1 || exit 1
    echo "baseline recorded in $2 from $(git rev-parse --short HEAD)"
    exit 0
fi
baseline=${1:-}
[ -z "$baseline" ] || [ -d "$baseline" ] || { echo "no baseline at $baseline" >&2; exit 2; }
cd platform || exit 1
cargo fmt
out=$(cargo clippy --offline --locked --all-targets -q 2>&1 | grep -E '^(warning|error)' -A6)
[ -z "$out" ] || { echo "$out" | head -30; echo "GATE FAIL clippy"; exit 1; }
out=$(cargo check --offline --locked -q --target armv7-unknown-linux-gnueabihf 2>&1 | grep -E '^(warning|error)' -A6)
[ -z "$out" ] || { echo "$out" | head -20; echo "GATE FAIL armv7"; exit 1; }
if ! out=$(python3 "$project/tools/development/code-quality.py" 2>&1); then
    echo "$out" | head -30
    echo "GATE FAIL code quality (docs/CODING-STANDARD.md; an improvement is recorded with --update)"
    exit 1
fi
log=$(cargo test --offline --locked -q 2>&1)
if echo "$log" | grep -qE 'FAILED|^error'; then
    echo "$log" | grep -E 'FAILED|panicked|^error' -A6 | head -30
    echo "GATE FAIL tests"
    exit 1
fi
cargo build --offline --locked -q || exit 1
if [ -z "$baseline" ]; then
    cd "$project" || exit 1
    log=$(Y2_PLATFORM_RUST="$project/platform/target/debug/y2-platform" PYTHONPATH=.:tests python3 -m unittest \
        tests.test_platform_differential tests.test_platform_devices tests.test_platform_reports \
        tests.test_platform_observation tests.test_platform_suspend tests.test_platform_power_rpc 2>&1)
    if ! echo "$log" | tail -1 | grep -q '^OK'; then
        echo "$log" | grep -E '^(FAIL|ERROR)' | head -20
        echo "GATE FAIL goldens"
        exit 1
    fi
    python3 tools/development/record-compat.py || { echo "GATE FAIL /data records"; exit 1; }
    reborn="$project/../Y2Reborn"
    if [ -d "$reborn" ]; then
        python3 tools/development/reborn-contract.py "$reborn" >/dev/null || exit 1
        out=$(cd "$reborn" && cargo test --offline -q -p reborn-platform reborn_reads_the_platform 2>&1)
        if ! echo "$out" | grep -q 'test result: ok. 1 passed'; then
            echo "$out" | grep -A40 'differences:' | head -60
            echo "GATE FAIL Reborn contract"
            exit 1
        fi
    fi
    echo "GATE OK"
    exit 0
fi
now="$baseline.now"
rm -rf "$now"
python3 "$project/tools/development/platform-raw-goldens.py" target/debug/y2-platform "$now" >/dev/null 2>&1 || {
    echo "GATE FAIL raw goldens did not run"
    exit 1
}
differences=$(diff -rq "$baseline" "$now")
if [ -n "$differences" ]; then
    echo "$differences" | head -20
    echo "GATE FAIL raw ($(echo "$differences" | wc -l) cases differ from the baseline)"
    exit 1
fi
echo "GATE OK"
