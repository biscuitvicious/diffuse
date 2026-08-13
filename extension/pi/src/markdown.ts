/**
 * Markdown formatting utilities for pi-review notes
 */

import { getActiveTheme, MD_SGR, SGR_OFF } from "./theme.ts";

function mdInnerTokens(
	inner: string,
	parentSgr: number,
	parentColor: string,
): { color: string; text: string; sgr?: number }[] {
	const inner_re = /(`[^`\n]+`)/g;
	const out: { color: string; text: string; sgr?: number }[] = [];
	let last = 0;
	let m: RegExpExecArray | null;
	while ((m = inner_re.exec(inner))) {
		if (m.index > last) out.push({ color: parentColor, sgr: parentSgr, text: inner.slice(last, m.index) });
		const tok = m[0]!;
		out.push({ color: "accent", text: "`" });
		out.push({ color: "syntaxString", sgr: parentSgr, text: tok.slice(1, -1) });
		out.push({ color: "accent", text: "`" });
		last = m.index + tok.length;
	}
	if (last < inner.length) out.push({ color: parentColor, sgr: parentSgr, text: inner.slice(last) });
	return out;
}

export function markdownTokens(text: string): { color: string; text: string; sgr?: number }[] {
	const heading = /^(#{1,6})(\s+)(.*)$/.exec(text);
	if (heading) {
		return [
			{ color: "accent", text: heading[1]! },
			{ color: "text", text: heading[2]! },
			{ color: "accent", sgr: MD_SGR.bold, text: heading[3]! },
		];
	}
	const out: { color: string; text: string; sgr?: number }[] = [];
	let last = 0;
	const re = /(`[^`\n]+`|~~[^~\n]+~~|\*\*[^*\n]+\*\*|\*[^*\s\n][^*\n]*\*|\[[^\]\n]+\]\([^)\n]+\))/g;
	let m: RegExpExecArray | null;
	while ((m = re.exec(text))) {
		if (m.index > last) out.push({ color: "text", text: text.slice(last, m.index) });
		const tok = m[0]!;
		if (tok.startsWith("`")) {
			out.push({ color: "accent", text: "`" });
			out.push({ color: "syntaxString", text: tok.slice(1, -1) });
			out.push({ color: "accent", text: "`" });
		} else if (tok.startsWith("~~")) {
			out.push({ color: "accent", text: "~~" });
			out.push(...mdInnerTokens(tok.slice(2, -2), MD_SGR.strike, "mdStrike"));
			out.push({ color: "accent", text: "~~" });
		} else if (tok.startsWith("**")) {
			out.push({ color: "accent", text: "**" });
			out.push(...mdInnerTokens(tok.slice(2, -2), MD_SGR.bold, "mdBold"));
			out.push({ color: "accent", text: "**" });
		} else if (tok.startsWith("[")) {
			const close = tok.indexOf("](");
			out.push({ color: "accent", text: "[" });
			out.push(...mdInnerTokens(tok.slice(1, close), MD_SGR.underline, "mdLink"));
			out.push({ color: "accent", text: "](" });
			out.push({ color: "mdUrl", sgr: MD_SGR.underline, text: tok.slice(close + 2, -1) });
			out.push({ color: "accent", text: ")" });
		} else {
			out.push({ color: "accent", text: "*" });
			out.push(...mdInnerTokens(tok.slice(1, -1), MD_SGR.italic, "mdItalic"));
			out.push({ color: "accent", text: "*" });
		}

		last = m.index + tok.length;
	}
	if (last < text.length) out.push({ color: "text", text: text.slice(last) });
	return out;
}

export function renderMarkdown(text: string, fg: (c: string, t: string) => string): string {
	const fence = /^\s*```([A-Za-z0-9_+-]*)\s*$/.exec(text);
	if (fence) {
		const rest = text.slice(fence[0].length);
		const body = rest.startsWith("\n") ? rest.slice(1) : rest;
		const end = body.lastIndexOf("```");
		const code = end >= 0 ? body.slice(0, end) : body;
		const codeOut = fg("text", code);
		return fg("muted", fence[0]) + "\n" + codeOut + (end >= 0 ? "\n" + fg("muted", "```") : "");
	}
	const re = /(`[^`\n]+`|~~[^~\n]+~~|\*\*[^*\n]+\*\*|\*[^*\s\n][^*\n]*\*|\[[^\]\n]+\]\([^)\n]+\))/g;
	const sgr = (on: number, t: string): string => `\x1b[${on}m${t}\x1b[${SGR_OFF[on]!}m`;
	const activeTheme = getActiveTheme();
	let out = "";
	let last = 0;
	for (const m of text.matchAll(re)) {
		const idx = m.index ?? 0;
		if (idx > last) out += fg("warning", text.slice(last, idx));
		const tok = m[0]!;
		if (tok.startsWith("`"))
			out +=
				activeTheme.renderToken({ color: "accent", text: "`" }, "ctx", fg) +
				activeTheme.renderToken({ color: "syntaxString", text: tok.slice(1, -1) }, "ctx", fg) +
				activeTheme.renderToken({ color: "accent", text: "`" }, "ctx", fg);
		else if (tok.startsWith("~~"))
			out +=
				activeTheme.renderToken({ color: "accent", text: "~~" }, "ctx", fg) +
				sgr(MD_SGR.strike, activeTheme.renderToken({ color: "mdStrike", text: tok.slice(2, -2) }, "ctx", fg)) +
				activeTheme.renderToken({ color: "accent", text: "~~" }, "ctx", fg);
		else if (tok.startsWith("**"))
			out +=
				activeTheme.renderToken({ color: "accent", text: "**" }, "ctx", fg) +
				sgr(MD_SGR.bold, activeTheme.renderToken({ color: "mdBold", text: tok.slice(2, -2) }, "ctx", fg)) +
				activeTheme.renderToken({ color: "accent", text: "**" }, "ctx", fg);
		else if (tok.startsWith("[")) {
			const close = tok.indexOf("](");
			out +=
				activeTheme.renderToken({ color: "accent", text: "[" }, "ctx", fg) +
				sgr(MD_SGR.underline, activeTheme.renderToken({ color: "mdLink", text: tok.slice(1, close) }, "ctx", fg)) +
				activeTheme.renderToken({ color: "accent", text: "](" }, "ctx", fg) +
				sgr(MD_SGR.underline, activeTheme.renderToken({ color: "mdUrl", text: tok.slice(close + 2, -1) }, "ctx", fg)) +
				activeTheme.renderToken({ color: "accent", text: ")" }, "ctx", fg);
		} else
			out +=
				activeTheme.renderToken({ color: "accent", text: "*" }, "ctx", fg) +
				sgr(MD_SGR.italic, activeTheme.renderToken({ color: "mdItalic", text: tok.slice(1, -1) }, "ctx", fg)) +
				activeTheme.renderToken({ color: "accent", text: "*" }, "ctx", fg);
		last = idx + tok.length;
	}
	if (last < text.length) out += fg("warning", text.slice(last));
	return out;
}
