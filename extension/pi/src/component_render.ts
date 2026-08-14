/**
 * Rendering & Layout methods for ReviewComponent
 */

import { truncateToWidth, visibleWidth } from "@earendil-works/pi-tui";
import { fileCounts, pad4, prefixOf, rowCoord, truncateMiddle } from "./model.ts";
import { notesFor } from "./persistence.ts";
import { getActiveTheme, sAdd, sDel, sMod, sMov, SGR_OFF } from "./theme.ts";
import type { ReviewNote, ReviewSession, Row } from "./types.ts";

export function safeFg(theme: { fg: (c: string, t: string) => string }, color: string, text: string): string {
	if (!color || !theme || typeof theme.fg !== "function") return text;
	try {
		return theme.fg(color, text) ?? text;
	} catch {
		return text;
	}
}

export function renderNoteRuns(
	runs: { kind?: string; color?: string; text: string; sgr?: number }[],
	fg: (c: string, t: string) => string,
): string {
	const activeTheme = getActiveTheme();
	let out = "";
	for (const r of runs) {
		out += activeTheme.renderToken({ color: r.color ?? "warning", text: r.text, sgr: r.sgr }, "ctx", fg);
	}
	return out;
}

export interface RenderState {
	fileIdx: number;
	rowIdx: number;
	visLine: number;
	scroll: number;
	colOffset: number;
	numBuffer: string;
	lastMouse: string | null;
	syntaxOn: boolean;
	wordDiff: boolean;
	showHelp: boolean;
	noteDraft: string | null;
	anchor: number | null;
	confirmed: { file: number; start: number; end: number; side: "new" | "old" } | null;
	selfRender: boolean;
	forcedRenderPending: boolean;
	flat: Row[];
	rowFile: number[];
	hunkStarts: { flat: number; file: number }[];
}

