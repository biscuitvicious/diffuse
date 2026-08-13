/**
 * Git actions, editor invocation, and note management for ReviewComponent
 */

import { spawnSync } from "node:child_process";
import { randomUUID } from "node:crypto";
import { existsSync, readFileSync, unlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { loadReview } from "./backend.ts";
import { editorArgs, editorBinary } from "./editor.ts";
import { rowCoord } from "./model.ts";
import { addNote, removeNotes } from "./persistence.ts";
import type { DiffFile, ReviewSession, Row } from "./types.ts";

export interface ComponentActionContext {
	session: ReviewSession;
	rowIdx: number;
	flat: Row[];
	fileIdx: number;
	file: DiffFile;
	noteDraft: string | null;
	safetyStashes: string[];
	tui: {
		requestRender: (force?: boolean) => void;
		stop: () => void;
		start: () => void;
	};
	notify: ((msg: string, kind: "info" | "warning" | "error") => void) | null;
	confirm: ((msg: string) => Promise<boolean>) | null;
	done: (result: number) => void;
	computeSelection: () => { file: number; start: number; end: number; side: "new" | "old" } | null;
	currentHunkIdx: () => number;
	firstRowOfFile: (fi: number) => number;
	rebuild: () => void;
	requestRender: () => void;
	setRowIdx: (idx: number) => void;
	setVisLine: (line: number) => void;
	setNoteDraft: (draft: string | null) => void;
	setConfirmed: (conf: any) => void;
}

export function commitNoteAction(ctx: ComponentActionContext): void {
	const text = ctx.noteDraft?.trim() ?? "";
	ctx.setNoteDraft(null);
	if (!text) return;
	const sel = ctx.computeSelection();
	const row = ctx.flat[ctx.rowIdx];
	const side = sel?.side ?? (row && row.newLine !== null ? "new" : "old");
	const c = row ? rowCoord(row) : null;
	const start = sel?.start ?? c;
	const end = sel?.end ?? c;
	if (start === null || end === null) return;
	addNote(ctx.session.notes, ctx.session.repo, {
		id: randomUUID(),
		file: ctx.file.path,
		side,
		start,
		end,
		text,
		createdAt: Date.now(),
	});
	ctx.setConfirmed(null);
}

export function deleteNotesAction(ctx: ComponentActionContext): void {
	const sel = ctx.computeSelection();
	const removed = sel
		? removeNotes(ctx.session.notes, ctx.session.repo, ctx.file.path, sel.side, sel.start, sel.end)
		: (() => {
				const c = rowCoord(ctx.flat[ctx.rowIdx]!);
				if (c === null) return 0;
				return (
					removeNotes(ctx.session.notes, ctx.session.repo, ctx.file.path, "new", c, c) +
					removeNotes(ctx.session.notes, ctx.session.repo, ctx.file.path, "old", c, c)
				);
			})();
	if (sel) ctx.setConfirmed(null);
	ctx.notify?.(removed > 0 ? `deleted ${removed} note${removed === 1 ? "" : "s"}` : "no note here", "info");
	ctx.requestRender();
}

export function openEditorAction(ctx: ComponentActionContext): void {
	const f = ctx.file;
	const row = ctx.flat[ctx.rowIdx];
	if (!row) return;
	const abs = join(ctx.session.repoRoot, f.path);
	if (f.status === "deleted" || !existsSync(abs)) {
		ctx.notify?.(`cannot edit: ${f.path} is deleted`, "warning");
		return;
	}
	const coord = row.newLine ?? row.oldLine;
	if (coord === null) {
		ctx.notify?.("cursor is on a marker or deleted line — no new-file position", "warning");
		return;
	}
	const sel = ctx.computeSelection();
	const range = sel && sel.side === "new" ? { start: sel.start, end: sel.end } : null;
	const bin = editorBinary();
	const built = editorArgs(bin, abs, coord, range);
	if (!built) return;

	if (built.detached) {
		spawnSync(built.argv[0]!, built.argv.slice(1), { stdio: "ignore", detached: true, cwd: ctx.session.repoRoot });
		ctx.notify?.(`opened ${f.path}:${coord} in ${bin}`, "info");
		return;
	}

	ctx.notify?.(`opening ${bin} at ${f.path}:${coord}${range ? ` (${range.start}-${range.end})` : ""}`, "info");
	ctx.tui.stop();
	process.stdout.write("\x1b[2J\x1b[H");
	const shell = process.env.SHELL || "/bin/sh";
	const cmd = built.argv.map((a) => `'${a.replace(/'/g, "'\\''")}'`).join(" ");
	spawnSync(shell, ["-c", cmd], { stdio: "inherit", env: process.env, cwd: ctx.session.repoRoot });
	ctx.tui.start();
	ctx.tui.requestRender(true);
	refreshFileContentAction(ctx);
}

export function refreshFileContentAction(ctx: ComponentActionContext): void {
	const f = ctx.file;
	try {
		if (existsSync(join(ctx.session.repoRoot, f.path))) {
			f.newContent = readFileSync(join(ctx.session.repoRoot, f.path), "utf8").split("\n");
			f.newLineCount = f.newContent.length;
			f.noContent = f.newContent.length === 0 && f.oldContent.length === 0;
		}
	} catch {
		// keep previous content
	}
	ctx.rebuild();
	ctx.requestRender();
}

export async function hunkPatchAction(ctx: ComponentActionContext): Promise<string | null> {
	const f = ctx.file;
	const h = f.hunks[ctx.currentHunkIdx()]!;

	if (f.status === "untracked") {
		const body = [`@@ -0,0 +1,${h.newCount} @@`, ...h.rows.map((r) => `+${r.text}`)].join("\n");
		return `diff --git a/${f.path} b/${f.path}\n--- /dev/null\n+++ b/${f.path}\n${body}\n`;
	}

	const res = await ctx.session.git([
		"diff", "-M", "--no-color", "-U3", ...ctx.session.base.baseArgs, "--", f.path,
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
	const a = f.status === "added" ? "/dev/null" : `a/${f.path}`;
	const b = f.status === "deleted" ? "/dev/null" : `b/${f.path}`;
	return `diff --git ${a} ${b}\n--- ${a}\n+++ ${b}\n${block}`;
}

export async function applyPatchAction(ctx: ComponentActionContext, patch: string, flags: string[]): Promise<boolean> {
	const tmp = join(tmpdir(), `review-${process.pid}-${randomUUID().slice(0, 8)}.patch`);
	writeFileSync(tmp, patch);
	try {
		const check = await ctx.session.git(["apply", "--check", ...flags, tmp]);
		if (check.code !== 0) {
			ctx.notify?.(`patch does not apply: ${(check.stderr || check.stdout).trim()}`, "error");
			return false;
		}
		const res = await ctx.session.git(["apply", ...flags, tmp]);
		if (res.code !== 0) {
			ctx.notify?.(`apply failed: ${(res.stderr || res.stdout).trim()}`, "error");
			return false;
		}
		return true;
	} finally {
		unlinkSync(tmp);
	}
}

export async function stageAction(ctx: ComponentActionContext, wholeFile: boolean): Promise<void> {
	if (ctx.file.status !== "untracked" && (ctx.session.base.cached || ctx.session.base.baseArgs.length > 0)) {
		ctx.notify?.("staging requires the default worktree review (/review)", "warning");
		return;
	}
	if (wholeFile) {
		const res = await ctx.session.git(["add", "--", ctx.file.path]);
		if (res.code !== 0) return void ctx.notify?.(`git add failed: ${res.stderr.trim()}`, "error");
	} else {
		const patch = await hunkPatchAction(ctx);
		if (!patch) return void ctx.notify?.("cannot build hunk patch", "error");
		if (!(await applyPatchAction(ctx, patch, ["--cached"]))) return;
	}
	await refreshAfterGitOpAction(ctx, "staged");
}

export async function unstageAction(ctx: ComponentActionContext, wholeFile: boolean): Promise<void> {
	if (!ctx.session.base.cached) {
		ctx.notify?.("unstage requires reviewing the index (/review --cached)", "warning");
		return;
	}
	if (wholeFile) {
		const res = await ctx.session.git(["reset", "-q", "--", ctx.file.path]);
		if (res.code !== 0) return void ctx.notify?.(`git reset failed: ${res.stderr.trim()}`, "error");
	} else {
		const patch = await hunkPatchAction(ctx);
		if (!patch) return void ctx.notify?.("cannot build hunk patch", "error");
		if (!(await applyPatchAction(ctx, patch, ["--cached", "-R"]))) return;
	}
	await refreshAfterGitOpAction(ctx, "unstaged");
}

export async function discardAction(ctx: ComponentActionContext, wholeFile: boolean): Promise<void> {
	if (ctx.session.base.cached) {
		ctx.notify?.("discard works on the worktree, not the index", "warning");
		return;
	}
	const label = wholeFile ? `discard all changes to ${ctx.file.path}?` : `discard this hunk in ${ctx.file.path}?`;
	const ok = ctx.confirm ? await ctx.confirm(label) : false;
	if (!ok) return;
	if (wholeFile) {
		await stashCheckpointAction(ctx);
		const res = await ctx.session.git(["checkout", "--", ctx.file.path]);
		if (res.code !== 0) return void ctx.notify?.(`checkout failed: ${res.stderr.trim()}`, "error");
	} else {
		const patch = await hunkPatchAction(ctx);
		if (!patch) return void ctx.notify?.("cannot build hunk patch", "error");
		await stashCheckpointAction(ctx);
		if (!(await applyPatchAction(ctx, patch, ["-R"]))) return;
	}
	await refreshAfterGitOpAction(ctx, "discarded");
}

export async function stashCheckpointAction(ctx: ComponentActionContext): Promise<void> {
	try {
		const res = await ctx.session.git(["stash", "create", "review-checkpoint"]);
		const ref = res.stdout.trim();
		if (res.code === 0 && ref) ctx.safetyStashes.push(ref);
	} catch {
		// checkpoint is best-effort
	}
}

export async function refreshAfterGitOpAction(ctx: ComponentActionContext, action: string): Promise<void> {
	const current = ctx.file.path;
	const { files } = await loadReview(ctx.session.git, ctx.session.repoRoot, ctx.session.base, ctx.session.context);
	if (files.length === 0) {
		ctx.notify?.(`all changes ${action} — review clean!`, "info");
		ctx.done(0);
		return;
	}
	ctx.session.files = files;
	ctx.rebuild();
	const fi = ctx.session.files.findIndex((f) => f.path === current);
	const targetFile = fi >= 0 ? fi : Math.min(ctx.fileIdx, ctx.session.files.length - 1);
	const start = ctx.firstRowOfFile(targetFile);
	ctx.setRowIdx(Math.min(start + 1, Math.max(0, ctx.flat.length - 1)));
	ctx.setVisLine(0);
	const stashes = ctx.safetyStashes.length > 0 ? ` (stash: ${ctx.safetyStashes[ctx.safetyStashes.length - 1]})` : "";
	ctx.notify?.(`${action}${stashes}`, "info");
	ctx.requestRender();
}
