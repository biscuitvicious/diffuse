#!/usr/bin/env bash
set -euo pipefail

# scripts/install.sh: build and install the canonical diffuse binary
# into the local user environment (~/.cargo/bin/diffuse) using the
# toolchain pinned in rust-toolchain.toml (single source of truth).
#
#   ./scripts/install.sh                   # release binary (default)
#   PROFILE=dev ./scripts/install.sh       # debug binary (used by check.sh)
#   TARGET=... ./scripts/install.sh        # override target triple
#   INSTALL_HARVEST=1 ./scripts/install.sh # also install diffuse-harvest
#
# check.sh shells out to this script for the dev install so install logic
# lives in exactly one place.

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
. "$SCRIPT_DIR/target.sh"

HOST_TARGET="$(detect_host_target)"
TARGET="${TARGET:-${DIFFUSE_TARGET:-$(detect_best_target "$HOST_TARGET")}}"

echo "Target platform: $TARGET (host: $HOST_TARGET)"

# Ensure required toolchain and target are installed before performing operations
ensure_toolchain_and_target "$TARGET" false

CARGO=(cargo)
if [ -n "$PINNED_TOOLCHAIN" ]; then
  CARGO=(cargo "+$PINNED_TOOLCHAIN")
fi

# diffuse-harvest is opt-in: the default install ships only the diffuse CLI.
BINS=(--bin diffuse)
if [ "${INSTALL_HARVEST:-}" = "1" ]; then
  BINS+=(--bin diffuse-harvest)
fi

PROFILE="${PROFILE:-release}"
CARGO_INSTALL_ARGS=(install --locked --path crates/diffuse "${BINS[@]}" --target "$TARGET" --force)

if [ "$PROFILE" = "dev" ]; then
  CARGO_INSTALL_ARGS+=(--debug)
fi

"${CARGO[@]}" "${CARGO_INSTALL_ARGS[@]}" "$@"

# Determine installed binary path
BIN_NAME="diffuse"
case "$TARGET" in
  *windows*) BIN_NAME="diffuse.exe" ;;
esac
INSTALLED_BIN="$(command -v "$BIN_NAME" 2>/dev/null || echo "$HOME/.cargo/bin/$BIN_NAME")"
echo "diffuse binary installed to $INSTALLED_BIN ($PROFILE, $TARGET)"

# Only install the Pi extension if Pi is actually installed (target extensions folder exists)
PI_EXTENSIONS_DIR="${INSTALL_DIR:-$HOME/.pi/agent/extensions}"
if [ -d "$PI_EXTENSIONS_DIR" ] && [ -f "$WORKSPACE_ROOT/extension/pi/scripts/install.sh" ]; then
  echo "Pi extensions directory detected ($PI_EXTENSIONS_DIR). Syncing diffuse-review extension..."
  PROFILE="$PROFILE" TARGET="$TARGET" "$WORKSPACE_ROOT/extension/pi/scripts/install.sh"
else
  echo "Pi extensions directory not found at $PI_EXTENSIONS_DIR; skipping Pi extension install."
fi
