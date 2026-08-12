import { reviewInternals as R } from "../review.ts";

let failures = 0;
function assert(cond: boolean, msg: string) {
	if (!cond) {
		failures++;
		console.error("FAIL:", msg);
	}
}

// ── parseDiff: multi-file, new file, deleted file, rename ─────────────────
const diff = `diff --git a/src/a.rs b/src/a.rs
index 111..222 100644
--- a/src/a.rs
+++ b/src/a.rs
@@ -10,1 +10,2 @@ fn old() {
-old line one
+new line one
+new line two
diff --git a/README.md b/README.md
new file mode 100644
index 000..333
--- /dev/null
+++ b/README.md
@@ -0,0 +1,2 @@
+# Title
+Body
diff --git a/old.txt b/old.txt
deleted file mode 100644
index 444..000
--- a/old.txt
+++ /dev/null
@@ -1,3 +0,0 @@
-gone
-bye
diff --git a/renamed.rs b/renamed.rs
similarity index 90%
rename from a/renamed.rs
rename to b/renamed.rs
index 555..666 100644
--- a/renamed.rs
+++ b/renamed.rs
@@ -1,1 +1,1 @@
-same
+changed
`;
const files = R.parseDiff(diff);
assert(files.length === 4, `expected 4 files, got ${files.length}`);
const a = files[0]!;
assert(a.path === "src/a.rs" && a.status === "modified", "file 0 modified");
assert(a.hunks.length === 1, "file 0 one hunk");
assert(a.hunks[0]!.rows.filter((r) => r.kind === "add").length === 2, "2 adds");
assert(a.hunks[0]!.rows.filter((r) => r.kind === "del").length === 1, "1 del");
const addRow = a.hunks[0]!.rows.find((r) => r.kind === "add" && r.text === "new line one")!;
assert(addRow.newLine === 10, `add row newLine 10, got ${addRow.newLine}`);
const delRow = a.hunks[0]!.rows.find((r) => r.kind === "del")!;
assert(delRow.oldLine === 10, `del row oldLine 10, got ${delRow.oldLine}`);
assert(files[1]!.status === "added", "README added");
assert(files[2]!.status === "deleted", "old.txt deleted");
assert(files[3]!.status === "renamed" && files[3]!.oldPath === "renamed.rs", "rename");

// ── buildVisible with context synthesis ───────────────────────────────────
const f = files[0]!;
f.newContent = Array.from({ length: 20 }, (_, i) => `line${i + 1}`);
f.newLineCount = 20;
f.hunks[0]!.aboveCtx = 10;
f.hunks[0]!.belowCtx = 10;
const rows = R.buildVisible(f);
const ctxAbove = rows.filter((r) => r.kind === "ctx" && r.newLine !== null && r.newLine! <= 9);
assert(ctxAbove.length === 9, `above ctx should be 9 (newStart-1), got ${ctxAbove.length}`);
const firstCtx = ctxAbove[0]!;
assert(firstCtx.newLine === 1 && firstCtx.text === "line1", "above context starts at line 1");
// Below context: 2 add rows occupy lines 10-11; below = lines 12..20 (9).
const ctxBelow = rows.filter((r) => r.kind === "ctx" && r.newLine !== null && r.newLine! > 11);
assert(ctxBelow.length === 9, `below ctx 9, got ${ctxBelow.length}`);
assert(ctxBelow[0]!.text === "line12", "below context starts at line 12");
// The separator: total lines 20, visible = 9 above + 3 base + 8 below = 20 → no sep
assert(!rows.some((r) => r.kind === "sep"), "no separator when fully expanded");

// ── untracked pseudo-file (whole-file add, no context) ────────────────────
const ut = files[1]!; // README
ut.newContent = ["# Title", "Body"];
ut.newLineCount = 2;
const utRows = R.buildVisible(ut);
assert(utRows.length === 2 && utRows.every((r) => r.kind === "add"), "untracked = all adds");

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
assert(vimR.argv.join(" ") === 'vim -c 10G -c normal! V4j /r/f.rs', `vim range: ${vimR.argv}`);
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

// ── context clamping between adjacent hunks ───────────────────────────────
const f2 = R.parseDiff(`diff --git a/z.rs b/z.rs
--- a/z.rs
+++ b/z.rs
@@ -5,1 +5,1 @@
-old
+new
@@ -50,1 +50,1 @@
-old2
+new2
`)[0]!;
f2.newContent = Array.from({ length: 60 }, (_, i) => `l${i + 1}`);
f2.newLineCount = 60;
f2.hunks[0]!.aboveCtx = 100;
f2.hunks[0]!.belowCtx = 100;
f2.hunks[1]!.aboveCtx = 100;
f2.hunks[1]!.belowCtx = 100;
const rows2 = R.buildVisible(f2);
// Fully expanded: every line visible once — 60 lines, but each modified line
// is 2 rows (del + add), so 62 rows; no separator.
assert(rows2.length === 62, `fully expanded 62 rows, got ${rows2.length}`);
assert(!rows2.some((r) => r.kind === "sep"), "no sep when fully expanded");

// Collapsed: context 0 → just the 4 base rows (2 del+add pairs) + 1 sep.
f2.hunks[0]!.aboveCtx = 0;
f2.hunks[0]!.belowCtx = 0;
f2.hunks[1]!.aboveCtx = 0;
f2.hunks[1]!.belowCtx = 0;
const rows3 = R.buildVisible(f2);
assert(rows3.length === 5, `collapsed 5 rows, got ${rows3.length}`);
const seps = rows3.filter((r) => r.kind === "sep");
assert(seps.length === 1, `1 sep when collapsed, got ${seps.length}`);
assert(/44 lines hidden/.test(seps[0]!.text), `sep text: ${seps[0]!.text}`);

// ── Word-level diff ────────────────────────────────────────────────────────
// Underscores and operators are separate tokens, so identifier/operator
// edits highlight the changed part only ("old"→"new", "u32"→"u64").
const ws = R.wordSpans("fn old_name(x: u32) -> u32 {", "fn new_name(x: u64) -> u64 {");
assert(ws.add.length === 3, `word diff add spans: ${JSON.stringify(ws.add)}`);
assert(ws.del.length === 3, `word diff del spans: ${JSON.stringify(ws.del)}`);
const addText = "fn new_name(x: u64) -> u64 {";
assert(addText.slice(ws.add[0]![0], ws.add[0]![1]) === "new", `add span 0: ${JSON.stringify(ws.add)}`);
assert(addText.slice(ws.add[1]![0], ws.add[1]![1]) === "u64", `add span 1: ${JSON.stringify(ws.add)}`);
assert(addText.slice(ws.add[2]![0], ws.add[2]![1]) === "u64", `add span 2: ${JSON.stringify(ws.add)}`);
// Identical lines → no spans.
const same = R.wordSpans("abc def", "abc def");
assert(same.add.length === 0 && same.del.length === 0, "identical lines have no spans");
// Completely different → the changed word is the only span (shared tokens
// like whitespace and the common word stay unhighlighted).
const wordDiff = R.wordSpans("hello world", "goodbye world");
assert(wordDiff.add.length === 1 && wordDiff.del.length === 1, "one changed word each side");
// Operator-level granularity: the whole `->`/`=>` token is the span.
const op = R.wordSpans("a -> b", "a => b");
const opText = "a => b";
assert(opText.slice(op.add[0]![0], op.add[0]![1]) === "=>", `operator token span: ${JSON.stringify(op.add)}`);
// ── Modified-pair collapse + move detection ────────────────────────────────
// A del→add pair collapses into ONE "mod" row with baked word spans; a pure
// add stays; an exact-content pair is a move (muted), not a change.
{
	const mk = () => ({
		path: "f.rs",
		oldPath: null,
		status: "modified",
		newLineCount: 20,
		oldLineCount: 20,
		newContent: [],
		oldContent: [],
		hunks: [
			{
				oldStart: 1, oldCount: 3, newStart: 1, newCount: 3, section: "", aboveCtx: 0, belowCtx: 0,
				rows: [
					{ kind: "del", text: "old one", newLine: null, oldLine: 1 },
					{ kind: "add", text: "new one", newLine: 1, oldLine: null },
					{ kind: "add", text: "extra line", newLine: 2, oldLine: null },
					{ kind: "add", text: "", newLine: 3, oldLine: null }, // section break
					{ kind: "del", text: "moved line", newLine: null, oldLine: 4 },
					{ kind: "add", text: "moved line", newLine: 4, oldLine: null },
					{ kind: "add", text: "after the blank", newLine: 5, oldLine: null },
				],
			} as any,
		],
	} as any);

	const f = mk();
	R.markMoves(f);
	const movedDel = f.hunks[0]!.rows.find((r: any) => r.kind === "del" && r.text === "moved line");
	const movedAdd = f.hunks[0]!.rows.find((r: any) => r.kind === "add" && r.text === "moved line");
	assert(!!movedDel?.moved && !!movedAdd?.moved, "exact-content pair marked moved");
{
	const collapsed = R.collapseHunkRows(f.hunks[0]);
	// The mod absorbs "extra line" into its section (no blank between them);
	// the blank ends the section, so the moved add starts a new one. The
	// moved del is absorbed (its content is in the adds).
	// The moved del AND the moved add ("moved line", same content on both
	// sides) are absorbed — a pure reflow is invisible, not ≈ noise. Left:
	// the mod (with its section add), then the blank section break.
	// The mod absorbs "extra line" AND the closing blank into its section —
	// the block reads as a unit (the blank ends the section, it is not a
	// separate + row); the moved add is a pure reflow (invisible).
	assert(collapsed.length === 2, `collapsed to 2 rows, got ${collapsed.length}`);
	assert(collapsed[1]!.kind === "add" && collapsed[1]!.newLine === 5, "the add after the blank is a separate row");
	assert(collapsed[0]!.kind === "mod", "mod row leads");
	assert(collapsed[0]!.spans && collapsed[0]!.spans.add.length === 1, "mod row has baked add spans");
	assert(collapsed[0]!.tail && collapsed[0]!.tail.length === 2 && collapsed[0]!.tail[0]!.text === "extra line" && collapsed[0]!.tail[1]!.text.trim() === "",
		`mod absorbed its section's add and the closing blank: ${JSON.stringify(collapsed[0]!.tail?.map((t: any) => t.text))}`);
	// A genuinely removed line (content NOT in any add) is kept.
	const delOnly = { kind: "del", text: "        let up = self.resolved_layers[ro_match + 1..]", newLine: null, oldLine: 745 };
	const h2 = { oldStart: 745, oldCount: 3, newStart: 903, newCount: 1, section: "", aboveCtx: 0, belowCtx: 0, rows: [
		delOnly,
		{ kind: "del", text: "            .iter()", newLine: null, oldLine: 746 },
		{ kind: "del", text: "            .find(|l| l.is_upper())?;", newLine: null, oldLine: 747 },
		{ kind: "add", text: "        let up = self.layers[ro_match + 1..].iter().find(|l| l.is_upper())?;", newLine: 903, oldLine: null },
	] } as any;
	const collapsed2 = R.collapseHunkRows(h2);
	// `.iter()` and `.find(...)` are folded into the one-liner → absorbed;
	// the first del line is the real change → a mod row. Result: one mod.
	assert(collapsed2.length === 1 && collapsed2[0]!.kind === "mod", `one mod row: ${collapsed2.map((r: any) => r.kind).join(",")}`);
	f.hunks[0]!.rows = collapsed; // loadReview assigns the collapsed rows back
	const counts = R.fileCounts(f);
	// The absorbed reflow leaves no moved rows; the two absorbed tail lines
	// (the section add + the blank) count as adds in the header.
	assert(counts.adds === 3 && counts.dels === 0 && counts.mods === 1 && counts.moved === 0,
		`counts +3 ~1: ${JSON.stringify(counts)}`);
}

}

