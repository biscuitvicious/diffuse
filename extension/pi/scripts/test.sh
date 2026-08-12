#!/bin/sh
# Run the unit suite + the golden regression compare (updates go through
# scripts/golden.sh with an explicit GOLDEN_EXPECT — never a blanket
# regeneration). The bundled bin is rebuilt first — the tests always run
# against the CURRENT engine (a stale bin silently tests the old one).
set -e
cd "$(dirname "$(readlink -f "$0")")/.."
sh scripts/build-bin.sh >/dev/null
node --experimental-transform-types test/review.test.ts
node --experimental-transform-types test/golden.test.ts
