#!/bin/sh
# Uninstall the pi review extension from Pi's auto-discovered extensions
# dir, mirroring install.sh destinations (plus the legacy flat layout that
# install.sh cleans up).
#
#   ./scripts/uninstall.sh          # remove ~/.pi/agent/extensions/diffuse-review
#   INSTALL_DIR=/custom ./scripts/uninstall.sh
set -e
DEST="${INSTALL_DIR:-$HOME/.pi/agent/extensions/diffuse-review}"
rm -rf "$DEST"

if [ -z "${INSTALL_DIR-}" ]; then
	rm -rf "$HOME/.pi/agent/extensions/review.ts" \
	       "$HOME/.pi/agent/extensions/src" \
	       "$HOME/.pi/agent/extensions/bin"
fi

echo "diffuse-review extension uninstalled from $DEST"
