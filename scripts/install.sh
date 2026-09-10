#!/usr/bin/env bash
set -euo pipefail

# scripts/install.sh: build and install the canonical diffuse binary
# into the local user environment (~/.cargo/bin/diffuse) using the
# toolchain pinned in rust-toolchain.toml (single source of truth).
#
#   ./scripts/install.sh            # release binary (default)
#   PROFILE=dev ./scripts/install.sh  # debug binary (used by check.sh)
#   INSTALL_HARVEST=1 ./scripts/install.sh  # also install diffuse-harvest
#
# check.sh shells out to this script for the dev install so install logic
# lives in exactly one place.

TOOLCHAIN="$(sed -n 's/^[[:space:]]*channel[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p' rust-toolchain.toml)"
CARGO=(cargo)
if [ -n "$TOOLCHAIN" ]; then
  CARGO=(cargo "+$TOOLCHAIN")
fi

# diffuse-harvest is opt-in: the default install ships only the diffuse CLI.
BINS=(--bin diffuse)
if [ "${INSTALL_HARVEST:-}" = "1" ]; then
  BINS+=(--bin diffuse-harvest)
fi

PROFILE="${PROFILE:-release}"
if [ "$PROFILE" = "dev" ]; then
  "${CARGO[@]}" install --locked --debug --path crates/diffuse "${BINS[@]}" --force "$@"
  echo "diffuse binary installed to $(which diffuse) (dev)"
else
  "${CARGO[@]}" install --locked --path crates/diffuse "${BINS[@]}" --force "$@"
  echo "diffuse binary installed to $(which diffuse)"
fi
