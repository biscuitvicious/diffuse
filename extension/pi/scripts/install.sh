#!/bin/sh
# Install (or sync) the extension into Pi's auto-discovered extensions dir,
# with the bundled diffuse binary alongside it — the installed review.ts
# resolves bin/diffuse next to itself, so the copy works without any env.
#
#   ./scripts/install.sh          # copy to ~/.pi/agent/extensions/
#   INSTALL_DIR=/custom ./scripts/install.sh
set -e
cd "$(dirname "$(readlink -f "$0")")/.."     # extension/pi
DEST="${INSTALL_DIR:-$HOME/.pi/agent/extensions}"
if [ ! -f bin/diffuse ]; then
	echo "no bundled binary — run ./scripts/build-bin.sh first" >&2
	exit 1
fi
mkdir -p "$DEST/bin"
cp review.ts "$DEST/review.ts"
cp bin/diffuse "$DEST/bin/diffuse"
chmod +x "$DEST/bin/diffuse"
echo "installed: $DEST/review.ts + $DEST/bin/diffuse"
echo "reload in pi: /reload, then /review"