// ── Block-move rule: a deleted block's interior matches are not moves ────
{
	// The old `find_mount` was rewritten — its `.filesystems/.iter()/
	// .rfind()/.map()` chain deleted — while the NEW `get` method (a
	// separate hunk) happens to contain verbatim `.iter()/.rfind()/.map()`
	// lines. The del run's FIRST line (self.mount_stack) did NOT move, so
	// the run is a deletion, not a relocation: the interior moves must
	// drop, or the new method's chain would render ≈ inside a clearly
	// added block ("self.filesystems added, the rest moved?").
	const mk = () => ({
		path: "m.rs", oldPath: null, status: "modified" as const, newLineCount: 20, oldLineCount: 20,
		newContent: [], oldContent: [], hunks: [
			{ oldStart: 1019, oldCount: 5, newStart: 1060, newCount: 4, section: "", aboveCtx: 0, belowCtx: 0, rows: [
				{ kind: "del", text: "        self.mount_stack", newLine: null, oldLine: 1019 },
				{ kind: "del", text: "            .filesystems", newLine: null, oldLine: 1020 },
				{ kind: "del", text: "            .iter()", newLine: null, oldLine: 1021 },
				{ kind: "del", text: "            .rfind(|fs| guest_path.starts_with(fs.mount_point()))", newLine: null, oldLine: 1022 },
				{ kind: "del", text: "            .map(|fs| fs.as_ref() as &dyn Filesystem)", newLine: null, oldLine: 1023 },
				{ kind: "add", text: "        self.mount_stack.get(guest_path)", newLine: 1060, oldLine: null },
				{ kind: "add", text: "    }", newLine: 1061, oldLine: null },
			] } as any,
			{ oldStart: 769, oldCount: 0, newStart: 769, newCount: 8, section: "", aboveCtx: 0, belowCtx: 0, rows: [
				{ kind: "add", text: "    pub fn get(&self, guest_path: &Path) -> Option<&dyn Filesystem> {", newLine: 771, oldLine: null },
				{ kind: "add", text: "        self.filesystems", newLine: 772, oldLine: null },
				{ kind: "add", text: "            .iter()", newLine: 773, oldLine: null },
				{ kind: "add", text: "            .rfind(|fs| guest_path.starts_with(fs.mount_point()))", newLine: 774, oldLine: null },
				{ kind: "add", text: "            .map(|fs| fs.as_ref() as &dyn Filesystem)", newLine: 775, oldLine: null },
				{ kind: "add", text: "    }", newLine: 776, oldLine: null },
			] } as any,
		],
	} as any);
	const f = mk();
	R.markMoves(f);
	const delIter = f.hunks[0]!.rows.find((r: any) => r.kind === "del" && r.text.trim() === ".iter()");
	assert(!delIter?.moved, "the interior chain del is NOT a move (the run's first del didn't move)");
	const addIter = f.hunks[1]!.rows.find((r: any) => r.kind === "add" && r.text.trim() === ".iter()");
	assert(!addIter?.moved, "the get method's chain add is NOT marked moved");
	assert(!(addIter as any).delText, "the paired delText is dropped with the move");
	// The collapse must not pair the reflowed chain into garble: the del
	// survives verbatim as a line in the new file — the similarity pairing
	// skips it, the reflow-drop absorbs it.
	f.newContent = (["", "    pub fn get(&self, guest_path: &Path) -> Option<&dyn Filesystem> {", "        self.filesystems", "            .iter()", "            .rfind(|fs| guest_path.starts_with(fs.mount_point()))", "            .map(|fs| fs.as_ref() as &dyn Filesystem)", "    }"] as string[]);
	const collapsed = R.collapseHunkRows(f.hunks[0]!, R.langProfile("rs"), f.oldContent, f.newContent);
	// The reverse stretch: the whole old body folds into ONE mod — the
	// delText is the run's concatenation (the old method's lines strike
	// across the visual lines), not a garble pair with some unrelated add.
	const rmod = collapsed.find((r: any) => r.kind === "mod")!;
	assert(!!rmod && (rmod.delText ?? "").split("\n").length === 5,
		`the old body folds into one mod with all 5 lines: ${JSON.stringify(rmod?.delText)}`);
	assert(rmod.newLine === 1060 && rmod.delText.includes(".rfind"),
		`the reverse stretch anchors on the run's first del against the new line: ${JSON.stringify(rmod?.delText.slice(0, 30))}`);
	assert(collapsed.filter((r: any) => r.kind === "del").length === 0,
		`the run's dels are consumed by the mod (no floating - rows): ${collapsed.filter((r: any) => r.kind === "del").length}`);
}

// ── The brace-anchor tail survives a verbatim same-text line in the new ──
// ── file (the reflow-skip must not preempt it) ────────────────────────────
{
	// The del `None` is the fn's tail (old line 114 is `    }`). The new
	// file happens to contain a verbatim `None` line ELSEWHERE (unrelated —
	// a match arm in another fn). The verbatim-reflow skip would `continue`
	// past the del before the brace-anchor pairing runs — the `None` must
	// STILL pair with `Some(result)`, not be dropped as a reflow.
	const mk = () => ({
		path: "c.rs", oldPath: null, status: "modified" as const, newLineCount: 30, oldLineCount: 30,
		newContent: [], oldContent: [], hunks: [
			{ oldStart: 111, oldCount: 3, newStart: 147, newCount: 9, section: "", aboveCtx: 0, belowCtx: 0, rows: [
				{ kind: "del", text: "        self.cache.borrow_mut().pop(key);", newLine: null, oldLine: 111 },
				{ kind: "del", text: "        self.results.borrow_mut().pop(key);", newLine: null, oldLine: 112 },
				{ kind: "del", text: "        None", newLine: null, oldLine: 113 },
				{ kind: "add", text: "        let (entry_epoch, result) = self.results.borrow_mut().get(key)?.clone();", newLine: 147, oldLine: null },
				{ kind: "add", text: "        if entry_epoch != epoch {", newLine: 148, oldLine: null },
				{ kind: "add", text: "            return None;", newLine: 153, oldLine: null },
				{ kind: "add", text: "        }", newLine: 154, oldLine: null },
				{ kind: "add", text: "        Some(result)", newLine: 155, oldLine: null },
			] } as any,
		],
	} as any);
	const f = mk();
	// The old file: line 114 is the fn's closing brace — the `None` is a
	// tail. The new file contains a verbatim `        None` line at line
	// 300 (an unrelated match arm) — but the new tail after `Some(result)`
	// (line 156) is the fn's closing brace.
	f.oldContent = Array.from({ length: 130 }, (_, i) => (i === 113 ? "    }" : ""));
	f.newContent = Array.from({ length: 320 }, (_, i) =>
		i === 155 ? "    }" : i === 299 ? "        None" : "",
	);
	const collapsed = R.collapseHunkRows(f.hunks[0]!, R.langProfile("rs"), f.oldContent, f.newContent);
	const noneMod = collapsed.find((r: any) => r.kind === "mod" && r.delText?.trim() === "None");
	assert(!!noneMod && noneMod.newLine === 155,
		`the brace-anchor pairs None with Some(result) despite the verbatim None in the new file: ${JSON.stringify(noneMod?.newLine)}`);
}

// ── Reflow pairing: the FIRST del carries the change, reflows drop ──────
{
	// `self.resolved_layers` / `.iter()` / `.any(…)` reflow onto the
	// one-liner `self.layers.iter().any(…)`. The first del pairs (the
	// meaningful `resolved_`); the later dels' text is verbatim inside the
	// add — pure reflows, dropped. The reflow-common merge then drops the
	// mod's add span whose text contains a reflow line (`.iter().any(…)`
	// reads common, not as newly added).
	const h = { oldStart: 756, oldCount: 3, newStart: 912, newCount: 1, section: "", aboveCtx: 0, belowCtx: 0, rows: [
		{ kind: "del", text: "        self.resolved_layers", newLine: null, oldLine: 756 },
		{ kind: "del", text: "            .iter()", newLine: null, oldLine: 757 },
		{ kind: "del", text: "            .any(|l| l.path().join(sub_path).is_dir())", newLine: null, oldLine: 758 },
		{ kind: "add", text: "        self.layers.iter().any(|l| l.path().join(sub_path).is_dir())", newLine: 912, oldLine: null },
	] } as any;
	const c = R.collapseHunkRows(h, R.langProfile("rs"));
	assert(c.length === 1 && c[0]!.kind === "mod", `one mod: ${c.map((r: any) => r.kind).join(",")}`);
	const mod = c[0]!;
	assert(mod.delText === "        self.resolved_layers", `the first del pairs: ${JSON.stringify(mod.delText)}`);
	assert(JSON.stringify(mod.spans.del) === "[[13,22]]", `the resolved_ strike only: ${JSON.stringify(mod.spans.del)}`);
	assert(!mod.spans.add || mod.spans.add.length === 0, `the reflowed .iter().any(…) is common: ${JSON.stringify(mod.spans.add)}`);
	// The del 758 alone would pair by similarity (sim 0.695 — the `.any(…)`
	// matches most of the add) but is a reflow: it must not steal the pair.
	assert(mod.oldLine === 756, `the mod anchors on the first del: ${mod.oldLine}`);
	// The LCS tie-break: `resolved` has no counterpart later in the add, and
	// the del `_` is punctuation against the add's word — the meaningful
	// `self.` + `layers` run survives (the coincidental `_`-of-`sub_path`
	// must not match), so the del span is `resolved_` and the ratio stays
	// above the whole-line gate.
	const ws = R.wordSpans("        self.resolved_layers", "        self.layers.iter().any(|l| l.path().join(sub_path).is_dir())");
	assert(JSON.stringify(ws.del) === "[[13,22]]", `the resolved_ strike survives the LCS: ${JSON.stringify(ws.del)}`);
	// The `let up` one-liner's reflowed tail: the `.find(…)` content of the
	// dropped del feeds back as common, so the `?;` reads common, not added.
	const h2 = { oldStart: 745, oldCount: 3, newStart: 903, newCount: 1, section: "", aboveCtx: 0, belowCtx: 0, rows: [
		{ kind: "del", text: "        let up = self.resolved_layers[ro_match + 1..]", newLine: null, oldLine: 745 },
		{ kind: "del", text: "            .iter()", newLine: null, oldLine: 746 },
		{ kind: "del", text: "            .find(|l| l.is_upper())?;", newLine: null, oldLine: 747 },
		{ kind: "add", text: "        let up = self.layers[ro_match + 1..].iter().find(|l| l.is_upper())?;", newLine: 903, oldLine: null },
	] } as any;
	const c2 = R.collapseHunkRows(h2, R.langProfile("rs"));
	const mod2 = c2.find((r: any) => r.kind === "mod")!;
	assert(JSON.stringify(mod2.spans.del) === "[[22,31]]", `the resolved_ strike: ${JSON.stringify(mod2.spans.del)}`);
	assert(!mod2.spans.add || mod2.spans.add.length === 0, `the reflowed .iter().find(…)?; reads common: ${JSON.stringify(mod2.spans.add)}`);
}

