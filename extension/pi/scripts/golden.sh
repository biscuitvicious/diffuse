#!/bin/sh
# Surgical golden update: the changed rows must equal GOLDEN_EXPECT exactly.
# First run ./scripts/test.sh and read the failing rows from the mismatch
# report, then re-run with the expectation, e.g.:
#   GOLDEN_EXPECT="734,735,736" ./scripts/golden.sh     (ranges: 734-740)
set -e
cd "$(dirname "$(readlink -f "$0")")/.."
if [ -z "$GOLDEN_EXPECT" ]; then
	echo "usage: GOLDEN_EXPECT='734,735,736' ./scripts/golden.sh   (ranges: 734-740)"
	echo "First run ./scripts/test.sh and read the failing rows from the mismatch report."
	exit 1
fi
sh scripts/build-bin.sh >/dev/null
UPDATE_GOLDEN=1 GOLDEN_EXPECT="$GOLDEN_EXPECT" node --experimental-transform-types test/golden.test.ts
echo "review the delta: git diff ../../tests/golden/"
