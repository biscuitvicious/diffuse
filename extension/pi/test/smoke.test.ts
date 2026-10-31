/**
 * Diffuse pi extension smoke test — verifies binary resolution & wire execution
 */

import { strictEqual, ok } from "node:assert";
import { existsSync } from "node:fs";
import test from "node:test";
import { diffuseBinPath, parseReviewArgs, runDiffuseBin } from "../src/backend.ts";

test("diffuse binary exists and resolves", () => {
	const bin = diffuseBinPath();
	ok(existsSync(bin), `diffuse binary must exist at ${bin}`);
});

test("parseReviewArgs handles flags, paths, and staged/cached", () => {
	const parsedStaged = parseReviewArgs("--staged -- src/main.rs", process.cwd(), "");
	strictEqual(parsedStaged.staged, true);
	strictEqual(parsedStaged.cached, true);
	ok(parsedStaged.baseArgs.includes("--staged"));
	ok(parsedStaged.paths.includes("src/main.rs"));

	const parsedCached = parseReviewArgs("--cached -- src/main.rs", process.cwd(), "");
	strictEqual(parsedCached.staged, true);
	strictEqual(parsedCached.cached, true);
	ok(parsedCached.paths.includes("src/main.rs"));
});

test("diffuse binary json wire contract smoke test", async () => {
	const bin = diffuseBinPath();
	const diff = `diff --git a/test.rs b/test.rs
index 0000000..1111111 100644
--- a/test.rs
+++ b/test.rs
@@ -1 +1 @@
-old
+new
`;
	const wire = await runDiffuseBin(bin, diff);
	strictEqual(wire.files.length, 1);
	strictEqual(wire.files[0].path, "test.rs");
	strictEqual(wire.files[0].hunks.length, 1);
});
