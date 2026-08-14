/**
 * Code Review extension for pi — Entrypoint & Command Registration
 */

import { existsSync, statSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { Key, matchesKey } from "@earendil-works/pi-tui";

import {
	diffuseBinPath,
	filesFromWire,
	loadReview,
	makeGitRunner,
	parseReviewArgs,
	runDiffuseBin,
	runDiffuseBinBatch,
} from "./src/backend.ts";
import { ReviewComponent } from "./src/component.ts";
import { editorArgs, editorBinary, splitTemplate, substitute } from "./src/editor.ts";
import {
	allocateContext,
	buildVisible,
	contextRows,
	fileCounts,
	hiddenGap,
	rowCoord,
	rowsCoveredBy,
} from "./src/model.ts";
import { rowTokens } from "./src/component_render.ts";
import {
	allNotes,
	flushReviewState,
	loadNotes,
	loadReviewState,
	NOTES_DIR,
	resolveInitialPosition,
	saveReviewState,
} from "./src/persistence.ts";
import type { ReviewSession } from "./src/types.ts";
import { defaultContext } from "./src/types.ts";
export { FG_RESET } from "./src/theme.ts";

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

			const prefixRes = await pi.exec("git", ["rev-parse", "--show-prefix"], { cwd: ctx.cwd });
			const repoPrefix = prefixRes.code === 0 ? prefixRes.stdout.trim() : "";
			const base = parseReviewArgs(args, ctx.cwd, repoPrefix);
			base.repo = repoName;
			const context = defaultContext();
			const { files, branch, untrackedSkipped } = await loadReview(git, repoRoot, base, context);
			if (files.length === 0) {
				ctx.ui.notify("no changes to review", "info");
				return;
			}

			{
				try {
					const debugRows: unknown[] = [];
					for (const f of files) {
						if (!f.path.includes("cache.rs")) continue;
						for (const h of f.hunks) {
							for (const r of h.rows) {
								const nl = r.newLine;
								if (nl !== null && nl >= 148 && nl <= 158) {
									debugRows.push({
										kind: r.kind,
										newLine: nl,
										oldLine: r.oldLine,
										text: r.text.slice(0, 40),
										delText: (r as { delText?: string }).delText?.slice(0, 40),
									});
								}
							}
						}
					}
					const dbg = {
						repo: repoName,
						module: fileURLToPath(import.meta.url),
						moduleMtime: existsSync(fileURLToPath(import.meta.url))
							? statSync(fileURLToPath(import.meta.url)).mtime.toISOString()
							: null,
						bin: diffuseBinPath(),
						binMtime: existsSync(diffuseBinPath()) ? statSync(diffuseBinPath()).mtime.toISOString() : null,
						files: files.map((f) => f.path),
						cacheRows: debugRows,
					};
					writeFileSync(join(NOTES_DIR, "debug.json"), JSON.stringify(dbg, null, 1));
				} catch {
					/* diagnostics must never break the review */
				}
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
											helpLines.map((l) =>
												theme2.fg("text", l.length > w - 2 ? l.slice(0, Math.max(0, w - 5)) + "…" : l),
											),
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

// ── Test hook (exposed for test suite) ──────────────────────────────────────

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
	loadReviewState,
	saveReviewState,
	flushReviewState,
	resolveInitialPosition,
	diffuseBinPath,
	runDiffuseBin,
	runDiffuseBinBatch,
	filesFromWire,
	rowTokens,
};
