/**
 * Golden-file regression test: run the full pipeline (parse → moves →
 * collapse → merge → render) on a real captured diff (the mirage repo's
 * working tree at a point in time), render the entire scrolldown of every
 * file, and assert the output against the locked "ideal" rendering.
 *
 * The corpus is a frozen fixture (test/fixtures/mirage-2025-08.json) — NOT
 * the live repo — so the golden is stable.
 *
 * UPDATE IS SURGICAL — never a blanket regeneration:
 *   1. Run the test WITHOUT UPDATE_GOLDEN: it reports the exact failing
 *      rows (the change your code produced).
 *   2. Review the report: the changed rows must be ONLY the region your
 *      change was supposed to touch.
 *   3. Re-run WITH the explicit expectation, naming exactly those rows
 *      (row indices are the golden file's line numbers, 0-based):
 *        UPDATE_GOLDEN=1 GOLDEN_EXPECT="734,735,736" node .../golden.test.ts
 *      (ranges work too: "734-740, 812")
 *      The update then verifies the changed set EQUALS the expected set —
 *      any row outside it (a regression elsewhere in the corpus) or any
 *      expected row that did NOT change fails WITHOUT writing, exit 1.
 *   The golden diff is therefore always the exact intended change set,
 *   reviewed row by row.
 *
 * Also walks the cursor through every visual line (the whole scrolldown),
 * asserting the width invariant and geometry consistency at each step — the
 * viewport never crashes and every row is reachable.
 */

