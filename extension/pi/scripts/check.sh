#!/bin/sh
# Syntax-check the extension without resolving imports (parse only).
set -e
cd "$(dirname "$(readlink -f "$0")")/.."
node --experimental-transform-types --check review.ts
echo "review.ts: syntax OK"
