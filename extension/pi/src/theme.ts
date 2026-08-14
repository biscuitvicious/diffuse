/**
 * Theme Engine & Color Systems for pi-review
 */

export interface RowStyle {
	icon: string;
	tint: number;
	cursor: number;
	fill?: number;
	fillCursor?: number;
	fg: string;
}

export const ROW_STYLES: Record<"add" | "del" | "mod" | "mov", RowStyle> = {
	add: { icon: "+", tint: 22, cursor: 28, fill: 28, fillCursor: 34, fg: "toolDiffAdded" },
	del: { icon: "-", tint: 52, cursor: 88, fg: "toolDiffRemoved" },
	mod: { icon: "~", tint: 236, cursor: 237, fill: 28, fillCursor: 34, fg: "toolDiffContext" },
	mov: { icon: "≈", tint: 236, cursor: 237, fg: "muted" },
};

export const sAdd = ROW_STYLES.add;
export const sDel = ROW_STYLES.del;
export const sMod = ROW_STYLES.mod;
export const sMov = ROW_STYLES.mov;

export const FG_RESET = "\x1b[39m";

export const MD_SGR = { bold: 1, italic: 3, underline: 4, strike: 9 } as const;
export const SGR_OFF: Record<number, number> = { 1: 22, 3: 23, 4: 24, 9: 29 };

export interface DiffuseTheme {
	name: string;
	renderToken(
		tok: { color: string; text: string; sgr?: number; kind?: string },
		rowKind: string,
		hostThemeFg: (c: string, t: string) => string,
	): string;
}

export const THEME_MAGIC: DiffuseTheme = {
	name: "magic",
	renderToken(tok, rowKind, hostThemeFg) {
		const c = tok.color;
		const isAdd = rowKind === "add" || tok.kind === "add";
		const isDel = rowKind === "del" || tok.kind === "del";

		let ansiColor: string | null = null;

		if (isAdd) {
			switch (c) {
				case "mdBold":
					ansiColor = "\x1b[38;5;172m";
					break;
				case "syntaxType":
				case "syntaxNumber":
					ansiColor = "\x1b[38;5;221m";
					break;
				case "syntaxKeyword":
					ansiColor = "\x1b[38;5;147m";
					break;
				case "syntaxFunction":
					ansiColor = "\x1b[38;5;123m";
					break;
				case "syntaxOperator":
					ansiColor = "\x1b[38;5;117m";
					break;
				case "syntaxComment":
					ansiColor = "\x1b[38;5;150m";
					break;
				case "mdItalic":
				case "muted":
					ansiColor = "\x1b[38;5;229m";
					break;
				case "syntaxString":
					ansiColor = "\x1b[38;5;121m";
					break;
				case "mdLink":
					ansiColor = "\x1b[38;5;117m";
					break;
				case "mdUrl":
				case "mdStrike":
					ansiColor = "\x1b[38;5;250m";
					break;
				case "accent":
				case "mdDelim":
					ansiColor = "\x1b[38;5;51m";
					break;
				default:
					ansiColor = null;
			}
		} else if (isDel) {
			switch (c) {
				case "mdBold":
				case "syntaxType":
				case "syntaxNumber":
					ansiColor = "\x1b[38;5;222m";
					break;
				case "syntaxKeyword":
					ansiColor = "\x1b[38;5;183m";
					break;
				case "syntaxFunction":
					ansiColor = "\x1b[38;5;159m";
					break;
				case "syntaxOperator":
					ansiColor = "\x1b[38;5;153m";
					break;
				case "mdItalic":
				case "syntaxComment":
				case "muted":
					ansiColor = "\x1b[38;5;224m";
					break;
				case "syntaxString":
					ansiColor = "\x1b[38;5;122m";
					break;
				case "mdLink":
					ansiColor = "\x1b[38;5;153m";
					break;
				case "mdUrl":
				case "mdStrike":
					ansiColor = "\x1b[38;5;248m";
					break;
				case "accent":
				case "mdDelim":
					ansiColor = "\x1b[38;5;51m";
					break;
				default:
					ansiColor = null;
			}
		} else {
			switch (c) {
				case "mdBold":
					ansiColor = "\x1b[38;5;172m";
					break;
				case "syntaxType":
					ansiColor = "\x1b[38;5;173m";
					break;
				case "syntaxKeyword":
					ansiColor = "\x1b[38;5;141m";
					break;
				case "syntaxFunction":
					ansiColor = "\x1b[38;5;117m";
					break;
				case "syntaxNumber":
					ansiColor = "\x1b[38;5;179m";
					break;
				case "syntaxOperator":
					ansiColor = "\x1b[38;5;110m";
					break;
				case "mdItalic":
					ansiColor = "\x1b[38;5;187m";
					break;
				case "syntaxString":
					ansiColor = "\x1b[38;5;114m";
					break;
				case "mdLink":
					ansiColor = "\x1b[38;5;75m";
					break;
				case "mdUrl":
				case "mdStrike":
				case "syntaxComment":
				case "muted":
					ansiColor = "\x1b[38;5;243m";
					break;
				case "accent":
				case "mdDelim":
					ansiColor = "\x1b[38;5;51m";
					break;
				default:
					ansiColor = null;
			}
		}

		let host = tok.text;
		if (c) {
			try {
				host = hostThemeFg(c, tok.text) ?? tok.text;
			} catch {
				host = tok.text;
			}
		}
		if (host !== tok.text) {
			return host;
		}
		if (ansiColor) {
			return `${ansiColor}${tok.text}${FG_RESET}`;
		}
		return host;
	},
};

export const THEME_LEGACY: DiffuseTheme = {
	name: "legacy",
	renderToken(tok, rowKind, hostThemeFg) {
		let c = tok.color;
		if (c === "mdBold") c = "syntaxType";
		else if (c === "mdItalic") c = "muted";
		else if (c === "mdLink") c = "text";
		else if (c === "mdUrl") c = "muted";
		else if (c === "mdStrike") c = "muted";
		else if (c === "mdDelim") c = "accent";
		const isAdd = rowKind === "add" || tok.kind === "add";
		if (isAdd && (c === "syntaxComment" || c === "muted")) {
			return `\x1b[38;5;250m${tok.text}${FG_RESET}`;
		}
		if (c === "syntaxType") {
			return `\x1b[38;5;173m${tok.text}${FG_RESET}`;
		}
		if (!c) return tok.text;
		try {
			return hostThemeFg(c, tok.text) ?? tok.text;
		} catch {
			return tok.text;
		}
	},
};

export const THEMES: Record<string, DiffuseTheme> = {
	magic: THEME_MAGIC,
	sorcery: THEME_MAGIC,
	legacy: THEME_LEGACY,
	classic: THEME_LEGACY,
};

export function getActiveTheme(): DiffuseTheme {
	const name = (process.env.REVIEW_THEME || "magic").toLowerCase();
	return THEMES[name] || THEME_MAGIC;
}

export const COLOR: Record<string, number> = {
	text: 231,
	muted: 244,
	accent: 214,
	border: 240,
	warning: 220,
	toolDiffRemoved: 167,
	syntaxKeyword: 141,
	syntaxString: 114,
	syntaxComment: 243,
	syntaxFunction: 117,
	syntaxOperator: 110,
	syntaxNumber: 179,
	syntaxType: 173,
};
