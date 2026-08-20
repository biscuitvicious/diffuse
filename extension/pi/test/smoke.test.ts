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

test("parseReviewArgs handles flags, paths, and cached", () => {
	const parsed = parseReviewArgs("--cached -- src/main.rs", process.cwd(), "");
	strictEqual(parsed.cached, true);
	ok(parsed.baseArgs.includes("--cached"));
	ok(parsed.paths.includes("src/main.rs"));
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
	const payload = {
		diff,
		contents: { "test.rs": "new\n" },
		oldContents: { "test.rs": "old\n" },
	};
	const wire = await runDiffuseBin(bin, JSON.stringify(payload));
	strictEqual(wire.files.length, 1);
	strictEqual(wire.files[0].path, "test.rs");
	strictEqual(wire.files[0].hunks.length, 1);
});
