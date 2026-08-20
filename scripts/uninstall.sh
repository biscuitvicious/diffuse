#!/usr/bin/env bash
set -euo pipefail

# scripts/uninstall.sh: remove diffuse from user environment.

cargo uninstall diffuse || rm -f ~/.cargo/bin/diffuse
echo "diffuse binary uninstalled from user environment."
