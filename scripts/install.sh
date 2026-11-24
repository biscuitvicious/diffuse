#!/usr/bin/env bash
set -euo pipefail

# scripts/install.sh: build and install the canonical diffuse binary
# into the local user environment (~/.cargo/bin/diffuse) using the
# toolchain pinned in rust-toolchain.toml (single source of truth).
#
#   ./scripts/install.sh            # release binary (default)
#   PROFILE=dev ./scripts/install.sh  # debug binary (used by check.sh)
#   TARGET=... ./scripts/install.sh   # override target triple (e.g. musl static)
#   INSTALL_HARVEST=1 ./scripts/install.sh  # also install harvest
#
# Without an explicit TARGET (or DIFFUSE_TARGET), no --target flag is
# passed to cargo, so the build shares the warm artifact cache with
# manual cargo runs instead of rebuilding into target/<triple>/.
#
# check.sh shells out to this script for the dev install so install logic
# lives in exactly one place.

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
. "$SCRIPT_DIR/target.sh"

# A standalone install builds at low priority with a bounded job budget, so it
# does not monopolize the machine (see target.sh).
reexec_low_priority "$0" "$@"
shape_build_resources

HOST_TARGET="$(detect_host_target)"
TARGET="${TARGET:-${DIFFUSE_TARGET:-}}"

echo "Target platform: ${TARGET:-$HOST_TARGET (cargo default)} (host: $HOST_TARGET)"

# Ensure required toolchain and target are installed before performing operations
ensure_toolchain_and_target "$TARGET" false

CARGO=(cargo)
if [ -n "$PINNED_TOOLCHAIN" ] && command -v rustup >/dev/null 2>&1; then
  CARGO=(cargo "+$PINNED_TOOLCHAIN")
fi

# harvest is opt-in: the default install ships only the diffuse CLI.
BINS=(--bin diffuse)
if [ "${INSTALL_HARVEST:-}" = "1" ]; then
  BINS+=(--bin harvest)
fi

PROFILE="${PROFILE:-release}"
CARGO_INSTALL_ARGS=(install --quiet --locked --offline --path crates/diffuse "${BINS[@]}" --force)
if [ -n "$TARGET" ]; then
    CARGO_INSTALL_ARGS+=(--target "$TARGET")
fi

if [ "$PROFILE" = "dev" ]; then
  CARGO_INSTALL_ARGS+=(--debug)
fi

"${CARGO[@]}" "${CARGO_INSTALL_ARGS[@]}" "$@"

# Determine installed binary path
BIN_NAME="diffuse"
EFFECTIVE_TARGET="${TARGET:-$HOST_TARGET}"
case "$EFFECTIVE_TARGET" in
  *windows*) BIN_NAME="diffuse.exe" ;;
esac
INSTALLED_BIN="$(command -v "$BIN_NAME" 2>/dev/null || echo "$HOME/.cargo/bin/$BIN_NAME")"
echo "diffuse binary installed to $INSTALLED_BIN ($PROFILE, ${TARGET:-$HOST_TARGET (cargo default)})"

# Install the Pi extension (only if Pi is installed)
PROFILE="$PROFILE" TARGET="$TARGET" "$WORKSPACE_ROOT/extension/pi/scripts/install.sh"
