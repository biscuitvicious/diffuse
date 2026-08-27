#!/usr/bin/env bash
set -euo pipefail

# scripts/install.sh: build and install the canonical diffuse binary
# into the local user environment (~/.cargo/bin/diffuse) using the
# toolchain pinned in rust-toolchain.toml (single source of truth).

TOOLCHAIN="$(sed -n 's/^[[:space:]]*channel[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p' rust-toolchain.toml)"
if [ -n "$TOOLCHAIN" ]; then
  cargo "+$TOOLCHAIN" install --locked --path crates/diffuse --force "$@"
else
  cargo install --locked --path crates/diffuse --force "$@"
fi
echo "diffuse binary installed to $(which diffuse)"
