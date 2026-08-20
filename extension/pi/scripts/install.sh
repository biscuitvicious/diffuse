#!/bin/sh
# Install (or sync) the extension into Pi's auto-discovered extensions dir,
# with the bundled diffuse binary alongside it — the installed review.ts
# resolves bin/diffuse next to itself, so the copy works without any env.
# The review.ts build stamp (@BUILD@) is substituted with the git commit +
# date, so the status line shows which build pi is actually running (a
# process that didn't hot-reload shows the old stamp).
#
#   ./scripts/install.sh          # copy to ~/.pi/agent/extensions/
#   INSTALL_DIR=/custom ./scripts/install.sh
set -e
cd "$(dirname "$(readlink -f "$0")")/.."     # extension/pi
PROFILE=release ./scripts/build-bin.sh
DEST="${INSTALL_DIR:-$HOME/.pi/agent/extensions/diffuse-review}"
BUILD="$(git -C ../.. log -1 --format='%h %ad' --date=short 2>/dev/null || echo dev)"
# Remove old flat installation if it exists
if [ -z "$INSTALL_DIR" ]; then
	rm -rf "$HOME/.pi/agent/extensions/review.ts" "$HOME/.pi/agent/extensions/src" "$HOME/.pi/agent/extensions/bin"
fi
mkdir -p "$DEST/bin"
rm -rf "$DEST/src"
cp -r src "$DEST/"
sed "s/@BUILD@/$BUILD/g" review.ts > "$DEST/review.ts"
cp index.ts "$DEST/index.ts"
cp --remove-destination bin/diffuse "$DEST/bin/diffuse"
chmod +x "$DEST/bin/diffuse"
echo "installed: $DEST/review.ts + $DEST/index.ts + $DEST/src/ + $DEST/bin/diffuse (build $BUILD)"
echo "reload in pi: /reload, then /review"
