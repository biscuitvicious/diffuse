/**
 * Code Review extension for pi — Thin Client for the native diffuse binary TUI
 */

import { spawn } from "node:child_process";
import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { diffuseBinPath, parseReviewArgs } from "./src/backend.ts";
import { allNotes, loadNotes, NOTES_FILE } from "./src/persistence.ts";

export default function (pi: ExtensionAPI) {
	pi.registerCommand("review", {
		description: "Interactive review of a git diff (native diffuse TUI)",
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

			const prefixRes = await pi.exec("git", ["rev-parse", "--show-prefix"], { cwd: ctx.cwd });
			const repoPrefix = prefixRes.code === 0 ? prefixRes.stdout.trim() : "";
			const parsed = parseReviewArgs(args, ctx.cwd, repoPrefix);

			const bin = diffuseBinPath();
			const argv: string[] = [
				...parsed.baseArgs,
				...parsed.paths,
				"--repo",
				repoName,
				"--notes-file",
				NOTES_FILE,
			];

			ctx.tui.stop();
			try {
				await new Promise<number>((resolve, reject) => {
					const p = spawn(bin, argv, {
						cwd: repoRoot,
						stdio: "inherit",
						env: process.env,
					});
					p.on("error", reject);
					p.on("close", (code) => resolve(code ?? 0));
				});
			} finally {
				ctx.tui.start();
				ctx.tui.requestRender(true);
			}

			const notes = loadNotes();
			const total = allNotes(notes, repoName).length;
			ctx.ui.notify(
				`review closed · ${total} note${total === 1 ? "" : "s"} in this repo`,
				"info",
			);
		},
	});
}
