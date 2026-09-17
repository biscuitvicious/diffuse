#!/usr/bin/env bash
# scripts/target.sh: Target resolution, toolchain provisioning, and emulation runner detection.
# Sourced by check.sh and install.sh.

set -euo pipefail

WORKSPACE_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PINNED_TOOLCHAIN="$(sed -n 's/^[[:space:]]*channel[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p' "$WORKSPACE_ROOT/rust-toolchain.toml" 2>/dev/null || echo "")"

# Supported target triples from rust-toolchain.toml
SUPPORTED_TARGETS=(
    "x86_64-unknown-linux-gnu"
    "x86_64-unknown-linux-musl"
    "aarch64-unknown-linux-gnu"
    "aarch64-unknown-linux-musl"
    "x86_64-apple-darwin"
    "aarch64-apple-darwin"
    "x86_64-pc-windows-msvc"
    "x86_64-pc-windows-gnu"
    "aarch64-pc-windows-msvc"
    "aarch64-linux-android"
)

# Detect host target triple
detect_host_target() {
    local host=""
    if command -v rustc >/dev/null 2>&1; then
        host=$(rustc -vV 2>/dev/null | sed -n 's/^host: //p')
    fi
    if [ -n "$host" ]; then
        echo "$host"
        return 0
    fi
    local os arch
    os=$(uname -s | tr '[:upper:]' '[:lower:]')
    arch=$(uname -m)
    case "$os" in
        linux*)
            case "$arch" in
                x86_64|amd64) echo "x86_64-unknown-linux-gnu" ;;
                aarch64|arm64) echo "aarch64-unknown-linux-gnu" ;;
                *) echo "${arch}-unknown-linux-gnu" ;;
            esac
            ;;
        darwin*)
            case "$arch" in
                arm64|aarch64) echo "aarch64-apple-darwin" ;;
                *) echo "x86_64-apple-darwin" ;;
            esac
            ;;
        msys*|mingw*|cygwin*|windows*)
            case "$arch" in
                aarch64|arm64) echo "aarch64-pc-windows-msvc" ;;
                *) echo "x86_64-pc-windows-msvc" ;;
            esac
            ;;
        *)
            echo "x86_64-unknown-linux-gnu"
            ;;
    esac
}

# Check whether a target is installed in rustup for a specific toolchain
is_target_installed() {
    local target="$1"
    local toolchain="${2:-}"
    local tc_arg=()
    if [ -n "$toolchain" ]; then
        tc_arg=(--toolchain "$toolchain")
    fi
    if command -v rustup >/dev/null 2>&1; then
        rustup target list "${tc_arg[@]}" --installed 2>/dev/null | grep -Fxq "$target"
        return $?
    fi
    return 1
}

# Resolve the best target for the host platform vs available targets.
# On Linux, prefers musl for portable static binaries (install/release use).
detect_best_target() {
    local host="$1"
    case "$host" in
        x86_64-unknown-linux-*)
            # On Linux x86_64, musl produces a zero-dependency static PIE binary
            if is_target_installed "x86_64-unknown-linux-musl" "$PINNED_TOOLCHAIN"; then
                echo "x86_64-unknown-linux-musl"
            else
                echo "x86_64-unknown-linux-gnu"
            fi
            ;;
        aarch64-unknown-linux-*)
            if is_target_installed "aarch64-unknown-linux-musl" "$PINNED_TOOLCHAIN"; then
                echo "aarch64-unknown-linux-musl"
            else
                echo "aarch64-unknown-linux-gnu"
            fi
            ;;
        aarch64-apple-darwin)
            echo "aarch64-apple-darwin"
            ;;
        x86_64-apple-darwin)
            echo "x86_64-apple-darwin"
            ;;
        *windows*|*mingw*|*msys*)
            case "$host" in
                aarch64*) echo "aarch64-pc-windows-msvc" ;;
                *)
                    if is_target_installed "x86_64-pc-windows-msvc" "$PINNED_TOOLCHAIN"; then
                        echo "x86_64-pc-windows-msvc"
                    elif is_target_installed "x86_64-pc-windows-gnu" "$PINNED_TOOLCHAIN"; then
                        echo "x86_64-pc-windows-gnu"
                    else
                        echo "x86_64-pc-windows-msvc"
                    fi
                    ;;
            esac
            ;;
        *)
            echo "$host"
            ;;
    esac
}

