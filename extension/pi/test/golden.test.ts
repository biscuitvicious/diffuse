/**
 * Golden-file regression test: run the full pipeline (parse → moves →
 * collapse → merge → render) on the frozen corpora (real captured diffs),
 * render the entire scrolldown of every file, and assert the output against
 * the locked "ideal" rendering for each.
 *
 * The corpora live at the workspace root (`tests/golden/`) — the single
 * canonical copy shared with the Rust acceptance tests. This file must never
 * fork its own fixture/golden pairs.
 *
 * UPDATE IS SURGICAL — the standard process for targeted changes:
 *   1. Run the test WITHOUT any env flag: it reports the exact failing
 *      rows (the change your code produced), per corpus.
 *   2. Review the report: the changed rows must be ONLY the region your
 *      change was supposed to touch.
 *   3. Re-run WITH the explicit expectation, naming exactly those rows
 *      (row indices are the golden file's line numbers, 0-based):
 *        UPDATE_GOLDEN=1 GOLDEN_EXPECT="734,735,736" node .../golden.test.ts
 *      (ranges work too: "734-740, 812")
 *      The update then verifies the changed set EQUALS the expected set —
 *      any row outside it (a regression elsewhere in the corpus) or any
 *      expected row that did NOT change fails WITHOUT writing, exit 1.
 *      Use GOLDEN_CORPUS=mirage-2025-08 to scope the update to one corpus.
 *
 * FULL REGENERATION (GEN_GOLDEN=1) — RESTRICTED USE ONLY:
 *   Use ONLY for large-scale structural changes (such as theme overhauls or
 *   reset code changes) where the golden output is already fully validated.
 *   GEN_GOLDEN=1 writes the full frame unconditionally without the surgical
 *   safety guard. Never use GEN_GOLDEN=1 for small, focused bug fixes or
 *   feature changes — use UPDATE_GOLDEN=1 with GOLDEN_EXPECT instead.
 *     GEN_GOLDEN=1 node .../golden.test.ts                 # every corpus
 *     GEN_GOLDEN=1 GOLDEN_CORPUS=mirage-2025-08 node .../golden.test.ts  # one
 *   Use only after reviewing the mismatch report row by row.
 *
 * TODO: move the golden generation into the Rust backend. The renderer's
 * TS harness is pathologically slow (~6s for the diffuse corpus — the node
 * TS transform + the per-row ReviewComponent walks dominate); the Rust
 * engine already produces the token stream, so a Rust-side render + golden
 * comparison (a new `diffuse --render-golden` subcommand over the same
 * `tests/golden/*.json` fixtures) would cut the regen cycle from seconds
 * to milliseconds and keep the golden-generation logic beside the engine
 * it exercises. The on-disk coverage walk stays (it pins the render to
 * the fixture's real content); only the render frame capture moves.
 *
 * Also walks the cursor through every visual line (the whole scrolldown),
 * asserting the width invariant and geometry consistency at each step — the
 * viewport never crashes and every row is reachable.
 */

