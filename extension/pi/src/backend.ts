/**
 * Diff loading, git plumbing & Rust backend execution for pi-review
 */

import { spawn } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { NOTES_FILE } from "./persistence.ts";
import type { DiffFile, ReviewArgs, WireOutput } from "./types.ts";

export type GitRunner = (args: string[]) => Promise<{ stdout: string; stderr: string; code: number }>;

export function makeGitRunner(pi: ExtensionAPI, cwd: string): GitRunner {
	return async (args) => {
		const res = await pi.exec("git", args, { cwd });
		return { stdout: res.stdout, stderr: res.stderr, code: res.code ?? 1 };
	};
}

export interface ReviewBase {
	baseArgs: string[];
	label: string;
	cached: boolean;
	paths: string[];
	repo: string;
}

export function parseReviewArgs(args: string | undefined, cwd: string, repoPrefix = ""): ReviewBase {
	const tokens = args ? args.split(/\s+/).filter(Boolean) : [];
	const baseArgs: string[] = [];
	const paths: string[] = [];
	let cached = false;
	let afterDash = false;
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
			baseArgs.push(t);
		}
	}
	const label = cached ? "staged" : baseArgs.length > 0 ? baseArgs.join(" ") : "HEAD";
	return { baseArgs, label, cached, paths, repo: "" };
}

export function diffuseBinPath(): string {
	const explicit = process.env.REVIEW_DIFFUSE_BIN;
	if (explicit) return explicit;
	const here = dirname(fileURLToPath(import.meta.url));
	const profile = process.env.REVIEW_PROFILE === "release" ? "release" : "debug";
	const bundled = join(here, "..", "bin", "diffuse");
	if (existsSync(bundled)) return bundled;
	return join(here, "..", "..", "..", "target", profile, "diffuse");
}

export function runDiffuseBin(bin: string, input: string): Promise<WireOutput> {
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

export function runDiffuseBinBatch(bin: string, input: string): Promise<WireOutput[]> {
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
					resolve(JSON.parse(out) as WireOutput[]);
				} catch (e) {
					reject(new Error(`diffuse output was not JSON: ${String(e)}`));
				}
			}
		});
		p.stdin.write(input);
		p.stdin.end();
	});
}

export function filesFromWire(
	wire: WireOutput,
	contentOf: (path: string) => { newContent: string[]; oldContent: string[]; oldPath: string | null } | undefined,
	context: number,
): DiffFile[] {
	const out: DiffFile[] = [];
	for (const wf of wire.files) {
		const src = contentOf(wf.path);
		out.push({
			path: wf.path,
			status: wf.status as any,
			newLineCount: (wf as any).newLineCount ?? 0,
			oldContent: src?.oldContent ?? [],
			newContent: src?.newContent ?? [],
			hunks: wf.hunks.map((h) => ({
				oldStart: h.oldStart,
				oldCount: h.oldCount,
				newStart: h.newStart,
				newCount: h.newCount,
				rows: h.rows.map((r) => ({
					kind: r.kind,
					text: r.text,
					newLine: r.newLine,
					oldLine: r.oldLine,
					delText: r.delText,
					moved: (r as any).moved,
					wholesale: (r as any).wholesale,
					spans: (r as any).spans,
					runs: r.runs,
					tail: (r as any).tail,
				})),
				aboveCtx: context,
				belowCtx: context,
			})),
			noContent: !src?.newContent?.length && !src?.oldContent?.length,
			notes: wf.notes ?? [],
			rows: wf.rows,
		});
	}
	return out;
}

export interface RefPair {
	worktreeNew: boolean;
	newRef: string | null;
	oldRef: string | null;
}

export async function resolveRefs(git: GitRunner, baseArgs: string[], cached: boolean): Promise<RefPair> {
	const refs = baseArgs.filter((t) => t !== "--cached" && !t.startsWith("-"));
	if (refs.length === 1) {
		const r = refs[0]!;
		if (r.endsWith("^!")) {
			const c = r.slice(0, -2);
			return { worktreeNew: false, newRef: c, oldRef: `${c}^` };
		}
		const twoDot = /^(.+\.\.)(.+)$/.exec(r);
		if (twoDot) return { worktreeNew: false, newRef: twoDot[2]!, oldRef: twoDot[1]!.slice(0, -2) };
		const threeDot = /^(.+)\.{3}(.+)$/.exec(r);
		if (threeDot) {
			const mb = await git(["merge-base", threeDot[1]!, threeDot[2]!]);
			return { worktreeNew: false, newRef: threeDot[2]!, oldRef: mb.code === 0 ? mb.stdout.trim() : threeDot[1]! };
		}
		return { worktreeNew: true, newRef: null, oldRef: r };
	}
	if (refs.length === 2) {
		return { worktreeNew: false, newRef: refs[1]!, oldRef: refs[0]! };
	}
	return { worktreeNew: true, newRef: null, oldRef: cached ? "HEAD" : null };
}

