#!/usr/bin/env bash

# Exit on error
set -e
set -o pipefail

# Default modes
fmt_mode=apply
clippy_mode=apply
no_test=false
no_install=false
TARGET_EXPLICIT=false
if [ -n "${TARGET:-}" ] || [ -n "${DIFFUSE_TARGET:-}" ]; then
    TARGET_EXPLICIT=true
fi
TARGET="${TARGET:-${DIFFUSE_TARGET:-}}"

# Option parsing loop
while [[ $# -gt 0 ]]; do
    case "$1" in
        --no-fmt)
            fmt_mode="no"
            shift
            ;;
        --no-clippy)
            clippy_mode="no"
            shift
            ;;
        --no-test)
            no_test=true
            shift
            ;;
        --no-install)
            no_install=true
            shift
            ;;
        --target)
            TARGET="$2"
            TARGET_EXPLICIT=true
            shift 2
            ;;
        --target=*)
            TARGET="${1#*=}"
            TARGET_EXPLICIT=true
            shift
            ;;
        *)
            echo "Unknown flag: $1" >&2
            exit 1
            ;;
    esac
done

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
. "$SCRIPT_DIR/target.sh"

HOST_TARGET="$(detect_host_target)"
# Only pass an explicit --target to cargo invocations when the user asked
# for one (--target flag or TARGET/DIFFUSE_TARGET env). Otherwise cargo
# uses its default target directory, sharing the warm artifact cache with
# manual cargo runs instead of rebuilding into target/<triple>/.
TARGET_ARGS=()
if [ "$TARGET_EXPLICIT" = true ] && [ -n "$TARGET" ]; then
    TARGET_ARGS=(--target "$TARGET")
fi

# Helper to measure duration in seconds
time_diff_sec() {
    local start="$1"
    local end="$2"
    awk "BEGIN {printf \"%.2f\", ($end - $start)/1000000000}"
}

total_start=$(date +%s%N)
echo "-----{ diffuse: CHECK, FIX & VERIFY }-----"
if [ -n "$TARGET" ]; then
    echo "Target platform: $TARGET (host: $HOST_TARGET)"
else
    echo "Target platform: $HOST_TARGET (cargo default; shared cache)"
fi

# Ensure required toolchain, components, and target are installed before performing operations
ensure_toolchain_and_target "$TARGET" true

# 1. Format checking / applying
if [ "$fmt_mode" != "no" ]; then
    echo
    echo "-----{ FORMAT: CHECK & APPLY (NIGHTLY) }-----"
    t_start=$(date +%s%N)
    if ! cargo +nightly fmt --all -- --check 2>&1; then
        echo "-----{ FORMAT: CHANGES DETECTED, APPLYING }-----"
        cargo +nightly fmt --all
    fi
    t_end=$(date +%s%N)
    echo "-----{ FORMAT: ALL CLEAR ($(time_diff_sec "$t_start" "$t_end")s) }-----"
fi

# 2. Clippy checking / auto-fixing
if [ "$clippy_mode" != "no" ]; then
    echo
    echo "-----{ CLIPPY: CHECK & FIX }-----"
    t_start=$(date +%s%N)
    CLIPPY_CMD=(cargo clippy "${TARGET_ARGS[@]}" --quiet --no-deps --workspace --all-targets -- -D warnings)
    if ! clippy_out=$("${CLIPPY_CMD[@]}" 2>&1); then
        echo "-----{ CLIPPY: WARNINGS DETECTED, APPLYING FIXES }-----"
        cargo clippy --fix --allow-dirty --no-deps --all-targets --workspace "${TARGET_ARGS[@]}" -- -D warnings 2>&1 | grep -vE '^[[:space:]]*(Checking|Compiling|Finished)[[:space:]]' || true
        echo "-----{ CLIPPY: RE-CHECKING AFTER FIX }-----"
        "${CLIPPY_CMD[@]}" 2>&1
    fi
    t_end=$(date +%s%N)
    echo "-----{ CLIPPY: ALL CLEAR ($(time_diff_sec "$t_start" "$t_end")s) }-----"
fi

# 3. Pre-build all workspace test targets
if [ "$no_test" != true ]; then
    echo
    echo "-----{ BUILD: CARGO BUILD WORKSPACE (TEST TARGETS) }-----"
    t_start=$(date +%s%N)
    cargo test "${TARGET_ARGS[@]}" --no-run --workspace --all-targets --quiet
    t_end=$(date +%s%N)
    echo "-----{ BUILD: ALL CLEAR ($(time_diff_sec "$t_start" "$t_end")s) }-----"

    # 4. Run test suite — either natively or via detected emulator.
    #    Without an explicit target the binaries are host-native by
    #    construction, so no emulator is ever needed.
    CAN_RUN_TESTS=false
    if [ -z "$TARGET" ] || target_runs_natively "$TARGET" "$HOST_TARGET"; then
        CAN_RUN_TESTS=true
    elif runner=$(resolve_target_runner "$TARGET" "$HOST_TARGET"); then
        upper_target=$(echo "$TARGET" | tr '[:lower:]-' '[:upper:]_')
        export "CARGO_TARGET_${upper_target}_RUNNER"="$runner"
        CAN_RUN_TESTS=true
        echo "-----{ TEST: RUNNER DETECTED ($runner) }-----"
    fi

    if [ "$CAN_RUN_TESTS" = true ]; then
        echo
        echo "-----{ TEST: CARGO TEST (WARM RUN) }-----"
        t_start=$(date +%s%N)
        cargo test "${TARGET_ARGS[@]}" --quiet --workspace 2>&1 | grep -vE '^(running [[:digit:]]+ test|test result: |$|all doctests ran in )'
        t_end=$(date +%s%N)
        echo "-----{ TEST: ALL CLEAR ($(time_diff_sec "$t_start" "$t_end")s) }-----"
    else
        echo
        echo "-----{ TEST: EXECUTION SKIPPED (NO EMULATOR) }-----"
        echo "Host platform ($HOST_TARGET) cannot execute target binaries ($TARGET) without an emulator (e.g. qemu, wine)."
        echo "All test targets were compiled and verified successfully."
    fi
fi

# 5. Automatic Dev Install & Extension Sync
if [ "$no_install" != true ]; then
    echo
    echo "-----{ INSTALL: DEV BINARY & PI EXTENSION }-----"
    t_start=$(date +%s%N)
    PROFILE=dev TARGET="$TARGET" "$SCRIPT_DIR/install.sh"
    t_end=$(date +%s%N)
    echo "-----{ INSTALL: ALL CLEAR ($(time_diff_sec "$t_start" "$t_end")s) }-----"
fi

total_end=$(date +%s%N)
echo
echo "-----{ DONE: ALL CHECKS PASSED CLEANLY! ($(time_diff_sec "$total_start" "$total_end")s) }-----"
