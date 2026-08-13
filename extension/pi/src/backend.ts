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

export async function loadReview(
	_git: GitRunner,
	repoRoot: string,
	base: ReviewBase,
	context: number,
): Promise<{ files: DiffFile[]; branch: string; untrackedSkipped: string[] }> {
	const payload = {
		repoRoot,
		baseArgs: base.baseArgs,
		paths: base.paths,
		cached: base.cached,
		context,
		notesFile: NOTES_FILE,
		repo: base.repo,
	};
	const wire = await runDiffuseBin(diffuseBinPath(), JSON.stringify(payload));
	const files = filesFromWire(wire, () => undefined, context);
	const branch = (wire as any).branch ?? "";
	const untrackedSkipped = (wire as any).untrackedSkipped ?? [];
	return { files, branch, untrackedSkipped };
}
