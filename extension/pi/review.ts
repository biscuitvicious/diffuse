/**
 * Code Review extension for pi.
 *
 * `/review [base] [--cached] [--] [path...]` — interactive review of a git
 * diff:
 *
 *   /review                 worktree vs HEAD (default)
 *   /review --cached        index (staged) vs HEAD
 *   /review main            worktree vs a ref
 *   /review main..HEAD      a committed range
 *   /review -- src/tracer/  path-filtered
 *
 * - Colorized, hunk-focused diff browser (context around each hunk is
 *   expandable/collapsible upwards and downwards).
 * - Exact line-range selection that can cross hunks (clamped to file
 *   top/bottom), with notes attached to (file, line-range).
 * - Notes persist across sessions (`~/.pi/review/notes.json`, keyed by repo)
 *   and render inline into the diff on every launch.
 * - Drop into your editor at the cursor line (`o`), with the selected range
 *   pre-selected where the editor supports it.
 * - Deep git: stage/unstage/discard the current hunk or whole file. Discards
 *   are safety-stashed first (`git stash create`; the ref is reported).
 *
 * Keys:
 *   j/k        move row (J/K: 5 rows)      g/G        top/bottom (N+G: line N)
 *   h/l ^/$    horizontal scroll ±10 cols / to start / to end
 *   n/p        next/previous hunk (also ]/[)   tab/shift+tab  next/prev file
 *   v          start/end line-range selection (j/k to extend, esc to cancel)
 *   m          add note on selection (or cursor row)
 *   d          delete note(s) on selection (or cursor row)
 *   o          open editor at cursor (range pre-selected where supported)
 *   e/E        expand hunk context downward/upward   c/C  collapse
 *   a/A        expand current/all hunks to full context
 *   r/R        reset current/all hunk context to the default
 *   w          toggle word-level diff fill      t  toggle syntax highlighting
 *   mouse      wheel scrolls; press-and-drag scrolls (touch) — SGR mouse
 *              tracking (1002+1006) is enabled for the session and disabled
 *              on close. If nothing moves, your terminal does not forward
 *              mouse events (the status line shows "mouse <btn>@x,y" when it
 *              does); the h/l/^/$ keys always work.
 *   s/S        stage hunk / file          u/U  unstage hunk / file
 *   x/X        discard hunk / file (safety-stashed)
 *   ?          popup key-binding overlay       q/esc  quit
 *
 * Editor invocation — a per-binary pattern table covers the common styles:
 *   vim family   `+{line} {file}`                  (vim/vi/nvim/nano/micro/emacs)
 *   positional   `{file}:{line}`                   (helix/hx/zed/fresh)
 *   goto         `--goto {file}:{line}:1`          (code/codium/cursor)
 *   GUI editors  fire-and-forget, no TUI takeover  (zed/code/antigravity/cursor)
 * Range pre-selection is supported for the vim family (visual line mode).
 * Unknown editors fall back to the vim style.
 *
 * Config (env):
 *   REVIEW_CONTEXT       default context lines around each hunk (default 6)
 *   REVIEW_EDITOR        editor binary or full path (default $VISUAL/$EDITOR → vim)
 *   REVIEW_EDITOR_ARGS   argv template override: {bin} {file} {line} {start} {end} {count}
 *                        (space-separated; quote args that contain spaces)
 *   REVIEW_EDITOR_RANGE_ARGS  argv template used when a range is selected
 */

import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { Key, matchesKey, truncateToWidth, visibleWidth } from "@earendil-works/pi-tui";

// ── Row-type theme ─────────────────────────────────────────────────────────

/**
 * The four diff row types — the extension's theme. `tint` is the row's
 * background in the neutral state, `cursor` the selected lift, `fill` the
 * word-level fill (the mod rows' added words), `icon` the prefix character,
 * and `fg` the pi theme key for the row's own text. Configure the rows
 * here: the moved icon is the "double wave" `≈` by default — swap in a
 * printable fallback if your terminal cannot render it.
 */
interface RowStyle {
	icon: string;
	tint: number;
	cursor: number;
	fill?: number;
	fillCursor?: number;
	fg: string;
}

const ROW_STYLES: Record<"add" | "del" | "mod" | "mov", RowStyle> = {
	add: { icon: "+", tint: 22, cursor: 28, fill: 28, fillCursor: 34, fg: "toolDiffAdded" },
	del: { icon: "-", tint: 52, cursor: 88, fg: "toolDiffRemoved" },
	mod: { icon: "~", tint: 236, cursor: 237, fill: 28, fillCursor: 34, fg: "toolDiffContext" },
	mov: { icon: "≈", tint: 236, cursor: 237, fg: "muted" },
};
// The 3-letter key set (add/del/mod/mov) keeps the table a parallel unit —
// and `mov` never collides with a row's `moved` boolean flag.
const sAdd = ROW_STYLES.add;
const sDel = ROW_STYLES.del;
const sMod = ROW_STYLES.mod;
const sMov = ROW_STYLES.mov;

/**
 * Inline Markdown for the note lines: `` `code` `` (the syntax-string
 * color), `**bold**` (the accent), `*italic*` (muted); everything else
 * stays in the note's warning tint. Review notes are short — nested
 * markup is not worth parsing; each top-level span is styled and
 * concatenated.
 */
function renderMarkdown(text: string, fg: (c: string, t: string) => string): string {
	// A fenced code block (the note is, or starts with, one): the fence
	// line muted, the code with the fence language's syntax highlighting.
	const fence = /^\s*```([A-Za-z0-9_+-]*)\s*$/.exec(text);
	if (fence) {
		const lang = fence[1]!.toLowerCase();
		const rest = text.slice(fence[0].length);
		const body = rest.startsWith("\n") ? rest.slice(1) : rest;
		const end = body.lastIndexOf("```");
		const code = end >= 0 ? body.slice(0, end) : body;
		const profile = langProfile(lang);
		const codeOut = profile
			? classifyTokens(code, profile).map((t) => fg(t.color, t.text)).join("")
			: fg("text", code);
		return fg("muted", fence[0]) + "\n" + codeOut + (end >= 0 ? "\n" + fg("muted", "```") : "");
	}
	const re = /(`[^`\n]+`|~~[^~\n]+~~|\*\*[^*\n]+\*\*|\*[^*\s\n][^*\n]*\*|\[[^\]\n]+\]\([^)\n]+\))/g;
	const sgr = (on: number, t: string): string => `\x1b[${on}m${t}\x1b[${SGR_OFF[on]!}m`;
	let out = "";
	let last = 0;
	for (const m of text.matchAll(re)) {
		const idx = m.index ?? 0;
		if (idx > last) out += fg("warning", text.slice(last, idx));
		const tok = m[0]!;
		if (tok.startsWith("`")) out += fg("syntaxString", tok);
		else if (tok.startsWith("~~")) out += sgr(MD_SGR.strike, fg("muted", tok));
		else if (tok.startsWith("**")) out += sgr(MD_SGR.bold, fg("accent", tok));
		else if (tok.startsWith("[")) {
			const close = tok.indexOf("](");
			out += sgr(MD_SGR.underline, fg("text", tok.slice(0, close + 1))) + sgr(MD_SGR.underline, fg("muted", tok.slice(close + 1)));
		} else out += sgr(MD_SGR.italic, fg("muted", tok));
		last = idx + tok.length;
	}
	if (last < text.length) out += fg("warning", text.slice(last));
	return out;
}

/** SGR-on codes per markdown span (visual aids only — the control chars
 * stay in the content, the diff text is unchanged). */
const MD_SGR = { bold: 1, italic: 3, underline: 4, strike: 9 } as const;
const SGR_OFF: Record<number, number> = { 1: 22, 3: 23, 4: 24, 9: 29 };

/**
 * Inline Markdown tokens for `.md` file rows: `#`-headings (the marker
 * muted, the title in the accent + bold), `` `code` `` (syntax-string),
 * `**bold**` (accent + bold), `*italic*` (muted + italic), `~~strike~~`
 * (muted + strikethrough), and `[text](url)` links (the label plain +
 * underline, the url muted + underline). The `sgr` field carries the
 * SGR-on code; the control characters are part of the token text — a
 * diffing utility must show them.
 */