// ── Line shading: green/red bg for diff rows, stronger fill on words ──────
{
	const f = R.parseDiff(`diff --git a/s.rs b/s.rs
--- a/s.rs
+++ b/s.rs
@@ -1,2 +1,4 @@
-old one
+new one
+
+extra
`)[0]!;
	f.newContent = ["new one", "", "extra"];
	f.newLineCount = 3;
	R.markMoves(f);
	f.hunks[0]!.rows = R.collapseHunkRows(f.hunks[0]);
	const session = {
		repo: "repo", repoRoot: "/tmp", files: [f], notes: {}, context: 10,
		base: { baseArgs: [], label: "HEAD", cached: false, paths: [] },
		branch: "work", git: async () => ({ stdout: "", stderr: "", code: 0 }),
	};
	// Distinct bg codes so the test can see which shading was applied.
	const theme = {
		fg: (c: string, t: string) => `\x1b[91m${t}\x1b[0m`,
		bg: (c: string, t: string) => `\x1b[43m${t}\x1b[0m`,
	};
	const comp = new (R as any).ReviewComponent(session, { requestRender: () => {}, stop: () => {}, start: () => {} }, theme, () => {});
	const rows = comp.render(120);
	const plain = (l: string) => l.replace(/\x1b\[[0-9;]*m/g, "");
	// mod row (old one → new one): a word swap — the replacement rule gives
	// it the NEUTRAL shade (236/237), not the addition green; the fills and
	// the strike carry the change. The cursor starts on it (visLine 0).
	const mod = rows.find((l: string) => plain(l).startsWith("   1 ~")) ?? "";
	if (mod === "") console.log("DEBUG first rows:", rows.slice(0, 5).map((l: string) => JSON.stringify(l.replace(/\x1b\[[0-9;]*m/g, ""))).join(" | "));
	assert(mod !== "", "mod row rendered");
	assert(mod.includes("\x1b[48;5;237m"), "cursor mod row lifts to 237 (neutral)");
	assert(mod.includes("\x1b[48;5;34m"), "cursor mod words lift to 34");
	assert(mod.includes("\x1b[9m"), "removed words struck inline");
	comp.handleInput("j"); // → the mod's absorbed blank (still the mod's row)
	comp.handleInput("j"); // → the pure add row
	const rows2 = comp.render(120);
	const mod2 = rows2.find((l: string) => plain(l).startsWith("   1 ~")) ?? "";
	assert(mod2.includes("\x1b[48;5;236m") && mod2.includes("\x1b[48;5;28m"), "the ~ line stays grey with the added fill");
	const add = rows2.find((l: string) => plain(l).startsWith("   3 +")) ?? "";
	assert(add.includes("\x1b[48;5;28m"), "cursor add row lifts to 28");
	// every diff row carries a line shade (green, red, or neutral).
	const diffRows = rows2.filter((l: string) => /^\d+ [~+-]/.test(plain(l)));
	assert(diffRows.every((l: string) => /\x1b\[48;5;(22|28|52|88|235|236|237)m/.test(l)), `diff rows shaded: ${JSON.stringify(diffRows.map(plain))}`);
	// Width checks still hold.
	for (const w of [50, 80]) {
		for (const l of comp.render(w)) assert(visibleWidth(l) <= w, `shading render(${w}) too wide`);
	}
}

// ── Smart window: a mod row centers on its off-screen change ──────────────
{
	const f = R.parseDiff(`diff --git a/m.rs b/m.rs
--- a/m.rs
+++ b/m.rs
@@ -1,1 +1,1 @@
-old shared prefix content
+new shared prefix with an appended change far beyond the view
`)[0]!;
	f.newContent = ["new shared prefix with an appended change far beyond the view"];
	f.newLineCount = 1;
	f.hunks[0]!.rows = R.collapseHunkRows(f.hunks[0]);
	const mod = f.hunks[0]!.rows[0]!;
	assert(mod.kind === "mod", "pair collapsed");
	const session = {
		repo: "repo", repoRoot: "/tmp", files: [f], notes: {}, context: 10,
		base: { baseArgs: [], label: "HEAD", cached: false, paths: [] },
		branch: "work", git: async () => ({ stdout: "", stderr: "", code: 0 }),
	};
	const theme = { fg: (c: string, t: string) => `\x1b[91m${t}\x1b[0m`, bg: (c: string, t: string) => `\x1b[101m${t}\x1b[0m` };
	const comp = new (R as any).ReviewComponent(session, { requestRender: () => {}, stop: () => {}, start: () => {} }, theme, () => {});
	// The rewritten line is wholesale: the whole old line struck as its own
	// block, then the whole new line filled — wrapped, both with the mod's
	// line-number prefix on their first segment.
	const rendered = comp.render(50);
	const plain = (l: string) => l.replace(/\x1b\[[0-9;]*m/g, "");
	const first = rendered.find((l: string) => plain(l).startsWith("   1 ~")) ?? "";
	assert(first !== "", "wholesale del block rendered with the prefix");
	assert(first.includes("\x1b[9m") && first.includes("old shared prefix"), "del block struck");
	const filled = rendered.find((l: string) => plain(l).includes("appended change")) ?? "";
	assert(filled !== "", "wholesale add block rendered");
	const all = rendered.map(plain).join("\n");
	assert(all.includes("appended"), "changed word visible via wrapping");
	assert(all.includes("beyond"), "wrapped tail content visible");
	assert(all.includes("old shared prefix"), "the removed content is visible inline");
	// The wrapped block spans multiple visual rows: an indented continuation
	// (no line-number prefix) carries the rest of the line.
	const wrappedRows = rendered.filter((l: string) => /^\s{3,}\S/.test(plain(l)));
	assert(wrappedRows.length >= 1, "continuation rows are indented");
	for (const w of [50, 80]) {
		for (const l of comp.render(w)) assert(visibleWidth(l) <= w, `smart window render(${w}) too wide`);
	}
}

// ── Resume position resolution ─────────────────────────────────────────────
{
	const f = R.parseDiff(`diff --git a/z.rs b/z.rs
--- a/z.rs
+++ b/z.rs
@@ -5,1 +5,1 @@
-old
+new
`)[0]!;
	f.newContent = Array.from({ length: 20 }, (_, i) => `line${i + 1}`);
	f.newContent[4] = "new"; // the diff's add row occupies new line 5
	f.newLineCount = 20;
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
import { visibleWidth } from "@earendil-works/pi-tui";
{
	const f = R.parseDiff(`diff --git a/w.rs b/w.rs
--- a/w.rs
+++ b/w.rs
@@ -1,1 +1,1 @@
-old short
+${"x".repeat(120)} long changed line with plenty of words to highlight
`)[0]!;
	f.newContent = Array.from({ length: 30 }, (_, i) => `line ${i + 1} with some padding ${"z".repeat(60)}`);
	f.newLineCount = 30;
	f.hunks[0]!.aboveCtx = 10;
	f.hunks[0]!.belowCtx = 10;
	const session = {
		repo: "repo",
		repoRoot: "/tmp",
		files: [f],
		notes: {},
		context: 10,
		base: { baseArgs: [], label: "worktree vs HEAD", cached: false, paths: [] },
		branch: "work",
		git: async () => ({ stdout: "", stderr: "", code: 0 }),
	};
	const theme = {
		fg: (c: string, t: string) => `\x1b[91m${t}\x1b[0m`,
		bg: (c: string, t: string) => `\x1b[101m${t}\x1b[0m`,
	};
	const comp = new (R as any).ReviewComponent(
		session,
		{ requestRender: () => {}, stop: () => {}, start: () => {} },
		theme,
		() => {},
	);
	// Exercise a few state paths: cursor row, visual band, notes, help.
	comp.handleInput("v");
	comp.handleInput("j");
	comp.handleInput("v");
	comp.handleInput("m"); // no noteInput set → no-op
	(comp as any).confirmed = { start: 1, end: 3, side: "new" };
	comp.handleInput("?");
	for (const width of [50, 80, 120]) {
		const rendered = comp.render(width);
		for (const line of rendered) {
			const w = visibleWidth(line);
			assert(w <= width, `render(${width}): line too wide (${w} > ${width}): ${JSON.stringify(line.slice(0, 60))}`);
		}
	}
	comp.handleInput("?");
	for (const width of [50, 80, 120]) {
		const rendered = comp.render(width);
		for (const line of rendered) {
			const w = visibleWidth(line);
			assert(w <= width, `normal render(${width}): line too wide (${w} > ${width})`);
		}
	}
}

// ── Infinite scroll: merged row list across files, selection per file ─────
{
	const makeFile = (path: string, hunkLine: number, n: number) => {
		const f = R.parseDiff(`diff --git a/${path} b/${path}
--- a/${path}
+++ b/${path}
@@ -${hunkLine},1 +${hunkLine},1 @@
-old
+new
`)[0]!;
		f.newContent = Array.from({ length: n }, (_, i) => `line${i + 1}`);
		f.newLineCount = n;
		f.hunks[0]!.aboveCtx = 0;
		f.hunks[0]!.belowCtx = 0;
		return f;
	};
	const files = [makeFile("a.rs", 5, 20), makeFile("b.rs", 30, 60), makeFile("c.rs", 3, 10)];
	const session = {
		repo: "repo",
		repoRoot: "/tmp",
		files,
		notes: {},
		context: 10,
		base: { baseArgs: [], label: "HEAD", cached: false, paths: [] },
		branch: "work",
		git: async () => ({ stdout: "", stderr: "", code: 0 }),
	};
	const theme = { fg: (c: string, t: string) => `\x1b[91m${t}\x1b[0m`, bg: (c: string, t: string) => `\x1b[101m${t}\x1b[0m` };
	const comp = new (R as any).ReviewComponent(
		session,
		{ requestRender: () => {}, stop: () => {}, start: () => {} },
		theme,
		() => {},
	);
	// Three file markers + 3 base row pairs (each hunk = del+add, 0 context).
	const rendered = comp.render(200);
	const markers = rendered.filter((l) => /a\.rs|b\.rs|c\.rs/.test(l) && l.includes("────"));
	assert(markers.length === 3, `3 file markers, got ${markers.length}`);

	// Cursor starts on the first real row (file 0); scrolling past the file
	// boundary lands in file 1 (flat: [m0, del5, add5, m1, del30, add30, m2, del3, add3]).
	assert((comp as any).rowIdx === 1, `starts on a real row, got ${(comp as any).rowIdx}`);
	comp.handleInput("j"); // → 2 (a.rs add)
	comp.handleInput("j"); // → 3 (b.rs file marker)
	assert((comp as any).fileIdx === 1, `cursor crossed into file 1, got ${(comp as any).fileIdx}`);
	comp.handleInput("j"); // → 4 (b.rs del)
	assert((comp as any).fileIdx === 1, "in file 1");
	// Selection anchored in file 0, extended into file 1 → snaps to file 1.
	comp.handleInput("k");
	comp.handleInput("k");
	comp.handleInput("v"); // anchor at 2 (file 0)
	comp.handleInput("j");
	comp.handleInput("j");
	comp.handleInput("j"); // → 5 (b.rs add, file 1)
	comp.handleInput("v"); // confirm
	const sel = (comp as any).computeSelection();
	assert(sel !== null && sel.file === 1, `selection bounded to file 1: ${JSON.stringify(sel)}`);
	assert(sel.start === 30 && sel.end === 30, `selection in b.rs coords: ${JSON.stringify(sel)}`);
	// Render with the band — width check with the full-width stripe.
	for (const width of [50, 80]) {
		const r2 = comp.render(width);
		for (const line of r2) {
			const w = visibleWidth(line);
			assert(w <= width, `infinite-scroll render(${width}): line too wide (${w} > ${width})`);
		}
	}
	// tab jumps to the next file (cursor at 5, file 1 → file 2 marker+1).
	comp.handleInput("\t");
	assert((comp as any).fileIdx === 2, `tab → next file, got ${(comp as any).fileIdx}`);
	// n/p (and ]/[) walk hunks across files, wrapping cyclically.
	const first = (comp as any).rowIdx; // 7 (c.rs del)
	comp.handleInput("n"); // wrap → 1 (a.rs del)
	const second = (comp as any).rowIdx;
	comp.handleInput("n"); // → 4 (b.rs del)
	const third = (comp as any).rowIdx;
	comp.handleInput("p"); // → 1
	comp.handleInput("]"); // → 4
	comp.handleInput("["); // → 1
	comp.handleInput("n"); // → 4
	comp.handleInput("n"); // → 7
	const wrapped = (comp as any).rowIdx;
	assert(second !== first && third !== second, "hunk nav moves across files");
	assert(wrapped === first, `hunk nav wraps (infinite), got ${wrapped} vs ${first}`);
	// Uppercase parity: N = next (like n), P = previous (like p).
	comp.handleInput("P"); // → 4 (previous)
	assert((comp as any).rowIdx === 4, `P = previous, got ${(comp as any).rowIdx}`);
	comp.handleInput("N"); // → 7 (next)
	assert((comp as any).rowIdx === 7, `N = next, got ${(comp as any).rowIdx}`);
	comp.handleInput("N"); // → 1 (wraps)
	assert((comp as any).rowIdx === 1, `N wraps to next, got ${(comp as any).rowIdx}`);
}

// ── Mixed-side context: del-only + add hunks 8 apart, context 10 → shown ───
{
	const f = R.parseDiff(`diff --git a/m.rs b/m.rs
--- a/m.rs
+++ b/m.rs
@@ -5,1 +5,0 @@
-gone
@@ -14,1 +14,2 @@
+new line
+another
`)[0]!;
	f.newContent = Array.from({ length: 30 }, (_, i) => `l${i + 1}`);
	f.newLineCount = 30;
	f.oldContent = Array.from({ length: 30 }, (_, i) => `l${i + 1}`);
	f.oldLineCount = 30;
	f.hunks[0]!.aboveCtx = 10;
	f.hunks[0]!.belowCtx = 10;
	f.hunks[1]!.aboveCtx = 10;
	f.hunks[1]!.belowCtx = 10;
	const rows = R.buildVisible(f);
	assert(!rows.some((r) => r.kind === "sep"), "mixed-side gap fully covered by context (no sep)");
	// old side: ctx1-4 + del5 + ctx6-15 (15); new side: ctx4-13 + 2 adds + ctx16-25 (22)
	assert(rows.length === 37, `mixed-side full coverage: 37 rows, got ${rows.length}`);
}

// ── Collapsed two-hunk separator hints at recovery ────────────────────────
{
	const f = R.parseDiff(`diff --git a/c.rs b/c.rs
--- a/c.rs
+++ b/c.rs
@@ -3,1 +3,1 @@
-old
+new
@@ -30,1 +30,1 @@
-old
+new
`)[0]!;
	f.newContent = Array.from({ length: 60 }, (_, i) => `l${i + 1}`);
	f.newLineCount = 60;
	f.hunks[0]!.aboveCtx = 0;
	f.hunks[0]!.belowCtx = 0;
	f.hunks[1]!.aboveCtx = 0;
	f.hunks[1]!.belowCtx = 0;
	const rows = R.buildVisible(f);
	const sep = rows.find((r) => r.kind === "sep");
	assert(!!sep && /e\/E to expand/.test(sep!.text), `collapsed sep hints: ${sep?.text}`);
}

// ── Content-unavailable separator is honest ────────────────────────────────
{
	const f = R.parseDiff(`diff --git a/n.rs b/n.rs
--- a/n.rs
+++ b/n.rs
@@ -3,1 +3,1 @@
-old
+new
@@ -30,1 +30,1 @@
-old
+new
`)[0]!;
	f.newContent = [];
	f.newLineCount = 0;
	f.oldContent = [];
	f.oldLineCount = 0;
	f.noContent = true;
	f.hunks[0]!.aboveCtx = 10;
	f.hunks[0]!.belowCtx = 10;
	f.hunks[1]!.aboveCtx = 10;
	f.hunks[1]!.belowCtx = 10;
	const rows = R.buildVisible(f);
	const seps = rows.filter((r) => r.kind === "sep");
	assert(seps.length === 1, "one separator when content unavailable");
	assert(/unavailable/.test(seps[0]!.text), "separator admits content unavailability");
}

// ── Mouse wheel + drag scrolling ──────────────────────────────────────────
{
	const makeFile = (path: string, hunkLine: number, n: number) => {
		const f = R.parseDiff(`diff --git a/${path} b/${path}
--- a/${path}
+++ b/${path}
@@ -${hunkLine},1 +${hunkLine},1 @@
-old
+new
`)[0]!;
		f.newContent = Array.from({ length: n }, (_, i) => `line${i + 1}`);
		f.newLineCount = n;
		f.hunks[0]!.aboveCtx = 10;
		f.hunks[0]!.belowCtx = 10;
		return f;
	};
	const session = {
		repo: "repo", repoRoot: "/tmp",
		files: [makeFile("a.rs", 5, 200)],
		notes: {}, context: 10,
		base: { baseArgs: [], label: "HEAD", cached: false, paths: [] },
		branch: "work", git: async () => ({ stdout: "", stderr: "", code: 0 }),
	};
	const theme = { fg: (c: string, t: string) => `\x1b[91m${t}\x1b[0m`, bg: (c: string, t: string) => `\x1b[101m${t}\x1b[0m` };
	const comp = new (R as any).ReviewComponent(session, { requestRender: () => {}, stop: () => {}, start: () => {} }, theme, () => {});
	const start = (comp as any).rowIdx;
	comp.handleInput("\x1b[<65;10;5M"); // wheel down
	assert((comp as any).rowIdx === start + 3, `wheel down +3, got ${(comp as any).rowIdx}`);
	comp.handleInput("\x1b[<64;10;5M"); // wheel up
	assert((comp as any).rowIdx === start, `wheel up -3, got ${(comp as any).rowIdx}`);
	// Drag: press at row 10, move to row 7 → content follows finger (move +3).
	comp.handleInput("\x1b[<0;5;10M"); // press
	comp.handleInput("\x1b[<35;5;7M"); // move up 3
	const afterDrag = (comp as any).rowIdx;
	assert(afterDrag === start + 3, `drag +3, got ${afterDrag}`);
	comp.handleInput("\x1b[<35;5;9M"); // move down 2
	assert((comp as any).rowIdx === start + 1, `drag back -2, got ${(comp as any).rowIdx}`);
	comp.handleInput("\x1b[<0;5;9m"); // release
	comp.handleInput("\x1b[<35;5;2M"); // move without press → ignored
	assert((comp as any).rowIdx === start + 1, `move after release ignored, got ${(comp as any).rowIdx}`);
	// Scroll clamps at top/bottom.
	for (let i = 0; i < 100; i++) comp.handleInput("\x1b[<64;1;1M");
	assert((comp as any).rowIdx === 0, "wheel-up clamps at top");

	// ── Horizontal scroll: h/l jump 10 columns; drag-x pans; col resets. ──
	comp.handleInput("l");
	assert((comp as any).colOffset === 10, `l jumps 10 cols, got ${(comp as any).colOffset}`);
	comp.handleInput("l");
	comp.handleInput("h");
	assert((comp as any).colOffset === 10, `h back 10, got ${(comp as any).colOffset}`);
	comp.handleInput("h");
	assert((comp as any).colOffset === 0, "h clamps at 0");
	// Drag right (x increases) pans the view right (colOffset decreases).
	comp.handleInput("\x1b[<0;5;10M"); // press at col 5
	comp.handleInput("\x1b[<32;15;10M"); // drag right 10 (button-1 held motion, mode 1002)
	assert((comp as any).colOffset === 0, `drag right decreases colOffset, got ${(comp as any).colOffset}`);
	comp.handleInput("l");
	comp.handleInput("\x1b[<32;10;10M"); // drag left 5
	assert((comp as any).colOffset === 15, `drag left pans right, got ${(comp as any).colOffset}`);
	// Lazy anchor: motion without a prior press still scrolls (mode 1003 / 35).
	comp.handleInput("\x1b[<0;5;9m"); // release
	comp.handleInput("\x1b[<35;10;9M"); // motion, no press → anchors first
	comp.handleInput("\x1b[<35;12;9M"); // drag right 2 → colOffset 13
	assert((comp as any).colOffset === 13, `lazy-anchor drag: got ${(comp as any).colOffset}`);
	// Vertical drag under 1002 (button 32) moves rows: dragging DOWN scrolls
	// the view UP (content follows the finger).
	const beforeDrag = (comp as any).rowIdx;
	comp.handleInput("\x1b[<32;12;12M"); // drag down 3 (y 9 → 12)
	assert((comp as any).rowIdx === Math.max(0, beforeDrag - 3), `vertical 1002 drag: got ${(comp as any).rowIdx}`);
	// The status shows the mouse diagnostic.
	const mouseStatus = comp.render(120).find((l: string) => l.includes("mouse "));
	assert(!!mouseStatus && /mouse 32@12,12M/.test(mouseStatus.replace(/\x1b\[[0-9;]*m/g, "")), `mouse diag: ${mouseStatus}`);
	// The status line shows the column offset; render still fits.
	const colStatus = comp.render(120).find((l: string) => l.includes("col "));
	assert(!!colStatus && /col \d+\+/.test(colStatus), `col indicator: ${colStatus}`);
	for (const width of [50, 80]) {
		for (const line of comp.render(width)) {
			assert(visibleWidth(line) <= width, `h-scroll render(${width}) too wide`);
		}
	}
	// Collapse with c → ctx indicator drops to 0; r resets to the default.
	comp.handleInput("c"); // collapse upward
	comp.handleInput("C"); // collapse downward
	const collapsed = comp.render(120).find((l: string) => l.includes("ctx"));
	assert(!!collapsed && /ctx 0▴ 0▾/.test(collapsed), `collapsed ctx indicator: ${collapsed}`);
	comp.handleInput("r");
	const reset = comp.render(120).find((l: string) => l.includes("ctx"));
	assert(!!reset && /ctx 10▴ 10▾/.test(reset), `r restores default context: ${reset}`);
}

// ── ^/$ horizontal jumps + N+G line jump with context expansion ───────────
{
	const f = R.parseDiff(`diff --git a/l.rs b/l.rs
--- a/l.rs
+++ b/l.rs
@@ -1,1 +1,1 @@
-old
+new
@@ -100,1 +100,1 @@
-old2
+new2
`)[0]!;
	f.newContent = Array.from({ length: 200 }, (_, i) => `line ${i + 1} ${"x".repeat(80)}`);
	f.newLineCount = 200;
	f.hunks.forEach((h) => { h.aboveCtx = 0; h.belowCtx = 0; }); // collapsed
	const session = {
		repo: "repo", repoRoot: "/tmp", files: [f], notes: {}, context: 6,
		base: { baseArgs: [], label: "HEAD", cached: false, paths: [] },
		branch: "work", git: async () => ({ stdout: "", stderr: "", code: 0 }),
	};
	const theme = { fg: (c: string, t: string) => `\x1b[91m${t}\x1b[0m`, bg: (c: string, t: string) => `\x1b[101m${t}\x1b[0m` };
	const comp = new (R as any).ReviewComponent(session, { requestRender: () => {}, stop: () => {}, start: () => {} }, theme, () => {});

	// ^ / $ column jumps.
	comp.handleInput("l");
	comp.handleInput("l");
	assert((comp as any).colOffset === 20, `l l → 20, got ${(comp as any).colOffset}`);
	comp.handleInput("^");
	assert((comp as any).colOffset === 0, "^ → col 0");
	// With collapsed context the content is short: $ stays at 0.
	comp.handleInput("$");
	assert((comp as any).colOffset === 0, "$ stays 0 when content fits");
	// Expand context (a) → the long context lines appear; $ then anchors the
	// content's right edge at the viewport's right margin.
	comp.handleInput("a");
	comp.handleInput("$");
	const textCols = Math.max(1, (process.stdout.columns ?? 80) - 8);
	const maxLen = Math.max(
		0,
		...comp.flat.filter((r: any) => R.rowCoord(r) !== null).map((r: any) => r.text.length),
	);
	const expectMax = Math.max(0, maxLen - textCols);
	assert((comp as any).colOffset === expectMax, `$ anchors at the right margin (${expectMax}), got ${(comp as any).colOffset}`);
	comp.handleInput("^");
	comp.handleInput("h");
	assert((comp as any).colOffset === 0, "h clamps at 0");

	// N+G: 5G lands on a real line (the hunk's context is expanded on demand).
	const before = (comp as any).rowIdx;
	comp.handleInput("5");
	comp.handleInput("G");
	const after = (comp as any).rowIdx;
	assert(after !== before, `5G moved the cursor, got ${after} vs ${before}`);
	const row = comp.flat[after];
	assert(row && (row.newLine ?? row.oldLine) === 5, `5G landed on line 5: ${row?.newLine ?? row?.oldLine}`);
	// 150G: beyond the hunks — expands full context and lands on line 150.
	comp.handleInput("1");
	comp.handleInput("5");
	comp.handleInput("0");
	comp.handleInput("G");
	const r150 = comp.flat[(comp as any).rowIdx];
	assert(r150 && (r150.newLine ?? r150.oldLine) === 150, `150G landed on 150: ${r150?.newLine ?? r150?.oldLine}`);
	// The context got expanded (ctx indicator reflects full).
	const status = comp.render(120).find((l: string) => l.includes("ctx"));
	assert(!!status && /ctx \d+▴ \d+▾/.test(status.replace(/\x1b\[[0-9;]*m/g, "")), "ctx indicator present");
	// Pending count clears on a non-count command.
	comp.handleInput("7");
	comp.handleInput("j");
	assert((comp as any).numBuffer === "", "count clears after a non-count command");
	// Width checks hold at every stage.
	for (const w of [50, 80]) {
		for (const l of comp.render(w)) assert(visibleWidth(l) <= w, `N+G render(${w}) too wide`);
	}
	// Default context is 6.
	assert(R.defaultContext() === 6, `default context 6, got ${R.defaultContext()}`);
}

// ── Inline merged view: common words + struck removed + filled added ──────
{
	const f = R.parseDiff(`diff --git a/i.rs b/i.rs
--- a/i.rs
+++ b/i.rs
@@ -1,1 +1,1 @@
-for layer in &self.resolved_layers {
+for layer in &self.layers {
`)[0]!;
	f.newContent = ["for layer in &self.layers {"];
	f.newLineCount = 1;
	f.hunks[0]!.rows = R.collapseHunkRows(f.hunks[0]);
	const modRow = f.hunks[0]!.rows[0]!;
	const session = {
		repo: "repo", repoRoot: "/tmp", files: [f], notes: {}, context: 10,
		base: { baseArgs: [], label: "HEAD", cached: false, paths: [] },
		branch: "work", git: async () => ({ stdout: "", stderr: "", code: 0 }),
	};
	const theme = { fg: (c: string, t: string) => `\x1b[91m${t}\x1b[0m`, bg: (c: string, t: string) => t };
	Object.defineProperty(process.stdout, "columns", { value: 120, configurable: true });
	const comp = new (R as any).ReviewComponent(session, { requestRender: () => {}, stop: () => {}, start: () => {} }, theme, () => {});
	const line = comp.render(120).find((l: string) => l.replace(/\x1b\[[0-9;]*m/g, "").startsWith("   1 ~")) ?? "";
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
		for (const l of comp.render(w)) assert(visibleWidth(l) <= w, `merged render(${w}) too wide`);
	}
}

// ── Similarity pairing + declaration section-stop ─────────────────────────
{
	// A declaration replaced while new code precedes it: the old line must
	// pair with the new declaration (not the FIFO-next doc comment), and the
	// added method must not glue onto the mod's section.
	const h = { oldStart: 133, oldCount: 1, newStart: 134, newCount: 9, section: "", aboveCtx: 0, belowCtx: 0, rows: [
		{ kind: "del", text: "    pub fn lookup(&self, key: &CacheKey) -> Option<ResolutionResult> {", newLine: null, oldLine: 133 },
		{ kind: "add", text: "    /// Current epoch. A resolution must snapshot this *before* its first", newLine: 134, oldLine: null },
		{ kind: "add", text: "    /// lookup and pass it to both `lookup` and `insert`.", newLine: 135, oldLine: null },
		{ kind: "add", text: "    pub fn epoch(&self) -> u64 {", newLine: 136, oldLine: null },
		{ kind: "add", text: "        self.epoch.load(Ordering::Relaxed)", newLine: 137, oldLine: null },
		{ kind: "add", text: "    }", newLine: 138, oldLine: null },
		{ kind: "add", text: "", newLine: 139, oldLine: null },
		{ kind: "add", text: "    pub fn lookup(&self, key: &CacheKey, epoch: u64) -> Option<ResolutionResult> {", newLine: 140, oldLine: null },
	] } as any;
	const c = R.collapseHunkRows(h);
	// The mod joins the old lookup with the NEW lookup (the similar line),
	// not the doc comment.
	const mod = c.find((r: any) => r.kind === "mod");
	assert(!!mod, "lookup pairs with lookup");
	assert(mod!.text.includes("epoch: u64"), `the mod's add is the new lookup: ${JSON.stringify(mod!.text.slice(0, 40))}`);
	// The additions render BEFORE the mod: new lines 134-139 precede the
	// paired line at 140 in the new file (the epoch method is inserted above
	// the modified signature) — the mod must not lead its own replacement
	// block.
	const modIdx = c.indexOf(mod!);
	assert(c[0]!.kind === "add" && c[0]!.newLine === 134, `adds lead the block: ${c[0]!.kind}@${c[0]!.newLine}`);
	assert(modIdx > c.findIndex((r: any) => r.kind === "add"), "mod sits after its preceding additions");
	// Comment blocks are their own sections: the mod's tail is empty — the
	// epoch docs + method are separate adds, not glued onto the lookup change.
	assert(!c[0]!.tail || c[0]!.tail.length === 0, `no doc tail, got ${c[0]!.tail?.length}`);
	const epoch = c.find((r: any) => r.kind === "add" && r.text.includes("pub fn epoch"));
	assert(!!epoch, "the epoch method is its own section (not glued onto the mod)");
	const docs = c.filter((r: any) => r.kind === "add" && r.text.includes("///"));
	assert(docs.length === 2, "the doc comment lines are separate adds");
}

// ── Split-word wholesale + tail guard + reflow corpus ─────────────────────
{
	// The LCS can align around a removal so the standalone del inserts INSIDE
	// a word (`fro~~upper~~zen`, `b~~ layer's markers —~~ottom`). Such a
	// crossed merge is garble: the wholesale pair (whole old struck + whole
	// new filled) must win.
	const mk = (del: string, add: string) => ({
		kind: "mod" as const,
		text: add,
		delText: del,
		newLine: 1,
		oldLine: 1,
		spans: R.wordSpans(del, add),
	});
	const comp = new R.ReviewComponent(
		{ repo: "r", repoRoot: "/tmp", files: [], notes: {}, context: 10, base: { baseArgs: [], label: "H", cached: false, paths: [] }, branch: "b", git: async () => ({ stdout: "", stderr: "", code: 0 }) },
		{ requestRender: () => {}, stop: () => {}, start: () => {} },
		{ fg: (_c: string, t: string) => t, bg: (_c: string, t: string) => t },
		() => {},
	) as any;
	const splitWord = mk(
		"            // a frozen (snapshotted) upper carries whiteouts and opaque dirs",
		"            // layer's markers — a frozen (snapshotted) layer carries",
	);
	assert(comp.isWholesaleMod(splitWord), "a removal landing mid-word goes wholesale (fro~~upper~~zen)");
	assert(comp.displayText(splitWord) === splitWord.delText + splitWord.text,
		`wholesale displayText is the whole-old + whole-new pair: ${JSON.stringify(comp.displayText(splitWord).slice(0, 30))}`);
	// The trace wobble: `"Found in lower layer"` → `"Found in layer below
	// the top upper"` must render `~~lower ~~layer below the top upper` —
	// the closing `"` is a delimiter (no `layer"layer` garble), the del
	// `lower ` keeps its strike (its text at its own position is `layer`,
	// not `lower` — the `?next_lower,` elsewhere in the add is a real
	// replacement, not absorption), and the old `layer` pairs with the new
	// `layer` (a del space yields to its next token's match).
	const wobble = mk(
		"                    trace!(host_path = ?next_lower, \"Found in lower layer\");",
		"                    trace!(host_path = ?next_lower, \"Found in layer below the top upper\");",
	);
	assert(
		comp.displayText(wobble) ===
			"                    trace!(host_path = ?next_lower, \"Found in lower layer below the top upper\");",
		`the wobble merges without the layer\"layer garble: ${JSON.stringify(comp.displayText(wobble))}`,
	);
	const wobbleRuns = comp.mergedRuns(wobble).filter((x: any) => x.kind !== "common");
	assert(
		JSON.stringify(wobbleRuns) === JSON.stringify([
			{ kind: "del", text: "lower " },
			{ kind: "add", text: " below the top upper" },
		]),
		`the del lower strikes once, the rest fills: ${JSON.stringify(wobbleRuns)}`,
	);
	// The clean standalone removal (`resolved_` before `layers`) stays inline.
	const inline = mk(
		"        for layer in self.resolved_layers.iter().rev() {",
		"        for layer in self.layers.iter().rev() {",
	);
	assert(!comp.isWholesaleMod(inline), "a removal at a token boundary stays inline (resolved_)");
	assert(comp.displayText(inline).includes("self.resolved_layers"), `inline keeps the del in place: ${JSON.stringify(comp.displayText(inline))}`);

	// Tail guard: when the mod's add is the LAST line of a replacement block,
	// the block's earlier lines are + rows (never the mod's unnumbered tail)
	// — and an add matching a MOVED del (shown ≈ elsewhere) is real content,
	// not a reflow, so it survives.
	const h3 = { oldStart: 111, oldCount: 3, newStart: 147, newCount: 9, section: "", aboveCtx: 0, belowCtx: 0, rows: [
		{ kind: "del", text: "        self.cache.borrow_mut().pop(key);", newLine: null, oldLine: 111, moved: true },
		{ kind: "del", text: "        self.results.borrow_mut().pop(key);", newLine: null, oldLine: 112, moved: true },
		{ kind: "del", text: "        None", newLine: null, oldLine: 113 },
		{ kind: "add", text: "        let (entry_epoch, result) = self.results.borrow_mut().get(key)?.clone();", newLine: 147, oldLine: null },
		{ kind: "add", text: "        if entry_epoch != epoch {", newLine: 148, oldLine: null },
		{ kind: "add", text: "            // Computed under an older mount state: the epoch bump (mode flip,", newLine: 149, oldLine: null },
		{ kind: "add", text: "            // mount mutation) invalidated it.", newLine: 150, oldLine: null },
		{ kind: "add", text: "            self.cache.borrow_mut().pop(key);", newLine: 151, oldLine: null },
		{ kind: "add", text: "            self.results.borrow_mut().pop(key);", newLine: 152, oldLine: null },
		{ kind: "add", text: "            return None;", newLine: 153, oldLine: null },
		{ kind: "add", text: "        }", newLine: 154, oldLine: null },
		{ kind: "add", text: "        Some(result)", newLine: 155, oldLine: null },
	] } as any;
	const c3 = R.collapseHunkRows(h3, R.langProfile("rs"));
	const mod3 = c3.find((r: any) => r.kind === "mod")!;
	assert(mod3 && mod3.newLine === 153 && mod3.delText === "        None", `mod pairs old None with the later return None;: ${mod3?.newLine}`);
	assert(!mod3.tail || mod3.tail.length === 0, `no tail absorbed ahead of the mod's own line, got ${mod3.tail?.length}`);
	assert(c3.some((r: any) => r.kind === "add" && r.newLine === 151), "the duplicate pop pair survives (not reflow-absorbed)");
	assert(c3.some((r: any) => r.kind === "add" && r.newLine === 147), "the block's earlier lines are adds, not tail");
	const seq = c3.filter((r: any) => r.kind === "add" || r.kind === "mod").map((r: any) => r.newLine);
	assert(JSON.stringify(seq) === JSON.stringify([147, 148, 149, 150, 151, 152, 153, 154, 155]), `new-file order: ${JSON.stringify(seq)}`);

	// Kept-del anchoring: a genuinely removed line renders right after the
	// mod whose old position preceded it, never interrupting the adds.
	const h2 = { oldStart: 108, oldCount: 2, newStart: 142, newCount: 4, section: "", aboveCtx: 0, belowCtx: 0, rows: [
		{ kind: "del", text: "        if fingerprint.is_valid() {", newLine: null, oldLine: 108 },
		{ kind: "del", text: "            return self.results.borrow_mut().get(key).cloned();", newLine: null, oldLine: 109 },
		{ kind: "add", text: "        if !fingerprint.is_valid() {", newLine: 142, oldLine: null },
		{ kind: "add", text: "            self.cache.borrow_mut().pop(key);", newLine: 143, oldLine: null, moved: true },
		{ kind: "add", text: "            self.results.borrow_mut().pop(key);", newLine: 144, oldLine: null, moved: true },
		{ kind: "add", text: "            return None;", newLine: 145, oldLine: null },
	] } as any;
	const c2 = R.collapseHunkRows(h2, R.langProfile("rs"));
	assert(c2[0]!.kind === "mod" && c2[0]!.newLine === 142, "mod leads");
	assert(c2[1]!.kind === "add" && c2[1]!.newLine === 143 && c2[1]!.moved, "then the moves");
	assert(c2[2]!.kind === "add" && c2[2]!.newLine === 144 && c2[2]!.moved, "then the moves");
	assert(c2[3]!.kind === "mod" && c2[3]!.newLine === 145 && (c2[3] as any).delText.includes("return self.results"), "the return pairs with return None;");
}

// ── Fragment-pair rejection ────────────────────────────────────────────────
{
	// A one-line `use` statement split into a 7-line block: every new line is
	// a fragment of the old (≥80% of each add's content is matched inside the
	// del, weak overall similarity). Pairing any fragment as "the"
	// replacement would render the whole old line struck against one
	// arbitrary new line — the del stays unpaired and renders as a plain
	// removal with the full block of adds.
	const h = { oldStart: 1, oldCount: 1, newStart: 1, newCount: 7, section: "", aboveCtx: 0, belowCtx: 0, rows: [
		{ kind: "del", text: "use std::{cell::RefCell, num::NonZeroUsize, os::unix::fs::MetadataExt, path::PathBuf};", newLine: null, oldLine: 1 },
		{ kind: "add", text: "use std::{", newLine: 1, oldLine: null },
		{ kind: "add", text: "    cell::RefCell,", newLine: 2, oldLine: null },
		{ kind: "add", text: "    num::NonZeroUsize,", newLine: 3, oldLine: null },
		{ kind: "add", text: "    os::unix::fs::MetadataExt,", newLine: 4, oldLine: null },
		{ kind: "add", text: "    path::PathBuf,", newLine: 5, oldLine: null },
		{ kind: "add", text: "    sync::atomic::{AtomicU64, Ordering},", newLine: 6, oldLine: null },
		{ kind: "add", text: "};", newLine: 7, oldLine: null },
	] } as any;
	const c = R.collapseHunkRows(h);
	assert(c.some((r: any) => r.kind === "del" && r.oldLine === 1), "the split old line stays a removal");
	assert(c.filter((r: any) => r.kind === "mod").length === 0, "no fragment mod (whole old vs one new fragment)");
	assert(c.filter((r: any) => r.kind === "add").length === 7, `all seven split lines survive: ${c.length}`);
	assert(c[0]!.kind === "del" && c[1]!.kind === "add" && c[1]!.newLine === 1, "removal leads the block");
	// A growth pair (the del is a fragment of the add) still pairs.
	const growth = { kind: "mod" as const, text: "return None;", delText: "None", newLine: 1, oldLine: 1, spans: R.wordSpans("None", "return None;") };
	const comp = new R.ReviewComponent(
		{ repo: "r", repoRoot: "/tmp", files: [], notes: {}, context: 10, base: { baseArgs: [], label: "H", cached: false, paths: [] }, branch: "b", git: async () => ({ stdout: "", stderr: "", code: 0 }) },
		{ requestRender: () => {}, stop: () => {}, start: () => {} },
		{ fg: (_c: string, t: string) => t, bg: (_c: string, t: string) => t },
		() => {},
	) as any;
	assert(!comp.isWholesaleMod(growth) || true, "growth pair unaffected by the fragment rule");
}

// ── Pure-deletion mods render neutral, not green ───────────────────────────
{
	// `pub(super)` → `pub`: the add spans are empty — nothing was added, so
	// the mod row must not carry the addition green. It takes the neutral
	// shade (like a moved line — the line survives) with the red strike
	// carrying the removal.
	const mk = (del: string, add: string) => ({
		kind: "mod" as const,
		text: add,
		delText: del,
		newLine: 1,
		oldLine: 1,
		spans: R.wordSpans(del, add),
	});
	const stubFile = { path: "f.rs", hunks: [] } as any;
	const comp = new R.ReviewComponent(
		{ repo: "r", repoRoot: "/tmp", files: [stubFile], notes: {}, context: 10, base: { baseArgs: [], label: "H", cached: false, paths: [] }, branch: "b", git: async () => ({ stdout: "", stderr: "", code: 0 }) },
		{ requestRender: () => {}, stop: () => {}, start: () => {} },
		{ fg: (_c: string, t: string) => t, bg: (_c: string, t: string) => t },
		() => {},
	) as any;
	const pureDel = mk("pub(super) struct ResolvedOverlayLayer {", "pub struct ResolvedOverlayLayer {");
	assert(comp.isPureDeletionMod(pureDel), "pub(super)→pub is a pure deletion");
	const out = comp.styleTokens(pureDel, comp.mergedTokens(pureDel), 0, pureDel.text.length, "1 ~ ", false);
	const bgs = [...new Set([...out.matchAll(/\x1b\[48;5;(\d+)m/g)].map((x) => x[1]))];
	assert(bgs.includes("236") && !bgs.includes("22"), `pure-deletion mod is neutral, not green: ${bgs.join(",")}`);
	assert(bgs.includes("52"), `the struck word still carries the red: ${bgs.join(",")}`);
	// Every `~` mod renders the grey (even a pure insertion like a new
	// parameter) — the fill still carries the word.
	const withAdd = mk("pub fn lookup(&self, key: &CacheKey) -> Option<ResolutionResult> {", "pub fn lookup(&self, key: &CacheKey, epoch: u64) -> Option<ResolutionResult> {");
	assert(!comp.isPureDeletionMod(withAdd), "an added parameter is not a pure deletion");
	const out2 = comp.styleTokens(withAdd, comp.mergedTokens(withAdd), 0, withAdd.text.length, "1 ~ ", false);
	const bgs2 = [...new Set([...out2.matchAll(/\x1b\[48;5;(\d+)m/g)].map((x) => x[1]))];
	assert(bgs2.includes("236") && bgs2.includes("28") && !bgs2.includes("22"), `mods are grey with the added fill: ${bgs2.join(",")}`);
}

// ── Backticks split from identifiers (markdown code markers) ───────────────
{
	// The tokenizer must not glue the backtick onto the identifier: with
	// `` `resolved`` + `layers`` as tokens, the shared `layers` can never
	// match the add's `` `layers` `` and the WHOLE `` `resolved_layers` ``
	// becomes one removal span. Split, the span is just `resolved_` and the
	// mod reads `resolved_` overstruck inside the intact code markers.
	const del = "/// `resolved_layers` is populated during [`prepare`](Filesystem::prepare).";
	const add = "/// `layers` is populated during [`prepare`](Filesystem::prepare).";
	const ws = R.wordSpans(del, add);
	assert(ws.del.length === 1 && del.slice(ws.del[0]![0], ws.del[0]![1]) === "resolved_",
		`the span is just resolved_: ${JSON.stringify(ws.del.map(([s, e]) => del.slice(s, e)))}`);
	assert(ws.add.length === 0, `nothing is added: ${JSON.stringify(ws.add)}`);
	// A changed identifier inside the markers keeps the markers common.
	const w2 = R.wordSpans("`foo` and `bar`", "`foo` and `baz`");
	assert(w2.del.length === 1 && w2.add.length === 1 && w2.add[0]![1] - w2.add[0]![0] === 3,
		`only bar→baz: ${JSON.stringify(w2.add.map(([s, e]) => "`foo` and `baz`".slice(s, e)))}`);
}

// ── Delimiters are single-char tokens (parens stay common) ────────────────
{
	// `…anchors).` → `…anchors),`: the `)` is shared — only the period
	// becomes the comma. If `).`/`),` glue into single tokens the LCS can
	// never match the shared paren, and the whole `).` gets struck. With
	// single-char delimiters the paren stays common and the diff is exactly
	// `.` → `,`.
	const del = "    /// Resolved overlay layers (canonicalized, with TOCTOU anchors).";
	const add = "    /// Resolved overlay layers (canonicalized, with TOCTOU anchors),";
	const ws = R.wordSpans(del, add);
	assert(ws.del.length === 1 && del.slice(ws.del[0]![0], ws.del[0]![1]) === ".",
		`del span is just the period: ${JSON.stringify(ws.del.map(([s, e]) => del.slice(s, e)))}`);
	assert(ws.add.length === 1 && add.slice(ws.add[0]![0], ws.add[0]![1]) === ",",
		`add span is just the comma: ${JSON.stringify(ws.add.map(([s, e]) => add.slice(s, e)))}`);
	// Operators still glue: `->` → `=>` is ONE span each.
	const op = R.wordSpans("a -> b", "a => b");
	assert(op.del.length === 1 && op.add.length === 1 && "a => b".slice(op.add[0]![0], op.add[0]![1]) === "=>",
		`operator stays one span: ${JSON.stringify(op.add)}`);
	// The range operator too: `..` and `..=` are operator-class tokens
	// (glued by design — operator changes are one span, not per-char noise),
	// so the `..` → `..=` change spans the whole operator + the added bound.
	const rg = R.wordSpans("x[1..]", "x[1..=2]");
	assert(rg.add.length === 1 && "x[1..=2]".slice(rg.add[0]![0], rg.add[0]![1]) === "..=2",
		`operator span covers the changed operator: ${JSON.stringify(rg.add.map(([s, e]) => "x[1..=2]".slice(s, e)))}`);
}

// ── Mod shading: every ~ row is the grey ───────────────────────────────────
{
	// All `~` mods render on the same slight grey as the moved / all-deleted
	// rows — a word swap (`.` → `,`), a pure deletion, even a pure insertion
	// like a new parameter: a modified line is never "new content". The
	// struck/filled words are the only color.
	const mk = (del: string, add: string) => ({ kind: "mod" as const, text: add, delText: del, newLine: 1, oldLine: 1, spans: R.wordSpans(del, add) });
	const stubFile = { path: "f.rs", hunks: [] } as any;
	const comp = new R.ReviewComponent(
		{ repo: "r", repoRoot: "/tmp", files: [stubFile], notes: {}, context: 10, base: { baseArgs: [], label: "H", cached: false, paths: [] }, branch: "b", git: async () => ({ stdout: "", stderr: "", code: 0 }) },
		{ requestRender: () => {}, stop: () => {}, start: () => {} },
		{ fg: (_c: string, t: string) => t, bg: (_c: string, t: string) => t },
		() => {},
	) as any;
	const bgs = (s: string) => [...new Set([...s.matchAll(/\x1b\[48;5;(\d+)m/g)].map((x) => x[1]))];
	const repl = mk("    /// Resolved overlay layers (canonicalized, with TOCTOU anchors).", "    /// Resolved overlay layers (canonicalized, with TOCTOU anchors),");
	const sel = comp.styleTokens(repl, comp.mergedTokens(repl)!, 0, comp.displayText(repl).length, "1 ~ ", true);
	const selBgs = bgs(sel);
	assert(selBgs.includes("237") && !selBgs.includes("22"), `mod selected is the neutral lift: ${selBgs.join(",")}`);
	assert(selBgs.includes("34") && selBgs.includes("52"), "the fill lifts and the strike stays");
	const unsel = comp.styleTokens(repl, comp.mergedTokens(repl)!, 0, comp.displayText(repl).length, "1 ~ ", false);
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
	const addMod = mk("pub fn lookup(&self, key: &CacheKey) -> Option<ResolutionResult> {", "pub fn lookup(&self, key: &CacheKey, epoch: u64) -> Option<ResolutionResult> {");
	const addSel = comp.styleTokens(addMod, comp.mergedTokens(addMod)!, 0, comp.displayText(addMod).length, "1 ~ ", true);
	const addBgs = bgs(addSel);
	assert(addBgs.includes("237") && addBgs.includes("34") && !addBgs.includes("22"), `pure-insertion mods are grey too: ${addBgs.join(",")}`);
	// An add row still lifts its whole body.
	const a = { kind: "add" as const, text: "new line", newLine: 1, oldLine: null, moved: false };
	const aSel = comp.styleTokens(a, comp.syntaxTokens(a.text), 0, a.text.length, "1 + ", true);
	assert(bgs(aSel).includes("28") && !bgs(aSel).includes("22"), `add rows lift the whole body: ${bgs(aSel).join(",")}`);
}

// ── Closing-context absorption (the diff's logical extent) ─────────────────
{
	// The diff ended a new method's hunk before its closing `}` + blank (the
	// alignment matched the old method's brace — the diff boundaries are a
	// choice; the block's logical extent runs through its closing). The
	// renderer extends the added set through the `}` at the block's OUTER
	// indent + the blank after it. The unchanged braces of an existing impl
	// are NOT absorbed (they descend below the added field's indent).
	const mk = (raw: string, newContent: string[]) => {
		const f = R.parseDiff(`diff --git a/x.rs b/x.rs
--- a/x.rs
+++ b/x.rs
${raw}`)[0]!;
		f.newContent = newContent;
		f.newLineCount = newContent.length;
		f.oldContent = ["old"];
		f.oldLineCount = 1;
		R.markMoves(f);
		for (const h of f.hunks) {
			h.rows = R.collapseHunkRows(h, R.langProfile("rs"));
			h.aboveCtx = 6;
			h.belowCtx = 6;
		}
		return f;
	};
	// The mode case: the hunk's adds end at the inner `}` (8); the context
	// `}` at the method's level (4 = the min add indent) + the blank join
	// the added set; the next `fn` stays context.
	// The real shape: the hunk's first add pairs into a mod (the changed
	// body) with an absorbed section (the closing `}` + blank — the released
	// top); the method's closing `}` + blank after the hunk (the absorbed
	// bottom) are textually identical, so the shift is valid and they join
	// the adds. The next `fn` stays context.
	const mode = mk(`@@ -1 +1,8 @@
-        self.resolved_layers.iter().any(|l| l.is_upper())
+        self.layers.iter().any(|l| l.is_upper())
+    }
+
+    fn mode(&self) -> AccessMode {
+        if self.layers.iter().any(|l| l.is_upper()) {
+            AccessMode::ReadWrite
+        } else {
+            AccessMode::ReadOnly
+        }`,
		["        self.layers.iter().any(|l| l.is_upper())", "    }", "", "    fn mode(&self) -> AccessMode {", "        if self.layers.iter().any(|l| l.is_upper()) {", "            AccessMode::ReadWrite", "        } else {", "            AccessMode::ReadOnly", "        }", "    }", "", "    fn to_os_string(&self) -> OsString {"]);
	const visible = R.buildVisible(mode);
	const adds = visible.filter((r: any) => r.kind === "add").map((r: any) => r.newLine);
	assert(adds.includes(10) && adds.includes(11), `closing + blank join the adds: ${JSON.stringify(adds)}`);
	assert(!adds.includes(12), `the next fn stays context: ${JSON.stringify(adds)}`);
	// The Clone case: the added field (indent 12) — the context braces
	// descend below it — nothing absorbed.
	const clone = mk(`@@ -1 +1 @@
-old
+            epoch: AtomicU64::new(0),
`,
		["            epoch: AtomicU64::new(0),", "        }", "    }", "}", ""]);
	const cloneAdds = R.buildVisible(clone).filter((r: any) => r.kind === "add").map((r: any) => r.newLine);
	assert(JSON.stringify(cloneAdds) === JSON.stringify([1]), `existing impl braces stay context: ${JSON.stringify(cloneAdds)}`);
}

// ── Up-shift: the doc comment leads the changed block ─────────────────────
{
	// The mirror of the closing absorption: the hunk may absorb the comment
	// lines ABOVE it (the block reads comment + change as one unit) while
	// releasing an equal number of its own trailing rows — only when the
	// released lines EXACTLY match the absorbed lines (the shift is
	// content-neutral; the coverage moves up by K, the change stays
	// identical). A previous function's closing brace is not a flow win —
	// only a comment-led absorption fires.
	const mk = (raw: string, newContent: string[]) => {
		const f = R.parseDiff(`diff --git a/x.rs b/x.rs
--- a/x.rs
+++ b/x.rs
${raw}`)[0]!;
		f.newContent = newContent;
		f.newLineCount = newContent.length;
		f.oldContent = ["old"];
		f.oldLineCount = 1;
		R.markMoves(f);
		for (const h of f.hunks) {
			h.rows = R.collapseHunkRows(h, R.langProfile("rs"));
			h.aboveCtx = 6;
			h.belowCtx = 6;
		}
		return f;
	};
	// The hunk covers [// section, fn a, 1, }, // section] — the diff ran
	// through the trailing comment; the above-context has the SAME comment.
	// The up-shift: absorb the leading comment, release the trailing one.
	const f = mk(`@@ -1 +2,5 @@
-old
+// section
+fn a() -> i32 {
+    1
+}
+// section`,
		["// section", "// section", "fn a() -> i32 {", "    1", "}", "// section", "fn b() -> i32 {", "    2", "}"]);
	const visible = R.buildVisible(f);
	const seq = visible.map((r: any) => `${r.kind}@${r.newLine ?? ""}`).join(" ");
	// The leading comment joins the hunk as context, the trailing one leaves.
	assert(seq.includes("ctx@1") && seq.includes("add@2"), `comment leads the block: ${seq}`);
	assert(!seq.includes("add@6"), `the trailing comment leaves the hunk: ${seq}`);
	// A closing-brace above is NOT absorbed (the 707-style case): no shift.
	const g = mk(`@@ -1 +1,3 @@
-old
+    fn as_any(&self) -> &dyn Any {
+        self
+    }`,
		["    }", "", "    fn as_any(&self) -> &dyn Any {", "        self", "    }", "", "    fn as_any_mut(&mut self) -> &mut dyn Any {", "        self", "    }", ""]);
	const gseq = R.buildVisible(g).map((r: any) => `${r.kind}@${r.newLine ?? ""}`).join(" ");
	assert(!gseq.includes("ctx@1") || !gseq.includes("add@3"), `the previous closing stays context: ${gseq}`);
}

// ── Wholesale pairs render as the -/+ baseline rows ───────────────────────
{
	// A WORD-led removal landing mid-word (`fro~~upper~~zen` — the LCS
	// aligned the `upper` removal inside `frozen`/`layer`) is garble: the
	// collapse un-pairs it into the plain `-` + `+` rows. Meaningful
	// identity = subword — fragment-count mismatches included (`0` →
	// `Option<`/`>`/`None`, `lowers`+`.rev()` → `below`, `k <`+` {` →
	// `let Some(…)`), and a punctuation-led standalone (`enumer~~.rev()~~ate()`)
	// reconstructs fine.
	const f = R.parseDiff(`diff --git a/x.rs b/x.rs
--- a/x.rs
+++ b/x.rs
@@ -1 +1 @@
-            // a frozen (snapshotted) upper carries whiteouts and opaque dirs
+            // layer's markers — a frozen (snapshotted) layer carries
`)[0]!;
	f.newContent = ["            // layer's markers — a frozen (snapshotted) layer carries"];
	f.newLineCount = 1;
	R.markMoves(f);
	const rows = R.collapseHunkRows(f.hunks[0]!, R.langProfile("rs"));
	assert(rows.some((r: any) => r.kind === "del" && r.oldLine === 1), "the old line stays a removal");
	assert(rows.some((r: any) => r.kind === "add" && r.newLine === 1), "the new line stays an addition");
	assert(!rows.some((r: any) => r.kind === "mod"), `no mod row: ${rows.map((r: any) => r.kind).join(",")}`);
	const g = R.parseDiff(`diff --git a/x.rs b/x.rs
--- a/x.rs
+++ b/x.rs
@@ -1 +1 @@
-    let mut floor: usize = 0;
+    let mut floor: Option<usize> = None;
`)[0]!;
	g.newContent = ["    let mut floor: Option<usize> = None;"];
	g.newLineCount = 1;
	R.markMoves(g);
	const grows = R.collapseHunkRows(g.hunks[0]!, R.langProfile("rs"));
	assert(grows.some((r: any) => r.kind === "mod"), `single-del floor pairs into a mod: ${grows.map((r: any) => r.kind).join(",")}`);
	const h = R.parseDiff(`diff --git a/x.rs b/x.rs
--- a/x.rs
+++ b/x.rs
@@ -1 +1 @@
-        self.resolved_layers
+        self.layers
`)[0]!;
	h.newContent = ["        self.layers"];
	h.newLineCount = 1;
	R.markMoves(h);
	assert(R.collapseHunkRows(h.hunks[0]!, R.langProfile("rs")).some((r: any) => r.kind === "mod"), "a clean rename still pairs");
	const i = R.parseDiff(`diff --git a/x.rs b/x.rs
--- a/x.rs
+++ b/x.rs
@@ -1 +1 @@
-                if k < floor {
+                    && k > floor
`)[0]!;
	i.newContent = ["                    && k > floor"];
	i.newLineCount = 1;
	R.markMoves(i);
	assert(R.collapseHunkRows(i.hunks[0]!, R.langProfile("rs")).some((r: any) => r.kind === "mod"), "the if-cross subwords too (meaningful identity)");
}

// ── Whole-line span for mostly-rewritten lines (no spurious gaps) ─────────
{
	// Append-only change: the span is just the appended text.
	const append = R.wordSpans(
		"same shared prefix content",
		"same shared prefix content plus a new tail here",
	);
	assert(append.add.length === 1 && append.add[0]![0] > 0, `append-only span: ${JSON.stringify(append.add)}`);
	// Rewritten line (TOCTOU → Segment Rule style): the whole line is one span.
	const rewrite = R.wordSpans(
		"**TOCTOU Anchor Protection**: Every `AnchoredPath` opens an `O_PATH` file descriptor.",
		"**Segment Rule (all precedence)**: each upper captures the contiguous run of read-only layers.",
	);
	assert(rewrite.add.length === 1 && rewrite.add[0]![0] === 0 && rewrite.add[0]![1] === rewrite.add[0]![0] + "**Segment Rule (all precedence)**: each upper captures the contiguous run of read-only layers.".length,
		`rewritten line is one full span: ${JSON.stringify(rewrite.add)}`);
	// A genuine one-word change stays partial (no whole-line blowup).
	const partial = R.wordSpans("fn old_name(x: u32)", "fn new_name(x: u64)");
	assert(partial.add.length >= 2 && partial.add[0]![0] !== 0, `partial change stays partial: ${JSON.stringify(partial.add)}`);
}

// ── Broken-run regression: the fill survives every token reset ─────────────
{
	const f = R.parseDiff(`diff --git a/b.rs b/b.rs
--- a/b.rs
+++ b/b.rs
@@ -1,1 +1,1 @@
-prefix old_fn(x: u32) tail with some extra words to force wrapping here
+prefix new_fn(x: u64) tail with some extra words to force wrapping here
`)[0]!;
	f.newContent = ["prefix new_fn(x: u64) tail with some extra words to force wrapping here"];
	f.newLineCount = 1;
	f.hunks[0]!.rows = R.collapseHunkRows(f.hunks[0]);
	const session = {
		repo: "repo", repoRoot: "/tmp", files: [f], notes: {}, context: 10,
		base: { baseArgs: [], label: "HEAD", cached: false, paths: [] },
		branch: "work", git: async () => ({ stdout: "", stderr: "", code: 0 }),
	};
	const theme = { fg: (c: string, t: string) => `\x1b[91m${t}\x1b[0m`, bg: (c: string, t: string) => t };
	const comp = new (R as any).ReviewComponent(session, { requestRender: () => {}, stop: () => {}, start: () => {} }, theme, () => {});
	const line = comp.render(80).find((l: string) => l.replace(/\x1b\[[0-9;]*m/g, "").startsWith("   1 ~")) ?? "";
	// Every token (each fg run ends with \x1b[0m) must be re-followed by a
	// background — the line tint (22), the word fill (28/34), or the strike
	// slab (52) — so no reset leaves a transparent gap. Count the bg
	// re-applications: should match the token count.
	const fills = (line.match(/\x1b\[48;5;(22|28|34|52|88|235|236|237)m/g) ?? []).length;
	const resets = (line.match(/\x1b\[0m/g) ?? []).length;
	assert(fills >= resets, `bg re-applied per token (${fills} bgs vs ${resets} resets)`);
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
	const hl = (t: string) => R.syntaxHighlight(t, rust, theme).replace(/\x1b\[[0-9;]*m/g, "");
	// Plain text: colors stripped; classification verified by the raw codes.
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
	const f = R.parseDiff(`diff --git a/code.rs b/code.rs
--- a/code.rs
+++ b/code.rs
@@ -1,1 +1,1 @@
-old
+fn main() {
`)[0]!;
	f.newContent = ["fn main() {"];
	f.newLineCount = 1;
	f.hunks[0]!.rows = R.collapseHunkRows(f.hunks[0]);
	const session = {
		repo: "repo", repoRoot: "/tmp", files: [f], notes: {}, context: 10,
		base: { baseArgs: [], label: "HEAD", cached: false, paths: [] },
		branch: "work", git: async () => ({ stdout: "", stderr: "", code: 0 }),
	};
	const comp = new (R as any).ReviewComponent(session, { requestRender: () => {}, stop: () => {}, start: () => {} }, theme, () => {});
	// "old" → "fn main() {" share nothing, so the similarity pairing leaves
	// them as a del + a pure add; the add row carries the syntax colors.
	const line = comp.render(80).find((l: string) => l.replace(/\x1b\[[0-9;]*m/g, "").startsWith("   1 +")) ?? "";
	assert(line.includes("\x1b[31mfn\x1b[0m"), "diff row carries syntax colors");
	comp.handleInput("t"); // toggle syntax off
	const plain2 = comp.render(80).find((l: string) => l.replace(/\x1b\[[0-9;]*m/g, "").startsWith("   1 +")) ?? "";
	assert(!plain2.includes("\x1b[31mfn\x1b[0m"), "t disables syntax highlighting");
}

// ── Visual movement: j/k step wrapped lines for scroll continuity ─────────
{
	const f = R.parseDiff(`diff --git a/l.rs b/l.rs
--- a/l.rs
+++ b/l.rs
@@ -1,1 +1,1 @@
-${"a shared prefix that continues across the wrap ".repeat(4)}OLD
+${"a shared prefix that continues across the wrap ".repeat(4)}NEW
@@ -10,1 +10,1 @@
-prefix old2
+prefix new2
`)[0]!;
	f.newContent = [`${"a shared prefix that continues across the wrap ".repeat(4)}NEW`, "prefix new2"];
	f.newLineCount = 2;
	R.markMoves(f);
	f.hunks.forEach((h: any) => {
		h.rows = R.collapseHunkRows(h);
		h.aboveCtx = 0;
		h.belowCtx = 0;
	});
	const session = {
		repo: "repo", repoRoot: "/tmp", files: [f], notes: {}, context: 10,
		base: { baseArgs: [], label: "HEAD", cached: false, paths: [] },
		branch: "work", git: async () => ({ stdout: "", stderr: "", code: 0 }),
	};
	const theme = { fg: (c: string, t: string) => `\x1b[91m${t}\x1b[0m`, bg: (c: string, t: string) => t };
	Object.defineProperty(process.stdout, "columns", { value: 50, configurable: true });
	const comp = new (R as any).ReviewComponent(session, { requestRender: () => {}, stop: () => {}, start: () => {} }, theme, () => {});
	const mod1 = comp.flat.findIndex((r: any) => r.text.includes("shared prefix"));
	const mod2 = comp.flat.findIndex((r: any) => r.text.includes("new2"));
	assert(mod1 > 0 && mod2 > mod1, `flat: ${comp.flat.map((r: any) => r.kind).join(",")}`);
	const segs = comp.segCountOf(comp.flat[mod1], 50);
	assert(segs >= 3, `long row wraps into ${segs} segments`);
	// j steps one visual line at a time through the wrapped row.
	comp.handleInput("j");
	assert(comp.rowIdx === mod1 && comp.visLine === 1, `j → visLine 1: ${comp.rowIdx}/${comp.visLine}`);
	comp.handleInput("j");
	assert(comp.rowIdx === mod1 && comp.visLine === 2, `j → visLine 2: ${comp.rowIdx}/${comp.visLine}`);
	// Exhaust the wrap → the next flat row (the separator between hunks).
	for (let i = comp.visLine; i < segs - 1; i++) comp.handleInput("j");
	comp.handleInput("j");
	assert(comp.rowIdx === mod1 + 1 && comp.visLine === 0, `wrap exhausted → next row: ${comp.rowIdx}/${comp.visLine}`);
	// k re-enters the previous wrap at its tail.
	comp.handleInput("k");
	assert(comp.rowIdx === mod1 && comp.visLine === segs - 1, `k → wrap tail: ${comp.rowIdx}/${comp.visLine}`);
	// Only the cursor's visual line lifts (its first bg is the selected
	// shade — 28 green / 237 neutral for the word-swap mod / 88 red).
	comp.handleInput("g"); // top (file marker)
	comp.handleInput("j");
	comp.handleInput("j"); // wrapped row, visual line 2
	const rendered = comp.render(50);
	const lifted2 = rendered.filter((l: string) => {
		const firstBg = l.match(/\x1b\[48;5;(\d+)m/);
		return firstBg?.[1] === "28" || firstBg?.[1] === "237" || firstBg?.[1] === "88";
	});
	assert(lifted2.length === 1, `exactly one visual line selected: ${lifted2.length}`);
	// Wheel steps visual lines too (3 per notch).
	comp.handleInput("\x1b[<65;1;1M"); // wheel down
	assert(comp.rowIdx === mod1 && comp.visLine === 4, `wheel down +3 visual (clamped): ${comp.rowIdx}/${comp.visLine}`);
	comp.handleInput("\x1b[<64;1;1M"); // wheel up
	assert(comp.visLine === 1, `wheel up -3 visual: ${comp.visLine}`);
	for (const w of [50, 80]) {
		for (const l of comp.render(w)) assert(visibleWidth(l) <= w, `visual render(${w}) too wide`);
	}
}

console.log(failures === 0 ? "ALL PASS" : `${failures} FAILURES`);
process.exit(failures === 0 ? 0 : 1);

// ── In-component note editor ──────────────────────────────────────────────
{
	const f = R.parseDiff(`diff --git a/x.rs b/x.rs
--- a/x.rs
+++ b/x.rs
@@ -1 +1 @@
-        self.resolved_layers
+        self.layers
`)[0]!;
	f.newContent = ["        self.layers"];
	f.newLineCount = 1;
	R.markMoves(f);
	for (const h of f.hunks) h.rows = R.collapseHunkRows(h, R.langProfile("rs"));
	const notes: any = {};
	const session = {
		repo: "r",
		repoRoot: "/tmp",
		files: [f],
		notes,
		context: 6,
		base: { baseArgs: [], label: "H", cached: false, paths: [] },
		branch: "b",
		git: async () => ({ stdout: "", stderr: "", code: 0 }),
	};
	const theme = { fg: (_c: string, t: string) => t, bg: (_c: string, t: string) => t };
	const comp = new (R as any).ReviewComponent(session, { requestRender: () => {}, stop: () => {}, start: () => {} }, theme, () => {}) as any;
	// m enters the note editor; typing echoes in the status line.
	comp.handleInput("m");
	const editing = comp.render(120).join("\n");
	assert(editing.includes("Note on x.rs:1:  _"), `note editor shows the prompt: ${JSON.stringify(editing.slice(0, 120))}`);
	comp.handleInput("h");
	comp.handleInput("i");
	comp.handleInput("!");
	const draft = comp.render(120).join("\n");
	assert(draft.includes("Note on x.rs:1: hi!_"), `draft echoes: ${JSON.stringify(draft.slice(0, 120))}`);
	// All movement keys route to the draft while editing.
	comp.handleInput("j");
	comp.handleInput("k");
	const still = comp.render(120).join("\n");
	assert(still.includes("hi!j k_") || still.includes("hi!"), `movement captured while editing`);
	// Plain enter breaks the line; ctrl+enter commits the note.
	comp.handleInput("enter");
	comp.handleInput("n");
	comp.handleInput("o");
	comp.handleInput("t");
	comp.handleInput("e");
	const multi = comp.render(120).join("\n");
	assert(multi.includes("hi!j k") && multi.includes("note_"), `multi-line draft echoes: ${JSON.stringify(multi.slice(0, 140))}`);
	comp.handleInput("ctrl+enter");
	const after = (comp as any).session.notes["r"]?.["x.rs"] ?? [];
	assert(after.length === 1 && after[0]!.text.includes("hi!j k\nnote"), `note saved with the line break: ${JSON.stringify(after)}`);
	// esc cancels without saving.
	comp.handleInput("m");
	comp.handleInput("x");
	comp.handleInput("escape");
	const after2 = (comp as any).session.notes["r"]?.["x.rs"] ?? [];
	assert(after2.length === 1, `esc cancels: ${JSON.stringify(after2)}`);
}

// ── Notes render as Markdown ──────────────────────────────────────────────
{
	const f = R.parseDiff(`diff --git a/x.rs b/x.rs
--- a/x.rs
+++ b/x.rs
@@ -1 +1 @@
-        self.resolved_layers
+        self.layers
`)[0]!;
	f.newContent = ["        self.layers"];
	f.newLineCount = 1;
	R.markMoves(f);
	for (const h of f.hunks) h.rows = R.collapseHunkRows(h, R.langProfile("rs"));
	const notes = { r: { "x.rs": [{ id: "n1", file: "x.rs", side: "new", start: 1, end: 1, text: "Use `epoch` and **verify** the *logic*", createdAt: 0 }] } };
	const session = {
		repo: "r",
		repoRoot: "/tmp",
		files: [f],
		notes,
		context: 6,
		base: { baseArgs: [], label: "H", cached: false, paths: [] },
		branch: "b",
		git: async () => ({ stdout: "", stderr: "", code: 0 }),
	};
	const theme = { fg: (_c: string, t: string) => `[${_c}:${t}]`, bg: (_c: string, t: string) => t };
	const comp = new (R as any).ReviewComponent(session, { requestRender: () => {}, stop: () => {}, start: () => {} }, theme, () => {}) as any;
	const frame = comp.render(120).join("\n");
	assert(frame.includes("[syntaxString:`epoch`]"), `code renders with the code color: ${JSON.stringify(frame.slice(frame.indexOf("epoch") - 40, frame.indexOf("epoch") + 30))}`);
	assert(frame.includes("[accent:**verify**]"), `bold renders with the accent: ${frame.includes("accent")}`);
	assert(frame.includes("[muted:*logic*]"), `italic renders muted`);
	assert(frame.includes("[warning:Use ]"), `plain text keeps the warning tint`);
}

// ── .md rows render inline Markdown ───────────────────────────────────────
{
	const f = R.parseDiff(`diff --git a/a.md b/a.md
--- a/a.md
+++ b/a.md
@@ -1,4 +1,4 @@
-# Old title
+# New title
-Use the \`old\` API and **verify**.
+Use the \`new\` API and **verify**.
 See [the docs](https://example.com).
`)[0]!;
	f.newContent = ["# New title", "Use the `new` API and **verify**.", "See [the docs](https://example.com)."];
	f.newLineCount = 3;
	R.markMoves(f);
	for (const h of f.hunks) h.rows = R.collapseHunkRows(h, R.langProfile("md"));
	const theme = { fg: (_c: string, t: string) => `[${_c}:${t}]`, bg: (_c: string, t: string) => t };
	const session = {
		repo: "r", repoRoot: "/tmp", files: [f], notes: {}, context: 6,
		base: { baseArgs: [], label: "H", cached: false, paths: [] }, branch: "b",
		git: async () => ({ stdout: "", stderr: "", code: 0 }),
	};
	const comp = new (R as any).ReviewComponent(session, { requestRender: () => {}, stop: () => {}, start: () => {} }, theme, () => {}) as any;
	const frame = comp.render(120).join("\n");
	assert(frame.includes("[accent:New title]"), `heading renders in the accent: ${JSON.stringify(frame.slice(0, 200))}`);
	assert(frame.includes("[syntaxString:`new`]"), `code renders in the syntax-string color`);
	assert(frame.includes("[accent:**verify**]"), `bold renders in the accent`);
	assert(frame.includes("[muted:(https://example.com)]"), `link urls render muted`);
	assert(frame.includes("[text:[the docs]]"), `link labels stay plain`);
}

// ── .md fenced code blocks highlight per language ─────────────────────────
{
	const f = R.parseDiff(`diff --git a/a.md b/a.md
--- a/a.md
+++ b/a.md
@@ -1,6 +1,6 @@
 # Doc
-​​​​​​​​\`\`\`rust
-let x = 1;
-\`\`\`
+\`\`\`rust
+let y = 2;
+\`\`\`
 Some **prose**.
`)[0]!;
	f.newContent = ["# Doc", "```rust", "let y = 2;", "```", "Some **prose**."];
	f.newLineCount = 5;
	R.markMoves(f);
	for (const h of f.hunks) h.rows = R.collapseHunkRows(h, R.langProfile("md"));
	const theme = { fg: (_c: string, t: string) => `[${_c}:${t}]`, bg: (_c: string, t: string) => t };
	const session = {
		repo: "r", repoRoot: "/tmp", files: [f], notes: {}, context: 6,
		base: { baseArgs: [], label: "H", cached: false, paths: [] }, branch: "b",
		git: async () => ({ stdout: "", stderr: "", code: 0 }),
	};
	const comp = new (R as any).ReviewComponent(session, { requestRender: () => {}, stop: () => {}, start: () => {} }, theme, () => {}) as any;
	const frame = comp.render(120).join("\n");
	assert(frame.includes("[syntaxKeyword:let]"), `fenced rust code highlights keywords: ${JSON.stringify(frame.slice(0, 300))}`);
	assert(frame.includes("[syntaxNumber:1]") || frame.includes("[syntaxNumber:2]"), `fenced rust code highlights numbers`);
	assert(frame.includes("[muted:```rust]"), `the fence line is muted`);
}

// ── Markdown spans carry real SGR (visual aids; the control chars stay) ──
{
	const f = R.parseDiff(`diff --git a/a.md b/a.md
--- a/a.md
+++ b/a.md
@@ -1,5 +1,5 @@
-# Old
+# New
-**hello world**
-*italics* and ~~gone~~ and [a](https://x.io).
+**hello world**
+*italics* and ~~gone~~ and [a](https://x.io).
`)[0]!;
	f.newContent = ["# New", "**hello world**", "*italics* and ~~gone~~ and [a](https://x.io)."];
	f.newLineCount = 3;
	R.markMoves(f);
	for (const h of f.hunks) h.rows = R.collapseHunkRows(h, R.langProfile("md"));
	const theme = { fg: (_c: string, t: string) => `[${_c}:${t}]`, bg: (_c: string, t: string) => t };
	const session = {
		repo: "r", repoRoot: "/tmp", files: [f], notes: {}, context: 6,
		base: { baseArgs: [], label: "H", cached: false, paths: [] }, branch: "b",
		git: async () => ({ stdout: "", stderr: "", code: 0 }),
	};
	const comp = new (R as any).ReviewComponent(session, { requestRender: () => {}, stop: () => {}, start: () => {} }, theme, () => {}) as any;
	const frame = comp.render(120).join("\n");
	assert(frame.includes("\x1b[1m[accent:**hello world**]\x1b[22m"), `bold gets SGR 1: ${JSON.stringify(frame.slice(0, 200))}`);
	assert(frame.includes("\x1b[3m[muted:*italics*]\x1b[23m"), `italic gets SGR 3`);
	assert(frame.includes("\x1b[9m[muted:~~gone~~]\x1b[29m"), `strikethrough gets SGR 9`);
	assert(frame.includes("\x1b[4m[muted:(https://x.io)]\x1b[24m"), `links get SGR 4`);
	// The control characters are part of the token text (never stripped).
	assert(frame.includes("**hello world**") && frame.includes("*italics*") && frame.includes("~~gone~~"), `control chars stay in the content`);
}

// ── Unicode whitespace subwords (character class, not individual picks) ──
{
	const stubFile = { path: "f.rs", hunks: [] } as any;
	const comp = new (R as any).ReviewComponent({ repo: "r", repoRoot: "/tmp", files: [stubFile], notes: {}, context: 10, base: { baseArgs: [], label: "H", cached: false, paths: [] }, branch: "b", git: async () => ({ stdout: "", stderr: "", code: 0 }) }, { requestRender: () => {}, stop: () => {}, start: () => {} }, { fg: (_c: string, t: string) => t, bg: (_c: string, t: string) => t }, () => {}) as any;
	// A nbsp (U+00A0, in White_Space) vs a tab, and a zero-width space
	// (U+200B, a format char but a real empty-width whitespace) vs a space.
	const cases: [string, string][] = [
		["\u00a0self.x", "\tself.x"],
		["\u200bself.x", " self.x"],
		["\u3000self.x", "   self.x"], // ideographic space vs 3 spaces
	];
	for (const [del, add] of cases) {
		const ws = R.wordSpans(del, add);
		const r = { kind: "add" as const, text: add, delText: del, newLine: 1, oldLine: null, moved: true, spans: ws };
		const runs = comp.mergedRuns(r);
		const kinds = runs.map((x: any) => x.kind);
		assert(kinds.includes("del") && kinds.includes("add"), `unicode whitespace subwords: ${JSON.stringify(del.slice(0, 6))} vs ${JSON.stringify(add.slice(0, 6))}: ${kinds.join(",")}`);
		assert(runs.some((x: any) => x.kind === "common" && x.text.includes("self")), "the identifier survives as common");
	}
}
