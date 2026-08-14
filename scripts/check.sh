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

echo "-----{ diffuse: CHECK & VERIFY }-----"

# 1. Format checking / applying with +nightly
if [ "$fmt_mode" != "no" ]; then
    echo
    echo "-----{ FORMAT: CHECK & APPLY (NIGHTLY) }-----"
    if ! cargo +nightly fmt --all -- --check 2>&1; then
        echo "-----{ FORMAT: CHANGES DETECTED, APPLYING }-----"
        cargo +nightly fmt --all
    fi
    echo "-----{ FORMAT: ALL CLEAR }-----"
fi

# 2. Clippy checking / auto-fixing with +nightly
if [ "$clippy_mode" != "no" ]; then
    echo
    echo "-----{ CLIPPY: CHECK & FIX (NIGHTLY) }-----"
    cargo +nightly clippy --fix --allow-dirty --no-deps --all-targets \
        --workspace \
        -- -D warnings
    echo "-----{ CLIPPY: ALL CLEAR }-----"
fi

# 3. Standard test suite (cargo test --all)
if [ "$no_test" != true ]; then
    echo
    echo "-----{ TEST: CARGO TEST (ALL) }-----"
    cargo test --quiet --all
    echo "-----{ TEST: ALL CLEAR }-----"
fi

# 4. Extension test suite (extension/pi/ npm test)
if [ "$no_extension" != true ] && [ -d "extension/pi" ]; then
    echo
    echo "-----{ EXTENSION: PI REVIEW EQUIVALENCE GATE }-----"
    (cd extension/pi && npm test)
    echo "-----{ EXTENSION: ALL CLEAR }-----"
fi

echo
echo "-----{ DONE: ALL CHECKS PASSED CLEANLY! }-----"
