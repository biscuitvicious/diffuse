// The extension's unit suite — now running against the DIFFUSE backend: every
// file/row is built by spawning the bundled binary (the TS engine is gone —
// the engine is tested in Rust). The renderer-side behaviors (shading, the
// merged view, movement, scrolling, notes, markdown) are what remain testable
// here; the golden walk (`golden.test.ts`) covers the full render of the
// frozen corpora.
import { reviewInternals as R } from "../review.ts";
import { visibleWidth } from "@earendil-works/pi-tui";

let failures = 0;
function assert(cond: boolean, msg: string) {
	if (!cond) {
		failures++;
		console.error("FAIL:", msg);
	}
}

// Reconstruct the old file from the diff (the context + `-` lines) — the
// renderer reads it for del-side context rows.
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

/** Run a diff through the bundled diffuse binary; return the DiffFiles. */
async function wireFiles(diff: string, contents: Record<string, string>): Promise<any[]> {
	const wire = await R.runDiffuseBin(R.diffuseBinPath(), JSON.stringify({ diff, contents }));
	return R.filesFromWire(
		wire,
		(p) => ({
			newContent: (contents[p] ?? "").split("\n"),
			oldContent: oldContentFromDiff(diff, p),
			oldPath: null,
		}),
		0,
	);
}

/** A single del→add pair's collapsed mod row, from the engine. */
async function wireRow(del: string, add: string, path = "f.rs"): Promise<any> {
	const diff = `diff --git a/${path} b/${path}\n--- a/${path}\n+++ b/${path}\n@@ -1 +1 @@\n-${del}\n+${add}\n`;
	const f = (await wireFiles(diff, { [path]: add }))[0]!;
	const rows = f.hunks[0]!.rows;
	const mod = rows.find((r) => r.kind === "mod");
	if (!mod) throw new Error(`wireRow: no mod for ${JSON.stringify(del)} → ${JSON.stringify(add)} (rows: ${rows.map((r) => r.kind).join(",")})`);
	return mod;
}

const session = (files: any[], context = 10, repo = "repo") => ({
	repo,
	repoRoot: "/tmp",
	files,
	notes: {},
	context,
	base: { baseArgs: [], label: "HEAD", cached: false, paths: [] },
	branch: "work",
	git: async () => ({ stdout: "", stderr: "", code: 0 }),
});
const theme = { fg: (_c: string, t: string) => `\x1b[91m${t}\x1b[0m`, bg: (_c: string, t: string) => t };
const comp = (files: any[], context = 10) =>
	new (R as any).ReviewComponent(session(files, context), { requestRender: () => {}, stop: () => {}, start: () => {} }, theme, () => {});

// ── parseReviewArgs ───────────────────────────────────────────────────────
const b1 = R.parseReviewArgs(undefined, "/tmp");
assert(b1.baseArgs.length === 0 && b1.label === "HEAD", "default base");
const b2 = R.parseReviewArgs("--cached", "/tmp");
assert(b2.cached && b2.label === "staged" && b2.baseArgs[0] === "--cached", "cached base");
const b3 = R.parseReviewArgs("main..HEAD -- src/x.rs", "/tmp");
assert(b3.baseArgs.join(" ") === "main..HEAD", "range base");
assert(b3.paths.join(" ") === "src/x.rs", "path filter");

// ── editorArgs patterns ───────────────────────────────────────────────────
const vim = R.editorArgs("vim", "/r/f.rs", 42, null)!;
assert(vim.argv.join(" ") === "vim +42 /r/f.rs", `vim open: ${vim.argv}`);
const vimR = R.editorArgs("vim", "/r/f.rs", 42, { start: 10, end: 14 })!;
assert(vimR.argv.join(" ") === "vim -c 10G -c normal! V4j /r/f.rs", `vim range: ${vimR.argv}`);
const zed = R.editorArgs("zed", "/r/f.rs", 42, null)!;
assert(zed.argv.join(" ") === "zed /r/f.rs:42", `zed: ${zed.argv}`);
assert(zed.detached === false, "zed not detached");
const code = R.editorArgs("code", "/r/f.rs", 7, null)!;
assert(code.argv.join(" ") === "code --goto /r/f.rs:7:1", `code: ${code.argv}`);
assert(code.detached === true, "code detached");
const unknown = R.editorArgs("kakoune", "/r/f.rs", 5, null)!;
assert(unknown.argv.join(" ") === "kakoune +5 /r/f.rs", `unknown fallback: ${unknown.argv}`);
const fresh = R.editorArgs("fresh", "/r/f.rs", 5, null)!;
assert(fresh.argv.join(" ") === "fresh /r/f.rs:5", `fresh positional: ${fresh.argv}`);
const antigravity = R.editorArgs("antigravity", "/r/f.rs", 5, null)!;
assert(antigravity.detached === true, "antigravity detached");