import { reviewInternals as R, FG_RESET } from "../review.ts";
import { readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { visibleWidth } from "@earendil-works/pi-tui";

const here = dirname(fileURLToPath(import.meta.url));
const GOLDEN_DIR = join(here, "..", "..", "..", "tests", "golden");
const WIDTH = 90;

/** The frozen corpora: name → the fixture's file count. */
const CORPORA: { name: string; minFiles: number }[] = [
	{ name: "mirage-2025-08", minFiles: 8 },
	{ name: "diffuse-2025-08", minFiles: 26 },
	{ name: "opt-1-cache-regex-window-slice", minFiles: 5 },
	{ name: "opt-2-cache-markdown-regex", minFiles: 2 },
	{ name: "opt-3-cache-lang-profile", minFiles: 3 },
	{ name: "opt-4-manual-scanner-prefilter", minFiles: 4 },
];

// The goldens lock the status line — the walk renders with a FIXED stamp
// ("test") so the golden proves the stamp renders; install.sh stamps the
// real build (the golden stays stable because the value is fixed here).
process.env.REVIEW_BUILD = "test";
let failures = 0;
function assert(cond: boolean, msg: string) {
	if (!cond) {
		failures++;
		console.error("FAIL:", msg);
	}
}

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

const corpusWireMap = new Map<string, any>();

async function prepareAllCorporaWires() {
	const items = CORPORA.map((corpus) => {
		const FIXTURE = join(GOLDEN_DIR, corpus.name + ".json");
		return JSON.parse(readFileSync(FIXTURE, "utf8")) as {
			diff: string;
			contents: Record<string, string>;
		};
	});
	const wires = await (R as any).runDiffuseBinBatch(R.diffuseBinPath(), JSON.stringify(items));
	CORPORA.forEach((corpus, idx) => {
		corpusWireMap.set(corpus.name, wires[idx]!);
	});
}

/** Build the session files for the extension: feed the fixture to the
 * diffuse binary and convert its token-stream JSON. The golden gate is the
 * rust backend's byte-identical render. */
async function buildFiles(corpusName: string, fixture: { diff: string; contents: Record<string, string> }) {
	const wire = corpusWireMap.get(corpusName) ?? await R.runDiffuseBin(
		R.diffuseBinPath(),
		JSON.stringify({ diff: fixture.diff, contents: fixture.contents }),
	);
	const files = R.filesFromWire(
		wire,
		(p) => ({
			newContent: (fixture.contents[p] ?? "").split("\n"),
			oldContent: oldContentFromDiff(fixture.diff, p),
			oldPath: null,
		}),
		6,
	);
		return files;
}

/** Deterministic theme: every key the extension uses maps to a FIXED
 * 256-color code, so the golden captures the extension's own color choices —
 * which key tints which token, the hardcoded backgrounds, the strikes — and
 * a change to any of them breaks the golden. (The host's real theme values
 * are the pi side's business; the golden regression-tests the extension's
 * usage.) */
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
const encode = (s: string): string => s.replace(/\x1b\[([0-9;]*)m/g, (_, c: string) => `{${c}}`);

/** One corpus: build the files, render the full scrolldown, compare against
 * (or surgically update) the golden, and walk every visual line. */
async function walkCorpus(corpus: { name: string; minFiles: number }) {
	const FIXTURE = join(GOLDEN_DIR, corpus.name + ".json");
	const GOLDEN = join(GOLDEN_DIR, corpus.name + ".golden.txt");
	const fixture = JSON.parse(readFileSync(FIXTURE, "utf8")) as {
		diff: string;
		contents: Record<string, string>;
	};
	const files = await buildFiles(corpus.name, fixture);
	assert(files.length >= corpus.minFiles, `${corpus.name}: parses into ${corpus.minFiles}+ files, got ${files.length}`);

	const session = {
		repo: corpus.name === "mirage-2025-08" ? "mirage" : "diffuse",
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
	const theme2 = {
		fg: (c: string, t: string) => `\x1b[38;5;${COLOR[c] ?? 231}m${t}${FG_RESET}`,
		bg: (_c: string, t: string) => t,
	};
	const comp2 = new (R as any).ReviewComponent(
		session,
		{ requestRender: () => {}, stop: () => {}, start: () => {} },
		theme2,
		() => {},
	);
	const frame = comp2.render(WIDTH).map(encode); // raw lines: the SGR codes become the tags
	const actual = frame.join("\n") + "\n";

	const scopedUpdate = !process.env.GOLDEN_CORPUS || process.env.GOLDEN_CORPUS === corpus.name;
	// GEN_GOLDEN=1: regenerate the golden from scratch — write the full
	// frame unconditionally (no surgical guard, no prev-existence check).
	// Use after a deliberate, reviewed rendering change to bake the new
	// output in one pass. The on-disk coverage walk below still runs after,
	// so a regen that drops a row fails loudly. Pair with GOLDEN_CORPUS to
	// scope, or omit it to regen every corpus in one shot:
	//   GEN_GOLDEN=1 node --experimental-transform-types test/golden.test.ts
	if (process.env.GEN_GOLDEN && scopedUpdate) {
		writeFileSync(GOLDEN, actual);
		console.log(`${corpus.name}: golden regenerated (${actual.split("\n").length} rows): ${GOLDEN}`);
		// Fall through to the on-disk coverage walk (below) — a regen must
		// still pass it, so a dropped or fabricated row fails the run.
	}
	if (process.env.UPDATE_GOLDEN && scopedUpdate && !process.env.GEN_GOLDEN) {
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
					`${corpus.name}: GOLDEN_EXPECT VIOLATION — ${unexpected.length} changed row(s) OUTSIDE the expected set: ${unexpected.join(",")}`,
				);
				console.error(`  expected: ${[...expected].sort((x, y) => x - y).join(",")}`);
				console.error("  a regression elsewhere in the corpus — fix the code, do not bake it in.");
				process.exit(1);
			}
			if (missing.length > 0) {
				console.error(
					`${corpus.name}: GOLDEN_EXPECT — ${missing.length} expected row(s) did NOT change: ${missing.join(",")}`,
				);
				console.error("  the expectation is stale — update GOLDEN_EXPECT to the actual change set.");
				process.exit(1);
			}
			writeFileSync(GOLDEN, actual);
			console.log(
				`${corpus.name}: golden updated: ${changed.length} row(s) changed, exactly the expected set [${[...expected].sort((x, y) => x - y).join(",")}]: ${GOLDEN}`,
			);
		} else {
			console.error(
				`${corpus.name}: UPDATE_GOLDEN requires GOLDEN_EXPECT with the exact rows to change — run the test first,`,
			);
			console.error("read the failing rows from the mismatch report, then re-run with, e.g.:");
			console.error(`  UPDATE_GOLDEN=1 GOLDEN_CORPUS=${corpus.name} GOLDEN_EXPECT="734,735,736" node --experimental-transform-types test/golden.test.ts`);
			process.exit(1);
		}
	} else if (!process.env.GEN_GOLDEN) {
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
			console.error(`${corpus.name}: GOLDEN MISMATCH — first differences:`);
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
				`${corpus.name}: ${failures} failure(s) — regenerate with GEN_GOLDEN=1 only after reviewing the change.`,
			);
		}
	}

	// ── 2. The ON-DISK content walk: every rendered row must match the real
	// file's content (the corpus's statically captured files), and the rows
	// must COVER every line of each hunk without gaps. The width walk below
	// only proves the render is self-consistent — a dropped ctx line (the fn
	// signature, a chain continuation: `+ .layers` floating without
	// `let ro_match = self`) passes it, because the render is internally
	// uniform. This walk pins the render to the disk: a ctx/add/mod row's
	// text must equal the fixture's line at its newLine, a del row's text
	// must equal the reconstructed old line, and the rows' line numbers must
	// be contiguous within every hunk (nothing dropped, nothing fabricated).
	{
		const flat = (comp as any).flat as any[];
		const rowFile = (comp as any).rowFile as number[];
		const hunkStarts = (comp as any).hunkStarts as { flat: number; file: number }[];
		for (let i = 0; i < flat.length; i++) {
			const r = flat[i]!;
			if (r.kind === "file" || r.kind === "sep") continue;
			const file = files[rowFile[i]!]!;
			const newContent = (fixture.contents[file.path] ?? "").split("\n");
			const oldContent = oldContentFromDiff(fixture.diff, file.path);
			if (r.newLine !== null && (r.kind === "ctx" || r.kind === "add" || r.kind === "mod")) {
				// A mod's text can span several new lines (a stretch/add run
				// folded into one row) — each segment must equal the disk's
				// consecutive lines.
				const segs = r.text.split("\n");
				let ok = true;
				for (let j = 0; j < segs.length; j++) {
					if ((newContent[r.newLine - 1 + j] ?? "") !== segs[j]) {
						ok = false;
						break;
					}
				}
				if (!ok) {
					failures++;
					console.error(`${corpus.name}: ${file.path}:${r.newLine}: rendered ${r.kind} row differs from the file on disk`);
					console.error(`   disk: ${JSON.stringify(newContent[r.newLine - 1]?.slice(0, 60))}`);
					console.error(`   row : ${JSON.stringify(r.text.slice(0, 60))}`);
				}
			}
			if (r.newLine === null && r.oldLine !== null && (r.kind === "del" || r.kind === "ctx")) {
				// The old-side rows (the removed lines + a del-only hunk's
				// context) must match the old file line for line.
				const disk = oldContent[r.oldLine - 1] ?? "";
				if (r.text !== disk) {
					failures++;
					console.error(`${corpus.name}: ${file.path}:${r.oldLine}: rendered ${r.kind} row differs from the old file`);
					console.error(`   old: ${JSON.stringify(disk.slice(0, 60))}`);
					console.error(`   row: ${JSON.stringify(r.text.slice(0, 60))}`);
				}
			}
		}
		// The DEL PLACEMENT check: a removed line must render where it sat in
		// the file — its old line mapped through the hunk's new↔old delta —
		// never at the hunk's top. The filesystem.rs `.resolved_layers`
		// (old 741) must sit inside its hunk's new-side rows (between
		// `let ro_match = self` and `.layers`), not after the previous
		// method's `}`. Scoped to what is unambiguous: a del may not render
		// BEFORE the first new-side row of its own hunk (the hunk-top bug)
		// or after the last (the hunk-bottom bug). The moved/reflowed hunks
		// scramble the exact interpolation — the check does not chase that.
		{
			for (let hi = 0; hi < hunkStarts.length; hi++) {
				const s0 = hunkStarts[hi]!.flat;
				const e0 = hi + 1 < hunkStarts.length ? hunkStarts[hi + 1]!.flat : flat.length;
				// The hunk index within its file = the count of hunkStarts
				// for the same file before hi.
				const fi = hunkStarts[hi]!.file;
				let hunkIdx = 0;
				for (let k = 0; k < hi; k++) if (hunkStarts[k]!.file === fi) hunkIdx++;
				const h = files[fi]!.hunks[hunkIdx]!;
				// A del-only hunk (no add/mod rows of its own) renders its dels on
				// the OLD side with their old line numbers — the standard
				// deletion-block render (delta=0 on the old side). The check's
				// premise — a del mis-placed relative to its paired ADD on the
				// new side, mapped via `delta = newStart - oldStart` — does not
				// apply. Skip it; otherwise the next hunk's new-side lead-in
				// context (which bleeds into this hunk's flat slice, making
				// `firstNew` non-null) trips a false positive.
				if (!h.rows.some((r) => r.kind === "add" || r.kind === "mod")) continue;
				const delta = h.newStart - h.oldStart;
				let firstNew = -1;
				let lastNew = -1;
				for (let i = s0; i < e0; i++) {
					if (flat[i]!.newLine !== null) {
						if (firstNew < 0) firstNew = i;
						lastNew = i;
					}
				}
				if (firstNew < 0) continue; // a del-only hunk — no new-side rows
				for (let i = s0; i < e0; i++) {
					const r = flat[i]!;
					if (r.kind !== "del" || r.oldLine === null) continue;
					// The del's expected placement (its old line through the
					// hunk's delta). A del at the hunk's START whose old
					// position precedes the first new row renders before it
					// legitimately; the hunk-top bug is a del whose placement
					// is INSIDE the new-side rows but renders before them all.
					const p = r.oldLine + delta;
					if (i < firstNew && p > (flat[firstNew]!.newLine ?? 0)) {
						failures++;
						console.error(`${corpus.name}: ${files[rowFile[i]!]!.path}: del (old ${r.oldLine}, placement ${p}) renders before its hunk's first new-side row (${flat[firstNew]!.newLine}) — the hunk-top bug`);
					}
					if (i > lastNew && p <= (flat[lastNew]!.newLine ?? 0)) {
						failures++;
						console.error(`${corpus.name}: ${files[rowFile[i]!]!.path}: del (old ${r.oldLine}, placement ${p}) renders after its hunk's last new-side row (${flat[lastNew]!.newLine})`);
					}
				}
			}
		}

		// Coverage: within every hunk, the rendered rows' line numbers must be
		// contiguous — a missing line (the dropped in-hunk ctx) breaks this.
		for (let hi = 0; hi < hunkStarts.length; hi++) {
			const s = hunkStarts[hi]!.flat;
			// The coverage range stops at the next hunk OR the next FILE's
			// marker — a last-of-file hunk's rows must not absorb the next
			// file's lines (that produced false "missing" reports).
			let e = hi + 1 < hunkStarts.length ? hunkStarts[hi + 1]!.flat : flat.length;
			for (let i = s + 1; i < e; i++) {
				if (flat[i]!.kind === "file") {
					e = i;
					break;
				}
			}
			const nls: number[] = [];
			const seps: number[] = [];
			for (let i = s; i < e; i++) {
				const r = flat[i]!;
				if (r.kind === "sep") {
					seps.push(i);
					continue;
				}
				if (r.newLine !== null) {
					// A mod's multi-line text covers its whole span.
					nls.push(r.newLine);
					if (r.kind === "mod") {
						const segs = r.text.split("\n").length;
						for (let j = 1; j < segs; j++) nls.push(r.newLine + j);
					}
				}
			}
			// The collapsed hidden gaps: a sep hides the lines between the
			// rows around it (the `⋯ N lines hidden` row) — those lines are
			// legitimately absent, not dropped.
			const hidden: [number, number][] = [];
			for (const si of seps) {
				let prev = 0;
				let next = Number.POSITIVE_INFINITY;
				for (let i = s; i < si; i++) {
					if (flat[i]!.newLine !== null) prev = Math.max(prev, flat[i]!.newLine!);
				}
				for (let i = si + 1; i < e; i++) {
					if (flat[i]!.newLine !== null) {
						next = Math.min(next, flat[i]!.newLine!);
						break;
					}
				}
				if (prev + 1 <= next - 1) hidden.push([prev + 1, next - 1]);
			}
			const check = (nums: number[], side: string) => {
				if (nums.length < 2) return;
				const mn = Math.min(...nums);
				const mx = Math.max(...nums);
				for (let n = mn; n <= mx; n++) {
					if (nums.includes(n)) continue;
					if (hidden.some(([h0, h1]) => n >= h0 && n <= h1)) continue;
					failures++;
					console.error(`${corpus.name}: hunk ${hi}: ${side} line ${n} missing from the render (a dropped line)`);
				}
			};
			check(nls, "new");
		}
	}

	// ── 3. The scrolldown walk: every visual line, width + geometry invariant. ──
	Object.defineProperty(process.stdout, "rows", { value: 40, configurable: true });
	const seen = new Set<string>();
	const firstFlat = (comp as any).flat;
	seen.add(String((comp as any).rowIdx) + ":" + String((comp as any).visLine)); // initial position
	for (let step = 0; step < visTotal; step++) {
		comp.handleInput("j");
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
			console.error(`${corpus.name}: FAIL: flat row ${i} never reached by the scroll walk`);
		}
	}

	console.log(
		failures === 0
			? `${corpus.name}: GOLDEN OK (${visTotal} visual lines walked, ${firstFlat.length} flat rows covered)`
			: `${corpus.name}: ${failures} FAILURES`,
	);
}

await prepareAllCorporaWires();
await Promise.all(CORPORA.map((corpus) => walkCorpus(corpus)));
process.exit(failures === 0 ? 0 : 1);
