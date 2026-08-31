#!/bin/sh
# Build the diffuse binary.  The binary stays in target/ — install.sh
# copies it from there directly into the extension destination, so there
# is no intermediate extension/pi/bin/ copy step.
#
#   ./scripts/build-bin.sh            # release (default — small, fast)
#   PROFILE=debug ./scripts/build-bin.sh
set -e
cd "$(dirname "$(readlink -f "$0")")/../../.." # the workspace root
PROFILE="${PROFILE:-release}"
TOOLCHAIN="$(sed -n 's/^[[:space:]]*channel[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p' rust-toolchain.toml)"
if [ -n "$TOOLCHAIN" ]; then
  CARGO="cargo +$TOOLCHAIN"
else
  CARGO=cargo
fi
if [ "$PROFILE" = "release" ]; then
	$CARGO build --release --bin diffuse
	echo "built: target/release/diffuse"
else
	$CARGO build --bin diffuse
	echo "built: target/debug/diffuse"
fi
