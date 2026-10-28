#!/bin/sh
# Quick host check before the container suite: the same curated production
# modules that tools/production/tests.sh runs (the release gate), on the host
# Python, about 40 s. Broad `unittest discover` also collects retained
# historical/profile tests and is not a qualification result (tests/README.md).
# Host prerequisites: clang, pyelftools; qemu-arm for the retained ARM tests.
# The container suite stays the release gate (pinned headers, ARM/QEMU checks).
set -eu
cd "$(dirname "$0")/../.."
modules=$(grep -o 'tests\.test_[a-z0-9_]*' tools/production/tests.sh | sort -u)
# As in the container (/build): the newest completed build, unless given.
if [ -z "${Y2_ARTIFACT_TEST_ROOT:-}" ]; then
    Y2_ARTIFACT_TEST_ROOT=$(ls -td "$PWD"/out/build "$PWD"/out/power-final-*-build 2>/dev/null | while read -r build; do
        [ -f "$build/kernel/.config" ] && [ -f "$build/y2.dtb" ] && { echo "$build"; break; }; done)
fi
[ -n "$Y2_ARTIFACT_TEST_ROOT" ] || { echo 'host-tests: no completed build in out/; build first' >&2; exit 2; }
export Y2_ARTIFACT_TEST_ROOT Y2_PRODUCTION_TEST=1
echo "host-tests: build $Y2_ARTIFACT_TEST_ROOT" >&2
missing=
python3 -c 'import elftools' 2>/dev/null || missing="$missing pyelftools"
[ -z "$missing" ] || echo "host-tests: missing$missing (Arch/CachyOS: python-pyelftools); its tests will error" >&2
# The Rust platform's host build, so tests.test_platform_differential compares it with Python
# (skipped without cargo, as in the container).
if [ -z "${Y2_PLATFORM_RUST:-}" ] && command -v cargo >/dev/null 2>&1; then
    (cd platform && cargo build --offline --locked -q) && export Y2_PLATFORM_RUST="$PWD/platform/target/debug/y2-platform"
fi
# shellcheck disable=SC2086 # one module name per word
PYTHONPATH=.:tests:tools/platform exec python3 -m unittest "$@" $modules