export function renderComponentFrame(
	state: RenderState,
	session: ReviewSession,
	width: number,
	theme: { fg: (c: string, t: string) => string; bg: (c: string, t: string) => string },
	computeSelection: () => { file: number; start: number; end: number; side: "new" | "old" } | null,
	currentHunkIdx: () => number,
	getRowSegments: (r: Row, width: number, flatIdx: number) => string[],
	visualGeometry: (width: number) => { starts: number[]; counts: number[]; total: number },
	firstRowOfFile: (fileIdx: number) => number,
	lastRowOfFile: (fileIdx: number) => number,
): string[] {
	const lines: string[] = [];
	const f = session.files[state.fileIdx];
	if (!f) return lines;

	const notes = notesFor(session.notes, session.repo, f.path);
	const sel = computeSelection();
	const dim = (s: string) => theme.fg("muted", s);
	const fit = (s: string): string => truncateToWidth(s, Math.max(1, width), "…");
	const stripe = (text: string, bg: string): string => {
		const pad = Math.max(0, width - text.length);
		return theme.bg(bg, text + " ".repeat(pad));
	};

	const { adds, dels, mods, moved } = fileCounts(f);
	const noteS = `${notes.length} note${notes.length === 1 ? "" : "s"}`;
	const counts = `+${adds}${dels > 0 ? ` −${dels}` : ""}${mods > 0 ? ` ~${mods}` : ""}${moved > 0 ? ` ≈${moved} moved` : ""} · ${noteS}`;
	const avail = width - 2;
	let headRaw = `${f.path} (${f.status}) · ${counts}`;
	const full = ` Review: ${session.repo}  ·  ${headRaw}`;
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
	const curHunk = f.hunks[currentHunkIdx()];
	const ctxTxt = curHunk ? ` · ctx ${curHunk.aboveCtx}▴ ${curHunk.belowCtx}▾` : "";
	const colTxt = state.colOffset > 0 ? ` · col ${state.colOffset}+` : "";
	const goTxt = state.numBuffer ? ` · go ${state.numBuffer}` : "";
	const mouseTxt = state.lastMouse ? ` · mouse ${state.lastMouse}` : "";
	const statusRaw = ` ${state.fileIdx + 1}/${session.files.length} files  ·  hunk ${currentHunkIdx() + 1}/${f.hunks.length}  ·  base: ${session.base.label}${ctxTxt}${colTxt}${goTxt}${mouseTxt}${session.branch ? `  ·  branch: ${session.branch}` : ""}`;
	lines.push(fit(dim(statusRaw)));

	if (state.noteDraft !== null) {
		const row = state.flat[state.rowIdx];
		const c = row ? rowCoord(row) : null;
		const start = sel?.start ?? c;
		const end = sel?.end ?? c;
		const loc = `${f.path}:${start ?? "?"}${end && end !== start ? `-${end}` : ""}`;
		const draftLines = state.noteDraft.split("\n");
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

	const total = state.flat.length;
	const maxRows = Math.max(8, (process.stdout.rows ?? 24) - 5);
	const { starts, counts: segCounts, total: visTotal } = visualGeometry(width);
	const cursorVis = (starts[state.rowIdx] ?? 0) + state.visLine;

	if (cursorVis < state.scroll) state.scroll = cursorVis;
	if (cursorVis >= state.scroll + maxRows) state.scroll = cursorVis - maxRows + 1;
	state.scroll = Math.max(0, Math.min(Math.max(0, visTotal - maxRows), state.scroll));

	const bandRows = new Set<number>();
	if (state.anchor !== null) {
		const cFile = state.fileIdx;
		let lo = Math.min(state.anchor, state.rowIdx);
		let hi = Math.max(state.anchor, state.rowIdx);
		if ((state.rowFile[state.anchor] ?? 0) !== cFile) lo = firstRowOfFile(cFile);
		const fs = firstRowOfFile(cFile);
		const fe = lastRowOfFile(cFile);
		for (let i = Math.max(fs, lo); i <= Math.min(fe, hi); i++) bandRows.add(i);
	} else if (state.confirmed && state.confirmed.file === state.fileIdx) {
		for (let i = 0; i < total; i++) {
			if (state.rowFile[i] !== state.confirmed.file) continue;
			const c = rowCoord(state.flat[i]!);
			if (c !== null && c >= state.confirmed.start && c <= state.confirmed.end) bandRows.add(i);
		}
	}

	const noteByRow = new Map<number, ReviewNote[]>();
	session.files.forEach((ff, fi) => {
		for (const n of notesFor(session.notes, session.repo, ff.path)) {
			let at = -1;
			for (let i = 0; i < total; i++) {
				if (state.rowFile[i] !== fi) continue;
				const c = rowCoord(state.flat[i]!);
				if (c !== null && c >= n.start && c <= n.end) at = i;
			}
			if (at === -1) at = firstRowOfFile(fi);
			const arr = noteByRow.get(at) ?? [];
			arr.push(n);
			noteByRow.set(at, arr);
		}
	});

	let vis = 0;
	for (let i = 0; i < total; i++) {
		const r = state.flat[i]!;
		const count = segCounts[i]!;
		if (vis + count <= state.scroll) {
			vis += count;
			continue;
		}
		const segs = getRowSegments(r, width, i);
		const segCount = segs.length;
		const from = Math.max(0, state.scroll - vis);
		const to = Math.min(segCount, state.scroll + maxRows - vis);
		for (let sIdx = from; sIdx < to; sIdx++) {
			let seg = segs[sIdx];
			if (!seg) continue;
			if (i !== state.rowIdx && bandRows.has(i)) seg = stripe(seg, "selectedBg");
			lines.push(fit(seg));
		}
		vis += segCount;

		const ns = noteByRow.get(i);
		if (ns) {
			for (const n of ns) {
				const where = `${n.start}${n.end !== n.start ? `-${n.end}` : ""}`;
				const renderedText = (n as any).runs && (n as any).runs.length > 0
					? renderNoteRuns((n as any).runs, theme.fg)
					: theme.fg("warning", n.text);
				lines.push(fit(`   💬 [${where}] ` + renderedText));
			}
		}
	}

	lines.push(fit(dim(" " + "─".repeat(Math.max(10, Math.min(width - 2, 60))))));
	const selTxt = sel ? ` · sel ${sel.start}-${sel.end}` : "";
	lines.push(fit(dim(` [j/k]move [v]select [m]note [o]edit [s]stage [x]discard [q]quit [?]help${selTxt}`)));
	lines.push("");

	const cap = process.stdout.rows ?? 24;
	if (lines.length > cap) lines.length = cap;
	return lines;
}

export function displayText(r: Row, colOffset: number, wordDiff: boolean): string {
	if ((r.kind !== "mod" && !r.moved) || r.delText === undefined || !wordDiff) {
		return colOffset > 0 ? r.text.slice(colOffset) : r.text;
	}
	const out = (r.runs ?? []).map((run) => run.text).join("");
	return colOffset > 0 ? out.slice(colOffset) : out;
}

export function isPureDeletionMod(r: Row): boolean {
	return (r.spans?.add ?? []).every(([s, e]) => r.text.slice(s, e).trim().length === 0);
}

export function wireTokens(
	runs: { kind?: "common" | "del" | "add" | "ctx"; color?: string; sgr?: number; text: string }[],
	syntaxOn: boolean,
): { kind?: "common" | "del" | "add" | "ctx"; color: string; sgr?: number; text: string }[] {
	return runs.map((x) => ({
		kind: x.kind,
		color: syntaxOn ? x.color ?? "text" : "text",
		sgr: syntaxOn ? x.sgr : undefined,
		text: x.text,
	}));
}

export function classifyPlainText(text: string, path: string): { kind?: "ctx"; color: string; text: string }[] {
	const ext = path.split(".").pop() ?? "";
	const rustKeywords = new Set([
		"pub", "fn", "struct", "enum", "impl", "use", "mod", "let", "mut", "const",
		"return", "if", "else", "match", "for", "while", "loop", "in", "as", "type",
		"where", "trait", "self", "Self", "super", "crate", "async", "await", "unsafe", "bool", "usize", "isize"
	]);
	const tsKeywords = new Set([
		"function", "const", "let", "var", "import", "export", "from", "return",
		"if", "else", "switch", "case", "for", "while", "class", "interface", "type",
		"async", "await", "try", "catch", "throw", "new", "this", "typeof", "boolean", "string", "number"
	]);
	const keywords = ext === "rs" ? rustKeywords : tsKeywords;

	const tokens: { kind?: "ctx"; color: string; text: string }[] = [];
	let i = 0;
	while (i < text.length) {
		const rest = text.slice(i);
		if (rest.startsWith("//") || rest.startsWith("#")) {
			tokens.push({ kind: "ctx", color: "syntaxComment", text: rest });
			break;
		}
		if (text[i] === '"' || text[i] === "'") {
			const quote = text[i];
			let j = i + 1;
			while (j < text.length && text[j] !== quote) {
				if (text[j] === "\\") j++;
				j++;
			}
			if (j < text.length) j++;
			tokens.push({ kind: "ctx", color: "syntaxString", text: text.slice(i, j) });
			i = j;
			continue;
		}
		const m = /^[A-Za-z_][A-Za-z0-9_]*/.exec(rest);
		if (m) {
			const word = m[0];
			if (keywords.has(word)) {
				tokens.push({ kind: "ctx", color: "syntaxKeyword", text: word });
			} else {
				const last = tokens[tokens.length - 1];
				if (last && last.color === "text") {
					last.text += word;
				} else {
					tokens.push({ kind: "ctx", color: "text", text: word });
				}
			}
			i += word.length;
			continue;
		}
		const numMatch = /^[0-9]+/.exec(rest);
		if (numMatch) {
			tokens.push({ kind: "ctx", color: "syntaxNumber", text: numMatch[0] });
			i += numMatch[0].length;
			continue;
		}
		const opMatch = /^[+\-*\/=<>!%&|^~:;,.]+/.exec(rest);
		if (opMatch) {
			tokens.push({ kind: "ctx", color: "syntaxOperator", text: opMatch[0] });
			i += opMatch[0].length;
			continue;
		}
		const last = tokens[tokens.length - 1];
		if (last && last.color === "text") {
			last.text += text[i]!;
		} else {
			tokens.push({ kind: "ctx", color: "text", text: text[i]! });
		}
		i++;
	}
	return tokens;
}

export function rowTokens(
	r: Row,
	flatIdx: number,
	rowFile: number[],
	session: ReviewSession,
	file: DiffFile,
	syntaxOn: boolean,
): { kind?: "common" | "del" | "add" | "ctx"; color: string; sgr?: number; text: string }[] {
	if (r.runs) {
		return wireTokens(r.runs, syntaxOn);
	}
	const currentFile = rowFile[flatIdx] !== undefined ? session.files[rowFile[flatIdx]!]! : file;
	const lineNum = r.newLine ?? r.oldLine;
	if (lineNum !== null && currentFile.contentRuns && currentFile.contentRuns[lineNum - 1]) {
		return wireTokens(currentFile.contentRuns[lineNum - 1]!, syntaxOn);
	}
	if (!syntaxOn) {
		return [{ kind: "ctx", color: "text", text: r.text }];
	}
	return classifyPlainText(r.text, currentFile.path);
}

export function highlightTokens(
	tokens: { color: string; text: string }[],
	from: number,
	to: number,
	theme: { fg: (c: string, t: string) => string },
	bg = `\x1b[48;5;${sAdd.tint}m`,
): string {
	const activeTheme = getActiveTheme();
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
		const styledPiece = activeTheme.renderToken(
			{ ...tok, text: piece },
			"add",
			(c, t) => theme.fg(c, t),
		);
		out += styledPiece + bg;
	}
	return out;
}

