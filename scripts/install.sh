#!/usr/bin/env bash
set -euo pipefail

# scripts/install.sh: build and install the canonical diffuse binary
# into the local user environment (~/.cargo/bin/diffuse) using stable toolchain.

cargo +stable install --path crates/diffuse-cli --force
echo "diffuse binary installed to $(which diffuse)"