export async function rustBackendLoad(
	git: GitRunner,
	repoRoot: string,
	tsFiles: DiffFile[],
	diffText: string,
	context: number,
	delOf: Record<string, number>,
	refs: RefPair,
	repo: string,
): Promise<DiffFile[]> {
	for (const f of tsFiles) {
		if (f.status === "deleted") continue;
		if (refs.worktreeNew) {
			const newAbs = join(repoRoot, f.path);
			if (existsSync(newAbs)) {
				try {
					f.newContent = readFileSync(newAbs, "utf8").split("\n");
				} catch {
					f.newContent = [];
				}
			}
		} else if (refs.newRef !== null) {
			const res = await git(["show", `${refs.newRef}:${f.path}`]);
			if (res.code === 0) f.newContent = res.stdout.split("\n");
		}
		f.newLineCount = f.newContent.length;
	}
	const oldTreeRef = refs.oldRef ?? "HEAD";
	for (const f of tsFiles) {
		const newEmpty = f.newContent.length === 0;
		const needsOld = f.status === "deleted" || (delOf[f.path] ?? 0) > 0 || newEmpty;
		if (!needsOld) continue;
		const oldRes = await git(["show", `${oldTreeRef}:${f.path}`]);
		if (oldRes.code === 0) {
			f.oldContent = oldRes.stdout.split("\n");
			if (newEmpty && f.status !== "deleted") {
				f.newContent = f.oldContent;
				f.newLineCount = f.oldContent.length;
			}
		}
		f.noContent = f.newContent.length === 0 && f.oldContent.length === 0;
	}
	const contents: Record<string, string> = {};
	for (const f of tsFiles) contents[f.path] = f.newContent.join("\n");
	const wire = await runDiffuseBin(
		diffuseBinPath(),
		JSON.stringify({ diff: diffText, contents, notesFile: NOTES_FILE, repo }),
	);
	const files = filesFromWire(
		wire,
		(p) => {
			const f = tsFiles.find((x) => x.path === p);
			return f
				? { newContent: f.newContent, oldContent: f.oldContent, oldPath: null }
				: undefined;
		},
		context,
	);
	for (const f of tsFiles) {
		if (f.status === "untracked") files.push(f);
	}
	return files;
}

export async function loadReview(
	git: GitRunner,
	repoRoot: string,
	base: ReviewBase,
	context: number,
): Promise<{ files: DiffFile[]; branch: string; untrackedSkipped: string[] }> {
	const branchRes = await git(["branch", "--show-current"]);
	const branch = branchRes.code === 0 ? branchRes.stdout.trim() : "";

	const diffRes = await git([
		"diff",
		`-U${context}`,
		"-M",
		"--no-color",
		...base.baseArgs,
		"--",
		...base.paths,
	]);
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
		let status: any = "modified";
		let path = parts[1] ?? "";
		if (st.startsWith("A")) status = "added";
		else if (st.startsWith("D")) status = "deleted";
		else if (st.startsWith("R")) {
			status = "renamed";
			path = parts[2] ?? "";
		}
		files.push({
			path,
			status,
			newLineCount: 0,
			newContent: [],
			oldContent: [],
			hunks: [],
			noContent: false,
		});
	}
	const untrackedSkipped: string[] = [];

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
					status: "untracked",
					newLineCount: lines.length,
					newContent: lines,
					oldContent: [],
					hunks: [
						{
							oldStart: 0,
							oldCount: 0,
							newStart: 1,
							newCount: lines.length,
							rows: lines.map((t, i) => ({ kind: "add", text: t, newLine: i + 1, oldLine: null })),
							aboveCtx: 0,
							belowCtx: 0,
						},
					],
					noContent: false,
				});
			}
		}
	}

	const refs = await resolveRefs(git, base.baseArgs, base.cached);
	return {
		files: await rustBackendLoad(git, repoRoot, files, diffRes.stdout, context, delOf, refs, base.repo),
		branch,
		untrackedSkipped,
	};
}