export function styleTokens(
	r: Row,
	tokens: { kind?: "common" | "del" | "add" | "ctx"; color: string; text: string }[],
	segStart: number,
	segEnd: number,
	prefix: string,
	selected: boolean,
	colOffset: number,
	wordDiff: boolean,
	theme: { fg: (c: string, t: string) => string },
): string {
	const activeTheme = getActiveTheme();
	const isCtx = r.kind === "ctx" || r.kind === "sep";
	const isMov = r.moved === true;
	const isDelRow = r.kind === "del";
	const isPureDelMod = r.kind === "mod" && isPureDeletionMod(r);
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
		if (kind === "add" && isModRow && !isPureDelMod) return wordBg;
		return lineBg;
	};
	const text = displayText(r, colOffset, wordDiff);
	if (isDelRow) {
		const bg = isCtx ? "" : lineBg;
		const piece = text.slice(segStart, segEnd);
		return (selected ? lineBg : "") + prefix + bg + safeFg(theme, sDel.fg, piece) + lineBg;
	}
	const slice = text.slice(segStart, segEnd);
	if (tokens.length === 0 || tokens.every((t) => t.text.length === 0)) {
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
			out += bg + "\x1b[9m" + safeFg(theme, sDel.fg, piece) + "\x1b[29m";
		} else if (tok.kind === "add") {
			const sgr = (tok as { sgr?: number }).sgr;
			const off = sgr !== undefined ? SGR_OFF[sgr] : undefined;
			const styledPiece = activeTheme.renderToken(
				{ ...tok, text: piece },
				r.kind,
				(c, t) => safeFg(theme, c, t),
			);
			out += bg + (sgr !== undefined ? `\x1b[${sgr}m` : "") + styledPiece + (off !== undefined ? `\x1b[${off}m` : "") + bg;
		} else {
			const sgr = (tok as { sgr?: number }).sgr;
			const off = sgr !== undefined ? SGR_OFF[sgr] : undefined;
			const styledPiece = activeTheme.renderToken(
				{ ...tok, text: piece },
				r.kind,
				(c, t) => safeFg(theme, c, t),
			);
			out += bg + (sgr !== undefined ? `\x1b[${sgr}m` : "") + styledPiece + (off !== undefined ? `\x1b[${off}m` : "") + bg;
		}
	}
	return prefixBg + prefix + out + (selected || !isCtx ? lineBg : "");
}