function markdownTokens(text: string): { color: string; text: string; sgr?: number }[] {
	const heading = /^(#{1,6})(\s+)(.*)$/.exec(text);
	if (heading) {
		return [
			{ color: "muted", text: heading[1]! + heading[2]! },
			{ color: "accent", sgr: MD_SGR.bold, text: heading[3]! },
		];
	}
	const out: { color: string; text: string; sgr?: number }[] = [];
	let last = 0;
	const re = /(`[^`\n]+`|~~[^~\n]+~~|\*\*[^*\n]+\*\*|\*[^*\s\n][^*\n]*\*|\[[^\]\n]+\]\([^)\n]+\))/g;
	let m: RegExpExecArray | null;
	while ((m = re.exec(text))) {
		if (m.index > last) out.push({ color: "text", text: text.slice(last, m.index) });
		const tok = m[0]!;
		if (tok.startsWith("`")) out.push({ color: "syntaxString", text: tok });
		else if (tok.startsWith("~~")) out.push({ color: "muted", sgr: MD_SGR.strike, text: tok });
		else if (tok.startsWith("**")) out.push({ color: "accent", sgr: MD_SGR.bold, text: tok });
		else if (tok.startsWith("[")) {
			const close = tok.indexOf("](");
			out.push({ color: "text", sgr: MD_SGR.underline, text: tok.slice(0, close + 1) });
			out.push({ color: "muted", sgr: MD_SGR.underline, text: tok.slice(close + 1) });
		} else out.push({ color: "muted", sgr: MD_SGR.italic, text: tok });
		last = m.index + tok.length;
	}
	if (last < text.length) out.push({ color: "text", text: text.slice(last) });
	return out;
}
import { spawn, spawnSync } from "node:child_process";
import { randomUUID } from "node:crypto";
import { existsSync, mkdirSync, readFileSync, unlinkSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

// ── Config ─────────────────────────────────────────────────────────────────

const DEFAULT_CONTEXT = 6;
const NOTES_DIR = join(homedir(), ".pi", "review");
const NOTES_FILE = join(NOTES_DIR, "notes.json");

function defaultContext(): number {
	const n = Number(process.env.REVIEW_CONTEXT);
	return Number.isFinite(n) && n >= 0 ? n : DEFAULT_CONTEXT;
}

// ── Data model ─────────────────────────────────────────────────────────────

type RowKind = "add" | "del" | "ctx" | "sep" | "file" | "mod";

interface Row {
	kind: RowKind;
	text: string;
	/** Line in the NEW file (null for pure-deletion rows). */
	newLine: number | null;
	/** Line in the OLD file (null for pure-addition rows). */
	oldLine: number | null;
	/** Cached word-level change spans (char offsets into `text`). */
	spans?: { del: [number, number][]; add: [number, number][] };
	/** Exact-content match on the other side: the line moved, it did not change. */
	moved?: boolean;
	/** The paired old text (mod rows) — for the struck-through removed words. */
	delText?: string;
	/** Pure-add lines absorbed into a mod row's section (until the next blank). */
	tail?: Row[];
	/** The diffuse backend's precomputed token stream (REVIEW_BACKEND=rust):
	 * the merged common/del/add runs with the semantic syntax colors — the
	 * renderer uses these instead of recomputing `mergedTokens`. */
	runs?: { kind: "common" | "del" | "add"; color?: string; text: string }[];
}

interface Hunk {
	oldStart: number;
	oldCount: number;
	newStart: number;
	newCount: number;
	section: string;
	/** Base rows (add/del only, parsed from `git diff -U0`). */
	rows: Row[];
	aboveCtx: number;
	belowCtx: number;
}

/** Row-derived new-side count: the *extent* from the hunk start to the last
 * new-side line, not a row count — absorbed reflow lines and tail lines
 * occupy real positions in between (a restructure that drops two reflowed
 * lines still spans them). Accurate for the @ header counts too. */
function addCount(h: Hunk): number {
	let maxNew = h.newStart;
	let hasNew = false;
	const scan = (r: Row) => {
		if (r.kind === "add" || r.kind === "mod") {
			// A stretched mod's text is its run's concatenation — it spans
			// its adds' visual lines (`if k < floor {` → the 3-line
			// `if let Some(floor) = floor` / `&& k > floor` / `{`). The mod
			// must cover all of them: otherwise the context rows after the
			// hunk re-emit the mod's own tail lines (the dup `&& k > floor`
			// + `{` under the mod).
			const lines = (r.text.match(/\n/g) ?? []).length;
			maxNew = Math.max(maxNew, (r.newLine ?? 0) + lines);
			hasNew = true;
		}
		for (const t of r.tail ?? []) {
			maxNew = Math.max(maxNew, t.newLine ?? 0);
			hasNew = true;
		}
	};
	for (const r of h.rows) scan(r);
	return hasNew ? Math.max(1, maxNew - h.newStart + 1) : 0;
}

function delCount(h: Hunk): number {
	return h.rows.filter((r) => r.kind === "del").length;
}

type FileStatus = "added" | "deleted" | "modified" | "renamed" | "untracked";

interface DiffFile {
	/** Repo-relative path of the NEW side (old path for deletions). */
	path: string;
	oldPath: string | null;
	status: FileStatus;
	newLineCount: number;
	oldLineCount: number;
	/** Content of the new side (worktree), for context synthesis. */
	newContent: string[];
	/** Content of the old side (HEAD/ref), for deleted-file context. */
	oldContent: string[];
	hunks: Hunk[];
	/** True when neither side's content could be loaded (no context source). */
	noContent?: boolean;
}

interface ReviewNote {
	id: string;
	file: string;
	side: "new" | "old";
	start: number;
	end: number;
	text: string;
	createdAt: number;
}

type NotesDB = Record<string, Record<string, ReviewNote[]>>;

// ── Git plumbing ───────────────────────────────────────────────────────────

type GitRunner = (args: string[]) => Promise<{ stdout: string; stderr: string; code: number }>;

function makeGitRunner(pi: ExtensionAPI, cwd: string): GitRunner {
	return async (args) => {
		const res = await pi.exec("git", args, { cwd });
		return { stdout: res.stdout, stderr: res.stderr, code: res.code ?? 1 };
	};
}

/** Parsed `/review` command-line: base selection + path filter. */
interface ReviewBase {
	/** git diff args for the base: e.g. [] (HEAD) | ["--cached"] | ["main"] */
	baseArgs: string[];
	label: string;
	cached: boolean;
	paths: string[];
}

function parseReviewArgs(args: string | undefined, cwd: string, repoPrefix = ""): ReviewBase {
	const tokens = args ? args.split(/\s+/).filter(Boolean) : [];
	const baseArgs: string[] = [];
	const paths: string[] = [];
	let cached = false;
	let afterDash = false;
	// Convert cwd-relative path args to repo-relative (git runs in the root).
	const toRepoPath = (p: string): string => (repoPrefix && !p.startsWith("/") ? join(repoPrefix, p) : p);
	for (const t of tokens) {
		if (afterDash) {
			paths.push(toRepoPath(t));
		} else if (t === "--") {
			afterDash = true;
		} else if (t === "--cached" || t === "-c") {
			cached = true;
			baseArgs.push("--cached");
		} else if (existsSync(join(cwd, t))) {
			paths.push(toRepoPath(t));
		} else {
			baseArgs.push(t); // ref or range
		}
	}
	const label = cached ? "staged" : baseArgs.length > 0 ? baseArgs.join(" ") : "HEAD";
	return { baseArgs, label, cached, paths };
}

// ── Diff model loading ─────────────────────────────────────────────────────

/**
 * Build the review model: parsed diff (worktree vs base) + untracked files
 * (worktree review only) + per-file content for context synthesis.
 */
// ── The diffuse backend (REVIEW_BACKEND=rust) ──────────────────────────────
// The engine pipeline runs in the diffuse binary; the extension renders its
// token-stream JSON. The TS engine stays the default until the rust backend
// is proven byte-identical (the golden's rust leg) and manually reviewed.

/** The diffuse binary: REVIEW_DIFFUSE_BIN overrides; the default is the
 * binary BUNDLED with this extension (`bin/diffuse` next to review.ts —
 * the extension ships with the engine it was tested against), with the
 * workspace's built target as a dev fallback. */
function diffuseBinPath(): string {
	const explicit = process.env.REVIEW_DIFFUSE_BIN;
	if (explicit) return explicit;
	const here = dirname(fileURLToPath(import.meta.url));
	const profile = process.env.REVIEW_PROFILE === "release" ? "release" : "debug";
	const bundled = join(here, "bin", "diffuse");
	if (existsSync(bundled)) return bundled;
	return join(here, "..", "..", "..", "target", profile, "diffuse");
}

/** The wire contract (§2): the bin's token-stream JSON. */
interface WireOutput {
	files: {
		path: string;
		status: string;
		newLineCount: number;
		oldLineCount: number;
		hunks: {
			oldStart: number;
			oldCount: number;
			newStart: number;
			newCount: number;
			rows: {
				kind: "add" | "del" | "mod";
				newLine: number | null;
				oldLine: number | null;
				text: string;
				delText?: string;
				spans: { del: [number, number][]; add: [number, number][] };
				runs: { kind: "common" | "del" | "add"; color?: string; text: string }[];
				moved: boolean;
			tail: { newLine: number | null; oldLine: number | null; text: string; moved: boolean }[];
			}[];
		}[];
	}[];
}

/** Spawn the diffuse bin with the wire input on stdin; resolve with the
 * decoded token-stream JSON. */
function runDiffuseBin(bin: string, input: string): Promise<WireOutput> {
	return new Promise((resolve, reject) => {
		const p = spawn(bin, [], { stdio: ["pipe", "pipe", "pipe"] });
		let out = "";
		let err = "";
		p.stdout.on("data", (d: Buffer) => (out += d.toString("utf8")));
		p.stderr.on("data", (d: Buffer) => (err += d.toString("utf8")));
		p.on("error", reject);
		p.on("close", (code) => {
			if (code !== 0) reject(new Error(`diffuse exited ${code}: ${err}`));
			else {
				try {
					resolve(JSON.parse(out) as WireOutput);
			} catch (e) {
				reject(new Error(`diffuse output was not JSON: ${String(e)}`));
			}
		}
		});
		p.stdin.write(input);
		p.stdin.end();
	});
}

/** Convert the diffuse backend's wire output into the extension's DiffFile
 * shape. The content arrays are the renderer's own (buildVisible reads
 * newContent/oldContent for the context rows) — the wire carries only the
 * counts; the caller supplies the content it already loaded. The wire rows
 * carry their precomputed `runs`; `aboveCtx`/`belowCtx` start at the
 * session's context. */
function filesFromWire(
	wire: WireOutput,
	contentOf: (path: string) => { newContent: string[]; oldContent: string[]; oldPath: string | null } | undefined,
	context: number,
): DiffFile[] {
	const out: DiffFile[] = [];
	for (const wf of wire.files) {
		const src = contentOf(wf.path);
		out.push({
			path: wf.path,
			oldPath: src?.oldPath ?? null,
			status: wf.status as FileStatus,
			newLineCount: wf.newLineCount,
			oldLineCount: wf.oldLineCount,
			newContent: src?.newContent ?? [],
			oldContent: src?.oldContent ?? [],
			hunks: wf.hunks.map((h) => ({
				oldStart: h.oldStart,
				oldCount: h.oldCount,
				newStart: h.newStart,
				newCount: h.newCount,
				section: "",
				rows: h.rows.map((r) => ({
					kind: r.kind,
					text: r.text,
					newLine: r.newLine,
					oldLine: r.oldLine,
					spans: r.spans,
					moved: r.moved,
					delText: r.delText,
					tail: (r.tail ?? []).map((t) => ({
						kind: "add",
						text: t.text,
						newLine: t.newLine,
						oldLine: t.oldLine,
						moved: t.moved,
					})),
					runs: r.runs,
				})),
				aboveCtx: context,
				belowCtx: context,
			})),
		});
	}
	return out;
}

async function loadReview(
	git: GitRunner,
	repoRoot: string,
	base: ReviewBase,
	context: number,
): Promise<{ files: DiffFile[]; branch: string; untrackedSkipped: string[] }> {
	const branchRes = await git(["branch", "--show-current"]);
	const branch = branchRes.code === 0 ? branchRes.stdout.trim() : "";

	const diffRes = await git([
		"diff",
		"-M",
		"--no-color",
		"-U0",
		...base.baseArgs,
		"--",
		...base.paths,
	]);
	// The engine parses the diff itself; the extension only enumerates the
	// changed paths (to load the content the renderer reads) and their
	// del-counts (which files need the old side).
	const nameStatus = await git(["diff", "-M", "--name-status", ...base.baseArgs, "--", ...base.paths]);
	const numstat = await git(["diff", "-M", "--numstat", ...base.baseArgs, "--", ...base.paths]);
	const delOf: Record<string, number> = {};
	for (const l of numstat.stdout.split("\n")) {
		const m = /^(\d+)\t(\d+)\t(.+)$/.exec(l);
		if (m) delOf[m[3]!] = Number(m[2]);
	}
	const files: DiffFile[] = [];
	for (const l of nameStatus.stdout.split("\n")) {
		const parts = l.split("\t");
		if (!parts[0]) continue;
		const st = parts[0]!;
		let status: FileStatus = "modified";
		let path = parts[1] ?? "";
		let oldPath: string | null = null;
		if (st.startsWith("A")) status = "added";
		else if (st.startsWith("D")) status = "deleted";
		else if (st.startsWith("R")) {
			status = "renamed";
			oldPath = parts[1] ?? null;
			path = parts[2] ?? "";
		}
		files.push({
			path,
			oldPath,
			status,
			newLineCount: 0,
			oldLineCount: 0,
			newContent: [],
			oldContent: [],
			hunks: [],
			noContent: false,
		});
	}
	const untrackedSkipped: string[] = [];

	// Untracked files are part of a worktree review (not --cached, no ref base
	// that would include them in the index... untracked are never in the index).
	if (!base.cached && base.paths.length === 0) {
		const ut = await git(["ls-files", "--others", "--exclude-standard"]);
		if (ut.code === 0) {
			for (const rel of ut.stdout.split("\n").filter(Boolean)) {
				const abs = join(repoRoot, rel);
				let content: string;
				try {
					content = readFileSync(abs, "utf8");
				} catch {
					continue;
				}
				if (content.includes("\0")) {
					untrackedSkipped.push(rel);
					continue;
				}
				const lines = content.split("\n");
				files.push({
					path: rel,
					oldPath: null,
					status: "untracked",
					newLineCount: lines.length,
					oldLineCount: 0,
					newContent: lines,
					oldContent: [],
					hunks: [
						{
							oldStart: 0,
							oldCount: 0,
							newStart: 1,
							newCount: lines.length,
							section: "new file",
							rows: lines.map((t, i) => ({ kind: "add", text: t, newLine: i + 1, oldLine: null })),
							aboveCtx: 0,
							belowCtx: 0,
						},
					],
				});
			}
		}
	}

	return {
		files: await rustBackendLoad(git, repoRoot, files, diffRes.stdout, context, delOf),
		branch,
		untrackedSkipped,
	};
}

/** The rust backend path: the same git + content loading, but the engine
 * pipeline (parse → moves → collapse → runs) runs in the diffuse binary.
 * The extension keeps the content it loads (buildVisible reads it) and the
 * untracked synthesis; the wire supplies the collapsed rows + token runs.
 * `delOf` is the per-file del count (from `git diff --numstat`) — which
 * files need the old side for del-only context. */
async function rustBackendLoad(
	git: GitRunner,
	repoRoot: string,
	tsFiles: DiffFile[],
	diffText: string,
	context: number,
	delOf: Record<string, number>,
): Promise<DiffFile[]> {
	// New-side content (the same loading the TS path does).
	for (const f of tsFiles) {
		const newAbs = join(repoRoot, f.path);
		if (f.status !== "deleted" && existsSync(newAbs)) {
			try {
				f.newContent = readFileSync(newAbs, "utf8").split("\n");
			} catch {
				f.newContent = [];
			}
		}
		f.newLineCount = f.newContent.length;
	}
	// Old-side content: needed for del-only hunks + as a context fallback.
	for (const f of tsFiles) {
		const newEmpty = f.newContent.length === 0;
		const needsOld = f.status === "deleted" || (delOf[f.path] ?? 0) > 0 || newEmpty;
		if (!needsOld) continue;
		const oldPath = f.oldPath ?? f.path;
		const oldRes = await git(["show", `HEAD:${oldPath}`]);
		if (oldRes.code === 0) {
			f.oldContent = oldRes.stdout.split("\n");
			f.oldLineCount = f.oldContent.length;
			// Fallback: without new content, del-side context still renders.
			if (newEmpty && f.status !== "deleted") {
				f.newContent = f.oldContent;
				f.newLineCount = f.oldLineCount;
			}
		}
		f.noContent = f.newContent.length === 0 && f.oldContent.length === 0;
	}
	// The wire input: the diff + the new-side contents (the fixture shape).
	const contents: Record<string, string> = {};
	for (const f of tsFiles) contents[f.path] = f.newContent.join("\n");
	const wire = await runDiffuseBin(diffuseBinPath(), JSON.stringify({ diff: diffText, contents }));
	const files = filesFromWire(
		wire,
		(p) => {
			const f = tsFiles.find((x) => x.path === p);
			return f
				? { newContent: f.newContent, oldContent: f.oldContent, oldPath: f.oldPath }
				: undefined;
		},
		context,
	);
	// The untracked pseudo-files are TS-synthesized (whole-file adds) —
	// appended after the tracked files, matching the TS path's order.
	for (const f of tsFiles) {
		if (f.status === "untracked") files.push(f);
	}
	return files;
}

// ── Persistence ────────────────────────────────────────────────────────────

function loadNotes(): NotesDB {
	try {
		return JSON.parse(readFileSync(NOTES_FILE, "utf8")) as NotesDB;
	} catch {
		return {};
	}
}

function saveNotes(db: NotesDB): void {
	try {
		mkdirSync(NOTES_DIR, { recursive: true });
		writeFileSync(NOTES_FILE, JSON.stringify(db, null, 1));
	} catch (err) {
		console.error("review: failed to persist notes:", err);
	}
}

function notesFor(db: NotesDB, repo: string, file: string): ReviewNote[] {
	return db[repo]?.[file] ?? [];
}

function allNotes(db: NotesDB, repo: string): ReviewNote[] {
	return Object.values(db[repo] ?? {}).flat();
}

function addNote(db: NotesDB, repo: string, note: ReviewNote): void {
	(db[repo] ??= {});
	(db[repo][note.file] ??= []).push(note);
	saveNotes(db);
}

/** Resume position: last (file, line, col, line-text) per repo. */
interface ReviewState {
	file: string;
	line: number;
	col: number;
	text: string;
}

const STATE_FILE = join(NOTES_DIR, "state.json");

function loadReviewState(repo: string): ReviewState | null {
	try {
		const all = JSON.parse(readFileSync(STATE_FILE, "utf8")) as Record<string, ReviewState>;
		return all[repo] ?? null;
	} catch {
		return null;
	}
}

function saveReviewState(repo: string, state: ReviewState): void {
	try {
		mkdirSync(NOTES_DIR, { recursive: true });
		let all: Record<string, ReviewState> = {};
		try {
			all = JSON.parse(readFileSync(STATE_FILE, "utf8"));
		} catch {
			all = {};
		}
		all[repo] = state;
		writeFileSync(STATE_FILE, JSON.stringify(all, null, 1));
	} catch (err) {
		console.error("review: failed to persist position:", err);
	}
}

/**
 * Compute the resume position from the stored state. Full restore (same
 * file + line + col) only when the stored line's content still matches;
 * when the line moved or changed, fall back to the same file (line: null →
 * the file's first row). Nothing to restore when the file left the diff.
 */
function resolveInitialPosition(
	files: DiffFile[],
	state: ReviewState | null,
): { fileIdx: number; line: number | null; col: number } | null {
	if (!state) return null;
	const fi = files.findIndex((f) => f.path === state.file);
	if (fi < 0) return null;
	const content = files[fi]!.newContent[state.line - 1];
	if (content !== undefined && content === state.text) {
		return { fileIdx: fi, line: state.line, col: state.col ?? 0 };
	}
	return { fileIdx: fi, line: null, col: 0 };
}

function removeNotes(
	db: NotesDB,
	repo: string,
	file: string,
	side: "new" | "old",
	start: number,
	end: number,
): number {
	const list = db[repo]?.[file];
	if (!list) return 0;
	const before = list.length;
	db[repo][file] = list.filter((n) => !(n.side === side && n.start <= end && n.end >= start));
	saveNotes(db);
	return before - db[repo][file].length;
}

// ── Diff parsing ───────────────────────────────────────────────────────────

function allocateContext(f: DiffFile): { above: number; below: number }[] {
	const alloc: { above: number; below: number; freeBelow: number }[] = [];
	// Pass 1: each hunk clamps its own above/below to the file bounds and to
	// the free space between base hunk regions (independent).
	for (let i = 0; i < f.hunks.length; i++) {
		const h = f.hunks[i];
		const newSide = addCount(h) > 0;
		const start = newSide ? h.newStart : h.oldStart;
		const count = newSide ? addCount(h) : delCount(h);
		const total = newSide ? f.newLineCount : f.oldLineCount;
		if (total === 0) {
			alloc.push({ above: 0, below: 0, freeBelow: 0 });
			continue;
		}
		let freeAbove = start - 1;
		if (i > 0) {
			const p = f.hunks[i - 1];
			const pNew = addCount(p) > 0;
			if (pNew === newSide) {
				const pEnd = (pNew ? p.newStart : p.oldStart) + (pNew ? addCount(p) : delCount(p)) - 1;
				freeAbove = Math.min(freeAbove, Math.max(0, start - pEnd - 1));
			}
		}
		let freeBelow = total - (start + count) + 1;
		if (i < f.hunks.length - 1) {
			const n = f.hunks[i + 1];
			const nNew = addCount(n) > 0;
			if (nNew === newSide) {
				const nStart = nNew ? n.newStart : n.oldStart;
				freeBelow = Math.min(freeBelow, Math.max(0, nStart - (start + count)));
			}
		}
		alloc.push({
			above: Math.min(h.aboveCtx, freeAbove),
			below: Math.min(h.belowCtx, freeBelow),
			freeBelow,
		});
	}
	// Pass 2: same-side adjacent hunks compete for the gap between them —
	// the first hunk's below-context wins; the next hunk's above-context
	// shrinks to what remains, so a same-side gap smaller than the context
	// can never produce a "lines hidden" separator. Mixed-side pairs share
	// no lines (old side vs new side) and need no reconciliation.
	for (let i = 0; i < f.hunks.length - 1; i++) {
		const a = f.hunks[i];
		const b = f.hunks[i + 1];
		if ((addCount(a) > 0) === (addCount(b) > 0)) {
			const gap = gapBetween(f, i);
			alloc[i + 1]!.above = Math.min(
				alloc[i + 1]!.above,
				Math.max(0, gap - alloc[i]!.below),
			);
		}
	}
	return alloc.map(({ above, below }) => ({ above, below }));
}

function contextRows(f: DiffFile, side: "new" | "old", start: number, count: number): Row[] {
	const src = side === "new" ? f.newContent : f.oldContent;
	const rows: Row[] = [];
	for (let i = 0; i < count; i++) {
		const lineNo = start + i;
		rows.push({
			kind: "ctx",
			text: src[lineNo - 1] ?? "",
			newLine: side === "new" ? lineNo : null,
			oldLine: side === "old" ? lineNo : null,
		});
	}
	return rows;
}

/**
 * Lines strictly between hunk i's base end and hunk i+1's base start.
 * Same-side hunks use their exact side; mixed-side pairs estimate with the
 * larger of the two sides' gaps (the side where context can show more).
 */
function gapBetween(f: DiffFile, i: number): number {
	const a = f.hunks[i];
	const b = f.hunks[i + 1];
	const aNew = addCount(a) > 0;
	const bNew = addCount(b) > 0;
	if (aNew === bNew) {
		const aEnd = (aNew ? a.newStart : a.oldStart) + (aNew ? addCount(a) : delCount(a)) - 1;
		const bStart = bNew ? b.newStart : b.oldStart;
		return Math.max(0, bStart - aEnd - 1);
	}
	const newGap = Math.max(0, b.newStart - (a.newStart + addCount(a)) - 1);
	const oldGap = Math.max(0, b.oldStart - (a.oldStart + delCount(a)) - 1);
	const newC = f.newLineCount > 0 ? f.newLineCount : Number.MAX_SAFE_INTEGER;
	const oldC = f.oldLineCount > 0 ? f.oldLineCount : Number.MAX_SAFE_INTEGER;
	return Math.max(Math.min(newGap, newC), Math.min(oldGap, oldC));
}

function hiddenGap(f: DiffFile, i: number, alloc: { above: number; below: number }[]): number {
	return Math.max(0, gapBetween(f, i) - alloc[i].below - alloc[i + 1].above);
}

function buildVisible(f: DiffFile): Row[] {
	const alloc = allocateContext(f);
	const rows: Row[] = [];
	for (let i = 0; i < f.hunks.length; i++) {
		const h = f.hunks[i];
		if (i > 0) {
			const hidden = hiddenGap(f, i - 1, alloc);
			if (hidden > 0) {
				const collapsed = alloc[i - 1]!.below === 0 && alloc[i]!.above === 0 ? " (e/E to expand, r to reset)" : "";
				const unavailable = f.noContent ? " (content unavailable — expand in the editor)" : "";
				rows.push({ kind: "sep", text: `⋯ ${hidden} line${hidden === 1 ? "" : "s"} hidden${collapsed}${unavailable}`, newLine: null, oldLine: null });
			}
		}
		const a = alloc[i];
		const newSide = addCount(h) > 0;
		const start = newSide ? h.newStart : h.oldStart;
		const count = newSide ? addCount(h) : delCount(h);
		const lead = absorbLeadingContext(f, newSide ? "new" : "old", start, a.above, h);
		rows.push(...lead.above);
		rows.push(...lead.rows);
		rows.push(...absorbClosingContext(f, newSide ? "new" : "old", start, count, a.below, h));
	}
	return rows;
}

/**
 * The up-shift mirror of `absorbClosingContext`: absorb the above-context
 * rows adjacent to the hunk's start (a doc comment that belongs to the
 * changed block — the block reads as one unit) while releasing an equal
 * number of the hunk's own trailing rows — only when the released lines
 * EXACTLY match the absorbed lines (the shift is content-neutral; the
 * coverage moves up by K, the change stays identical).
 */
function absorbLeadingContext(
	f: DiffFile,
	side: "new" | "old",
	start: number,
	above: number,
	h: Hunk,
): { above: Row[]; rows: Row[] } {
	const ctx = contextRows(f, side, start - above, above);
	const rows = h.rows;
	if (ctx.length === 0) return { above: ctx, rows };
	for (let k = 1; k <= Math.min(3, rows.length, ctx.length); k++) {
		const released = rows.slice(rows.length - k);
		const absorbed = ctx.slice(ctx.length - k);
		// The up-shift's purpose is the block's START — the doc comment that
		// belongs to the changed code — so the absorbed lines must begin
		// with a comment (a previous function's closing brace is not a flow
		// win, and a blank-only trade moves nothing), and the released lines
		// must exactly match them (the shift is content-neutral).
		const commentLead = (r: Row) => {
			const t = r.text.trim();
			return t.startsWith("//") || t.startsWith("/*") || t.startsWith("*") || t.startsWith("#");
		};
		if (commentLead(absorbed[0]!) && released.every((r, i) => r.text === absorbed[i]!.text)) {
			return {
				above: absorbed.map((r) => ({ kind: "ctx" as const, text: r.text, newLine: r.newLine, oldLine: null })),
				rows: rows.slice(0, rows.length - k),
			};
		}
	}
	return { above: ctx, rows }; // no shift: the plain above-context
}

/**
 * The context rows immediately after a hunk can be the CLOSING of an added
 * block the diff ended early — a new method's `}` and the blank after it,
 * left as context by the diff's own alignment. Absorbing them is a DOWNWARD
 * SHIFT of the hunk's coverage, and a shift is only valid when the lines
 * released ABOVE exactly match the lines absorbed BELOW (the change stays
 * identical). The released lines are the mod's absorbed section at the
 * hunk's start — the unchanged closing+blank of the previous function. If
 * those do not textually equal the leading context rows after the hunk, the
 * shift would alter the change: no absorption.
 */
function absorbClosingContext(
	f: DiffFile,
	side: "new" | "old",
	start: number,
	count: number,
	below: number,
	h: Hunk,
): Row[] {
	const ctx = contextRows(f, side, start + count, below);
	if (ctx.length === 0) return ctx;
	const lastRow = h.rows[h.rows.length - 1];
	if (!lastRow || lastRow.kind !== "add") return ctx;
	const released = h.rows[0]?.tail ?? [];
	if (released.length === 0 || released.length > ctx.length) return ctx;
	if (released.every((r) => r.text.trim().length === 0)) return ctx; // blank-trades move nothing meaningful
	for (let i = 0; i < released.length; i++) {
		if (released[i]!.text !== ctx[i]!.text) return ctx;
	}
	const absorbed = ctx.slice(0, released.length).map((r) => ({
		kind: "add" as const,
		text: r.text,
		newLine: r.newLine,
		oldLine: null,
	}));
	return [...absorbed, ...ctx.slice(released.length)];
}

// ── Note ↔ row mapping ─────────────────────────────────────────────────────

function rowCoord(r: Row): number | null {
	return r.newLine ?? r.oldLine;
}

/** Truncate in the middle so the tail (file extension, ref) stays visible. */
function truncateMiddle(text: string, max: number): string {
	if (text.length <= max) return text;
	const keep = Math.max(4, Math.floor(max * 0.55));
	const tail = Math.max(3, Math.floor(max * 0.35));
	return text.slice(0, keep) + "…" + text.slice(text.length - tail);
}

/** Line numbers pad to 4 columns, right-aligned — the gutter stays
 * stable across the scrolldown (1 → "   1"). */
const pad4 = (n: number | null): string => (n === null ? "    " : String(n).padStart(4));

function prefixOf(r: Row): string {
	if (r.kind === "add") return r.moved ? `${pad4(r.newLine)} ${sMov.icon}` : `${pad4(r.newLine)} ${sAdd.icon}`;
	if (r.kind === "del") return r.moved ? `${pad4(r.oldLine)} ${sMov.icon}` : `${pad4(r.oldLine)} ${sDel.icon}`;
	if (r.kind === "mod") return `${pad4(r.newLine)} ${sMod.icon}`;
	return `${pad4(r.newLine ?? r.oldLine)}  `;
}

function prefixLen(r: Row): number {
	return prefixOf(r).length;
}

function rowsCoveredBy(note: ReviewNote, rows: Row[]): number[] {
	const idx: number[] = [];
	rows.forEach((r, i) => {
		const c = rowCoord(r);
		if (c !== null && c >= note.start && c <= note.end) idx.push(i);
	});
	return idx;
}

// ── Word-level diff ────────────────────────────────────────────────────────

/**
 * Tokenize a diff line into word/operator tokens (whitespace + punctuation
 * are separate tokens so `->` → `=>` highlights a single character).
 */
function wholesalePair(delText: string, addText: string, spans: { del: [number, number][]; add: [number, number][] }): boolean {
	const delSpans = spans.del.filter(([ds, de]) => {
		const t = delText.slice(ds, de).trim();
		return t.length > 0 && !addText.includes(t);
	});
	const addSpans = spans.add ?? [];
	// Meaningful identity = subword: any pair recognisably "the same line"
	// renders as the inline mod — fragment-count mismatches included (`0` →
	// `Option<`/`>`/`None`, `lowers`+`.rev()` → `below`, `k <`+` {` →
	// `let Some(…)`). Only the split-word check below (a standalone del
	// landing mid-word) is garble — that one stays wholesale.
	const delPos: number[] = [];
	let cum = 0;
	for (const [ds, de] of delSpans) {
		delPos.push(ds - cum);
		cum += de - ds;
	}
	const pair = new Map<number, number>();
	const usedAdds = new Set<number>();
	for (let j = 0; j < delSpans.length; j++) {
		let best = -1;
		let bestGap = 8;
		for (let i = 0; i < addSpans.length; i++) {
			if (usedAdds.has(i)) continue;
			// Overlapping non-identical ranges interleave in the merge —
			// they stay separate events (same guard as `delPairing`).
			const [as, ae] = addSpans[i]!;
			const [ds, de] = delSpans[j]!;
			const delEnd = delPos[j]! + (de - ds);
			const same = delPos[j]! === as && delEnd === ae;
			const disjoint = delEnd <= as || ae <= delPos[j]!;
			if (!same && !disjoint) continue;
			// Prefer the CONTENT counterpart over a closer punctuation
			// fragment (`0` ↔ `None`, never the type syntax `>` — the strike
			// must read `= ~~0~~None;`).
			// OPERATOR flips pair operator-to-operator with precedence: a
			// `<`→`>` comparison flip is either a bug or the logical twin of
			// another change — it must stand out (`k ~~<~~> floor`), never
			// bury itself inside a nearer word span. The add's operator
			// anchors at its own offset inside the span.
			const delOp = delText.slice(ds, de).trim();
			let opGap = Infinity;
			for (let c = as; c < ae; c++) {
				if ("<>!=+*/-%&|^~".includes(addText[c]!)) {
					opGap = Math.abs(delPos[j]! - c);
					break;
				}
			}
			const gap = Math.min(Math.abs(delPos[j]! - as) + (addText.slice(as, as + 1).match(/[A-Za-z0-9_]/) ? 0 : 5), opGap);
			if (gap <= bestGap) {
				bestGap = gap;
				best = i;
			}
			if (/^[<>]=?$/.test(delOp)) {
				for (let c = as; c < ae; c++) {
					if (addText[c] === "<" || addText[c] === ">") {
						const g = Math.abs(delPos[j]! - c) + (addText[c] === delOp ? 0 : 2);
						if (g < bestGap) {
							bestGap = g;
							best = i;
							opAnchor.set(j, c);
						}
					}
				}
			}
		}
		if (best >= 0) {
			usedAdds.add(best);
			pair.set(j, best);
		}
	}
	const word = (c: string) => /[A-Za-z0-9_]/.test(c);
	// A WORD-led standalone del landing mid-word is garble (the comment
	// rewrite's `them.` in `m~~them~~ust`); a punctuation-led fragment
	// (`.rev()`, ` {`) reads fine struck mid-word — `enumer~~.rev()~~ate()`
	// still reconstructs `enumerate()`. Meaningful identity = subword.
	const splitWord = delSpans.some((span, j) => {
		if (pair.has(j)) return false;
		// A multi-line span (a whole deleted block folded into a
		// reverse-stretch mod) cannot be a split-word garble — the strike
		// spans the visual lines, the new line fills beside it. The
		// reverse-stretch of the old `find_mount` body must stay the
		// subword mod (`self.mount_stack` common + struck chain + filled
		// `.get(guest_path)`), not fall to the `-`/`+` baseline.
		if (delText.slice(span[0], span[1]).includes("\n")) return false;
		const ins = Math.max(0, Math.min(addText.length, delPos[j]!));
		if (!(ins > 0 && ins < addText.length && word(addText[ins - 1]!) && word(addText[ins]!))) return false;
		const t = delText.slice(span[0], span[1]).trimStart();
		return word(t[0] ?? "");
	});
	return splitWord;
}

/**
 * Mark exact-content del/add pairs as *moved*: the same line at a new
 * position (e.g. renumbered sections) — rendered muted and excluded from the
 * change counts. Matching is greedy per file (a multiset of add texts).
 */
function fileCounts(f: DiffFile): { adds: number; dels: number; mods: number; moved: number } {
	let adds = 0;
	let dels = 0;
	let mods = 0;
	let moved = 0;
	for (const h of f.hunks) {
		for (const r of h.rows) {
			if (r.kind === "add" && r.moved) moved++;
			else if (r.kind === "add") adds++;
			else if (r.kind === "del") dels++;
			else if (r.kind === "mod") mods++;
			for (const t of r.tail ?? []) {
				if (t.moved) moved++;
				else adds++;
			}
		}
	}
	// Each moved add represents one relocated line-pair (the matching del is
	// absorbed or displayed muted — either way it is not a change).
	return { adds, dels, mods, moved };
}

// ── Editor invocation ──────────────────────────────────────────────────────

interface EditorPattern {
	names: string[];
	/** Argv templates: {bin} {file} {line} {start} {end} {count}. */
	open: string[];
	/** Argv templates when a range is selected (defaults to `open`). */
	range?: string[];
	/** GUI editors: fire-and-forget — no TUI takeover, do not wait. */
	detached?: boolean;
}

const EDITOR_PATTERNS: EditorPattern[] = [
	{
		names: ["vim", "vi", "nvim"],
		open: ["{bin}", "+{line}", "{file}"],
		range: ["{bin}", "-c", "{start}G", "-c", "normal! V{count}j", "{file}"],
	},
	{ names: ["nano", "micro", "pico", "emacs"], open: ["{bin}", "+{line}", "{file}"] },
	{ names: ["helix", "hx", "zed"], open: ["{bin}", "{file}:{line}"] },
	{ names: ["fresh"], open: ["{bin}", "{file}:{line}"] }, // best-effort: positional
	{ names: ["code", "codium", "cursor"], open: ["{bin}", "--goto", "{file}:{line}:1"], detached: true },
	{ names: ["antigravity"], open: ["{bin}", "{file}:{line}"], detached: true }, // best-effort
];

function splitTemplate(tpl: string): string[] {
	const out: string[] = [];
	let cur = "";
	let inSingle = false;
	let inDouble = false;
	let esc = false;
	for (const ch of tpl) {
		if (esc) {
			cur += ch;
			esc = false;
		} else if (ch === "\\" && !inSingle) {
			esc = true;
		} else if (ch === "'" && !inDouble) {
			inSingle = !inSingle;
		} else if (ch === '"' && !inSingle) {
			inDouble = !inDouble;
		} else if (/\s/.test(ch) && !inSingle && !inDouble) {
			if (cur) {
				out.push(cur);
				cur = "";
			}
		} else {
			cur += ch;
		}
	}
	if (cur) out.push(cur);
	return out;
}

function substitute(tpl: string[], vars: Record<string, string>): string[] {
	return tpl.map((a) => a.replace(/\{(\w+)\}/g, (_, k: string) => vars[k] ?? `{${k}}`));
}

function editorBinary(): string {
	return process.env.REVIEW_EDITOR || process.env.VISUAL || process.env.EDITOR || "vim";
}

function editorPattern(bin: string): { pattern: EditorPattern; name: string } | null {
	const name = (bin.split(/\s+/)[0] ?? bin).split("/").pop()?.toLowerCase() ?? bin;
	for (const p of EDITOR_PATTERNS) {
		if (p.names.includes(name)) return { pattern: p, name };
	}
	return null;
}

function editorArgs(
	bin: string,
	file: string,
	line: number,
	range: { start: number; end: number } | null,
): { argv: string[]; detached: boolean } | null {
	const vars = {
		bin,
		file,
		line: String(Math.max(1, line)),
		start: range ? String(range.start) : String(Math.max(1, line)),
		end: range ? String(range.end) : String(Math.max(1, line)),
		count: range ? String(Math.max(0, range.end - range.start)) : "0",
	};
	const known = editorPattern(bin);
	let argv: string[];
	let detached = false;
	if (known) {
		const tpl = range && known.pattern.range ? known.pattern.range : known.pattern.open;
		argv = substitute(tpl, vars);
		detached = !!known.pattern.detached;
	} else {
		const override = process.env.REVIEW_EDITOR_ARGS;
		const rangeOverride = process.env.REVIEW_EDITOR_RANGE_ARGS;
		const tpl =
			range && rangeOverride
				? splitTemplate(rangeOverride)
				: override
					? splitTemplate(override)
					: ["{bin}", "+{line}", "{file}"];
		argv = substitute(tpl, vars);
		detached = false;
	}
	argv[0] = bin;
	return { argv, detached };
}

// ── Syntax highlighting ────────────────────────────────────────────────────
// A compact per-language tokenizer driving the theme's syntax palette. No
// external deps: keywords/comments/strings/numbers/types/functions cover the
// readability value; the diff line tints carry the rest.

const KEYWORDS_RUST = new Set("as async await break const continue crate dyn else enum extern fn for if impl in let loop match mod move mut pub ref return self Self static struct super trait type unsafe use where while macro_rules yield".split(" "));
const KEYWORDS_TS = new Set("as async await break case catch class const continue debugger default delete do else enum export extends false finally for from function get if import in instanceof interface let new null of return set static super switch this throw true try typeof var void while with yield".split(" "));
const KEYWORDS_PY = new Set("and as assert async await break class continue def del elif else except False finally for from global if import in is lambda None nonlocal not or pass raise return True try while with yield".split(" "));
const KEYWORDS_SH = new Set("if then else elif fi case esac for while until do done function in return exit local export echo set unset true false break continue".split(" "));
const KEYWORDS_C = new Set("auto break case char const continue default do double else enum extern float for goto if inline int long register restrict return short signed sizeof static struct switch typedef union unsigned void volatile while".split(" "));
const KEYWORDS_GO = new Set("break case chan const continue default defer else fallthrough for func go goto if import interface map package range return select struct switch type var".split(" "));
const KEYWORDS_MD = new Set<string>();

interface LangProfile {
	keywords: Set<string>;
	/** `#` to end of line is a comment (python/shell/yaml/toml/markdown). */
	hashComment?: boolean;
	/** `//` to end of line. */
	lineComment?: boolean;
	/** `/* ... *​/` block comment. */
	blockComment?: boolean;
	/** Capitalized identifiers are types (Rust/TS/C/Go...). */
	typesByCase?: boolean;
}

function langProfile(ext: string): LangProfile | null {
	switch (ext) {
		case "rs":
			return { keywords: KEYWORDS_RUST, lineComment: true, blockComment: true, typesByCase: true };
		case "ts":
		case "tsx":
		case "js":
		case "jsx":
		case "mjs":
			return { keywords: KEYWORDS_TS, lineComment: true, blockComment: true, typesByCase: true };
		case "py":
			return { keywords: KEYWORDS_PY, hashComment: true, lineComment: false, typesByCase: true };
		case "sh":
		case "bash":
			return { keywords: KEYWORDS_SH, hashComment: true };
		case "c":
		case "h":
		case "cpp":
		case "hpp":
		case "java":
			return { keywords: KEYWORDS_C, lineComment: true, blockComment: true, typesByCase: true };
		case "go":
			return { keywords: KEYWORDS_GO, lineComment: true, blockComment: true, typesByCase: true };
		case "md":
		case "markdown":
			return { keywords: KEYWORDS_MD, hashComment: true };
		case "toml":
		case "yaml":
		case "yml":
			return { keywords: KEYWORDS_MD, hashComment: true };
		case "json":
			return { keywords: KEYWORDS_MD };
		default:
			return null;
	}
}

// Regex literal parts; the per-language alternation is composed from
// `re.source` (regex escapes, no string-escaping pitfalls).
const RE_HASH = /#[^\n]*/;
const RE_LINE = /\/\/[^\n]*/;
const RE_BLOCK = /\/\*[\s\S]*?\*\//;
const RE_RAW = /r#*"(?:[^"\\]|\\.)*"#*/;
const RE_DSTR = /"(?:[^"\\\n]|\\.)*"/;
const RE_SSTR = /'(?:[^'\\\n]|\\.)*'/;
const RE_BT = /`(?:[^`\\\n]|\\.)*`/;
const RE_HEX = /\b0x[0-9a-fA-F_]+\b/;
const RE_NUM = /\b\d[\d_]*(?:\.[\d_]+)?(?:[eE][+-]?\d+)?\b/;
const RE_ID = /[A-Za-z_][A-Za-z0-9_]*/;
const RE_WS = /\s+/;
const RE_OTHER = /./;

const TOKEN_RE_CACHE = new Map<string, RegExp>();

function tokenReFor(profile: LangProfile): RegExp {
	const parts = [
		profile.hashComment ? RE_HASH.source : null,
		profile.lineComment ? RE_LINE.source : null,
		profile.blockComment ? RE_BLOCK.source : null,
		RE_RAW.source,
		RE_DSTR.source,
		RE_SSTR.source,
		RE_BT.source,
		RE_HEX.source,
		RE_NUM.source,
		RE_ID.source,
		RE_WS.source,
		RE_OTHER.source,
	]
		.filter(Boolean)
		.join("|");
	let re = TOKEN_RE_CACHE.get(parts);
	if (!re) {
		re = new RegExp(parts, "g");
		TOKEN_RE_CACHE.set(parts, re);
	}
	return re;
}

function classifyTokens(text: string, profile: LangProfile | null): { color: string; text: string }[] {
	if (!profile || text.length === 0) return [{ color: "text", text }];
	const re = tokenReFor(profile);
	re.lastIndex = 0;
	const out: { color: string; text: string }[] = [];
	let last = 0;
	let m: RegExpExecArray | null;
	while ((m = re.exec(text))) {
		if (m.index > last) out.push({ color: "text", text: text.slice(last, m.index) });
		const tok = m[0];
		const next = text[m.index + tok.length];
		let color = "text";
		if ((profile.hashComment && tok.startsWith("#")) || tok.startsWith("//") || tok.startsWith("/*")) {
			color = "syntaxComment";
		} else if (tok.startsWith('"') || tok.startsWith("'") || tok.startsWith("`") || tok.startsWith('r"')) {
			color = "syntaxString";
		} else if (/^0x/i.test(tok) || /^\d/.test(tok)) {
			color = "syntaxNumber";
		} else if (/^[A-Za-z_]/.test(tok)) {
			if (profile.keywords.has(tok)) color = "syntaxKeyword";
			else if (profile.typesByCase && /^[A-Z]/.test(tok)) color = "syntaxType";
			else if (next === "(") color = "syntaxFunction";
		} else if (/^\s+$/.test(tok)) {
			color = "text";
		} else {
			color = "syntaxOperator";
		}
		out.push({ color, text: tok });
		last = m.index + tok.length;
	}
	if (last < text.length) out.push({ color: "text", text: text.slice(last) });
	return out;
}