# Resolve the native target for the host platform — always the exact host
# triple, never a cross-compiled variant. Use this for test/verify steps
# where sharing the artifact cache with the host toolchain is paramount.
detect_native_target() {
    local host="$1"
    # Strip any non-native libc variant and return the canonical host triple.
    case "$host" in
        x86_64-unknown-linux-*)  echo "x86_64-unknown-linux-gnu" ;;
        aarch64-unknown-linux-*) echo "aarch64-unknown-linux-gnu" ;;
        *)                        echo "$host" ;;
    esac
}

# Ensure toolchain and target are installed via rustup before executing operations
ensure_toolchain_and_target() {
    local target="$1"
    local need_nightly="${2:-false}"

    if ! command -v rustup >/dev/null 2>&1; then
        return 0
    fi

    # 1. Pinned toolchain from rust-toolchain.toml
    if [ -n "$PINNED_TOOLCHAIN" ]; then
        if ! rustup toolchain list 2>/dev/null | grep -Fq "$PINNED_TOOLCHAIN"; then
            echo "Installing pinned toolchain $PINNED_TOOLCHAIN..."
            rustup toolchain install "$PINNED_TOOLCHAIN" --profile minimal
        fi
        if ! is_target_installed "$target" "$PINNED_TOOLCHAIN"; then
            echo "Installing target $target for toolchain $PINNED_TOOLCHAIN..."
            rustup target add --toolchain "$PINNED_TOOLCHAIN" "$target"
        fi
    fi

    # 2. Nightly toolchain (for check.sh fmt)
    if [ "$need_nightly" = "true" ]; then
        if ! rustup toolchain list 2>/dev/null | grep -Fq "nightly"; then
            echo "Installing nightly toolchain..."
            rustup toolchain install nightly --profile minimal --component rustfmt
        fi
        # Ensure nightly has rustfmt component
        rustup component add --toolchain nightly rustfmt 2>/dev/null || true
    fi
}

# Check if target runs natively on host without emulator
target_runs_natively() {
    local target="$1"
    local host="$2"
    if [ "$target" = "$host" ]; then
        return 0
    fi
    case "$host" in
        x86_64-unknown-linux-*)
            case "$target" in
                x86_64-unknown-linux-*) return 0 ;;
            esac
            ;;
        aarch64-unknown-linux-*)
            case "$target" in
                aarch64-unknown-linux-*) return 0 ;;
            esac
            ;;
        aarch64-apple-darwin)
            case "$target" in
                x86_64-apple-darwin) return 0 ;; # Rosetta 2
            esac
            ;;
    esac
    return 1
}

# Resolve emulator / runner for target on host
resolve_target_runner() {
    local target="$1"
    local host="$2"

    if target_runs_natively "$target" "$host"; then
        echo ""
        return 0
    fi

    # Check if a CARGO_TARGET_<UPPER>_RUNNER is already explicitly set in env
    local upper_target
    upper_target=$(echo "$target" | tr '[:lower:]-' '[:upper:]_')
    local env_runner_var="CARGO_TARGET_${upper_target}_RUNNER"
    if [ -n "${!env_runner_var:-}" ]; then
        echo "${!env_runner_var}"
        return 0
    fi

    # Auto-detect emulator based on host and target architecture/OS
    case "$target" in
        aarch64-unknown-linux-*)
            if command -v qemu-aarch64 >/dev/null 2>&1; then
                echo "qemu-aarch64"
                return 0
            elif command -v qemu-aarch64-static >/dev/null 2>&1; then
                echo "qemu-aarch64-static"
                return 0
            fi
            ;;
        x86_64-unknown-linux-*)
            if command -v qemu-x86_64 >/dev/null 2>&1; then
                echo "qemu-x86_64"
                return 0
            elif command -v qemu-x86_64-static >/dev/null 2>&1; then
                echo "qemu-x86_64-static"
                return 0
            fi
            ;;
        *windows*)
            if command -v wine >/dev/null 2>&1; then
                echo "wine"
                return 0
            elif command -v wine64 >/dev/null 2>&1; then
                echo "wine64"
                return 0
            fi
            ;;
        arm-unknown-linux-*|armv7-unknown-linux-*)
            if command -v qemu-arm >/dev/null 2>&1; then
                echo "qemu-arm"
                return 0
            elif command -v qemu-arm-static >/dev/null 2>&1; then
                echo "qemu-arm-static"
                return 0
            fi
            ;;
    esac

    return 1
}