import { reviewInternals as R } from "../review.ts";
import { readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { visibleWidth } from "@earendil-works/pi-tui";

const here = dirname(fileURLToPath(import.meta.url));
// The frozen corpus lives at the workspace root (tests/golden/) — the single
// canonical copy shared with the Rust acceptance tests. This file must never
// fork its own fixture/golden pair.
const FIXTURE = join(here, "..", "..", "..", "tests", "golden", "mirage-2025-08.json");
const GOLDEN = join(here, "..", "..", "..", "tests", "golden", "mirage-2025-08.golden.txt");
const WIDTH = 90;

let failures = 0;
function assert(cond: boolean, msg: string) {
	if (!cond) {
		failures++;
		console.error("FAIL:", msg);
	}
}

const fixture = JSON.parse(readFileSync(FIXTURE, "utf8")) as {
	diff: string;
	contents: Record<string, string>;
};

// Reconstruct the old file from the diff (the context + `-` lines), so the
// brace-anchor pairing can read the del's fn on the old side.
function oldContentFromDiff(diff: string, path: string): string[] {
	const out: string[] = [];
	const lines = diff.split("\n");
	let cur: string | null = null;
	let oi = 0;
	for (const line of lines) {
		if (line.startsWith("diff --git ")) {
			cur = null;
			if (line.includes(path)) cur = "";
		} else if (cur === null) continue;
		else if (line.startsWith("@@ ")) {
			const m = /^@@ -(\d+)/.exec(line);
			oi = m ? Number(m[1]) : 1;
			while (out.length < oi - 1) out.push("");
		} else if (line.startsWith("-") && !line.startsWith("---")) {
			out[oi - 1] = line.slice(1);
			oi++;
		} else if (line.startsWith("+") && !line.startsWith("+++")) {
			// skip
		} else if (line.startsWith(" ") || line === "") {
			if (out.length >= oi - 1 && out[oi - 1] === undefined) out[oi - 1] = line.slice(1);
			oi++;
		}
	}
	return out.filter((l, i) => i < oi - 1 || l !== undefined);
}

/** Build the session files for the active backend. The TS leg runs the
 * extension's own engine (parse → moves → collapse); the rust leg (Phase 4's
 * equivalence gate) feeds the fixture to the diffuse binary and converts its
 * token-stream JSON — the renderer must produce the byte-identical golden. */
async function buildFiles() {
	if (process.env.REVIEW_BACKEND === "rust") {
		const wire = await R.runDiffuseBin(
			R.diffuseBinPath(),
			JSON.stringify({ diff: fixture.diff, contents: fixture.contents }),
		);
		// The engine's per-file classification must equal the TS's own
		// tokenizer over the same merged runs: the renderer re-derives the
		// colors with its own profile (the golden gate proves the STRUCTURE),
		// so this check pins the CLASSIFICATION to the reference.
		for (const wf of wire.files) {
			const ext = wf.path.includes(".") ? wf.path.split(".").pop() ?? "" : "";
			const profile = R.langProfile(ext);
			for (const wh of wf.hunks) {
				for (const wr of wh.rows) {
					// Coarse runs (merge adjacent same-kind), then classify each
					// coarse run with the file's profile — the wire runs must
					// equal that token stream exactly.
					const coarse: { kind: string; text: string }[] = [];
					for (const run of wr.runs) {
						const last = coarse[coarse.length - 1];
						if (last && last.kind === run.kind) last.text += run.text;
						else coarse.push({ kind: run.kind, text: run.text });
					}
					const expect: { kind: string; color: string; text: string }[] = [];
					for (const run of coarse) {
						if (run.kind === "del") {
							expect.push({ kind: "del", color: "text", text: run.text });
						} else {
							for (const t of R.classifyTokens(run.text, profile)) {
								expect.push({ kind: run.kind, color: t.color, text: t.text });
							}
						}
					}
					const got = wr.runs.map((r) => ({ kind: r.kind, color: r.color ?? "text", text: r.text }));
					assert(
						JSON.stringify(got) === JSON.stringify(expect),
						`${wf.path}:${wr.newLine ?? wr.oldLine}: wire runs differ from the TS per-file tokenization`,
					);
				}
			}
		}
		return R.filesFromWire(
			wire,
			(p) => ({
				newContent: (fixture.contents[p] ?? "").split("\n"),
				oldContent: oldContentFromDiff(fixture.diff, p),
				oldPath: null,
			}),
			6,
		);
	}
	const files = R.parseDiff(fixture.diff);
	for (const f of files) {
		const ext = f.path.includes(".") ? f.path.split(".").pop() ?? "" : "";
		const profile = R.langProfile(ext);
		R.markMoves(f);
		f.newContent = (fixture.contents[f.path] ?? "").split("\n");
		f.newLineCount = f.newContent.length;
		if (f.oldContent.length === 0) {
			f.oldContent = oldContentFromDiff(fixture.diff, f.path);
			f.oldLineCount = f.oldContent.length;
		}
		for (const h of f.hunks) h.rows = R.collapseHunkRows(h, profile, f.oldContent, f.newContent);
		f.hunks.forEach((h) => {
			h.aboveCtx = 6;
			h.belowCtx = 6;
		});
	}
	return files;
}

const files = await buildFiles();
assert(files.length >= 8, `fixture parses into 8+ files, got ${files.length}`);

const session = {
	repo: "mirage",
	repoRoot: "/tmp",
	files,
	notes: {},
	context: 6,
	base: { baseArgs: [], label: "HEAD", cached: false, paths: [] },
	branch: "work",
	git: async () => ({ stdout: "", stderr: "", code: 0 }),
};
const theme = {
	fg: (_c: string, t: string) => `\x1b[91m${t}\x1b[0m`,
	bg: (_c: string, t: string) => t,
};
Object.defineProperty(process.stdout, "columns", { value: WIDTH, configurable: true });
Object.defineProperty(process.stdout, "rows", { value: 40, configurable: true });

const comp = new (R as any).ReviewComponent(
	session,
	{ requestRender: () => {}, stop: () => {}, start: () => {} },
	theme,
	() => {},
);
const strip = (l: string) => l.replace(/\x1b\[[0-9;]*m/g, "");

// ── 1. The full-frame render: the whole scrolldown in one tall viewport. ──
const { total: visTotal } = comp.visualGeometry(WIDTH);
Object.defineProperty(process.stdout, "rows", { value: visTotal + 20, configurable: true });
// Deterministic theme: every key the extension uses maps to a FIXED
// 256-color code, so the golden captures the extension's own color choices —
// which key tints which token, the hardcoded backgrounds, the strikes — and
// a change to any of them breaks the golden. (The host's real theme values
// are the pi side's business; the golden regression-tests the extension's
// usage.)
const COLOR: Record<string, number> = {
	text: 231,
	muted: 244,
	accent: 214,
	border: 240,
	warning: 220,
	toolDiffRemoved: 167,
	syntaxKeyword: 141,
	syntaxString: 114,
	syntaxComment: 243,
	syntaxFunction: 117,
	syntaxOperator: 110,
	syntaxNumber: 179,
	syntaxType: 173,
};
const theme2 = {
	fg: (c: string, t: string) => `\x1b[38;5;${COLOR[c] ?? 231}m${t}\x1b[0m`,
	bg: (_c: string, t: string) => t,
};
const comp2 = new (R as any).ReviewComponent(
	session,
	{ requestRender: () => {}, stop: () => {}, start: () => {} },
	theme2,
	() => {},
);
/** Encode the ANSI SGR stream as readable tags: `\x1b[48;5;236m` →
 * `{48;5;236}`, `\x1b[9m` → `{9}`, `\x1b[0m` → `{0}`. Byte-exact for the
 * comparison, human-readable in diffs — a color change shows as a small
 * `{48;5;236}` → `{48;5;234}` hunk. */
const encode = (s: string): string => s.replace(/\x1b\[([0-9;]*)m/g, (_, c: string) => `{${c}}`);
const frame = comp2.render(WIDTH).map(encode); // raw lines: the SGR codes become the tags
const actual = frame.join("\n") + "\n";

if (process.env.UPDATE_GOLDEN) {
	// Surgical update: the changed set must EQUAL GOLDEN_EXPECT exactly.
	let prev = "";
	try {
		prev = readFileSync(GOLDEN, "utf8");
	} catch {
		/* first write */
	}
	const a = actual.split("\n");
	const g = prev.split("\n");
	const n = Math.max(a.length, g.length);
	const changed: number[] = [];
	for (let i = 0; i < n; i++) {
		if ((a[i] ?? "") !== (g[i] ?? "")) changed.push(i);
	}
	const expectRaw = process.env.GOLDEN_EXPECT;
	if (expectRaw) {
		const expected = new Set<number>();
		for (const part of expectRaw.split(",")) {
			const t = part.trim();
			if (!t) continue;
			const range = t.match(/^(\d+)-(\d+)$/);
			if (range) {
				for (let i = Number(range[1]); i <= Number(range[2]); i++) expected.add(i);
			} else if (/^\d+$/.test(t)) {
				expected.add(Number(t));
			}
		}
		const unexpected = changed.filter((i) => !expected.has(i));
		const missing = [...expected].filter((i) => !changed.includes(i));
		if (unexpected.length > 0) {
			console.error(
				`GOLDEN_EXPECT VIOLATION — ${unexpected.length} changed row(s) OUTSIDE the expected set: ${unexpected.join(",")}`,
			);
			console.error(`  expected: ${[...expected].sort((x, y) => x - y).join(",")}`);
			console.error("  a regression elsewhere in the corpus — fix the code, do not bake it in.");
			process.exit(1);
		}
		if (missing.length > 0) {
			console.error(
				`GOLDEN_EXPECT — ${missing.length} expected row(s) did NOT change: ${missing.join(",")}`,
			);
			console.error("  the expectation is stale — update GOLDEN_EXPECT to the actual change set.");
			process.exit(1);
		}
		writeFileSync(GOLDEN, actual);
		console.log(
			`golden updated: ${changed.length} row(s) changed, exactly the expected set [${[...expected].sort((x, y) => x - y).join(",")}]: ${GOLDEN}`,
		);
	} else {
		console.error(
			"UPDATE_GOLDEN requires GOLDEN_EXPECT with the exact rows to change — run the test first,",
		);
		console.error("read the failing rows from the mismatch report, then re-run with, e.g.:");
		console.error('  UPDATE_GOLDEN=1 GOLDEN_EXPECT="734,735,736" node --experimental-transform-types test/golden.test.ts');
		process.exit(1);
	}
} else {
	let golden: string;
	try {
		golden = readFileSync(GOLDEN, "utf8");
	} catch {
		golden = "";
	}
	if (actual !== golden) {
		failures++;
		const a = actual.split("\n");
		const g = golden.split("\n");
		const n = Math.max(a.length, g.length);
		let shown = 0;
		console.error("GOLDEN MISMATCH — first differences:");
		for (let i = 0; i < n && shown < 24; i++) {
			const ai = a[i] ?? "";
			const gi = g[i] ?? "";
			if (ai !== gi) {
				console.error(`  line ${i}:`);
				console.error(`    golden: ${JSON.stringify(gi.slice(0, 72))}`);
				console.error(`    actual: ${JSON.stringify(ai.slice(0, 72))}`);
				shown++;
			}
		}
		console.error(
			`${failures} failure(s) — regenerate with UPDATE_GOLDEN=1 only after reviewing the change.`,
		);
	}
}

// ── 2. The scrolldown walk: every visual line, width + geometry invariant. ──
Object.defineProperty(process.stdout, "rows", { value: 40, configurable: true });
const seen = new Set<string>();
const firstFlat = (comp as any).flat;
seen.add(String((comp as any).rowIdx) + ":" + String((comp as any).visLine)); // initial position
for (let step = 0; step < visTotal; step++) {
	comp.handleInput("j");
	const lines = comp.render(WIDTH);
	for (const l of lines) {
		const w = visibleWidth(l);
		if (w > WIDTH) {
			failures++;
			console.error(`FAIL: scroll step ${step} line exceeds width (${w} > ${WIDTH})`);
			break;
		}
	}
	seen.add(String((comp as any).rowIdx) + ":" + String((comp as any).visLine));
}
// The walk must cover every flat row at least once (no unreachable rows).
const flatReached = new Set(
	[...seen].map((s) => Number(s.split(":")[0])),
);
for (let i = 0; i < firstFlat.length; i++) {
	// File-marker rows are navigation landmarks, not cursor targets in the
	// j-walk (the cursor starts on the first real row and moves downward).
	if (firstFlat[i]!.kind === "file") continue;
	if (!flatReached.has(i)) {
		failures++;
		console.error(`FAIL: flat row ${i} never reached by the scroll walk`);
	}
}

console.log(
	failures === 0
		? `GOLDEN OK (${visTotal} visual lines walked, ${firstFlat.length} flat rows covered)`
		: `${failures} FAILURES`,
);
process.exit(failures === 0 ? 0 : 1);