/** Styled ANSI for a full line (tests); the component slices the token stream
 * per wrap segment instead, so colors continue across visual line breaks. */
function syntaxHighlight(
	text: string,
	profile: LangProfile | null,
	theme: { fg: (color: string, t: string) => string },
	/** Re-applied after every token's reset (keeps a background run intact). */
	bgAfter = "",
): string {
	if (!profile || text.length === 0) return theme.fg("text", text) + bgAfter;
	let out = "";
	for (const t of classifyTokens(text, profile)) {
		out += theme.fg(t.color, t.text) + bgAfter;
	}
	return out;
}

// ── Review component ───────────────────────────────────────────────────────

interface ReviewSession {
	repo: string;
	repoRoot: string;
	files: DiffFile[];
	notes: NotesDB;
	context: number;
	base: ReviewBase;
	branch: string;
	git: GitRunner;
}

interface Selection {
	start: number;
	end: number;
	side: "new" | "old";
}

class ReviewComponent {
	// ── merged (infinite-scroll) model ────────────────────────────────────
	// The whole diff is one continuous row list; the cursor scrolls freely
	// across files, but selection is bounded to a single file.
	private flat: Row[] = [];
	private rowFile: number[] = []; // file index per flat row
	private rowIdx = 0;
	private scroll = 0;
	private anchor: number | null = null;
	private confirmed: (Selection & { file: number }) | null = null;
	private hunkStarts: { flat: number; file: number }[] = [];
	private showHelp = false;
	private wordDiff = true;
	private safetyStashes: string[] = [];
	private dragY: number | null = null;
	private dragX: number | null = null;
	private colOffset = 0;
	private numBuffer = "";
	private lastMouse: string | null = null;
	/** In-component note editor: null = not editing, else the draft text. */
	private noteDraft: string | null = null;
	/** The open fenced-code language inside a `.md` file (null = prose). */
	private mdFence: string | null = null;
	private syntaxOn = true;
	/** Visual (wrapped) line offset within the cursor's flat row. */
	private visLine = 0;

