#!/usr/bin/env bash
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
if [ -n "$TOOLCHAIN" ] && command -v rustup >/dev/null 2>&1; then
  CARGO="cargo +$TOOLCHAIN"
else
  CARGO=cargo
fi
TARGET_FLAG=()
if [ -n "${TARGET:-}" ]; then
  TARGET_FLAG=(--target "$TARGET")
fi

if [ "$PROFILE" = "release" ]; then
	$CARGO build --quiet --release --bin diffuse "${TARGET_FLAG[@]}"
	echo "built: target/${TARGET:+$TARGET/}release/diffuse"
else
	$CARGO build --quiet --bin diffuse "${TARGET_FLAG[@]}"
	echo "built: target/${TARGET:+$TARGET/}debug/diffuse"
fi
