/**
 * Persistence (Notes DB and state storage) for pi-review
 */

import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";
import { diffuseBinPath, runDiffuseBin } from "./backend.ts";
import type { DiffFile, ReviewNote } from "./types.ts";

export type NotesDB = Record<string, Record<string, ReviewNote[]>>;

export const NOTES_DIR = join(homedir(), ".pi", "review");
export const NOTES_FILE = join(NOTES_DIR, "notes.json");
export const STATE_FILE = join(NOTES_DIR, "state.json");

export interface ReviewState {
	file: string;
	line: number;
	col: number;
	text: string;
}

export function loadNotes(): NotesDB {
	try {
		return JSON.parse(readFileSync(NOTES_FILE, "utf8")) as NotesDB;
	} catch {
		return {};
	}
}

export function saveNotes(db: NotesDB): void {
	try {
		mkdirSync(NOTES_DIR, { recursive: true });
		writeFileSync(NOTES_FILE, JSON.stringify(db, null, 1));
	} catch (err) {
		console.error("review: failed to persist notes:", err);
	}
}

export function notesFor(db: NotesDB, repo: string, file: string): ReviewNote[] {
	return db[repo]?.[file] ?? [];
}

export function allNotes(db: NotesDB, repo: string): ReviewNote[] {
	return Object.values(db[repo] ?? {}).flat();
}

export function addNote(db: NotesDB, repo: string, note: ReviewNote): void {
	(db[repo] ??= {});
	(db[repo][note.file] ??= []).push(note);
	saveNotes(db);
	runDiffuseBin(
		diffuseBinPath(),
		JSON.stringify({ action: "addNote", notesFile: NOTES_FILE, repo, note }),
	).catch(() => {});
}

export function removeNotes(
	db: NotesDB,
	repo: string,
	file: string,
	side: "new" | "old",
	start: number,
	end: number,
): number {
	const list = db[repo]?.[file];
	if (!list) return 0;
	const before = list.length;
	db[repo][file] = list.filter((n) => !(n.side === side && n.start <= end && n.end >= start));
	const removed = before - db[repo][file].length;
	if (removed > 0) {
		saveNotes(db);
		runDiffuseBin(
			diffuseBinPath(),
			JSON.stringify({ action: "removeNotes", notesFile: NOTES_FILE, repo, file, side, start, end }),
		).catch(() => {});
	}
	return removed;
}

export function loadReviewState(repo: string): ReviewState | null {
	try {
		const all = JSON.parse(readFileSync(STATE_FILE, "utf8")) as Record<string, ReviewState>;
		return all[repo] ?? null;
	} catch {
		return null;
	}
}

export function saveReviewState(repo: string, state: ReviewState): void {
	try {
		mkdirSync(NOTES_DIR, { recursive: true });
		let all: Record<string, ReviewState> = {};
		try {
			all = JSON.parse(readFileSync(STATE_FILE, "utf8"));
		} catch {
			all = {};
		}
		all[repo] = state;
		writeFileSync(STATE_FILE, JSON.stringify(all, null, 1));
	} catch (err) {
		console.error("review: failed to persist position:", err);
	}
}

export function resolveInitialPosition(
	files: DiffFile[],
	state: ReviewState | null,
): { fileIdx: number; line: number | null; col: number } | null {
	if (!state) return null;
	const fi = files.findIndex((f) => f.path === state.file);
	if (fi < 0) return null;
	const content = files[fi]!.newContent[state.line - 1];
	if (content !== undefined && content === state.text) {
		return { fileIdx: fi, line: state.line, col: state.col ?? 0 };
	}
	return { fileIdx: fi, line: null, col: 0 };
}