	constructor(
		private session: ReviewSession,
		private tui: {
			requestRender: (force?: boolean) => void;
			stop: () => void;
			start: () => void;
		},
		private theme: {
			fg: (color: string, text: string) => string;
			bg: (color: string, text: string) => string;
		},
		private done: (result: number) => void,
		initial: { fileIdx: number; line: number; col: number } | null = null,
	) {
		this.rebuild();
		if (initial && initial.fileIdx >= 0 && initial.fileIdx < this.session.files.length) {
			const fi = initial.fileIdx;
			if (initial.line !== null) {
				// Full resume: anchor to the stored (file, line) — the handler
				// verified the line content still matches.
				let idx = -1;
				for (let i = 0; i < this.flat.length; i++) {
					if (this.rowFile[i] !== fi) continue;
					if (rowCoord(this.flat[i]) === initial.line) {
						idx = i;
						break;
					}
				}
				if (idx >= 0) {
					this.rowIdx = idx;
					this.visLine = 0;
					this.colOffset = initial.col;
					return;
				}
			}
			// Line moved/changed: fall back to the same file's first row.
			const start = this.firstRowOfFile(fi);
			this.rowIdx = Math.min(start + 1, Math.max(0, this.flat.length - 1));
			this.visLine = 0;
		} else {
			// Start on the first real row, not the first file marker.
			this.rowIdx = this.nextCoordRow(0);
			this.visLine = 0;
		}
	}

	notify: ((msg: string, kind: "info" | "warning" | "error") => void) | null = null;
	confirm: ((msg: string) => Promise<boolean>) | null = null;
	/** Opens the key-binding popup overlay (set by the command handler). */
	helpPopup: (() => void) | null = null;

	// ── model ─────────────────────────────────────────────────────────────

	/** Rebuild the merged row list from all files (markers + hunk index). */
	private rebuild(): void {
		this.flat = [];
		this.rowFile = [];
		this.hunkStarts = [];
		this.session.files.forEach((f, fi) => {
			const marker: Row = { kind: "file", text: f.path, newLine: null, oldLine: null };
			this.flat.push(marker);
			this.rowFile.push(fi);
			for (const r of buildVisible(f)) {
				this.flat.push(r);
				this.rowFile.push(fi);
			}
			f.hunks.forEach((h) => {
				if (h.rows[0]) {
					const idx = this.flat.indexOf(h.rows[0]);
					if (idx >= 0) this.hunkStarts.push({ flat: idx, file: fi });
				}
			});
		});
		if (this.rowIdx >= this.flat.length) this.rowIdx = Math.max(0, this.flat.length - 1);
	}

	private get fileIdx(): number {
		return this.rowFile[this.rowIdx] ?? 0;
	}

	private get file(): DiffFile {
		return this.session.files[this.fileIdx]!;
	}

	private firstRowOfFile(fi: number): number {
		return this.flat.findIndex((r, i) => this.rowFile[i] === fi && r.kind === "file");
	}

	private lastRowOfFile(fi: number): number {
		const next = this.flat.findIndex((r, i) => this.rowFile[i] > fi);
		return next === -1 ? this.flat.length - 1 : next - 1;
	}

	/** The next flat row at/after `from` that carries a line coordinate. */
	private nextCoordRow(from: number): number {
		for (let i = Math.max(0, from); i < this.flat.length; i++) {
			if (rowCoord(this.flat[i]!) !== null) return i;
		}
		return from;
	}

	/** The previous flat row at/before `from` that carries a line coordinate. */
	private prevCoordRow(from: number): number {
		for (let i = Math.min(this.flat.length - 1, from); i >= 0; i--) {
			if (rowCoord(this.flat[i]!) !== null) return i;
		}
		return from;
	}

	// ── selection (bounded to one file) ───────────────────────────────────

	private computeSelection(): (Selection & { file: number }) | null {
		if (this.anchor !== null) {
			const cFile = this.fileIdx;
			let lo = Math.min(this.anchor, this.rowIdx);
			let hi = Math.max(this.anchor, this.rowIdx);
			// "Select within one file": when the anchor lands in another file,
			// snap it to the cursor file's first row.
			if ((this.rowFile[this.anchor] ?? 0) !== cFile) {
				lo = this.firstRowOfFile(cFile);
			}
			const fs = this.firstRowOfFile(cFile);
			const fe = this.lastRowOfFile(cFile);
			lo = Math.min(fe, Math.max(fs, lo));
			hi = Math.min(fe, Math.max(fs, hi));
			if (hi < lo) hi = lo;
			lo = this.nextCoordRow(lo);
			hi = this.prevCoordRow(hi);
			const first = this.flat[lo];
			const last = this.flat[hi];
			if (!first || !last) return null;
			// Endpoints use their own available coordinate (a del→add range
			// reads the del's old line and the add's new line); the note's
			// side prefers "new" when either endpoint has one.
			const side = first.newLine !== null || last.newLine !== null ? "new" : "old";
			const f = first.newLine ?? first.oldLine;
			const l = last.newLine ?? last.oldLine;
			if (f === null || l === null) return null;
			return { file: cFile, start: Math.min(f, l), end: Math.max(f, l), side };
		}
		if (this.confirmed && this.confirmed.file === this.fileIdx) return this.confirmed;
		return null;
	}

	private notesOnCurrent(): ReviewNote[] {
		const list = notesFor(this.session.notes, this.session.repo, this.file.path);
		const sel = this.computeSelection();
		if (sel) return list.filter((n) => n.start <= sel.end && n.end >= sel.start);
		const c = rowCoord(this.flat[this.rowIdx]);
		if (c === null) return [];
		return list.filter((n) => c >= n.start && c <= n.end);
	}

	// ── input ─────────────────────────────────────────────────────────────

