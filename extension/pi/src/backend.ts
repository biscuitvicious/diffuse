/**
 * Diffuse binary resolution & arg parsing for pi-review
 */

import { spawn } from "node:child_process";
import { existsSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

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

export function runDiffuseBin(bin: string, input: string): Promise<any> {
	return new Promise((resolve, reject) => {
		const p = spawn(bin, ["--json"], { stdio: ["pipe", "pipe", "pipe"] });
		let out = "";
		let err = "";
		p.stdout.on("data", (d: Buffer) => (out += d.toString("utf8")));
		p.stderr.on("data", (d: Buffer) => (err += d.toString("utf8")));
		p.on("error", reject);
		p.on("close", (code) => {
			if (code !== 0) reject(new Error(`diffuse exited ${code}: ${err}`));
			else {
				try {
					resolve(JSON.parse(out));
				} catch (e) {
					reject(new Error(`diffuse output was not JSON: ${String(e)}`));
				}
			}
		});
		p.stdin.write(input);
		p.stdin.end();
	});
}
