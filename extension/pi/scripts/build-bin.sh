#!/bin/sh
# Build the diffuse binary and bundle it with the extension (bin/diffuse next
# to review.ts) — the extension ships with the engine it was tested against.
#
#   ./scripts/build-bin.sh            # release (default — small, fast)
#   PROFILE=debug ./scripts/build-bin.sh
set -e
cd "$(dirname "$(readlink -f "$0")")/../../.." # the workspace root
PROFILE="${PROFILE:-release}"
cargo build --profile "$PROFILE" --bin diffuse
mkdir -p extension/pi/bin
if [ "$PROFILE" = "release" ]; then
	cp target/release/diffuse extension/pi/bin/diffuse
else
	cp target/debug/diffuse extension/pi/bin/diffuse
fi
echo "bundled: extension/pi/bin/diffuse ($PROFILE)"
