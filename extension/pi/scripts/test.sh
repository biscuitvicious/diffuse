#!/bin/sh
# Run the unit suite + the golden regression compare (updates go through
# scripts/golden.sh with an explicit GOLDEN_EXPECT — never a blanket
# regeneration).
set -e
cd "$(dirname "$(readlink -f "$0")")/.."
node --experimental-transform-types test/review.test.ts
node --experimental-transform-types test/golden.test.ts
