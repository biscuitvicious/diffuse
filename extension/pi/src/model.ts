/**
 * Diff model helper functions & row layout math for pi-review
 */

import { sAdd, sDel, sMod, sMov } from "./theme.ts";
import type { DiffFile, Hunk, ReviewNote, Row } from "./types.ts";

export function addCount(h: Hunk): number {
	return h.rows.filter((r) => r.kind === "add" || r.kind === "mod").length;
}

export function delCount(h: Hunk): number {
	return h.rows.filter((r) => r.kind === "del" || r.kind === "mod").length;
}

export function allocateContext(f: DiffFile): { above: number; below: number }[] {
	const alloc: { above: number; below: number; freeBelow: number }[] = [];
	for (let i = 0; i < f.hunks.length; i++) {
		const h = f.hunks[i]!;
		const newSide = addCount(h) > 0;
		const start = newSide ? h.newStart : h.oldStart;
		const count = newSide ? addCount(h) : delCount(h);
		const total = newSide ? f.newLineCount : f.oldContent.length;
		if (total === 0) {
			alloc.push({ above: 0, below: 0, freeBelow: 0 });
			continue;
		}
		let freeAbove = start - 1;
		if (i > 0) {
			const p = f.hunks[i - 1]!;
			const pNew = addCount(p) > 0;
			if (pNew === newSide) {
				const pEnd = (pNew ? p.newStart : p.oldStart) + (pNew ? addCount(p) : delCount(p)) - 1;
				freeAbove = Math.min(freeAbove, Math.max(0, start - pEnd - 1));
			}
		}
		let freeBelow = total - (start + count) + 1;
		if (i < f.hunks.length - 1) {
			const n = f.hunks[i + 1]!;
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
	for (let i = 0; i < f.hunks.length - 1; i++) {
		const a = f.hunks[i]!;
		const b = f.hunks[i + 1]!;
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

export function contextRows(f: DiffFile, side: "new" | "old", start: number, count: number): Row[] {
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

export function gapBetween(f: DiffFile, i: number): number {
	const a = f.hunks[i]!;
	const b = f.hunks[i + 1]!;
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
	const oldC = f.oldContent.length > 0 ? f.oldContent.length : Number.MAX_SAFE_INTEGER;
	return Math.max(Math.min(newGap, newC), Math.min(oldGap, oldC));
}

export function hiddenGap(f: DiffFile, i: number, alloc: { above: number; below: number }[]): number {
	return Math.max(0, gapBetween(f, i) - alloc[i]!.below - alloc[i + 1]!.above);
}

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
	return { above: ctx, rows };
}

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
	if (released.every((r) => r.text.trim().length === 0)) return ctx;
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

export function buildVisible(f: DiffFile): Row[] {
	if (f.rows && f.rows.length > 0) {
		const isDefault = f.hunks.every((h) => h.aboveCtx === 6 && h.belowCtx === 6);
		if (isDefault) return f.rows;
	}
	const alloc = allocateContext(f);
	const rows: Row[] = [];
	for (let i = 0; i < f.hunks.length; i++) {
		const h = f.hunks[i]!;
		if (i > 0) {
			const hidden = hiddenGap(f, i - 1, alloc);
			if (hidden > 0) {
				const collapsed = alloc[i - 1]!.below === 0 && alloc[i]!.above === 0 ? " (e/E to expand, r to reset)" : "";
				const unavailable = f.noContent ? " (content unavailable — expand in the editor)" : "";
				rows.push({ kind: "sep", text: `⋯ ${hidden} line${hidden === 1 ? "" : "s"} hidden${collapsed}${unavailable}`, newLine: null, oldLine: null });
			}
		}
		const a = alloc[i]!;
		const newSide = addCount(h) > 0;
		const start = newSide ? h.newStart : h.oldStart;
		const count = newSide ? addCount(h) : delCount(h);
		const side = newSide ? "new" : "old";
		const lead = absorbLeadingContext(f, side, start, a.above, h);
		rows.push(...lead.above);

		const src = side === "new" ? f.newContent : f.oldContent;
		const delta = newSide ? h.newStart - h.oldStart : 0;
		const placed = lead.rows.map((r) => {
			const at = newSide
				? (r.newLine !== null
					? r.newLine
					: (r.oldLine !== null ? r.oldLine + delta - 0.5 : 0))
				: (r.oldLine ?? 0);
			return { r, at };
		});
		const rank = (r: { kind: string }) => (r.kind === "del" ? 0 : 1);
		placed.sort((x, y) => x.at - y.at || rank(x.r) - rank(y.r));
		let cursor = start - 1;
		for (const { r, at } of placed) {
			if (at > cursor + 1) {
				for (let g = cursor + 1; g < at; g++) {
					rows.push({
						kind: "ctx",
						text: src[g - 1] ?? "",
						newLine: newSide ? g : null,
						oldLine: newSide ? null : g,
					});
				}
			}
			rows.push(r);
			if (at > cursor) cursor = Math.max(cursor, Math.floor(at));
		}
		const covEnd = start + count - 1;
		for (let g = cursor + 1; g <= covEnd; g++) {
			rows.push({
				kind: "ctx",
				text: src[g - 1] ?? "",
				newLine: newSide ? g : null,
				oldLine: newSide ? null : g,
			});
		}
		rows.push(...absorbClosingContext(f, side, start, count, a.below, h));
	}
	return rows;
}

export function rowCoord(r: Row): number | null {
	return r.newLine ?? r.oldLine;
}

export function truncateMiddle(text: string, max: number): string {
	if (text.length <= max) return text;
	const keep = Math.max(4, Math.floor(max * 0.55));
	const tail = Math.max(3, Math.floor(max * 0.35));
	return text.slice(0, keep) + "…" + text.slice(text.length - tail);
}

export const pad4 = (n: number | null): string => (n === null ? "    " : String(n).padStart(4));

export function prefixOf(r: Row): string {
	if (r.kind === "add") return r.moved ? `${pad4(r.newLine)} ${sMov.icon}` : `${pad4(r.newLine)} ${sAdd.icon}`;
	if (r.kind === "del") return r.moved ? `${pad4(r.oldLine)} ${sMov.icon}` : `${pad4(r.oldLine)} ${sDel.icon}`;
	if (r.kind === "mod") return `${pad4(r.newLine)} ${sMod.icon}`;
	return `${pad4(r.newLine ?? r.oldLine)}  `;
}

export function prefixLen(r: Row): number {
	return prefixOf(r).length;
}

export function rowsCoveredBy(note: ReviewNote, rows: Row[]): number[] {
	const idx: number[] = [];
	rows.forEach((r, i) => {
		const c = rowCoord(r);
		if (c !== null && c >= note.start && c <= note.end) idx.push(i);
	});
	return idx;
}

export function fileCounts(f: DiffFile): { adds: number; dels: number; mods: number; moved: number } {
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
				if ((t as any).moved) moved++;
				else adds++;
			}
		}
	}
	return { adds, dels, mods, moved };
}
