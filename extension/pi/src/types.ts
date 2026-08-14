/**
 * Types & Configuration for pi-review
 */

export interface ReviewArgs {
	base?: string;
	cached?: boolean;
	paths?: string[];
}

export type RowKind = "ctx" | "add" | "del" | "mod" | "file" | "sep";

export interface Row {
	kind: RowKind;
	text: string;
	oldLine: number | null;
	newLine: number | null;
	delText?: string;
	moved?: boolean;
	wholesale?: boolean;
	spans?: { add?: [number, number][]; del?: [number, number][] };
	runs?: { kind?: "common" | "del" | "add" | "ctx"; color?: string; sgr?: number; text: string }[];
	tail?: { text: string; newLine: number | null }[];
}

export interface Hunk {
	oldStart: number;
	oldCount: number;
	newStart: number;
	newCount: number;
	aboveCtx: number;
	belowCtx: number;
	rows: Row[];
}

export interface DiffFile {
	path: string;
	status: "modified" | "added" | "deleted" | "renamed" | "untracked";
	hunks: Hunk[];
	newContent: string[];
	oldContent: string[];
	newLineCount: number;
	noContent: boolean;
	rows?: Row[];
	contentRuns?: { kind?: "common" | "del" | "add" | "ctx"; color?: string; sgr?: number; text: string }[][];
}

export interface ReviewNote {
	id: string;
	file: string;
	side: "new" | "old";
	start: number;
	end: number;
	text: string;
	createdAt: number;
}

export interface ReviewSession {
	repo: string;
	repoRoot: string;
	files: DiffFile[];
	notes: Record<string, ReviewNote[]>;
	context: number;
	base: {
		baseArgs: string[];
		label: string;
		cached: boolean;
		paths: string[];
	};
	branch: string | null;
	git: (args: string[]) => Promise<{ stdout: string; stderr: string; code: number }>;
}

// ── Backend Wire Protocol Schemas ──────────────────────────────────────────

export interface WireRun {
	kind: "common" | "del" | "add" | "ctx";
	color?: string;
	sgr?: number;
	text: string;
}

export interface WireRow {
	kind: "ctx" | "add" | "del" | "mod";
	text: string;
	oldLine: number | null;
	newLine: number | null;
	delText?: string;
	moved?: boolean;
	wholesale?: boolean;
	runs?: WireRun[];
	tail?: { text: string; newLine: number | null }[];
}

export interface WireHunk {
	oldStart: number;
	oldCount: number;
	newStart: number;
	newCount: number;
	rows: WireRow[];
}

export interface WireCounts {
	adds: number;
	dels: number;
	mods: number;
	moved: number;
}

export interface WireFile {
	path: string;
	status: "modified" | "added" | "deleted" | "renamed" | "untracked";
	counts: WireCounts;
	hunks: WireHunk[];
	newContent?: string[];
	oldContent?: string[];
	contentRuns?: WireRun[][];
	rows?: Row[];
}

export interface WireNote {
	id: string;
	file: string;
	side: "new" | "old";
	start: number;
	end: number;
	text: string;
	createdAt: number;
	runs: WireRun[];
}

export interface WireOutput {
	files: WireFile[];
	notes?: WireNote[];
	rows?: Row[];
}

// ── Config Helpers ─────────────────────────────────────────────────────────

export function defaultContext(): number {
	const v = Number(process.env.REVIEW_CONTEXT);
	return Number.isInteger(v) && v >= 0 ? v : 6;
}

export function parseReviewArgs(args: string): ReviewArgs {
	const parts = args.trim().split(/\s+/).filter(Boolean);
	let cached = false;
	let base: string | undefined;
	const paths: string[] = [];
	let inPaths = false;

	for (const p of parts) {
		if (p === "--") {
			inPaths = true;
			continue;
		}
		if (inPaths) {
			paths.push(p);
			continue;
		}
		if (p === "--cached" || p === "--staged") {
			cached = true;
			continue;
		}
		if (p.startsWith("-")) continue;
		if (!base) {
			base = p;
		} else {
			paths.push(p);
		}
	}

	return { base, cached, paths: paths.length > 0 ? paths : undefined };
}
