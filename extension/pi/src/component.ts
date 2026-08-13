/**
 * ReviewComponent TUI component class and input handlers
 */

import { Key, matchesKey, visibleWidth } from "@earendil-works/pi-tui";
import type { ComponentActionContext } from "./component_actions.ts";
import {
	commitNoteAction,
	deleteNotesAction,
	discardAction,
	openEditorAction,
	stageAction,
	unstageAction,
} from "./component_actions.ts";
import {
	displayText,
	highlightTokens,
	renderComponentFrame,
	rowTokens,
	styleTokens,
	wireTokens,
} from "./component_render.ts";
import { buildVisible, fileCounts, pad4, prefixOf, rowCoord } from "./model.ts";
import { sAdd, sDel, sMod } from "./theme.ts";
import type { DiffFile, ReviewSession, Row } from "./types.ts";

export interface Selection {
	start: number;
	end: number;
	side: "new" | "old";
}

export class ReviewComponent {
	private flat: Row[] = [];
	private rowFile: number[] = [];
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
	private noteDraft: string | null = null;
	private syntaxOn = true;
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
				let idx = -1;
				for (let i = 0; i < this.flat.length; i++) {
					if (this.rowFile[i] !== fi) continue;
					if (rowCoord(this.flat[i]!) === initial.line) {
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
			const start = this.firstRowOfFile(fi);
			this.rowIdx = Math.min(start + 1, Math.max(0, this.flat.length - 1));
			this.visLine = 0;
		} else {
			this.rowIdx = this.nextCoordRow(0);
			this.visLine = 0;
		}
	}

	notify: ((msg: string, kind: "info" | "warning" | "error") => void) | null = null;
	confirm: ((msg: string) => Promise<boolean>) | null = null;
	helpPopup: (() => void) | null = null;

	private visualCache: { width: number; result: { starts: number[]; counts: number[]; total: number } } | null = null;
	private rowSegmentsCache: Map<number, string[]> = new Map();
	private lastSegWidth: number = -1;
	private lastSyntaxOn: boolean = true;
	private lastWordDiff: boolean = true;
	private lastColOffset: number = 0;

	private clearRowSegmentsCache(): void {
		this.rowSegmentsCache.clear();
	}

	private rebuild(): void {
		this.visualCache = null;
		this.clearRowSegmentsCache();
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
		const next = this.flat.findIndex((r, i) => (this.rowFile[i] ?? 0) > fi);
		return next === -1 ? this.flat.length - 1 : next - 1;
	}

	private nextCoordRow(from: number): number {
		for (let i = Math.max(0, from); i < this.flat.length; i++) {
			if (rowCoord(this.flat[i]!) !== null) return i;
		}
		return from;
	}

	private prevCoordRow(from: number): number {
		for (let i = Math.min(this.flat.length - 1, from); i >= 0; i--) {
			if (rowCoord(this.flat[i]!) !== null) return i;
		}
		return from;
	}

	private computeSelection(): (Selection & { file: number }) | null {
		if (this.anchor !== null) {
			const cFile = this.fileIdx;
			let lo = Math.min(this.anchor, this.rowIdx);
			let hi = Math.max(this.anchor, this.rowIdx);
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
			const side = first.newLine !== null || last.newLine !== null ? "new" : "old";
			const f = first.newLine ?? first.oldLine;
			const l = last.newLine ?? last.oldLine;
			if (f === null || l === null) return null;
			return { file: cFile, start: Math.min(f, l), end: Math.max(f, l), side };
		}
		if (this.confirmed && this.confirmed.file === this.fileIdx) return this.confirmed;
		return null;
	}

