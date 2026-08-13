/**
 * External Editor Invocation & Argv templates for pi-review
 */

export interface EditorPattern {
	names: string[];
	open: string[];
	range?: string[];
	detached?: boolean;
}

export const EDITOR_PATTERNS: EditorPattern[] = [
	{
		names: ["vim", "vi", "nvim"],
		open: ["{bin}", "+{line}", "{file}"],
		range: ["{bin}", "-c", "{start}G", "-c", "normal! V{count}j", "{file}"],
	},
	{ names: ["nano", "micro", "pico", "emacs"], open: ["{bin}", "+{line}", "{file}"] },
	{ names: ["helix", "hx", "zed"], open: ["{bin}", "{file}:{line}"] },
	{ names: ["fresh"], open: ["{bin}", "{file}:{line}"] },
	{ names: ["code", "codium", "cursor"], open: ["{bin}", "--goto", "{file}:{line}:1"], detached: true },
	{ names: ["antigravity"], open: ["{bin}", "{file}:{line}"], detached: true },
];

export function splitTemplate(tpl: string): string[] {
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

export function substitute(tpl: string[], vars: Record<string, string>): string[] {
	return tpl.map((a) => a.replace(/\{(\w+)\}/g, (_, k: string) => vars[k] ?? `{${k}}`));
}

export function editorBinary(): string {
	return process.env.REVIEW_EDITOR || process.env.VISUAL || process.env.EDITOR || "vim";
}

export function editorPattern(bin: string): { pattern: EditorPattern; name: string } | null {
	const name = (bin.split(/\s+/)[0] ?? bin).split("/").pop()?.toLowerCase() ?? bin;
	for (const p of EDITOR_PATTERNS) {
		if (p.names.includes(name)) return { pattern: p, name };
	}
	return null;
}

export function editorArgs(
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