// ── splitTemplate quoting ────────────────────────────────────────────────
const t = R.splitTemplate(`git commit -m "hello world" -m 'x y'`);
assert(t.join("|") === "git|commit|-m|hello world|-m|x y", `splitTemplate: ${t}`);

// ── Line shading: green/red bg for diff rows, stronger fill on words ──────
{
	const f = (await wireFiles(`diff --git a/s.rs b/s.rs
--- a/s.rs
+++ b/s.rs
@@ -1,2 +1,4 @@
-old one
+new one
+
+extra
`, { "s.rs": "new one\n\nextra" }))[0]!;
	const c = comp([f]);
	const rows = c.render(120);
	const plain = (l: string) => l.replace(/\x1b\[[0-9;]*m/g, "");
	// mod row (old one → new one): a word swap — the replacement rule gives
	// it the NEUTRAL shade (236/237), not the addition green; the fills and
	// the strike carry the change. The cursor starts on it (visLine 0).
	const mod = rows.find((l: string) => plain(l).startsWith("   1 ~")) ?? "";
	assert(mod !== "", "mod row rendered");
	assert(mod.includes("\x1b[48;5;237m"), "cursor mod row lifts to 237 (neutral)");
	assert(mod.includes("\x1b[48;5;34m"), "cursor mod words lift to 34");
	assert(mod.includes("\x1b[9m"), "removed words struck inline");
	c.handleInput("j"); // → the mod's absorbed blank (still the mod's row)
	c.handleInput("j"); // → the pure add row
	const rows2 = c.render(120);
	const mod2 = rows2.find((l: string) => plain(l).startsWith("   1 ~")) ?? "";
	assert(mod2.includes("\x1b[48;5;236m") && mod2.includes("\x1b[48;5;28m"), "the ~ line stays grey with the added fill");
	const add = rows2.find((l: string) => plain(l).startsWith("   3 +")) ?? "";
	assert(add.includes("\x1b[48;5;28m"), "cursor add row lifts to 28");
	// every diff row carries a line shade (green, red, or neutral).
	const diffRows = rows2.filter((l: string) => /^\d+ [~+-]/.test(plain(l)));
	assert(diffRows.every((l: string) => /\x1b\[48;5;(22|28|52|88|235|236|237)m/.test(l)), `diff rows shaded: ${JSON.stringify(diffRows.map(plain))}`);
	// Width checks still hold.
	for (const w of [50, 80]) {
		for (const l of c.render(w)) assert(visibleWidth(l) <= w, `shading render(${w}) too wide`);
	}
}

// ── Smart window: a mod row centers on its off-screen change ──────────────
{
	const f = (await wireFiles(`diff --git a/m.rs b/m.rs
--- a/m.rs
+++ b/m.rs
@@ -1,1 +1,1 @@
-old shared prefix content
+new shared prefix with an appended change far beyond the view
`, { "m.rs": "new shared prefix with an appended change far beyond the view" }))[0]!;
	const mod = f.hunks[0]!.rows[0]!;
	assert(mod.kind === "mod", "pair collapsed");
	const c = comp([f]);
	// The rewritten line: the whole old line struck, then the whole new line
	// filled — wrapped, both with the mod's line-number prefix on their
	// first segment.
	const rendered = c.render(50);
	const plain = (l: string) => l.replace(/\x1b\[[0-9;]*m/g, "");
	const first = rendered.find((l: string) => plain(l).startsWith("   1 ~")) ?? "";
	assert(first !== "", "del block rendered with the prefix");
	assert(first.includes("\x1b[9m") && first.includes("old shared prefix"), "del block struck");
	const filled = rendered.find((l: string) => plain(l).includes("appended change")) ?? "";
	assert(filled !== "", "add block rendered");
	const all = rendered.map(plain).join("\n");
	assert(all.includes("appended"), "changed word visible via wrapping");
	assert(all.includes("beyond"), "wrapped tail content visible");
	assert(all.includes("old shared prefix"), "the removed content is visible inline");
	// The wrapped block spans multiple visual rows: an indented continuation
	// (no line-number prefix) carries the rest of the line.
	const wrappedRows = rendered.filter((l: string) => /^\s{3,}\S/.test(plain(l)));
	assert(wrappedRows.length >= 1, "continuation rows are indented");
	for (const w of [50, 80]) {
		for (const l of c.render(w)) assert(visibleWidth(l) <= w, `smart window render(${w}) too wide`);
	}
}

// ── Resume position resolution ─────────────────────────────────────────────
{
	const f = (await wireFiles(`diff --git a/z.rs b/z.rs
--- a/z.rs
+++ b/z.rs
@@ -5,1 +5,1 @@
-old
+new
`, { "z.rs": Array.from({ length: 20 }, (_, i) => (i === 4 ? "new" : `line${i + 1}`)).join("\n") }))[0]!;
	// Exact match → full restore.
	const full = R.resolveInitialPosition([f], { file: "z.rs", line: 5, col: 12, text: "new" });
	assert(full && full.fileIdx === 0 && full.line === 5 && full.col === 12, `full restore: ${JSON.stringify(full)}`);
	// Line moved/changed → same file, line null.
	const fallback = R.resolveInitialPosition([f], { file: "z.rs", line: 5, col: 12, text: "changed-content" });
	assert(fallback && fallback.fileIdx === 0 && fallback.line === null, `file fallback: ${JSON.stringify(fallback)}`);
	// File gone → null.
	const gone = R.resolveInitialPosition([f], { file: "gone.rs", line: 5, col: 0, text: "x" });
	assert(gone === null, "file gone → no restore");
}

// ── Render-width regression (the TUI hard-errors on oversize lines) ────────
{
	const f = (await wireFiles(`diff --git a/w.rs b/w.rs
--- a/w.rs
+++ b/w.rs
@@ -1,1 +1,1 @@
-old short
+${"x".repeat(120)} long changed line with plenty of words to highlight
`, { "w.rs": Array.from({ length: 30 }, (_, i) => `line ${i + 1} with some padding ${"z".repeat(60)}`).join("\n") }))[0]!;
	f.hunks[0]!.aboveCtx = 10;
	f.hunks[0]!.belowCtx = 10;
	const c = comp([f]);
	// Exercise a few state paths: cursor row, visual band, notes, help.
	c.handleInput("v");
	c.handleInput("j");
	c.handleInput("v");
	c.handleInput("m"); // no noteInput set → no-op
	(c as any).confirmed = { start: 1, end: 3, side: "new" };
	c.handleInput("?");
	for (const width of [50, 80, 120]) {
		const rendered = c.render(width);
		for (const line of rendered) {
			const w = visibleWidth(line);
			assert(w <= width, `render(${width}): line too wide (${w} > ${width}): ${JSON.stringify(line.slice(0, 60))}`);
		}
	}
	c.handleInput("?");
	for (const width of [50, 80, 120]) {
		const rendered = c.render(width);
		for (const line of rendered) {
			const w = visibleWidth(line);
			assert(w <= width, `normal render(${width}): line too wide (${w} > ${width})`);
		}
	}
}

// ── Infinite scroll: merged row list across files, selection per file ─────
{
	const makeFile = (path: string, hunkLine: number, n: number) => {
		const diff = `diff --git a/${path} b/${path}\n--- a/${path}\n+++ b/${path}\n@@ -${hunkLine},1 +${hunkLine},1 @@\n-old\n+new\n`;
		return wireFiles(diff, { [path]: Array.from({ length: n }, (_, i) => `line${i + 1}`).join("\n") }).then((fs) => fs[0]!);
	};
	const files = [await makeFile("a.rs", 5, 20), await makeFile("b.rs", 30, 60), await makeFile("c.rs", 3, 10)];
	files.forEach((f) => {
		f.hunks[0]!.aboveCtx = 0;
		f.hunks[0]!.belowCtx = 0;
	});
	const c = comp(files);
	// Three file markers + 3 base row pairs (each hunk = del+add, 0 context —
	// "old" → "new" share nothing, so they stay a del + an add).
	const rendered = c.render(200);
	const markers = rendered.filter((l) => /a\.rs|b\.rs|c\.rs/.test(l) && l.includes("────"));
	assert(markers.length === 3, `3 file markers, got ${markers.length}`);

	// Cursor starts on the first real row (file 0); scrolling past the file
	// boundary lands in file 1 (flat: [m0, del5, add5, m1, del30, add30, m2, del3, add3]).
	assert((c as any).rowIdx === 1, `starts on a real row, got ${(c as any).rowIdx}`);
	c.handleInput("j"); // → 2 (a.rs add)
	c.handleInput("j"); // → 3 (b.rs file marker)
	assert((c as any).fileIdx === 1, `cursor crossed into file 1, got ${(c as any).fileIdx}`);
	c.handleInput("j"); // → 4 (b.rs del)
	assert((c as any).fileIdx === 1, "in file 1");
	// Selection anchored in file 0, extended into file 1 → snaps to file 1.
	c.handleInput("k");
	c.handleInput("k");
	c.handleInput("v"); // anchor at 2 (file 0)
	c.handleInput("j");
	c.handleInput("j");
	c.handleInput("j"); // → 5 (b.rs add, file 1)
	c.handleInput("v"); // confirm
	const sel = (c as any).computeSelection();
	assert(sel !== null && sel.file === 1, `selection bounded to file 1: ${JSON.stringify(sel)}`);
	assert(sel.start === 30 && sel.end === 30, `selection in b.rs coords: ${JSON.stringify(sel)}`);
	// Render with the band — width check with the full-width stripe.
	for (const width of [50, 80]) {
		const r2 = c.render(width);
		for (const line of r2) {
			const w = visibleWidth(line);
			assert(w <= width, `infinite-scroll render(${width}): line too wide (${w} > ${width})`);
		}
	}
	// tab jumps to the next file (cursor at 5, file 1 → file 2 marker+1).
	c.handleInput("\t");
	assert((c as any).fileIdx === 2, `tab → next file, got ${(c as any).fileIdx}`);
	// n/p (and ]/[) walk hunks across files, wrapping cyclically.
	const first = (c as any).rowIdx; // 7 (c.rs del)
	c.handleInput("n"); // wrap → 1 (a.rs del)
	const second = (c as any).rowIdx;
	c.handleInput("n"); // → 4 (b.rs del)
	const third = (c as any).rowIdx;
	c.handleInput("p"); // → 1
	c.handleInput("]"); // → 4
	c.handleInput("["); // → 1
	c.handleInput("n"); // → 4
	c.handleInput("n"); // → 7
	const wrapped = (c as any).rowIdx;
	assert(second !== first && third !== second, "hunk nav moves across files");
	assert(wrapped === first, `hunk nav wraps (infinite), got ${wrapped} vs ${first}`);
	// Uppercase parity: N = next (like n), P = previous (like p).
	c.handleInput("P"); // → 4 (previous)
	assert((c as any).rowIdx === 4, `P = previous, got ${(c as any).rowIdx}`);
	c.handleInput("N"); // → 7 (next)
	assert((c as any).rowIdx === 7, `N = next, got ${(c as any).rowIdx}`);
	c.handleInput("N"); // → 1 (wraps)
	assert((c as any).rowIdx === 1, `N wraps to next, got ${(c as any).rowIdx}`);
}

// ── Mouse wheel + drag scrolling ──────────────────────────────────────────
{
	const diff = `diff --git a/a.rs b/a.rs
--- a/a.rs
+++ b/a.rs
@@ -5,1 +5,1 @@
-old
+new
`;
	const f = (await wireFiles(diff, { "a.rs": Array.from({ length: 200 }, (_, i) => `line${i + 1}`).join("\n") }))[0]!;
	f.hunks[0]!.aboveCtx = 10;
	f.hunks[0]!.belowCtx = 10;
	const c = comp([f]);
	const start = (c as any).rowIdx;
	c.handleInput("\x1b[<65;10;5M"); // wheel down
	assert((c as any).rowIdx === start + 3, `wheel down +3, got ${(c as any).rowIdx}`);
	c.handleInput("\x1b[<64;10;5M"); // wheel up
	assert((c as any).rowIdx === start, `wheel up -3, got ${(c as any).rowIdx}`);
	// Drag: press at row 10, move to row 13 (down 3) → view down 3 (later —
	// the drag follows the wheel convention, like a scroll gesture).
	c.handleInput("\x1b[<0;5;10M"); // press
	c.handleInput("\x1b[<35;5;13M"); // move down 3
	const afterDrag = (c as any).rowIdx;
	assert(afterDrag === start + 3, `drag +3, got ${afterDrag}`);
	c.handleInput("\x1b[<35;5;11M"); // move up 2
	assert((c as any).rowIdx === start + 1, `drag back -2, got ${(c as any).rowIdx}`);
	c.handleInput("\x1b[<0;5;11m"); // release
	c.handleInput("\x1b[<35;5;2M"); // move without press → ignored
	assert((c as any).rowIdx === start + 1, `move after release ignored, got ${(c as any).rowIdx}`);
	// Scroll clamps at top/bottom.
	for (let i = 0; i < 100; i++) c.handleInput("\x1b[<64;1;1M");
	assert((c as any).rowIdx === 0, "wheel-up clamps at top");

		// ── Horizontal scroll is DISABLED (the wrapped layout makes it pointless;
	//    it shifted the display and wrapped fragments appeared at col 0). ──
	c.handleInput("l");
	c.handleInput("l");
	c.handleInput("h");
	assert((c as any).colOffset === 0, `h/l are no-ops, colOffset stays 0, got ${(c as any).colOffset}`);
	// Drag-x panning is disabled too.
	c.handleInput("\x1b[<0;5;10M"); // press at col 5
	c.handleInput("\x1b[<32;15;10M"); // drag right 10 (button-1 held motion, mode 1002)
	assert((c as any).colOffset === 0, `drag-x panning disabled, got ${(c as any).colOffset}`);
	// Lazy anchor: motion without a prior press still scrolls vertically.
	c.handleInput("\x1b[<0;5;9m"); // release
	c.handleInput("\x1b[<35;10;9M"); // motion, no press → anchors first
	c.handleInput("\x1b[<35;12;9M"); // drag right 2
	assert((c as any).colOffset === 0, `lazy-anchor drag-x disabled, got ${(c as any).colOffset}`);
	// Vertical drag under 1002 (button 32) moves rows: dragging DOWN scrolls
	// the view DOWN (the wheel convention).
// Vertical drag under 1002 (button 32) moves rows: dragging DOWN scrolls
	// the view DOWN (the wheel convention).
	const beforeDrag = (c as any).rowIdx;
	c.handleInput("\x1b[<32;12;12M"); // drag down 3 (y 9 → 12)
	assert((c as any).rowIdx === beforeDrag + 3, `vertical 1002 drag: got ${(c as any).rowIdx}`);
	// The status shows the mouse diagnostic.
	const mouseStatus = c.render(120).find((l: string) => l.includes("mouse "));
	assert(!!mouseStatus && /mouse 32@12,12M/.test(mouseStatus.replace(/\x1b\[[0-9;]*m/g, "")), `mouse diag: ${mouseStatus}`);
	// The status shows no column indicator when colOffset is 0.
	const colStatus = c.render(120).find((l: string) => l.includes("col "));
	assert(!colStatus, `no col indicator without horizontal scroll: ${colStatus}`);
	for (const width of [50, 80]) {
		for (const line of c.render(width)) {
			assert(visibleWidth(line) <= width, `h-scroll render(${width}) too wide`);
		}
	}
	// Collapse with c → ctx indicator drops to 0; r resets to the default.
	c.handleInput("c"); // collapse upward
	c.handleInput("C"); // collapse downward
	const collapsed = c.render(120).find((l: string) => l.includes("ctx"));
	assert(!!collapsed && /ctx 0▴ 0▾/.test(collapsed), `collapsed ctx indicator: ${collapsed}`);
	c.handleInput("r");
	const reset = c.render(120).find((l: string) => l.includes("ctx"));
	assert(!!reset && /ctx 10▴ 10▾/.test(reset), `r restores default context: ${reset}`);
}

// ── ^/$ horizontal jumps + N+G line jump with context expansion ───────────
{
	const f = (await wireFiles(`diff --git a/l.rs b/l.rs
--- a/l.rs
+++ b/l.rs
@@ -1,1 +1,1 @@
-old
+new
@@ -100,1 +100,1 @@
-old2
+new2
`, { "l.rs": Array.from({ length: 200 }, (_, i) => `line ${i + 1} ${"x".repeat(80)}`).join("\n") }))[0]!;
	f.hunks.forEach((h: any) => { h.aboveCtx = 0; h.belowCtx = 0; }); // collapsed
	const c = comp([f], 6);

	// ^ / $ horizontal jumps are disabled — colOffset stays 0 (the wrapped
	// layout makes horizontal scrolling pointless).
	c.handleInput("l");
	c.handleInput("l");
	assert((c as any).colOffset === 0, `l l are no-ops, got ${(c as any).colOffset}`);
	c.handleInput("^");
	assert((c as any).colOffset === 0, "^ → col 0");
	c.handleInput("$");
	assert((c as any).colOffset === 0, "$ is a no-op");
	c.handleInput("a"); // expand context
	c.handleInput("$");
	assert((c as any).colOffset === 0, `$ stays 0, got ${(c as any).colOffset}`);
	c.handleInput("^");
	c.handleInput("h");
	assert((c as any).colOffset === 0, "h clamps at 0");

	// N+G: 5G lands on a real line (the hunk's context is expanded on demand).
	const before = (c as any).rowIdx;
	c.handleInput("5");
	c.handleInput("G");
	const after = (c as any).rowIdx;
	assert(after !== before, `5G moved the cursor, got ${after} vs ${before}`);
	const row = c.flat[after];
	assert(row && (row.newLine ?? row.oldLine) === 5, `5G landed on line 5: ${row?.newLine ?? row?.oldLine}`);
	// 150G: beyond the hunks — expands full context and lands on line 150.
	c.handleInput("1");
	c.handleInput("5");
	c.handleInput("0");
	c.handleInput("G");
	const r150 = c.flat[(c as any).rowIdx];
	assert(r150 && (r150.newLine ?? r150.oldLine) === 150, `150G landed on 150: ${r150?.newLine ?? r150?.oldLine}`);
	// The context got expanded (ctx indicator reflects full).
	const status = c.render(120).find((l: string) => l.includes("ctx"));
	assert(!!status && /ctx \d+▴ \d+▾/.test(status.replace(/\x1b\[[0-9;]*m/g, "")), "ctx indicator present");
	// Pending count clears on a non-count command.
	c.handleInput("7");
	c.handleInput("j");
	assert((c as any).numBuffer === "", "count clears after a non-count command");
	// Width checks hold at every stage.
	for (const w of [50, 80]) {
		for (const l of c.render(w)) assert(visibleWidth(l) <= w, `N+G render(${w}) too wide`);
	}
	// Default context is 6.
	assert(R.defaultContext() === 6, `default context 6, got ${R.defaultContext()}`);
}

// ── Inline merged view: common words + struck removed + filled added ──────
{
	const f = (await wireFiles(`diff --git a/i.rs b/i.rs
--- a/i.rs
+++ b/i.rs
@@ -1,1 +1,1 @@
-for layer in &self.resolved_layers {
+for layer in &self.layers {
`, { "i.rs": "for layer in &self.layers {" }))[0]!;
	const modRow = f.hunks[0]!.rows[0]!;
	assert(modRow.kind === "mod", "pair collapsed");
	Object.defineProperty(process.stdout, "columns", { value: 120, configurable: true });
	const c = comp([f]);
	const line = c.render(120).find((l: string) => l.replace(/\x1b\[[0-9;]*m/g, "").startsWith("   1 ~")) ?? "";
	const plain = line.replace(/\x1b\[[0-9;]*m/g, "");
	// The common words appear in the same line, around the inline removal.
	assert(plain.includes("for layer in &self."), `common words inline: ${plain}`);
	assert(plain.includes("layers {"), "replacement inline");
	// The removed word is struck at its position, between the common parts.
	const strikeIdx = line.indexOf("\x1b[9m");
	assert(strikeIdx > -1, "strike present");
	const before = line.slice(0, strikeIdx).replace(/\x1b\[[0-9;]*m/g, "");
	assert(before.includes("for layer in &self."), `removal sits after the common prefix: ${before}`);
	const delText = line.slice(strikeIdx).replace(/\x1b\[[0-9;]*m/g, "");
	assert(delText.startsWith("resolved_"), `the removed word is resolved_: ${delText}`);
	// Width check.
	for (const w of [50, 80]) {
		for (const l of c.render(w)) assert(visibleWidth(l) <= w, `merged render(${w}) too wide`);
	}
}

// ── Pure-deletion mods render neutral, not green ───────────────────────────
{
	// `pub(super)` → `pub`: the add spans are empty — nothing was added, so
	// the mod row must not carry the addition green. It takes the neutral
	// shade (like a moved line — the line survives) with the red strike
	// carrying the removal.
	const stubFile = { path: "f.rs", hunks: [] } as any;
	const c = new (R as any).ReviewComponent(
		session([stubFile]),
		{ requestRender: () => {}, stop: () => {}, start: () => {} },
		{ fg: (_c: string, t: string) => t, bg: (_c: string, t: string) => t },
		() => {},
	) as any;
	const pureDel = await wireRow("pub(super) struct ResolvedOverlayLayer {", "pub struct ResolvedOverlayLayer {");
	assert(c.isPureDeletionMod(pureDel), "pub(super)→pub is a pure deletion");
	const out = c.styleTokens(pureDel, c.mergedTokens(pureDel), 0, pureDel.text.length, "1 ~ ", false);
	const bgs = [...new Set([...out.matchAll(/\x1b\[48;5;(\d+)m/g)].map((x) => x[1]))];
	assert(bgs.includes("236") && !bgs.includes("22"), `pure-deletion mod is neutral, not green: ${bgs.join(",")}`);
	assert(bgs.includes("52"), `the struck word still carries the red: ${bgs.join(",")}`);
	// Every `~` mod renders the grey (even a pure insertion like a new
	// parameter) — the fill still carries the word.
	const withAdd = await wireRow("pub fn lookup(&self, key: &CacheKey) -> Option<ResolutionResult> {", "pub fn lookup(&self, key: &CacheKey, epoch: u64) -> Option<ResolutionResult> {");
	assert(!c.isPureDeletionMod(withAdd), "an added parameter is not a pure deletion");
	const out2 = c.styleTokens(withAdd, c.mergedTokens(withAdd), 0, withAdd.text.length, "1 ~ ", false);
	const bgs2 = [...new Set([...out2.matchAll(/\x1b\[48;5;(\d+)m/g)].map((x) => x[1]))];
	assert(bgs2.includes("236") && bgs2.includes("28") && !bgs2.includes("22"), `mods are grey with the added fill: ${bgs2.join(",")}`);
}

// ── Mod shading: every ~ row is the grey ───────────────────────────────────
{
	// All `~` mods render on the same slight grey as the moved / all-deleted
	// rows — a word swap (`.` → `,`), a pure deletion, even a pure insertion
	// like a new parameter: a modified line is never "new content". The
	// struck/filled words are the only color.
	const stubFile = { path: "f.rs", hunks: [] } as any;
	const c = new (R as any).ReviewComponent(
		session([stubFile]),
		{ requestRender: () => {}, stop: () => {}, start: () => {} },
		{ fg: (_c: string, t: string) => t, bg: (_c: string, t: string) => t },
		() => {},
	) as any;
	const bgs = (s: string) => [...new Set([...s.matchAll(/\x1b\[48;5;(\d+)m/g)].map((x) => x[1]))];
	const repl = await wireRow("    /// Resolved overlay layers (canonicalized, with TOCTOU anchors).", "    /// Resolved overlay layers (canonicalized, with TOCTOU anchors),");
	const sel = c.styleTokens(repl, c.mergedTokens(repl)!, 0, c.displayText(repl).length, "1 ~ ", true);
	const selBgs = bgs(sel);
	assert(selBgs.includes("237") && !selBgs.includes("22"), `mod selected is the neutral lift: ${selBgs.join(",")}`);
	assert(selBgs.includes("34") && selBgs.includes("52"), "the fill lifts and the strike stays");
	const unsel = c.styleTokens(repl, c.mergedTokens(repl)!, 0, c.displayText(repl).length, "1 ~ ", false);
	const unBgs = bgs(unsel);
	assert(unBgs.includes("236") && unBgs.includes("28"), `mod unselected: ${unBgs.join(",")}`);
	// The row must END on the line tint, not the last token's bg (the comma
	// is the last character — a trailing word-fill would paint the rest of
	// the terminal width as "added").
	const tail = sel.match(/\x1b\[48;5;(\d+)m$/);
	assert(tail && tail[1] === "237", `selected row ends on the line tint: ${JSON.stringify(sel.slice(-20))}`);
	const tail2 = unsel.match(/\x1b\[48;5;(\d+)m$/);
	assert(tail2 && tail2[1] === "236", `unselected row ends on the line tint: ${JSON.stringify(unsel.slice(-20))}`);
	// Even a pure insertion (a new parameter) is grey — the fill carries the
	// word, the row is not an addition.
	const addMod = await wireRow("pub fn lookup(&self, key: &CacheKey) -> Option<ResolutionResult> {", "pub fn lookup(&self, key: &CacheKey, epoch: u64) -> Option<ResolutionResult> {");
	const addSel = c.styleTokens(addMod, c.mergedTokens(addMod)!, 0, c.displayText(addMod).length, "1 ~ ", true);
	const addBgs = bgs(addSel);
	assert(addBgs.includes("237") && addBgs.includes("34") && !addBgs.includes("22"), `pure-insertion mods are grey too: ${addBgs.join(",")}`);
	// An add row still lifts its whole body.
	const a = { kind: "add" as const, text: "new line", newLine: 1, oldLine: null, moved: false };
	const aSel = c.styleTokens(a, c.syntaxTokens(a.text), 0, a.text.length, "1 + ", true);
	assert(bgs(aSel).includes("28") && !bgs(aSel).includes("22"), `add rows lift the whole body: ${bgs(aSel).join(",")}`);
}

// ── Syntax highlighting ────────────────────────────────────────────────────
{
	// Distinct fg codes per syntax color so the test can verify classification.
	const theme = {
		fg: (c: string, t: string) => {
			const code: Record<string, string> = {
				syntaxKeyword: "31", syntaxComment: "32", syntaxString: "33",
				syntaxNumber: "34", syntaxType: "35", syntaxFunction: "36",
				syntaxOperator: "37", text: "90",
			};
			return `\x1b[${code[c] ?? "90"}m${t}\x1b[0m`;
		},
		bg: (_c: string, t: string) => t, // identity: no background but keep the text
	};
	const rust = R.langProfile("rs");
	const raw = (t: string) => R.syntaxHighlight(t, rust, theme);
	assert(raw("fn main() {").includes("\x1b[31mfn\x1b[0m"), "fn keyword");
	assert(raw("let mut x = 42;").includes("\x1b[31mlet\x1b[0m") && raw("let mut x = 42;").includes("\x1b[31mmut\x1b[0m"), "let/mut keywords");
	assert(raw("// a comment").includes("\x1b[32m// a comment\x1b[0m"), "line comment");
	assert(raw("let s = \"hi\";").includes("\x1b[33m\"hi\"\x1b[0m"), "string");
	assert(raw("x = 0x1F;").includes("\x1b[34m0x1F\x1b[0m"), "hex number");
	assert(raw("Foo::new()").includes("\x1b[35mFoo\x1b[0m"), "capitalized type");
	assert(raw("call_it()").includes("\x1b[36mcall_it\x1b[0m"), "function call");
	assert(raw("->").includes("\x1b[37m-\x1b[0m"), "operator");
	assert(raw("plain").includes("\x1b[90mplain\x1b[0m"), "plain identifier");
	// A comment/string inside a longer line keeps the rest tokenized.
	const mixed = R.syntaxHighlight("fn f() { // note", rust, theme);
	assert(mixed.includes("\x1b[32m// note\x1b[0m") && mixed.includes("\x1b[31mfn\x1b[0m"), "comment after code");
	// Hash comments only for hashComment languages (python vs rust attrs).
	assert(R.syntaxHighlight("# derive", R.langProfile("py"), theme).includes("\x1b[32m"), "py hash comment");
	const rustAttr = R.syntaxHighlight("#[derive]", rust, theme);
	assert(!rustAttr.includes("\x1b[32m"), "rust # is not a comment");
	// Unknown language → plain text.
	assert(R.syntaxHighlight("anything", null, theme).includes("\x1b[90manything\x1b[0m"), "no profile → plain");
	// The component applies syntax colors to diff rows (stub fg codes flow).
	const f = (await wireFiles(`diff --git a/code.rs b/code.rs
--- a/code.rs
+++ b/code.rs
@@ -1,1 +1,1 @@
-old
+fn main() {
`, { "code.rs": "fn main() {" }))[0]!;
	const c = new (R as any).ReviewComponent(
		session([f]),
		{ requestRender: () => {}, stop: () => {}, start: () => {} },
		theme,
		() => {},
	);
	// "old" → "fn main() {" share nothing, so the similarity pairing leaves
	// them as a del + a pure add; the add row carries the syntax colors.
	const line = c.render(80).find((l: string) => l.replace(/\x1b\[[0-9;]*m/g, "").startsWith("   1 +")) ?? "";
	assert(line.includes("\x1b[31mfn\x1b[0m"), "diff row carries syntax colors");
	c.handleInput("t"); // toggle syntax off
	const plain2 = c.render(80).find((l: string) => l.replace(/\x1b\[[0-9;]*m/g, "").startsWith("   1 +")) ?? "";
	assert(!plain2.includes("\x1b[31mfn\x1b[0m"), "t disables syntax highlighting");
}

// ── Visual movement: j/k step wrapped lines for scroll continuity ─────────
{
	const long = `${"a shared prefix that continues across the wrap ".repeat(4)}`;
	const f = (await wireFiles(`diff --git a/l.rs b/l.rs
--- a/l.rs
+++ b/l.rs
@@ -1,1 +1,1 @@
-${long}OLD
+${long}NEW
@@ -10,1 +10,1 @@
-prefix old2
+prefix new2
`, { "l.rs": `${long}NEW\nprefix new2` }))[0]!;
	f.hunks.forEach((h: any) => {
		h.aboveCtx = 0;
		h.belowCtx = 0;
	});
	const c = comp([f]);
	const mod1 = c.flat.findIndex((r: any) => r.text.includes("shared prefix"));
	const mod2 = c.flat.findIndex((r: any) => r.text.includes("new2"));
	assert(mod1 > 0 && mod2 > mod1, `flat: ${c.flat.map((r: any) => r.kind).join(",")}`);
	Object.defineProperty(process.stdout, "columns", { value: 50, configurable: true });
	const segs = c.segCountOf(c.flat[mod1], 50);
	assert(segs >= 3, `long row wraps into ${segs} segments`);
	// j steps one visual line at a time through the wrapped row.
	c.handleInput("j");
	assert(c.rowIdx === mod1 && c.visLine === 1, `j → visLine 1: ${c.rowIdx}/${c.visLine}`);
	c.handleInput("j");
	assert(c.rowIdx === mod1 && c.visLine === 2, `j → visLine 2: ${c.rowIdx}/${c.visLine}`);
	// Exhaust the wrap → the next flat row (the separator between hunks).
	for (let i = c.visLine; i < segs - 1; i++) c.handleInput("j");
	c.handleInput("j");
	assert(c.rowIdx === mod1 + 1 && c.visLine === 0, `wrap exhausted → next row: ${c.rowIdx}/${c.visLine}`);
	// k re-enters the previous wrap at its tail.
	c.handleInput("k");
	assert(c.rowIdx === mod1 && c.visLine === segs - 1, `k → wrap tail: ${c.rowIdx}/${c.visLine}`);
	// Only the cursor's visual line lifts (its first bg is the selected
	// shade — 28 green / 237 neutral for the word-swap mod / 88 red).
	c.handleInput("g"); // top (file marker)
	c.handleInput("j");
	c.handleInput("j"); // wrapped row, visual line 2
	const rendered = c.render(50);
	const lifted2 = rendered.filter((l: string) => {
		const firstBg = l.match(/\x1b\[48;5;(\d+)m/);
		return firstBg?.[1] === "28" || firstBg?.[1] === "237" || firstBg?.[1] === "88";
	});
	assert(lifted2.length === 1, `exactly one visual line selected: ${lifted2.length}`);
	// Wheel steps visual lines too (3 per notch).
	c.handleInput("\x1b[<65;1;1M"); // wheel down
	assert(c.rowIdx === mod1 && c.visLine === 4, `wheel down +3 visual (clamped): ${c.rowIdx}/${c.visLine}`);
	c.handleInput("\x1b[<64;1;1M"); // wheel up
	assert(c.visLine === 1, `wheel up -3 visual: ${c.visLine}`);
	for (const w of [50, 80]) {
		for (const l of c.render(w)) assert(visibleWidth(l) <= w, `visual render(${w}) too wide`);
	}
}

console.log(failures === 0 ? "ALL PASS" : `${failures} FAILURES`);
process.exit(failures === 0 ? 0 : 1);