	private createActionContext(): ComponentActionContext {
		return {
			session: this.session,
			rowIdx: this.rowIdx,
			flat: this.flat,
			fileIdx: this.fileIdx,
			file: this.file,
			noteDraft: this.noteDraft,
			safetyStashes: this.safetyStashes,
			tui: this.tui,
			notify: this.notify,
			confirm: this.confirm,
			done: this.done,
			computeSelection: () => this.computeSelection(),
			currentHunkIdx: () => this.currentHunkIdx(),
			firstRowOfFile: (fi) => this.firstRowOfFile(fi),
			rebuild: () => this.rebuild(),
			requestRender: () => this.requestRender(),
			setRowIdx: (idx) => (this.rowIdx = idx),
			setVisLine: (line) => (this.visLine = line),
			setNoteDraft: (draft) => (this.noteDraft = draft),
			setConfirmed: (conf) => (this.confirmed = conf),
		};
	}

	handleInput(data: string): void {
		if (this.noteDraft !== null) {
			if (matchesKey(data, Key.ctrl("enter"))) {
				commitNoteAction(this.createActionContext());
			} else if (matchesKey(data, Key.escape)) {
				this.noteDraft = null;
			} else if (matchesKey(data, Key.enter)) {
				this.noteDraft += "\n";
			} else if (matchesKey(data, Key.backspace)) {
				this.noteDraft = this.noteDraft.slice(0, -1);
			} else if (data.length === 1 && data >= " ") {
				this.noteDraft += data;
			}
			this.requestRender();
			return;
		}
		if (!/^[0-9]$/.test(data) && data !== "g" && data !== "G" && this.numBuffer) {
			this.numBuffer = "";
		}
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
				this.dragX = x;
				this.dragY = y;
			} else if ((btn === 32 || btn === 35) && mouse[4] === "M") {
				if (this.dragX === null || this.dragY === null) {
					this.dragX = x;
					this.dragY = y;
				} else {
					this.scrollBy(y - this.dragY);
					this.dragX = x;
					this.dragY = y;
				}
			} else if (btn === 0 && mouse[4] === "m") {
				this.dragX = null;
				this.dragY = null;
			}
			this.lastMouse = `${btn}@${x},${y}${mouse[4]}`;
			this.requestRender();
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
			this.requestRender();
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
			this.requestRender();
			return;
		}
		if (data === "^") {
			this.colOffset = 0;
			this.requestRender();
			return;
		}
		if (data === "$") {
			this.requestRender();
			return;
		}
		if (matchesKey(data, "left") || data === "h" || data === "H") {
			this.requestRender();
			return;
		}
		if (matchesKey(data, "right") || data === "l" || data === "L") {
			this.requestRender();
			return;
		}
		if (/^[jJ]+$/.test(data)) return this.move(data.length);
		if (/^[kK]+$/.test(data)) return this.move(-data.length);
		if (matchesKey(data, "down")) return this.move(1);
		if (matchesKey(data, "up")) return this.move(-1);
		if (matchesKey(data, "pageDown")) return this.move(20);
		if (matchesKey(data, "pageUp")) return this.move(-20);
		if (/^[0-9]$/.test(data)) {
			this.numBuffer += data;
			this.requestRender();
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
		if (matchesKey(data, Key.tab)) return this.switchFile(1);
		if (matchesKey(data, "shift+tab")) return this.switchFile(-1);

		if (data === "v" || data === "V") {
			if (this.anchor === null) {
				this.anchor = this.rowIdx;
				this.confirmed = null;
			} else {
				this.confirmed = this.computeSelection();
				this.anchor = null;
			}
			this.requestRender();
			return;
		}
		if (data === "m" || data === "M") {
			this.noteDraft = "";
			this.lastMouse = null;
			this.requestRender();
			return;
		}
		if (data === "d" || data === "D") return deleteNotesAction(this.createActionContext());
		if (data === "o" || data === "O") return openEditorAction(this.createActionContext());
		if (data === "w" || data === "W") {
			this.wordDiff = !this.wordDiff;
			this.requestRender();
			return;
		}
		if (data === "t" || data === "T") {
			this.syntaxOn = !this.syntaxOn;
			this.requestRender();
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
			this.requestRender();
			return;
		}

		if (data === "s" || data === "S") return void stageAction(this.createActionContext(), data === "S");
		if (data === "u" || data === "U") return void unstageAction(this.createActionContext(), data === "U");
		if (data === "x" || data === "X") return void discardAction(this.createActionContext(), data === "X");
	}

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

	private move(delta: number): void {
		if (this.flat.length === 0) return;
		const { starts } = this.visualGeometry(process.stdout.columns ?? 80);
		this.setCursorVis((starts[this.rowIdx]! + this.visLine) + delta);
		this.requestRender();
	}

	private requestRender(): void {
		this.tui.requestRender(process.env.REVIEW_FULL_REDRAW === "1");
	}

	private scrollBy(delta: number): void {
		if (this.flat.length === 0) return;
		const { starts, total } = this.visualGeometry(process.stdout.columns ?? 80);
		const maxRows = Math.max(8, (process.stdout.rows ?? 24) - 5);
		const maxScroll = Math.max(0, total - maxRows);
		this.setCursorVis(Math.max(0, Math.min(total - 1, starts[this.rowIdx]! + this.visLine + delta)));
		this.scroll = Math.max(0, Math.min(maxScroll, this.scroll + delta));
		this.requestRender();
	}

	private goto(idx: number): void {
		this.rowIdx = Math.max(0, Math.min(this.flat.length - 1, idx));
		this.visLine = 0;
		this.requestRender();
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

	private jumpToLine(n: number): void {
		const fi = this.fileIdx;
		const f = this.file;
		let find = (): number => {
			for (let i = 0; i < this.flat.length; i++) {
				if (this.rowFile[i] !== fi) continue;
				if (rowCoord(this.flat[i]!) === n) return i;
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
		this.requestRender();
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
		this.requestRender();
	}

	private expandHunkToFull(): void {
		const h = this.file.hunks[this.currentHunkIdx()]!;
		h.aboveCtx = Number.MAX_SAFE_INTEGER;
		h.belowCtx = Number.MAX_SAFE_INTEGER;
		this.rebuild();
		this.requestRender();
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
		this.requestRender();
	}

	private quit(): void {
		const stashes = this.safetyStashes.length > 0 ? ` (safety stashes: ${this.safetyStashes.join(" ")})` : "";
		if (stashes) this.notify?.(stashes, "info");
		this.done(0);
	}

	private segCountOf(r: Row, width: number): number {
		if (r.kind === "file" || r.kind === "sep") return 1;
		const prefix = prefixOf(r);
		const cols = Math.max(1, width - prefix.length - 2);
		const wrap = (t: string): number =>
			t.split("\n").reduce((sum, line) => sum + (line.length === 0 ? 1 : Math.max(1, Math.ceil(line.length / cols))), 0);
		const tailSegs = (r.tail ?? []).reduce((sum, t) => sum + wrap(t.text), 0);
		if (r.kind === "mod" && r.wholesale === true) {
			return wrap(r.delText ?? "") + wrap(r.text) + tailSegs;
		}
		const len = displayText(r, this.colOffset, this.wordDiff).length;
		const multi =
			len === 0
				? 1
				: displayText(r, this.colOffset, this.wordDiff)
						.split("\n")
						.reduce((sum, line) => sum + Math.max(1, Math.ceil(line.length / cols)), 0);
		return multi + tailSegs;
	}

	private visualGeometry(width: number): { starts: number[]; counts: number[]; total: number } {
		if (this.visualCache && this.visualCache.width === width) {
			return this.visualCache.result;
		}
		const starts: number[] = new Array(this.flat.length);
		const counts: number[] = new Array(this.flat.length);
		let total = 0;
		for (let i = 0; i < this.flat.length; i++) {
			starts[i] = total;
			const c = this.segCountOf(this.flat[i]!, width);
			counts[i] = c;
			total += c;
		}
		const res = { starts, counts, total };
		this.visualCache = { width, result: res };
		return res;
	}

	private getRowSegments(r: Row, width: number, flatIdx: number): string[] {
		if (
			width !== this.lastSegWidth ||
			this.syntaxOn !== this.lastSyntaxOn ||
			this.wordDiff !== this.lastWordDiff ||
			this.colOffset !== this.lastColOffset
		) {
			this.clearRowSegmentsCache();
			this.lastSegWidth = width;
			this.lastSyntaxOn = this.syntaxOn;
			this.lastWordDiff = this.wordDiff;
			this.lastColOffset = this.colOffset;
		}
		if (flatIdx === this.rowIdx) {
			return this.rowSegments(r, width, flatIdx);
		}
		let cached = this.rowSegmentsCache.get(flatIdx);
		if (!cached) {
			cached = this.rowSegments(r, width, flatIdx);
			this.rowSegmentsCache.set(flatIdx, cached);
		}
		return cached;
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

		const wholesale = r.kind === "mod" && r.wholesale === true;
		const movedDiff = r.moved === true && r.delText !== undefined && r.delText !== r.text;
		const merged = ((r.kind === "mod" && !wholesale) || movedDiff) && this.wordDiff ? r.runs ?? null : null;
		const tokens = merged ?? rowTokens(r, flatIdx, this.rowFile, this.session, this.file, this.syntaxOn);
		const text = displayText(r, this.colOffset, this.wordDiff);
		const emitSeg = (segStart: number, segEnd: number, segPrefix: string, visIdx: number) => {
			out.push(
				styleTokens(
					r,
					tokens,
					segStart,
					segEnd,
					segPrefix,
					this.rowIdx === flatIdx && this.visLine === visIdx,
					this.colOffset,
					this.wordDiff,
					this.theme,
				),
			);
		};
		if (wholesale) {
			const delText = this.colOffset > 0 ? (r.delText ?? "").slice(this.colOffset) : r.delText ?? "";
			const delTokens = wireTokens(
				(r.spans as any)?.delRuns ?? r.runs ?? [{ kind: "del", text: delText }],
				this.syntaxOn,
			);
			const addTokens = wireTokens(
				(r.spans as any)?.addRuns ?? r.runs ?? [{ kind: "add", text: r.text }],
				this.syntaxOn,
			);

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
				let segIdxInBlock = 0;
				for (const line of blockText.split("\n")) {
					if (line.length === 0) {
						const selected = this.rowIdx === flatIdx && this.visLine === segIdx;
						if (isDel) {
							const bg = selected ? `\x1b[48;5;${sDel.cursor}m` : `\x1b[48;5;${sDel.tint}m`;
							out.push(bg + segPrefix(segIdxInBlock) + "\x1b[9m" + this.theme.fg(sDel.fg, "") + "\x1b[29m");
						} else {
							const bg = selected ? `\x1b[48;5;${sMod.cursor}m` : `\x1b[48;5;${sMod.tint}m`;
							out.push(bg + segPrefix(segIdxInBlock) + bg);
						}
						segIdx++;
						segIdxInBlock++;
						continue;
					}
					let start = 0;
					while (start < line.length) {
						const end = Math.min(line.length, start + textCols);
						if (end <= start) break;
						const piece = line.slice(start, end);
						const selected = this.rowIdx === flatIdx && this.visLine === segIdx;
						if (isDel) {
							const bg = selected ? `\x1b[48;5;${sDel.cursor}m` : `\x1b[48;5;${sDel.tint}m`;
							out.push(bg + segPrefix(segIdxInBlock) + "\x1b[9m" + this.theme.fg(sDel.fg, piece) + "\x1b[29m");
						} else {
							const bg = selected ? `\x1b[48;5;${sMod.cursor}m` : `\x1b[48;5;${sMod.tint}m`;
							out.push(bg + segPrefix(segIdxInBlock) + highlightTokens(addTokens, 0, piece.length, this.theme, bg) + bg);
						}
						start = end;
						segIdx++;
						segIdxInBlock++;
					}
				}
			};
			emitBlock(delText, true);
			emitBlock(this.colOffset > 0 ? r.text.slice(this.colOffset) : r.text, false);
			for (const tl of r.tail ?? []) {
				const tlText = this.colOffset > 0 ? tl.text.slice(this.colOffset) : tl.text;
				const tlTokens = tl.runs
					? wireTokens(tl.runs, this.syntaxOn)
					: wireTokens([{ kind: "ctx", text: tlText }], this.syntaxOn);
				const tlPrefix = `${pad4(tl.newLine ?? tl.oldLine)}  `;
				if (tlText.length === 0) {
					emitSeg(0, 0, tlPrefix, -1);
					continue;
				}
				let start = 0;
				while (start < tlText.length) {
					const end = Math.min(tlText.length, start + textCols);
					if (end <= start) break;
					out.push(tlPrefix + highlightTokens(tlTokens, start, end, this.theme, ""));
					start = end;
				}
			}
			return out;
		}
		if (text.length === 0) {
			emitSeg(0, 0, prefix, 0);
		} else {
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
					if (le <= ls) break;
					emitSeg(start + ls, start + le, start + ls === 0 ? prefix : " ".repeat(prefix.length), segIdx++);
					ls = le;
				}
				start += line.length + 1;
			}
		}
		for (const tl of r.tail ?? []) {
			const tlText = this.colOffset > 0 ? tl.text.slice(this.colOffset) : tl.text;
			const tlTokens = tl.runs
				? wireTokens(tl.runs, this.syntaxOn)
				: wireTokens([{ kind: "ctx", text: tlText }], this.syntaxOn);
			const tlPrefix = `${pad4(tl.newLine ?? tl.oldLine)}  `;
			if (tlText.length === 0) {
				out.push(tlPrefix);
				continue;
			}
			let start = 0;
			while (start < tlText.length) {
				const end = Math.min(tlText.length, start + textCols);
				if (end <= start) break;
				out.push(tlPrefix + highlightTokens(tlTokens, start, end, this.theme, ""));
				start = end;
			}
		}

		return out;
	}

	displayText(r: Row): string {
		return displayText(r, this.colOffset, this.wordDiff);
	}

	rowTokens(r: Row, flatIdx: number): { kind?: "common" | "del" | "add" | "ctx"; color: string; sgr?: number; text: string }[] {
		return rowTokens(r, flatIdx, this.rowFile, this.session, this.file, this.syntaxOn);
	}

	isPureDeletionMod(r: Row): boolean {
		return (r.spans?.add ?? []).every(([s, e]) => r.text.slice(s, e).trim().length === 0);
	}

	styleTokens(
		r: Row,
		tokens: { kind?: "common" | "del" | "add" | "ctx"; color: string; text: string }[],
		segStart: number,
		segEnd: number,
		prefix: string,
		selected: boolean,
	): string {
		return styleTokens(r, tokens, segStart, segEnd, prefix, selected, this.colOffset, this.wordDiff, this.theme);
	}

	render(width: number): string[] {
		const state = {
			fileIdx: this.fileIdx,
			rowIdx: this.rowIdx,
			visLine: this.visLine,
			scroll: this.scroll,
			colOffset: this.colOffset,
			numBuffer: this.numBuffer,
			lastMouse: this.lastMouse,
			syntaxOn: this.syntaxOn,
			wordDiff: this.wordDiff,
			showHelp: this.showHelp,
			noteDraft: this.noteDraft,
			anchor: this.anchor,
			confirmed: this.confirmed,
			selfRender: false,
			forcedRenderPending: false,
			flat: this.flat,
			rowFile: this.rowFile,
			hunkStarts: this.hunkStarts,
		};
		return renderComponentFrame(
			state,
			this.session,
			width,
			this.theme,
			() => this.computeSelection(),
			() => this.currentHunkIdx(),
			(r, w, i) => this.getRowSegments(r, w, i),
			(w) => this.visualGeometry(w),
			(fi) => this.firstRowOfFile(fi),
			(fi) => this.lastRowOfFile(fi),
		);
	}
}
