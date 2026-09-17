#!/bin/sh
# Install (or sync) the extension into Pi's auto-discovered extensions dir,
# with the diffuse binary alongside it — the installed review.ts resolves
# bin/diffuse next to itself, so the copy must live there.
# The binary is pulled directly from target/ (no intermediate extension/pi/bin/).
# The review.ts build stamp (@BUILD@) is substituted with the git commit +
# date, so the status line shows which build pi is actually running (a
# process that didn't hot-reload shows the old stamp).
#
#   ./scripts/install.sh          # copy to ~/.pi/agent/extensions/
#   INSTALL_DIR=/custom ./scripts/install.sh
set -e
cd "$(dirname "$(readlink -f "$0")")/../../.."   # workspace root
PROFILE="${PROFILE:-release}"
EXT_DIR="extension/pi"
PROFILE="$PROFILE" "$EXT_DIR/scripts/build-bin.sh"

cd "$EXT_DIR"
DEST="${INSTALL_DIR:-$HOME/.pi/agent/extensions/diffuse-review}"
EXT_PARENT="$(dirname "$DEST")"
if [ -z "${INSTALL_DIR-}" ] && [ ! -d "$EXT_PARENT" ]; then
	echo "Pi extensions directory not found at $EXT_PARENT; skipping Pi extension installation."
	exit 0
fi

BUILD="$(git log -1 --format='%h %ad' --date=short 2>/dev/null || echo dev)"

mkdir -p "$DEST/bin"
rm -rf "$DEST/src"
cp -r src "$DEST/"
sed "s/@BUILD@/$BUILD/g" review.ts > "$DEST/review.ts"
cp index.ts "$DEST/index.ts"

# Pull the binary from target/ directly — no intermediate bin/ copy.
TARGET_SUBDIR=""
if [ -n "${TARGET:-}" ]; then
	TARGET_SUBDIR="$TARGET/"
fi

if [ "$PROFILE" = "release" ]; then
	SRC_BIN="../../target/${TARGET_SUBDIR}release/diffuse"
else
	SRC_BIN="../../target/${TARGET_SUBDIR}debug/diffuse"
fi
if [ ! -f "$SRC_BIN" ] && [ -f "${SRC_BIN}.exe" ]; then
	SRC_BIN="${SRC_BIN}.exe"
fi
cp --remove-destination "$SRC_BIN" "$DEST/bin/diffuse"
chmod +x "$DEST/bin/diffuse"

echo "installed: $DEST/review.ts + $DEST/index.ts + $DEST/src/ + $DEST/bin/diffuse (build $BUILD)"
echo "reload in pi: /reload, then /review"
