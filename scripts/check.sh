#!/bin/bash

# Exit on error
set -e
set -o pipefail

# Default modes
fmt_mode=apply
clippy_mode=apply
no_test=false
no_extension=false

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
        --no-extension)
            no_extension=true
            shift
            ;;
        *)
            echo "Unknown flag: $1" >&2
            exit 1
            ;;
    esac
done

# Helper to measure duration in seconds
time_diff_sec() {
    local start="$1"
    local end="$2"
    awk "BEGIN {printf \"%.2f\", ($end - $start)/1000000000}"
}

total_start=$(date +%s%N)
echo "-----{ diffuse: CHECK & VERIFY }-----"

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
    if ! cargo clippy --no-deps --workspace --all-targets -- -D warnings 2>&1; then
        echo "-----{ CLIPPY: WARNINGS DETECTED, APPLYING FIXES }-----"
        cargo clippy --fix --allow-dirty --no-deps --all-targets \
            --workspace \
            -- -D warnings
    fi
    t_end=$(date +%s%N)
    echo "-----{ CLIPPY: ALL CLEAR ($(time_diff_sec "$t_start" "$t_end")s) }-----"
fi

# 3. Pre-build all workspace test targets
if [ "$no_test" != true ]; then
    echo
    echo "-----{ BUILD: CARGO BUILD WORKSPACE (TEST TARGETS) }-----"
    t_start=$(date +%s%N)
    cargo test --no-run --workspace --all-targets --quiet
    t_end=$(date +%s%N)
    echo "-----{ BUILD: ALL CLEAR ($(time_diff_sec "$t_start" "$t_end")s) }-----"

    # 4. Run test suite on warm build
    echo
    echo "-----{ TEST: CARGO TEST (WARM RUN) }-----"
    t_start=$(date +%s%N)
    cargo test --quiet --all
    t_end=$(date +%s%N)
    echo "-----{ TEST: ALL CLEAR ($(time_diff_sec "$t_start" "$t_end")s) }-----"
fi

# 5. Extension test suite (extension/pi/ npm test)
if [ "$no_extension" != true ] && [ -d "extension/pi" ]; then
    echo
    echo "-----{ EXTENSION: PI REVIEW EQUIVALENCE GATE }-----"
    t_start=$(date +%s%N)
    mkdir -p extension/pi/bin
    cp target/debug/diffuse extension/pi/bin/diffuse
    (cd extension/pi && npm test --ignore-scripts)
    t_end=$(date +%s%N)
    echo "-----{ EXTENSION: ALL CLEAR ($(time_diff_sec "$t_start" "$t_end")s) }-----"
fi

total_end=$(date +%s%N)
echo
echo "-----{ DONE: ALL CHECKS PASSED CLEANLY! ($(time_diff_sec "$total_start" "$total_end")s) }-----"
