#!/bin/sh
# Build the diffuse binary and bundle it with the extension (bin/diffuse next
# to review.ts) — the extension ships with the engine it was tested against.
#
#   ./scripts/build-bin.sh            # release (default — small, fast)
#   PROFILE=debug ./scripts/build-bin.sh
set -e
cd "$(dirname "$(readlink -f "$0")")/../../.." # the workspace root
PROFILE="${PROFILE:-dev}"
TOOLCHAIN="$(sed -n 's/^[[:space:]]*channel[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p' rust-toolchain.toml)"
if [ -n "$TOOLCHAIN" ]; then
  CARGO="cargo +$TOOLCHAIN"
else
  CARGO=cargo
fi
if [ "$PROFILE" = "release" ]; then
	$CARGO build --release --bin diffuse
	cp --remove-destination target/release/diffuse extension/pi/bin/diffuse
else
	$CARGO build --bin diffuse
	cp --remove-destination target/debug/diffuse extension/pi/bin/diffuse
fi
echo "bundled: extension/pi/bin/diffuse ($PROFILE)"