	handleInput(data: string): void {
		// Note editing: everything goes to the draft until ctrl+enter/esc.
		// Plain enter inserts a line break — notes are multi-line text.
		if (this.noteDraft !== null) {
			if (matchesKey(data, Key.ctrl("enter"))) {
				this.commitNote();
			} else if (matchesKey(data, Key.escape)) {
				this.noteDraft = null;
			} else if (matchesKey(data, Key.enter)) {
				this.noteDraft += "\n";
			} else if (matchesKey(data, Key.backspace)) {
				this.noteDraft = this.noteDraft.slice(0, -1);
			} else if (data.length === 1 && data >= " ") {
				this.noteDraft += data;
			}
			this.tui.requestRender();
			return;
		}
		// Vim-style count: digits accumulate; any non-digit / non-gG command
		// clears it (checked before the movement handlers, which return early).
		if (!/^[0-9]$/.test(data) && data !== "g" && data !== "G" && this.numBuffer) {
			this.numBuffer = "";
		}
		// SGR mouse events: ESC[<btn;col;row[Mm].
		//   wheel:   64 (up) / 65 (down)
		//   press:   btn 0 'M'   release: btn 0 'm'
		//   drag:    btn 32 (button-1 held + motion, mode 1002)
		//            btn 35 (motion without button, mode 1003)
		// Motion is button + 32; 35 is the no-button motion code.
		const mouse = /^\x1b\[<(\d+);(\d+);(\d+)([Mm])$/.exec(data);
		if (mouse) {
			const btn = Number(mouse[1]);
			const x = Number(mouse[2]);
			const y = Number(mouse[3]);
			if (btn === 64) {
				this.scrollBy(-3);
			} else if (btn === 65) {
				this.scrollBy(3);
			} else if (btn === 0 && mouse[4] === "M") {
				this.dragX = x; // button-1 press: begin drag (both axes)
				this.dragY = y;
			} else if ((btn === 32 || btn === 35) && mouse[4] === "M") {
				// Motion: lazily anchor on the first event (phone terminals
				// may synthesize drags without a clean press).
				if (this.dragX === null || this.dragY === null) {
					this.dragX = x;
					this.dragY = y;
				} else {
					// Drag scrolls the VIEW like the wheel (a scroll gesture
					// drags the view, not the content): dragging down scrolls
					// down (later lines), dragging right pans right. A
					// touchpad's two-finger scroll often arrives as button-32
					// motion, and "content follows the finger" (the natural-
					// scroll inversion) reads as a scroll-direction bug.
					this.scrollBy(y - this.dragY); // vertical: view follows the drag
					this.colOffset = Math.max(0, this.colOffset + (x - this.dragX)); // horizontal
					this.dragX = x;
					this.dragY = y;
				}
			} else if (btn === 0 && mouse[4] === "m") {
				this.dragX = null; // release
				this.dragY = null;
			}
			// Live diagnostic: the status line shows the last mouse event, so
			// it is obvious whether the terminal forwards anything at all.
			this.lastMouse = `${btn}@${x},${y}${mouse[4]}`;
			this.tui.requestRender();
			return;
		}
		if (matchesKey(data, Key.escape)) {
			if (this.showHelp) {
				this.showHelp = false;
			} else if (this.anchor !== null) {
				this.anchor = null;
			} else {
				this.quit();
				return;
			}
			this.tui.requestRender();
			return;
		}
		if (data === "q" || data === "Q") {
			this.quit();
			return;
		}
		if (data === "?") {
			if (this.helpPopup) {
				this.helpPopup();
				return;
			}
			this.showHelp = !this.showHelp;
			this.tui.requestRender();
			return;
		}

		if (data === "^") {
			this.colOffset = 0;
			this.tui.requestRender();
			return;
		}
		if (data === "$") {
			// Anchor the content's right edge at the viewport's right margin
			// (last column of the longest line lands at the screen edge).
			const maxLen = this.flat.reduce(
				(m, r) => (rowCoord(r) !== null ? Math.max(m, r.text.length) : m),
				0,
			);
			const textCols = Math.max(1, (process.stdout.columns ?? 80) - 8);
			this.colOffset = Math.max(0, maxLen - textCols);
			this.tui.requestRender();
			return;
		}
		if (matchesKey(data, "left") || data === "h" || data === "H") {
			this.colOffset = Math.max(0, this.colOffset - 10);
			this.tui.requestRender();
			return;
		}
		if (matchesKey(data, "right") || data === "l" || data === "L") {
			this.colOffset += 10;
			this.tui.requestRender();
			return;
		}
		if (matchesKey(data, "down") || data === "j" || data === "J") return this.move(1);
		if (matchesKey(data, "up") || data === "k" || data === "K") return this.move(-1);
		if (matchesKey(data, "pageDown")) return this.move(20);
		if (matchesKey(data, "pageUp")) return this.move(-20);
		if (/^[0-9]$/.test(data)) {
			this.numBuffer += data;
			this.tui.requestRender();
			return;
		}
		if (data === "g" || data === "G") {
			if (this.numBuffer) {
				const n = Number(this.numBuffer);
				this.numBuffer = "";
				this.jumpToLine(n);
				return;
			}
			this.numBuffer = "";
			return this.goto(data === "G" ? this.flat.length - 1 : 0);
		}
		if (matchesKey(data, "home")) return this.goto(0);
		if (matchesKey(data, "end")) return this.goto(this.flat.length - 1);
		if (data === "]" || data === "n" || data === "N") return this.gotoHunk(1);
		if (data === "[" || data === "p" || data === "P") return this.gotoHunk(-1);
		if (matchesKey(data, Key.tab)) {
			this.mdFence = null;
			return this.switchFile(1);
		}
		if (matchesKey(data, "shift+tab")) {
			this.mdFence = null;
			return this.switchFile(-1);
		}

		if (data === "v" || data === "V") {
			if (this.anchor === null) {
				this.anchor = this.rowIdx;
				this.confirmed = null;
			} else {
				this.confirmed = this.computeSelection();
				this.anchor = null;
			}
			this.tui.requestRender();
			return;
		}

		if (data === "m" || data === "M") {
			// In-component note editor: the note is typed into the status
			// line (the pi host editor would open under the review overlay —
			// invisible, with keystrokes captured until esc).
			this.noteDraft = "";
			this.lastMouse = null;
			this.tui.requestRender();
			return;
		}
		if (data === "d" || data === "D") return this.deleteNotes();
		if (data === "o" || data === "O") return this.openEditor();
		if (data === "w" || data === "W") {
			this.wordDiff = !this.wordDiff;
			this.tui.requestRender();
			return;
		}
		if (data === "t" || data === "T") {
			this.syntaxOn = !this.syntaxOn;
			this.tui.requestRender();
			return;
		}

		if (data === "e") return this.adjustContext(1, 0);
		if (data === "E") return this.adjustContext(0, 1);
		if (data === "c") return this.adjustContext(-1, 0);
		if (data === "C") return this.adjustContext(0, -1);
		if (data === "a") return this.expandHunkToFull();
		if (data === "r" || data === "R") return this.resetContext(data === "R");
		if (data === "A") {
			for (const h of this.file.hunks) {
				h.aboveCtx = Number.MAX_SAFE_INTEGER;
				h.belowCtx = Number.MAX_SAFE_INTEGER;
			}
			this.rebuild();
			this.tui.requestRender();
			return;
		}

		if (data === "s" || data === "S") return void this.stage(data === "S");
		if (data === "u" || data === "U") return void this.unstage(data === "U");
		if (data === "x" || data === "X") return void this.discard(data === "X");
	}

	// ── movement ──────────────────────────────────────────────────────────

	private move(delta: number): void {
		if (this.flat.length === 0) return;
		const { starts, total } = this.visualGeometry(process.stdout.columns ?? 80);
		this.setCursorVis((starts[this.rowIdx]! + this.visLine) + delta);
		this.tui.requestRender();
	}

	/** Wheel/drag scroll: the VIEWPORT moves by `delta` visual lines and the
	 * cursor rides along (staying at its relative viewport position), so the
	 * content actually scrolls instead of the highlight jumping in place —
	 * a scroll gesture scrolls the view. Both are clamped. */
	private scrollBy(delta: number): void {
		if (this.flat.length === 0) return;
		const { starts, total } = this.visualGeometry(process.stdout.columns ?? 80);
		const maxRows = Math.max(8, (process.stdout.rows ?? 24) - 5);
		const maxScroll = Math.max(0, total - maxRows);
		this.setCursorVis(Math.max(0, Math.min(total - 1, starts[this.rowIdx]! + this.visLine + delta)));
		this.scroll = Math.max(0, Math.min(maxScroll, this.scroll + delta));
		// Force a full redraw: pi's differential redraw leaves stale rows when
		// the viewport shifts (the mashing the user saw). A forced render
		// resets pi's diff state.
		this.tui.requestRender(true);
	}

	private goto(idx: number): void {
		this.rowIdx = Math.max(0, Math.min(this.flat.length - 1, idx));
		this.visLine = 0;
		this.tui.requestRender();
	}

	private currentHunkIdx(): number {
		const r = this.flat[this.rowIdx];
		if (!r) return 0;
		const c = r.newLine ?? r.oldLine ?? 0;
		let idx = 0;
		this.file.hunks.forEach((h, i) => {
			const hs = h.newCount > 0 ? h.newStart : h.oldStart;
			if (c >= hs) idx = i;
		});
		return idx;
	}

	/** Cyclic hunk navigation across the whole diff ("infinite scroll"). */
	private gotoHunk(dir: 1 | -1): void {
		if (this.hunkStarts.length === 0) return;
		if (dir === 1) {
			const next = this.hunkStarts.find((h) => h.flat > this.rowIdx) ?? this.hunkStarts[0]!;
			this.goto(next.flat);
		} else {
			const prev = [...this.hunkStarts].reverse().find((h) => h.flat < this.rowIdx) ?? this.hunkStarts[this.hunkStarts.length - 1]!;
			this.goto(prev.flat);
		}
	}

	/**
	 * `N + G`: jump to line N of the current file. When the line is hidden by
	 * collapsed context, expand the file's hunks to full context and retry.
	 */
	private jumpToLine(n: number): void {
		const fi = this.fileIdx;
		const f = this.file;
		let find = (): number => {
			for (let i = 0; i < this.flat.length; i++) {
				if (this.rowFile[i] !== fi) continue;
				if (rowCoord(this.flat[i]) === n) return i;
			}
			return -1;
		};
		let idx = find();
		if (idx === -1 && n >= 1 && n <= f.newLineCount) {
			for (const h of f.hunks) {
				h.aboveCtx = Number.MAX_SAFE_INTEGER;
				h.belowCtx = Number.MAX_SAFE_INTEGER;
			}
			this.rebuild();
			idx = find();
		}
		if (idx >= 0) this.goto(idx);
		this.tui.requestRender();
	}

	private switchFile(dir: 1 | -1): void {
		const n = this.session.files.length;
		if (n <= 1) return;
		const target = (this.fileIdx + dir + n) % n;
		const marker = this.flat.findIndex((r, i) => this.rowFile[i] === target && r.kind === "file");
		if (marker >= 0) {
			this.colOffset = 0;
			this.goto(Math.min(marker + 1, this.flat.length - 1));
		}
	}

	private adjustContext(dirV: 1 | 0 | -1, dirH: 1 | 0 | -1): void {
		const h = this.file.hunks[this.currentHunkIdx()]!;
		const step = this.session.context;
		if (dirV === 1) h.aboveCtx += step;
		if (dirV === -1) h.aboveCtx = Math.max(0, h.aboveCtx - step);
		if (dirH === 1) h.belowCtx += step;
		if (dirH === -1) h.belowCtx = Math.max(0, h.belowCtx - step);
		this.rebuild();
		this.tui.requestRender();
	}

	private expandHunkToFull(): void {
		const h = this.file.hunks[this.currentHunkIdx()]!;
		h.aboveCtx = Number.MAX_SAFE_INTEGER;
		h.belowCtx = Number.MAX_SAFE_INTEGER;
		this.rebuild();
		this.tui.requestRender();
	}

	private resetContext(all: boolean): void {
		const def = this.session.context;
		if (all) {
			for (const f of this.session.files) {
				for (const h of f.hunks) {
					h.aboveCtx = def;
					h.belowCtx = def;
				}
			}
		} else {
			const h = this.file.hunks[this.currentHunkIdx()]!;
			h.aboveCtx = def;
			h.belowCtx = def;
		}
		this.rebuild();
		this.tui.requestRender();
	}

	// ── notes ─────────────────────────────────────────────────────────────

	private commitNote(): void {
		const text = this.noteDraft?.trim() ?? "";
		this.noteDraft = null;
		if (!text) return;
		const sel = this.computeSelection();
		const row = this.flat[this.rowIdx];
		const side = sel?.side ?? (row && row.newLine !== null ? "new" : "old");
		const c = row ? rowCoord(row) : null;
		const start = sel?.start ?? c;
		const end = sel?.end ?? c;
		if (start === null || end === null) return;
		addNote(this.session.notes, this.session.repo, {
			id: randomUUID(),
			file: this.file.path,
			side,
			start,
			end,
			text,
			createdAt: Date.now(),
		});
		this.confirmed = null;
	}

	private deleteNotes(): void {
		const sel = this.computeSelection();
		const removed = sel
			? removeNotes(this.session.notes, this.session.repo, this.file.path, sel.side, sel.start, sel.end)
			: (() => {
					const c = rowCoord(this.flat[this.rowIdx]);
					if (c === null) return 0;
					return (
						removeNotes(this.session.notes, this.session.repo, this.file.path, "new", c, c) +
						removeNotes(this.session.notes, this.session.repo, this.file.path, "old", c, c)
					);
				})();
		if (sel) this.confirmed = null;
		this.notify?.(removed > 0 ? `deleted ${removed} note${removed === 1 ? "" : "s"}` : "no note here", "info");
		this.tui.requestRender();
	}

	// ── editor ────────────────────────────────────────────────────────────

	private openEditor(): void {
		const f = this.file;
		const row = this.flat[this.rowIdx];
		if (!row) return;
		const abs = join(this.session.repoRoot, f.path);
		if (f.status === "deleted" || !existsSync(abs)) {
			this.notify?.(`cannot edit: ${f.path} is deleted`, "warning");
			return;
		}
		const coord = row.newLine ?? row.oldLine;
		if (coord === null) {
			this.notify?.("cursor is on a marker or deleted line — no new-file position", "warning");
			return;
		}
		const sel = this.computeSelection();
		const range = sel && sel.side === "new" ? { start: sel.start, end: sel.end } : null;
		const bin = editorBinary();
		const built = editorArgs(bin, abs, coord, range);
		if (!built) return;

		if (built.detached) {
			spawnSync(built.argv[0]!, built.argv.slice(1), { stdio: "ignore", detached: true, cwd: this.session.repoRoot });
			this.notify?.(`opened ${f.path}:${coord} in ${bin}`, "info");
			return;
		}

		this.notify?.(`opening ${bin} at ${f.path}:${coord}${range ? ` (${range.start}-${range.end})` : ""}`, "info");
		this.tui.stop();
		process.stdout.write("\x1b[2J\x1b[H");
		const shell = process.env.SHELL || "/bin/sh";
		const cmd = built.argv.map((a) => `'${a.replace(/'/g, "'\\''")}'`).join(" ");
		spawnSync(shell, ["-c", cmd], { stdio: "inherit", env: process.env, cwd: this.session.repoRoot });
		this.tui.start();
		this.tui.requestRender(true);
		this.refreshFileContent();
	}

	private refreshFileContent(): void {
		const f = this.file;
		try {
			if (existsSync(join(this.session.repoRoot, f.path))) {
				f.newContent = readFileSync(join(this.session.repoRoot, f.path), "utf8").split("\n");
				f.newLineCount = f.newContent.length;
				f.noContent = f.newContent.length === 0 && f.oldContent.length === 0;
			}
		} catch {
			// keep previous content
		}
		this.rebuild();
		this.tui.requestRender();
	}

	// ── git actions ───────────────────────────────────────────────────────

	/** Extract the current hunk as an applyable patch (with -U3 context). */
	private async hunkPatch(): Promise<string | null> {
		const f = this.file;
		const h = f.hunks[this.currentHunkIdx()]!;

		if (f.status === "untracked") {
			const body = [`@@ -0,0 +1,${h.newCount} @@`, ...h.rows.map((r) => `+${r.text}`)].join("\n");
			return `diff --git a/${f.path} b/${f.path}\n--- /dev/null\n+++ b/${f.path}\n${body}\n`;
		}

		const res = await this.session.git([
			"diff", "-M", "--no-color", "-U3", ...this.session.base.baseArgs, "--", f.path,
		]);
		if (res.code !== 0) return null;
		const lines = res.stdout.split("\n");
		let start = -1;
		for (let i = 0; i < lines.length; i++) {
			const m = /^@@ -(\d+)(?:,\d+)? \+(\d+)/.exec(lines[i]!);
			if (m && Number(m[1]) === h.oldStart && Number(m[2]) === h.newStart) {
				start = i;
				break;
			}
		}
		if (start === -1) return null;
		let end = lines.length;
		for (let i = start + 1; i < lines.length; i++) {
			if (lines[i]!.startsWith("@@ ")) {
				end = i;
				break;
			}
		}
		const block = lines.slice(start, end).join("\n") + "\n";
		const a = f.status === "added" || f.oldPath === null ? "/dev/null" : `a/${f.oldPath}`;
		const b = f.status === "deleted" ? "/dev/null" : `b/${f.path}`;
		return `diff --git ${a} ${b}\n--- ${a}\n+++ ${b}\n${block}`;
	}

	private async applyPatch(patch: string, flags: string[]): Promise<boolean> {
		const tmp = join(tmpdir(), `review-${process.pid}-${randomUUID().slice(0, 8)}.patch`);
		writeFileSync(tmp, patch);
		try {
			const check = await this.session.git(["apply", "--check", ...flags, tmp]);
			if (check.code !== 0) {
				this.notify?.(`patch does not apply: ${(check.stderr || check.stdout).trim()}`, "error");
				return false;
			}
			const res = await this.session.git(["apply", ...flags, tmp]);
			if (res.code !== 0) {
				this.notify?.(`apply failed: ${(res.stderr || res.stdout).trim()}`, "error");
				return false;
			}
			return true;
		} finally {
			unlinkSync(tmp);
		}
	}

	private async stage(wholeFile: boolean): Promise<void> {
		if (this.file.status !== "untracked" && (this.session.base.cached || this.session.base.baseArgs.length > 0)) {
			this.notify?.("staging requires the default worktree review (/review)", "warning");
			return;
		}
		if (wholeFile) {
			const res = await this.session.git(["add", "--", this.file.path]);
			if (res.code !== 0) return void this.notify?.(`git add failed: ${res.stderr.trim()}`, "error");
		} else {
			const patch = await this.hunkPatch();
			if (!patch) return void this.notify?.("cannot build hunk patch", "error");
			if (!(await this.applyPatch(patch, ["--cached"]))) return;
		}
		await this.refreshAfterGitOp("staged");
	}

	private async unstage(wholeFile: boolean): Promise<void> {
		if (!this.session.base.cached) {
			this.notify?.("unstage requires reviewing the index (/review --cached)", "warning");
			return;
		}
		if (wholeFile) {
			const res = await this.session.git(["reset", "-q", "--", this.file.path]);
			if (res.code !== 0) return void this.notify?.(`git reset failed: ${res.stderr.trim()}`, "error");
		} else {
			const patch = await this.hunkPatch();
			if (!patch) return void this.notify?.("cannot build hunk patch", "error");
			if (!(await this.applyPatch(patch, ["--cached", "-R"]))) return;
		}
		await this.refreshAfterGitOp("unstaged");
	}

	private async discard(wholeFile: boolean): Promise<void> {
		if (this.session.base.cached) {
			this.notify?.("discard works on the worktree, not the index", "warning");
			return;
		}
		const label = wholeFile ? `discard all changes to ${this.file.path}?` : `discard this hunk in ${this.file.path}?`;
		const ok = this.confirm ? await this.confirm(label) : false;
		if (!ok) return;
		if (wholeFile) {
			await this.stashCheckpoint();
			const res = await this.session.git(["checkout", "--", this.file.path]);
			if (res.code !== 0) return void this.notify?.(`checkout failed: ${res.stderr.trim()}`, "error");
		} else {
			const patch = await this.hunkPatch();
			if (!patch) return void this.notify?.("cannot build hunk patch", "error");
			await this.stashCheckpoint();
			if (!(await this.applyPatch(patch, ["-R"]))) return;
		}
		await this.refreshAfterGitOp("discarded");
	}

	private async stashCheckpoint(): Promise<void> {
		try {
			const res = await this.session.git(["stash", "create", "review-checkpoint"]);
			const ref = res.stdout.trim();
			if (res.code === 0 && ref) this.safetyStashes.push(ref);
		} catch {
			// checkpoint is best-effort
		}
	}

	private async refreshAfterGitOp(action: string): Promise<void> {
		const current = this.file.path;
		const { files } = await loadReview(this.session.git, this.session.repoRoot, this.session.base, this.session.context);
		if (files.length === 0) {
			this.notify?.(`${action}: no changes left in the diff`, "info");
			this.quit();
			return;
		}
		const idx = files.findIndex((f) => f.path === current);
		this.session.files = files;
		this.rowIdx = 0;
		this.rebuild();
		if (idx >= 0) {
			const start = this.firstRowOfFile(idx);
			this.rowIdx = Math.min(start + 1, this.flat.length - 1);
		}
		this.scroll = 0;
		this.colOffset = 0;
		this.anchor = null;
		this.confirmed = null;
		this.tui.requestRender();
		const stashNote =
			this.safetyStashes.length > 0
				? ` · safety stash: ${this.safetyStashes[this.safetyStashes.length - 1]} (git stash apply <ref> to restore)`
				: "";
		this.notify?.(`${action} ${current}${stashNote}`, "info");
	}

	private quit(): void {
		// Persist the resume position (file, line, col, line-text) so a
		// reopen can anchor the cursor to the same spot when it still matches.
		// A marker row (no coordinate) — e.g. after scrolling to the very top —
		// falls through to the first real row after it, so quitting at the top
		// resumes at the top, not at the stale previous position.
		let r = this.flat[this.rowIdx];
		let c = r ? rowCoord(r) : null;
		let fi = this.rowFile[this.rowIdx] ?? 0;
		if (r && c === null) {
			for (let i = this.rowIdx + 1; i < this.flat.length; i++) {
				const cc = rowCoord(this.flat[i]);
				if (cc !== null) {
					r = this.flat[i];
					c = cc;
					fi = this.rowFile[i] ?? fi;
					break;
				}
			}
		}
		if (r && c !== null) {
			saveReviewState(this.session.repo, {
				file: this.session.files[fi]?.path ?? this.file.path,
				line: c,
				col: this.colOffset,
				text: r.text,
			});
		}
		this.done(this.notesOnCurrent().length);
	}

	// ── rendering ─────────────────────────────────────────────────────────

	render(width: number): string[] {
		const lines: string[] = [];
		const theme = this.theme;
		const f = this.file;
		const notes = notesFor(this.session.notes, this.session.repo, f.path);
		const sel = this.computeSelection();
		const dim = (s: string) => theme.fg("muted", s);
		const fit = (s: string): string => truncateToWidth(s, Math.max(1, width), "…");
		// Full-width stripe: pad inside the background so the highlight runs
		// to the right edge of the terminal.
		const stripe = (text: string, bg: string): string => {
			const pad = Math.max(0, width - visibleWidth(text));
			return theme.bg(bg, text + " ".repeat(pad));
		};

		const { adds, dels, mods, moved } = fileCounts(f);
		const noteS = `${notes.length} note${notes.length === 1 ? "" : "s"}`;
		const counts = `+${adds}${dels > 0 ? ` −${dels}` : ""}${mods > 0 ? ` ~${mods}` : ""}${moved > 0 ? ` ≈${moved} moved` : ""} · ${noteS}`;
		const avail = width - 2;
		let headRaw = `${f.path} (${f.status}) · ${counts}`;
		const full = ` Review: ${this.session.repo}  ·  ${headRaw}`;
		if (full.length > avail) {
			headRaw = ` ${f.path} (${f.status}) · ${counts}`;
			if (headRaw.length > avail) {
				const statusC = ` (${f.status}) · ${counts}`;
				headRaw = ` ${truncateMiddle(f.path, Math.max(8, avail - statusC.length - 1))}${statusC}`;
				if (headRaw.length > avail) {
					const countsC = ` · ${counts}`;
					headRaw = ` ${truncateMiddle(f.path, Math.max(8, avail - countsC.length - 1))}${countsC}`;
				}
			}
		} else {
			headRaw = full;
		}
		lines.push(fit(stripe(theme.fg("text", headRaw), "selectedBg")));
		const curHunk = f.hunks[this.currentHunkIdx()];
		const ctxTxt = curHunk ? ` · ctx ${curHunk.aboveCtx}▴ ${curHunk.belowCtx}▾` : "";
		const colTxt = this.colOffset > 0 ? ` · col ${this.colOffset}+` : "";
		const goTxt = this.numBuffer ? ` · go ${this.numBuffer}` : "";
		const mouseTxt = this.lastMouse ? ` · mouse ${this.lastMouse}` : "";
		const statusRaw = ` ${this.fileIdx + 1}/${this.session.files.length} files  ·  hunk ${this.currentHunkIdx() + 1}/${f.hunks.length}  ·  base: ${this.session.base.label}${ctxTxt}${colTxt}${goTxt}${mouseTxt}${this.session.branch ? `  ·  branch: ${this.session.branch}` : ""}`;
		lines.push(fit(dim(statusRaw)));
		// The in-component note editor: a prompt line with the draft echoed
		// across lines (ctrl+enter saves, esc cancels, plain enter breaks
		// the line).
		if (this.noteDraft !== null) {
			const sel = this.computeSelection();
			const row = this.flat[this.rowIdx];
			const c = row ? rowCoord(row) : null;
			const start = sel?.start ?? c;
			const end = sel?.end ?? c;
			const loc = `${f.path}:${start ?? "?"}${end && end !== start ? `-${end}` : ""}`;
			const draftLines = this.noteDraft.split("\n");
			const last = draftLines.length - 1;
			const indent = " ".repeat(` Note on ${loc}:`.length);
			draftLines.forEach((dl, i) => {
				const cursor = i === last ? "_" : "";
				const text =
					(i === 0 ? ` Note on ${loc}: ${dl}${cursor}` : ` ${indent}${dl}${cursor}`) +
					(i === 0 ? "  [ctrl+enter save · esc cancel]" : "");
				lines.push(fit(theme.fg("warning", text)));
			});
		}

		const total = this.flat.length;
		const maxRows = Math.max(8, (process.stdout.rows ?? 24) - 5);

		// Visual-space layout: every diff row wraps at the text column width,
		// so one flat row may span several visual lines. Movement (j/k/wheel/
		// drag) steps by *visual* lines for scroll continuity; the cursor's
		// semantic row is the flat row under the visual position.
		const { starts, counts: segCounts, total: visTotal } = this.visualGeometry(width);
		const cursorVis = starts[this.rowIdx] + this.visLine;
		if (cursorVis < this.scroll) this.scroll = cursorVis;
		if (cursorVis >= this.scroll + maxRows) this.scroll = cursorVis - maxRows + 1;
		this.scroll = Math.max(0, Math.min(Math.max(0, visTotal - maxRows), this.scroll));

		// Band rows: active visual selection (snapped to one file) or the
		// sticky confirmed selection (only while the cursor is in its file).
		const bandRows = new Set<number>();
		if (this.anchor !== null) {
			const cFile = this.fileIdx;
			let lo = Math.min(this.anchor, this.rowIdx);
			let hi = Math.max(this.anchor, this.rowIdx);
			if ((this.rowFile[this.anchor] ?? 0) !== cFile) lo = this.firstRowOfFile(cFile);
			const fs = this.firstRowOfFile(cFile);
			const fe = this.lastRowOfFile(cFile);
			for (let i = Math.max(fs, lo); i <= Math.min(fe, hi); i++) bandRows.add(i);
		} else if (this.confirmed && this.confirmed.file === this.fileIdx) {
			for (let i = 0; i < total; i++) {
				if (this.rowFile[i] !== this.confirmed.file) continue;
				const c = rowCoord(this.flat[i]);
				if (c !== null && c >= this.confirmed.start && c <= this.confirmed.end) bandRows.add(i);
			}
		}

		// Notes attach after the last covered visible row of their file (or
		// the file's first row when hidden in a collapsed gap).
		const noteByRow = new Map<number, ReviewNote[]>();
		this.session.files.forEach((ff, fi) => {
			for (const n of notesFor(this.session.notes, this.session.repo, ff.path)) {
				let at = -1;
				for (let i = 0; i < total; i++) {
					if (this.rowFile[i] !== fi) continue;
					const c = rowCoord(this.flat[i]);
					if (c !== null && c >= n.start && c <= n.end) at = i;
				}
				if (at === -1) at = this.firstRowOfFile(fi);
				const arr = noteByRow.get(at) ?? [];
				arr.push(n);
				noteByRow.set(at, arr);
			}
		});

		// Render the visual window, wrapping each flat row into segments.
		let vis = 0;
		for (let i = 0; i < total; i++) {
			const r = this.flat[i]!;
			const count = segCounts[i]!;
			if (vis + count <= this.scroll) {
				vis += count;
				continue;
			}
			if (vis >= this.scroll + maxRows) break;
			const segs = this.rowSegments(r, width, i);
			const from = Math.max(0, this.scroll - vis);
			const to = Math.min(count, this.scroll + maxRows - vis);
			for (let sIdx = from; sIdx < to; sIdx++) {
				let seg = segs[sIdx]!;
				// The cursor row's lighter shade is applied inside the segment
				// styling; the selection band keeps the themed stripe.
				if (i !== this.rowIdx && bandRows.has(i)) seg = stripe(seg, "selectedBg");
				lines.push(fit(seg));
			}
			vis += count;

			const ns = noteByRow.get(i);
			if (ns) {
				for (const n of ns) {
					const where = `${n.start}${n.end !== n.start ? `-${n.end}` : ""}`;
					lines.push(fit(`   💬 [${where}] ` + renderMarkdown(n.text, theme.fg)));
				}
			}
		}

		lines.push(fit(dim(" " + "─".repeat(Math.max(10, Math.min(width - 2, 60))))));
			const selTxt = sel ? ` · sel ${sel.start}-${sel.end}` : "";
			// Compact single-line footer; the full key list lives in the ?
			// popup overlay. Every remaining line is file content.
			lines.push(fit(dim(` [j/k]move [v]select [m]note [o]edit [s]stage [x]discard [q]quit [?]help${selTxt}`)));
		// Trailing empty line: the TUI joins lines with \r\n but never
		// terminates the last one, so a full-viewport copy would be missing
		// its final newline. An empty final row yields the missing terminator.
		lines.push("");
		// The frame must fit the terminal EXACTLY: pi shifts a taller overlay
		// down (viewportStart = workingHeight - termHeight) and drops its top
		// rows — the review's header/status would vanish and the row-diff
		// redraw would leave stale rows. (maxRows = rows - 5 accounts for the
		// header + status + footer + the trailing empty line; this is a final
		// guard for the note rows.)
		const cap = process.stdout.rows ?? 24;
		if (lines.length > cap) lines.length = cap;
		return lines;
	}

	private rowSegments(r: Row, width: number, flatIdx: number): string[] {
		const theme = this.theme;
		if (r.kind === "file") {
			const ff = this.session.files[this.rowFile[flatIdx]!]!;
			const fc = fileCounts(ff);
			const label = ` ${ff.path}  (${ff.status})  ·  +${fc.adds}${fc.dels > 0 ? ` −${fc.dels}` : ""}${fc.mods > 0 ? ` ~${fc.mods}` : ""}${fc.moved > 0 ? ` ≈${fc.moved}` : ""}`;
			const styled = theme.fg("accent", label);
			const fill = Math.max(1, width - visibleWidth(styled) - 1);
			return [styled + theme.fg("border", "─".repeat(fill))];
		}
		if (r.kind === "sep") return [theme.fg("muted", `   ${r.text}`)];
		const prefix = prefixOf(r);
		const textCols = Math.max(1, width - prefix.length - 2);
		const out: string[] = [];

		// The inline merged view for mod rows: the common words (dark-green
		// tint) + the removed words struck at their position + the additions
		// filled — one wrapped line. Other rows take the plain token stream.
		// Wholesale mods render as two separate blocks: the whole old line
		// struck, then the whole new line filled — never concatenated.
		const wholesale = r.kind === "mod" && this.isWholesaleMod(r);
		const movedDiff = r.moved === true && (r as { delText?: string }).delText !== undefined && (r as { delText?: string }).delText !== r.text;
		const merged = (r.kind === "mod" && !wholesale) || movedDiff ? this.mergedTokens(r) : null;
		const tokens = merged ?? this.syntaxTokens(this.colOffset > 0 ? r.text.slice(this.colOffset) : r.text);
		const text = this.displayText(r);
		const emitSeg = (segStart: number, segEnd: number, segPrefix: string, visIdx: number) => {
			out.push(
				this.styleTokens(
					r,
					tokens,
					segStart,
					segEnd,
					segPrefix,
					this.rowIdx === flatIdx && this.visLine === visIdx,
				),
			);
		};
		if (wholesale) {
			// Block 1: the whole old line, struck, in the deletion tint.
			const delText = r.delText ?? "";
			let segIdx = 0;
			const emitBlock = (blockText: string, isDel: boolean) => {
				const segPrefix = (idx: number): string =>
					isDel && idx === 0 ? prefix : " ".repeat(prefix.length);
				if (blockText.length === 0) {
					out.push(
						(isDel ? `\x1b[48;5;${sDel.tint}m` : `\x1b[48;5;${sAdd.tint}m`) +
							segPrefix(0) +
							(isDel ? "\x1b[9m" + this.theme.fg(sDel.fg, "") + "\x1b[29m" : ""),
					);
					segIdx++;
					return;
				}
				let start = 0;
				let segIdxInBlock = 0;
				while (start < blockText.length) {
					const end = Math.min(blockText.length, start + textCols);
					const piece = blockText.slice(start, end);
					const selected = this.rowIdx === flatIdx && this.visLine === segIdx;
					if (isDel) {
						const bg = selected ? `\x1b[48;5;${sDel.cursor}m` : `\x1b[48;5;${sDel.tint}m`;
						out.push(bg + segPrefix(segIdxInBlock) + "\x1b[9m" + this.theme.fg(sDel.fg, piece) + "\x1b[29m");
					} else {
						// The add block (the surviving line) takes the mod's
						// grey — a `~` row is never "new content".
						const bg = selected ? `\x1b[48;5;${sMod.cursor}m` : `\x1b[48;5;${sMod.tint}m`;
						out.push(bg + segPrefix(segIdxInBlock) + this.highlightTokens(this.syntaxTokens(piece), 0, piece.length, bg) + bg);
					}
					start = end;
					segIdx++;
					segIdxInBlock++;
				}
			};
			emitBlock(delText, true);
			emitBlock(this.colOffset > 0 ? r.text.slice(this.colOffset) : r.text, false);
			// The absorbed section lines still follow.
			for (const tl of r.tail ?? []) {
				const tlText = this.colOffset > 0 ? tl.text.slice(this.colOffset) : tl.text;
				const tlTokens = this.syntaxTokens(tlText);
				const tlPrefix = `${tl.newLine ?? ""}  `;
				if (tlText.length === 0) {
					emitSeg(0, 0, tlPrefix, -1);
					continue;
				}
				let start = 0;
				while (start < tlText.length) {
					const end = Math.min(tlText.length, start + textCols);
					const piece = tlText.slice(start, end);
					const visIdx = -1;
					out.push(tlPrefix + this.highlightTokens(tlTokens, start, end, ""));
					start = end;
				}
			}
			return out;
		}
		if (text.length === 0) {
			emitSeg(0, 0, prefix, 0);
		} else {
			// A stretched mod's display spans logical lines — each line is
			// its own segment with the `~` prefix, wrapping within textCols.
			let start = 0;
			let segIdx = 0;
			for (const line of text.split("\n")) {
				if (line.length === 0) {
					emitSeg(start, start, start === 0 ? prefix : " ".repeat(prefix.length), segIdx++);
					start++;
					continue;
				}
				let ls = 0;
				while (ls < line.length) {
					const le = Math.min(line.length, ls + textCols);
					emitSeg(start + ls, start + le, start + ls === 0 ? prefix : " ".repeat(prefix.length), segIdx++);
					ls = le;
				}
				start += line.length + 1;
			}
		}
		// The absorbed section lines (the mod's tail) — pure context: no
		// tint, numbered (context-style prefix), the syntax colors only.
		// They are unchanged lines the mod absorbed; any shade would read as
		// a change.
		for (const tl of r.tail ?? []) {
			const tlText = this.colOffset > 0 ? tl.text.slice(this.colOffset) : tl.text;
			const tlTokens = this.syntaxTokens(tlText);
			const tlPrefix = `${tl.newLine ?? ""}  `;
			if (tlText.length === 0) {
				out.push(tlPrefix);
				continue;
			}
			let start = 0;
			while (start < tlText.length) {
				const end = Math.min(tlText.length, start + textCols);
				const piece = tlText.slice(start, end);
				const visIdx = -1;
				out.push(tlPrefix + this.highlightTokens(tlTokens, start, end, ""));
				start = end;
			}
		}
		return out;
	}

	/** Number of wrapped visual lines a flat row occupies at the given width. */
	private segCountOf(r: Row, width: number): number {
		if (r.kind === "file" || r.kind === "sep") return 1;
		const cols = Math.max(1, width - prefixOf(r).length - 2);
		const wrap = (t: string): number => (t.length === 0 ? 1 : Math.max(1, Math.ceil(t.length / cols)));
		if (this.isWholesaleMod(r)) {
			return wrap(r.delText ?? "") + wrap(r.text);
		}
		const len = this.displayText(r).length;
		const multi = len === 0 ? 1 : this.displayText(r).split("\n").reduce((sum, line) => sum + Math.max(1, Math.ceil(line.length / cols)), 0);
		const tailSegs = (r.tail ?? []).reduce((sum, t) => sum + wrap(t.text), 0);
		return multi + tailSegs;
	}

	/**
	 * The mod row's inline display text: the common words + the removed words
	 * + the additions merged in place. Non-mod rows return their own text.
	 */
	/**
	 * The del spans worth showing: an alignment artifact whose text already
	 * exists verbatim in the add (e.g. a trailing `..]` absorbed into `..].`)
	 * is not a real removal — skip it, or the merge would duplicate it.
	 */
	private delSpansToShow(r: Row, delSpansIn?: [number, number][]): [number, number][] {
		const delSpans = delSpansIn ?? r.spans?.del ?? [];
		if (r.delText === undefined) return [];
		return delSpans.filter(([ds, de]) => {
			const raw = r.delText!.slice(ds, de);
			const t = raw.trim();
			// A whitespace-only span is a real indent change (8 → 12 spaces
			// moves the line into a block): show it struck at the line
			// start. Drop it only when the add's leading run is identical.
			if (t.length === 0) return raw !== (/^\s*/.exec(r.text)?.[0] ?? "");
			// A single-span del stays even when its text is inside the add
			// (`None` → `return None;` is a growth — the mod must have
			// something struck). With MULTIPLE spans, a span is an absorbed
			// fragment only when its text sits in the add AT THE SAME
			// POSITION (`..]` → `..].` — the `]` survives where it was, the
			// `.` is appended). A word that merely appears elsewhere in the
			// add (`lower` in the del's `"Found in lower layer"` vs
			// `?next_lower,` in the add) is a REAL replacement, not
			// absorption — `r.text.includes(t)` would drop it (the
			// `lower` → `layer below the top upper` rewrite reads as a pure
			// fill with no strike).
			if (delSpans.length <= 1) return true;
			return r.text.slice(ds, de) !== t;
		});
	}

	private displayText(r: Row): string {
		if ((r.kind !== "mod" && !r.moved) || r.delText === undefined || !this.wordDiff) {
			return this.colOffset > 0 ? r.text.slice(this.colOffset) : r.text;
		}
		// The merged string shares `mergedRuns` with the token stream — the
		// del insertions happen at the same offsets, so the sliced text and
		// the tokens stay in one coordinate space. Wholesale mods yield the
		// whole old + whole new pair (never the event-walk garbage).
		const out = this.mergedRuns(r).map((run) => run.text).join("");
		return this.colOffset > 0 ? out.slice(this.colOffset) : out;
	}

	/** Visual layout: per-flat-row segment counts + start offsets. */
	private visualGeometry(width: number): { starts: number[]; counts: number[]; total: number } {
		const starts: number[] = new Array(this.flat.length);
		const counts: number[] = new Array(this.flat.length);
		let total = 0;
		for (let i = 0; i < this.flat.length; i++) {
			starts[i] = total;
			const c = this.segCountOf(this.flat[i]!, width);
			counts[i] = c;
			total += c;
		}
		return { starts, counts, total };
	}

	/** Position the cursor at a visual (wrapped) line index. */
	private setCursorVis(v: number): void {
		const { starts, total } = this.visualGeometry(process.stdout.columns ?? 80);
		const clamped = Math.max(0, Math.min(total - 1, v));
		let row = 0;
		for (let i = 0; i < this.flat.length; i++) {
			if (starts[i]! <= clamped) row = i;
			else break;
		}
		this.rowIdx = row;
		this.visLine = Math.max(0, clamped - starts[row]!);
	}

	/** The current file's syntax tokens (plain when the toggle is off). */
	private syntaxTokens(text: string): { color: string; text: string }[] {
		if (!this.syntaxOn) return [{ color: "text", text }];
		const ext = this.file.path.includes(".") ? this.file.path.split(".").pop() ?? "" : "";
		if (ext === "md" || ext === "markdown") {
			// Fenced code blocks: the fence line toggles the language; the
			// code inside gets that language's syntax highlighting.
			const fence = /^\s*```([A-Za-z0-9_+-]*)\s*$/.exec(text);
			if (fence) {
				if (this.mdFence !== null) this.mdFence = null;
				else this.mdFence = fence[1]!.toLowerCase() || "";
				return [{ color: "muted", text }];
			}
			if (this.mdFence !== null) {
				return classifyTokens(text, langProfile(this.mdFence));
			}
			return markdownTokens(text);
		}
		return classifyTokens(text, langProfile(ext));
	}

	/** Style one wrapped segment of a diff row (subtle line tint, stronger word fill). */
	/**
	 * The mod row's inline token stream: common words ("common", syntax
	 * colors), the removed words ("del", plain — struck later), and the
	 * additions ("add", syntax colors + word fill), merged at the position
	 * where the change happens.
	 */
	/**
	 * A mod is rendered wholesale (whole old line struck, then whole new line
	 * filled, as separate blocks) when a token-level interleave would garble:
	 * the lines are structurally different (a one-liner → a block), or the
	 * LCS found different numbers of changed fragments on each side (their
	 * spans live in different coordinate spaces).
	 */
	private isWholesaleMod(r: Row): boolean {
		if (r.delText === undefined || !this.wordDiff) return false;
		return wholesalePair(r.delText, r.text, r.spans ?? { del: [], add: [] });
	}

	/**
	 * A mod whose change is purely a removal: every add span is whitespace —
	 * the new line is the old line minus text (`pub(super) struct X {` →
	 * `pub struct X {`, `resolved_layers` → `layers`). Nothing was added, so
	 * the row must not render as an addition: it takes the neutral shade
	 * (like a moved line — the line survives) with the red strikes carrying
	 * the removal.
	 */
	private isPureDeletionMod(r: Row): boolean {
		return (r.spans?.add ?? []).every(([s, e]) => r.text.slice(s, e).trim().length === 0);
	}

	/**
	 * The add-text position of each del span (the del offset minus the del
	 * text removed before it) and the pairing: a del span pairs with the add
	 * span nearest its own position (within a small window — the aligned
	 * replacement); a del span with no nearby add is a standalone removal.
	 * Shared by `isWholesaleMod` (split-word detection) and `mergedRuns`
	 * (the event positions) — the wholesale decision and the walk stay in
	 * lockstep by construction.
	 */
	private delPairing(delSpans: [number, number][], addSpans: [number, number][], addText: string, delText: string): {
		delPos: number[];
		pair: Map<number, number>;
	} {
		const delPos: number[] = [];
		let cum = 0;
		for (const [ds, de] of delSpans) {
			delPos.push(ds - cum);
			cum += de - ds;
		}
		const pair = new Map<number, number>();
		const usedAdds = new Set<number>();
		const opAnchor = new Map<number, number>();
		for (let j = 0; j < delSpans.length; j++) {
			let best = -1;
			let bestGap = 8;
			for (let i = 0; i < addSpans.length; i++) {
				if (usedAdds.has(i)) continue;
				// The aligned replacement occupies the same range; anything
				// else that overlaps the del's range interleaves in the merge
				// (`resolved_` at 13 must not pair with `.iter()` at 19 —
				// that would push the removal past `layers`). Overlapping
				// non-identical spans stay separate events.
				const [as, ae] = addSpans[i]!;
				const [ds, de] = delSpans[j]!;
				const delEnd = delPos[j]! + (de - ds);
				const same = delPos[j]! === as && delEnd === ae;
				const disjoint = delEnd <= as || ae <= delPos[j]!;
				// Whitespace spans pair despite the overlap — the indent
				// change is expressed minimally (8 → 12 spaces = 8 common
				// + 4 added), not as two independent whole runs.
				const wsPair =
					/^\s+$/.test(delText.slice(ds, de)) && /^\s+$/.test(addText.slice(as, ae));
				if (!same && !disjoint && !wsPair) continue;
				// Prefer the CONTENT counterpart over a closer punctuation
				// fragment (`0` ↔ `None`, never the type syntax `>` — the
				// strike must read `= ~~0~~None;`).
				// OPERATOR flips pair operator-to-operator with precedence: a
				// `<`→`>` comparison flip is either a bug or the logical twin
				// of another change — it must stand out (`k ~~<~~> floor`),
				// never bury itself inside a nearer word span. The add's
				// operator anchors at its own offset inside the span.
				const delOp = delText.slice(ds, de).trim();
				if (/^[<>]=?$/.test(delOp)) {
					// A lone comparison operator pairs with a comparison
					// operator wherever it appears — this is the counterpart
					// by definition, so it WINS over the nearest-span rule
					// (`k <` → `k >` strikes the `<` against the `>`; the
					// flip must stand out).
					let opBest = -1;
					let opBestGap = Infinity;
					for (let c = as; c < ae; c++) {
						if (addText[c] === "<" || addText[c] === ">") {
							const g = Math.abs(delPos[j]! - c) + (addText[c] === delOp ? 0 : 2);
							if (g < opBestGap) {
								opBestGap = g;
								opBest = i;
								opAnchor.set(j, c);
							}
						}
					}
					if (opBest >= 0) {
						best = opBest;
						bestGap = -1; // op wins over any gap rule
						continue;
					}
				}
				let opGap = Infinity;
				if (/^[<>!=+*/-]/.test(delOp)) {
					for (let c = as; c < ae; c++) {
						if ("<>!=+*/-%&|^~".includes(addText[c]!)) {
							opGap = Math.abs(delPos[j]! - c);
							break;
						}
					}
				}
				const gap = Math.abs(delPos[j]! - as) + (addText.slice(as, as + 1).match(/[A-Za-z0-9_]/) ? 0 : 5);
				// Whitespace pairs express the MINIMUM wherever they land —
				// the old `floor {`'s space before the brace pairs with the
				// new `floor\n                {`'s newline+indent even across
				// the run (a big gap): the strike/fill must NOT leak into
				// the middle of `&& k > floor` as `&&  k`.
				if (wsPair) {
					bestGap = 0;
					best = i;
					continue;
				}
				// `<=` — ties prefer the LATER span (the value `None` over
				// the type syntax `Option<`; `= ~~0~~None;` reads).
				if (gap <= bestGap) {
					bestGap = gap;
					best = i;
				}
			}
			if (best >= 0) {
				usedAdds.add(best);
				pair.set(j, best);
			}
		}
		return { delPos, pair, opAnchor };
	}

	/**
	 * The ordered common/del/add runs of a mod's merged view. Each del span
	 * pairs with the add span nearest its own add-position (within a small
	 * window — the aligned replacement); a del span with no nearby add (a
	 * standalone removal like `resolved_`) inserts at its own position, and a
	 * standalone add fills at its own position. Shared by `mergedTokens` (adds
	 * the syntax colors) and `displayText` (joins the texts) — they stay in
	 * lockstep by construction.
	 */
	private mergedRuns(r: Row): { kind: "common" | "del" | "add"; text: string }[] {
		if (this.isWholesaleMod(r)) {
			return [
				{ kind: "del", text: r.delText ?? "" },
				{ kind: "add", text: r.text },
			];
		}
		// The reverse stretch: the delText is a whole deleted block (the old
		// `find_mount` body) folded against a single add. The shared prefix
		// reads common (`self.mount_stack`), the block's rest strikes across
		// the visual lines, the add's rest fills — the del spans' own
		// positions (the chain's delPos) would interleave the fill into the
		// middle of the struck block (`…iter` + `get` + `…`), garbling it.
		const delText = r.delText ?? "";
		if (delText.includes("\n")) {
			let share = 0;
			while (share < delText.length && share < r.text.length && delText[share] === r.text[share]) share++;
			// The fill reads FIRST — the new one-liner (`self.mount_stack`
			// + filled `.get(guest_path)`) — then the struck old block
			// spans the following visual lines (`~~\n.filesystems…~~`).
			return [
				{ kind: "common", text: delText.slice(0, share) },
				{ kind: "add", text: r.text.slice(share) },
				{ kind: "del", text: delText.slice(share) },
			];
		}
		// A moved row has no spans of its own — compute them from its paired
		// del text so the indentation change subwords (8 → 12 spaces is a
		// semantic move, not invisible relocation).
		// The engine bakes a moved row's subword spans into the wire.
		const spans = r.spans ?? { del: [], add: [] };
		const addSpans = spans.add ?? [];
		const delSpans = this.delSpansToShow(r, spans.del);
		const { delPos, pair, opAnchor } = this.delPairing(delSpans, addSpans, r.text, r.delText ?? "");
		const runs: { kind: "common" | "del" | "add"; text: string }[] = [];
		const emitRange = (from: number, to: number) => {
			if (to > from) runs.push({ kind: "common", text: r.text.slice(from, to) });
		};
		// Events in add-text position order: each add span, each standalone
		// del (no nearby add), and each del paired with its nearest add. A
		// standalone removal must not trail the line — a `resolved_` deleted
		// near the start inserts before the appended `.iter()` at the end.
		type Ev = { pos: number; del?: [number, number]; add?: [number, number] };
		const events: Ev[] = [];
		const usedAdds = new Set<number>();
		for (let j = 0; j < delSpans.length; j++) {
			const ai = pair.get(j);
			if (ai !== undefined) {
				usedAdds.add(ai);
				// An operator flip anchors at the operator's own offset
				// (`k ~~<~~> floor` — the `<` strikes against the `>`).
				const pos = opAnchor.get(j) ?? addSpans[ai]![0];
				events.push({ pos, del: delSpans[j]!, add: addSpans[ai]! });
			} else {
				// A standalone landing mid-word must not glue the strike
				// inside a surviving token (`enumer~~.rev()~~ate()` is
				// wrong — "rev" stuck to "enumerate"'s "r"); snap forward
				// to the end of the enclosing token run so the removal
				// reads as a unit (`enumerate()~~.rev()~~ {`).
				const word = (c: string) => /[A-Za-z0-9_]/.test(c);
				let ins = delPos[j]!;
				if (ins > 0 && ins < r.text.length && word(r.text[ins - 1]!) && word(r.text[ins]!)) {
					while (ins < r.text.length && !/\s/.test(r.text[ins]!)) ins++;
				}
				events.push({ pos: ins, del: delSpans[j]! });
			}
		}
		for (let i = 0; i < addSpans.length; i++) {
			if (!usedAdds.has(i)) events.push({ pos: addSpans[i]![0], add: addSpans[i]! });
		}
		events.sort((a, b) => a.pos - b.pos);
		let pos = 0;
		for (const ev of events) {
			if (ev.pos > pos) emitRange(pos, ev.pos);
			if (ev.del) {
				// Whitespace pairs express the MINIMUM change: the shared
				// prefix survives as common, only the difference is struck
				// or filled (8 → 12 spaces = 8 common + 4 added, never the
				// whole run on each side).
				const delText = r.delText.slice(ev.del[0], ev.del[1]);
				if (ev.add && /^[\p{White_Space}\u200b]+$/u.test(delText) && /^[\p{White_Space}\u200b]+$/u.test(r.text.slice(ev.add[0], ev.add[1]))) {
					const addText = r.text.slice(ev.add[0], ev.add[1]);
					// The shared prefix is the ACTUAL matching chars — a
					// type change (`\t` vs 4 spaces) shares nothing: the
					// tab strikes, the spaces fill, both subworded.
					let share = 0;
					while (share < delText.length && share < addText.length && delText[share] === addText[share]) share++;
					if (share > 0) runs.push({ kind: "common", text: delText.slice(0, share) });
					if (share < delText.length) runs.push({ kind: "del", text: delText.slice(share) });
					if (share < addText.length) runs.push({ kind: "add", text: addText.slice(share) });
					pos = ev.add[1];
					continue;
				}
				runs.push({ kind: "del", text: delText });
				pos = Math.max(pos, ev.pos);
			}
			if (ev.add) {
				runs.push({ kind: "add", text: r.text.slice(ev.add[0], ev.add[1]) });
				pos = ev.add[1];
			}
		}
		if (pos < r.text.length) emitRange(pos, r.text.length);
		return runs;
	}

	private mergedTokens(r: Row): { kind: "common" | "del" | "add"; color: string; text: string }[] | null {
		if (r.delText === undefined || !this.wordDiff) return null;
		if (this.isWholesaleMod(r)) {
			return [
				{ kind: "del", color: "text", text: r.delText },
				{ kind: "add", color: "text", text: r.text },
			];
		}
		const profile = this.syntaxOn ? this.syntaxProfile() : null;
		const out: { kind: "common" | "del" | "add"; color: string; text: string }[] = [];
		for (const run of this.mergedRuns(r)) {
			if (run.kind === "del") {
				out.push({ kind: "del", color: "text", text: run.text });
			} else {
				for (const t of classifyTokens(run.text, profile)) {
					out.push({ kind: run.kind, color: t.color, text: t.text });
				}
			}
		}
		return out;
	}

	/** The current file's syntax profile. */
	private syntaxProfile(): LangProfile | null {
		const ext = this.file.path.includes(".") ? this.file.path.split(".").pop() ?? "" : "";
		return langProfile(ext);
	}

	/** The current file's syntax tokens (plain when the toggle is off). */
	private syntaxTokens(text: string): { color: string; text: string }[] {
		if (!this.syntaxOn) return [{ color: "text", text }];
		return classifyTokens(text, this.syntaxProfile());
	}

	/** Style a token slice with the syntax colors, re-applying the CALLER's
	 * background after every token — the theme's fg resets kill whatever
	 * came before, and the re-application must not hardcode the green (a
	 * neutral wholesale block or a mod tail would render its later tokens
	 * green). */
	private highlightTokens(tokens: { color: string; text: string }[], from: number, to: number, bg = `\x1b[48;5;${sAdd.tint}m`): string {
		const theme = this.theme;
		let out = "";
		let pos = 0;
		for (const tok of tokens) {
			const tokStart = pos;
			const tokEnd = pos + tok.text.length;
			pos = tokEnd;
			if (tokEnd <= from || tokStart >= to) continue;
			const s = Math.max(from, tokStart);
			const e = Math.min(to, tokEnd);
			const piece = tok.text.slice(s - tokStart, e - tokStart);
			if (!piece) continue;
			out += theme.fg(tok.color, piece) + bg;
		}
		return out;
	}

	/** Style one wrap segment of a row's token stream (merged or plain).
	 * Syntax colors continue across segments because the stream is sliced,
	 * never re-tokenized. */
	private styleTokens(
		r: Row,
		tokens: { kind?: "common" | "del" | "add"; color: string; text: string }[],
		segStart: number,
		segEnd: number,
		prefix: string,
		selected: boolean,
	): string {
		const theme = this.theme;
		const isCtx = r.kind === "ctx" || r.kind === "sep";
		// Moved rows style like context: syntax-highlighted, with only a
		// whisper of shade (one step lighter than the old neutral) so they
		// read as relocated content, not plain context — and never as the
		// addition green (the content is not new, it moved).
		const isMov = r.moved === true;
		// Deleted rows carry the red tint (the inline removed words inside a
		// mod row share it); the selected lift steps one shade lighter.
		const isDelRow = r.kind === "del";
		// MOD rows all render on the same slight grey as the moved / all-
		// deleted rows: a `~` line is never "new content" — the struck and
		// filled words are the only color, so the actual differences stand
		// out. The green is reserved for the pure `+` additions. All four
		// tints come from the ROW_STYLES theme.
		const isPureDelMod = r.kind === "mod" && this.isPureDeletionMod(r);
		const isModRow = r.kind === "mod";
		const lineBg = isCtx
			? selected
				? "\x1b[48;5;237m"
				: ""
			: isMov
				? selected
					? `\x1b[48;5;${sMov.cursor}m`
					: `\x1b[48;5;${sMov.tint}m`
				: isModRow
					? selected
						? `\x1b[48;5;${sMod.cursor}m`
						: `\x1b[48;5;${sMod.tint}m`
					: isDelRow
						? selected
							? `\x1b[48;5;${sDel.cursor}m`
							: `\x1b[48;5;${sDel.tint}m`
						: selected
							? `\x1b[48;5;${sAdd.cursor}m`
							: `\x1b[48;5;${sAdd.tint}m`;
		const prefixBg = selected && !isCtx ? lineBg : "";
		const wordBg = selected ? `\x1b[48;5;${sMod.fillCursor}m` : `\x1b[48;5;${sMod.fill}m`;
		const delBg = `\x1b[48;5;${sDel.tint}m`;
		const baseBg = (kind: string | undefined): string => {
			if (isCtx && !selected) return "";
			if (kind === "del") return delBg;
			if (kind === "add" && !isPureDelMod) return wordBg;
			return lineBg;
		};
		const text = this.displayText(r);
		// Deleted rows: removed-text color on the red tint (same fg as the
		// struck words inside a mod row).
		if (isDelRow) {
			const bg = isCtx ? "" : lineBg;
			const piece = text.slice(segStart, segEnd);
			return (selected ? lineBg : "") + prefix + bg + theme.fg(sDel.fg, piece) + lineBg;
		}
		const slice = text.slice(segStart, segEnd);
		if (tokens.length === 0 || tokens.every((t) => t.text.length === 0)) {
			// Blank line: still carry the (selected) background tint so the
			// cursor row lights up even on empty context lines.
			const bg = isCtx ? (selected ? "\x1b[48;5;237m" : "") : lineBg;
			return (selected && !isCtx ? lineBg : "") + prefix + bg + slice;
		}
		let out = "";
		let pos = 0;
		for (const tok of tokens) {
			const tokStart = pos;
			const tokEnd = pos + tok.text.length;
			pos = tokEnd;
			if (tokEnd <= segStart || tokStart >= segEnd) continue;
			const s = Math.max(segStart, tokStart);
			const e = Math.min(segEnd, tokEnd);
			const piece = tok.text.slice(s - tokStart, e - tokStart);
			if (!piece) continue;
			const bg = baseBg(tok.kind);
			if (tok.kind === "del") {
				out += bg + "\x1b[9m" + theme.fg(sDel.fg, piece) + "\x1b[29m";
			} else {
				const sgr = (tok as { sgr?: number }).sgr;
				const off = sgr !== undefined ? SGR_OFF[sgr] : undefined;
				out += bg + (sgr !== undefined ? `\x1b[${sgr}m` : "") + theme.fg(tok.color, piece) + (off !== undefined ? `\x1b[${off}m` : "") + bg;
			}
		}
		// End the row on the LINE tint, never the last token's bg: a trailing
		// word-fill would paint the rest of the terminal width as "added".
		return prefixBg + prefix + out + (selected || !isCtx ? lineBg : "");
	}

	invalidate(): void {}
}

// ── Extension ──────────────────────────────────────────────────────────────

export default function (pi: ExtensionAPI) {
	pi.registerCommand("review", {
		description: "Interactive review of a git diff (notes, hunk staging, context expansion)",
		handler: async (args, ctx) => {
			if (ctx.mode !== "tui" || !ctx.hasUI) {
				ctx.ui.notify("/review requires the TUI", "warning");
				return;
			}

			const rootRes = await pi.exec("git", ["rev-parse", "--show-toplevel"], { cwd: ctx.cwd });
			if (rootRes.code !== 0 || !rootRes.stdout.trim()) {
				ctx.ui.notify("not a git work tree", "error");
				return;
			}
			const repoRoot = rootRes.stdout.trim();
			const repoName = repoRoot.split("/").pop() || repoRoot;
			const git = makeGitRunner(pi, repoRoot);

			// cwd may be a subdirectory of the repo: path args need the prefix.
			const prefixRes = await pi.exec("git", ["rev-parse", "--show-prefix"], { cwd: ctx.cwd });
			const repoPrefix = prefixRes.code === 0 ? prefixRes.stdout.trim() : "";
			const base = parseReviewArgs(args, ctx.cwd, repoPrefix);
			const context = defaultContext();
			const { files, branch, untrackedSkipped } = await loadReview(git, repoRoot, base, context);
			if (files.length === 0) {
				ctx.ui.notify("no changes to review", "info");
				return;
			}

			const session: ReviewSession = {
				repo: repoName,
				repoRoot,
				files,
				notes: loadNotes(),
				context,
				base,
				branch,
				git,
			};

			// Full-screen overlay: `ctx.ui.custom` with overlay:false hosts the
			// component in the bottom editor strip (a few rows) — the diff is
			// clipped and the pi transcript stays scrollable underneath. The
			// overlay covers the terminal, takes input ownership, and lets the
			// component's own wheel/drag handling scroll the diff.
			// Enable SGR mouse tracking so the terminal forwards wheel/drag
			// events (pi itself does not enable mouse reporting).
			const initial = resolveInitialPosition(files, loadReviewState(repoName));
			process.stdout.write("\x1b[?1002h\x1b[?1006h");
			try {
				await ctx.ui.custom<void>(
					(tui, theme, _kb, done) => {
						const comp = new ReviewComponent(session, tui, theme, done, initial);
						comp.notify = (msg, kind) => ctx.ui.notify(msg, kind);
						comp.confirm = async (msg) => {
							const r = await ctx.ui.confirm("Discard", msg);
							return r === true;
						};
						// The ? popup: a small overlay with the full key bindings.
						comp.helpPopup = () => {
							void ctx.ui.custom<void>(
								(tui2, theme2, _kb2, done2) => {
									const helpLines = [
										"  j/k move row · J/K ×5 · g/G top/bottom",
										"  n/p (or ]/[) next/previous hunk · tab file",
										"  v select range (j/k extend, v confirm, esc cancel)",
										"  m note on selection · d delete note",
										"  o open editor at cursor (range pre-selected in vim)",
										"  e/E expand ctx ▴/▾ · c/C collapse · a full · A all-full",
										"  r/R reset ctx · w word-diff · t syntax",
										"  ←/→ or h/l scroll ±10 cols · ^ to start · $ to end",
										"  NG jump to line (expands context if needed)",
										"  s/S stage hunk/file · u/U unstage · x/X discard (safety-stashed)",
										"  - removed · ≈ moved · + added · ~ changed (struck = removed words)",
										"  q/esc close this popup · q (outside) quit review",
									];
									return {
										render: (w: number) =>
											helpLines.map((l) => theme2.fg("text", l.length > w - 2 ? l.slice(0, Math.max(0, w - 5)) + "…" : l)),
										handleInput: (d: string) => {
											if (d === "q" || d === "Q" || matchesKey(d, Key.escape)) done2();
										},
										invalidate: () => {},
									};
								},
								{ overlay: true, overlayOptions: { width: "72%", height: "45%" } },
							);
						};
						return comp;
					},
					{
						overlay: true,
						overlayOptions: { width: "100%", height: "100%" },
					},
				);
			} finally {
				process.stdout.write("\x1b[?1002l\x1b[?1006l");
			}

			const total = allNotes(session.notes, session.repo).length;
			ctx.ui.notify(
				`review finished · ${session.files.length} file(s) · ${total} note${total === 1 ? "" : "s"} in this repo${
					untrackedSkipped.length > 0 ? ` · skipped ${untrackedSkipped.length} binary untracked` : ""
				}`,
				"info",
			);
		},
	});
}

// ── Test hook (not used by pi) ─────────────────────────────────────────────

/** Exposes the pure logic for `node`-side unit tests. */
export const reviewInternals = {
	buildVisible,
	allocateContext,
	contextRows,
	hiddenGap,
	rowsCoveredBy,
	rowCoord,
	editorArgs,
	editorBinary,
	splitTemplate,
	substitute,
	parseReviewArgs,
	defaultContext,
	ReviewComponent,
	fileCounts,
	syntaxHighlight,
	classifyTokens,
	langProfile,
	loadReviewState,
	saveReviewState,
	resolveInitialPosition,
	diffuseBinPath,
	runDiffuseBin,
	filesFromWire,
};
