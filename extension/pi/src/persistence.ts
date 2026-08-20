/**
 * Notes DB reader for pi-review summary notification
 */

import { readFileSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";

export interface ReviewNote {
	id: string;
	file: string;
	side: "new" | "old";
	start: number;
	end: number;
	text: string;
	created: number;
}

export type NotesDB = Record<string, Record<string, ReviewNote[]>>;

export const NOTES_DIR = join(homedir(), ".pi", "review");
export const NOTES_FILE = join(NOTES_DIR, "notes.json");

export function loadNotes(): NotesDB {
	try {
		return JSON.parse(readFileSync(NOTES_FILE, "utf8")) as NotesDB;
	} catch {
		return {};
	}
}

export function allNotes(db: NotesDB, repo: string): ReviewNote[] {
	return Object.values(db[repo] ?? {}).flat();
}
