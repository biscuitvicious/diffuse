#!/bin/sh
# Build the diffuse binary and bundle it with the extension (bin/diffuse next
# to review.ts) — the extension ships with the engine it was tested against.
#
#   ./scripts/build-bin.sh            # release (default — small, fast)
#   PROFILE=debug ./scripts/build-bin.sh
set -e
cd "$(dirname "$(readlink -f "$0")")/../../.." # the workspace root
PROFILE="${PROFILE:-dev}"
if [ "$PROFILE" = "release" ]; then
	cargo +stable build --release --bin diffuse
	cp target/release/diffuse extension/pi/bin/diffuse
else
	cargo +stable build --bin diffuse
	cp target/debug/diffuse extension/pi/bin/diffuse
fi
echo "bundled: extension/pi/bin/diffuse ($PROFILE)"
