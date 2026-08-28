import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

export type Confidence = 'exact' | 'high' | 'medium' | 'low';

/** What the backend decided about one game, from `preview_cover_downloads`. */
export interface Plan {
	game: string;
	console_id: string;
	source: string;
	matched_name: string;
	confidence: Confidence;
	url: string | null;
	/** Set when the folder already holds art and would be left alone. */
	existing: string | null;
}

export interface CoverProgress {
	done: number;
	total: number;
	found: number;
	written: number;
	skipped: number;
	current: string;
}

export type Scope = 'library' | 'folder' | 'console' | 'playlist';

/** Only these are applied without someone looking at them first. */
const TRUSTED: Confidence[] = ['exact', 'high'];

export function isTrusted(c: Confidence): boolean {
	return TRUSTED.includes(c);
}

/**
 * The bulk cover download.
 *
 * Deliberately preview-first. This writes files into the user's own music
 * folders, which are very likely inside a sync client, so a wrong cover has to
 * be deleted by hand on every device it reached. The preview costs nothing —
 * it resolves without downloading — and it is the difference between a button
 * people press and a button they don't.
 */
class CoversStore {
	previewing = $state(false);
	running = $state(false);
	progress = $state<CoverProgress | null>(null);
	plans = $state<Plan[]>([]);
	/** The id of the last run, for `undo`. */
	lastRun = $state<number | null>(null);
	undone = $state(false);
	error = $state<string | null>(null);
	/**
	 * A console the user asked about from elsewhere in the app — the sidebar's
	 * context menu. The covers screen picks it up on mount and preselects it.
	 */
	requestedConsole = $state<string | null>(null);
	/**
	 * Bumped when a run finishes, so anything showing artwork re-reads it.
	 *
	 * Without this the cover the run just saved for the playing track stays
	 * invisible until the track changes, which reads as the download not having
	 * worked. Android has the same hazard and clears its caches for the same
	 * reason.
	 */
	refreshToken = $state(0);

	private unlisten: UnlistenFn[] = [];

	get found(): Plan[] {
		return this.plans.filter((p) => p.source !== 'none');
	}
	get trusted(): Plan[] {
		return this.found.filter((p) => isTrusted(p.confidence));
	}
	get needsReview(): Plan[] {
		return this.found.filter((p) => !isTrusted(p.confidence));
	}
	get missing(): Plan[] {
		return this.plans.filter((p) => p.source === 'none');
	}
	/** Folders that already have art and will be left alone. */
	get untouched(): Plan[] {
		return this.plans.filter((p) => p.existing !== null);
	}

	async listen() {
		if (this.unlisten.length > 0) return;
		this.unlisten.push(
			await listen<CoverProgress>('cover-progress', (e) => {
				this.progress = e.payload;
			})
		);
		this.unlisten.push(
			await listen<Plan[]>('cover-complete', (e) => {
				this.plans = e.payload;
				this.running = false;
				this.progress = null;
				this.refreshToken++;
			})
		);
	}

	stopListening() {
		for (const fn of this.unlisten) fn();
		this.unlisten = [];
	}

	async preview(scope: Scope, target = '') {
		this.previewing = true;
		this.error = null;
		this.plans = [];
		this.undone = false;
		try {
			this.plans = await invoke<Plan[]>('preview_cover_downloads', { scope, target });
		} catch (e) {
			this.error = typeof e === 'string' ? e : 'Preview failed';
		} finally {
			this.previewing = false;
		}
	}

	async apply(scope: Scope, target = '', replaceExisting = false) {
		this.running = true;
		this.error = null;
		this.undone = false;
		this.progress = { done: 0, total: this.plans.length, found: 0, written: 0, skipped: 0, current: '' };
		try {
			this.lastRun = await invoke<number>('download_covers', { scope, target, replaceExisting });
		} catch (e) {
			this.error = typeof e === 'string' ? e : 'Download failed';
			this.running = false;
			this.progress = null;
		}
	}

	async cancel() {
		await invoke('cancel_cover_download');
	}

	/** Delete exactly the files the last run created. */
	async undo() {
		if (this.lastRun === null) return 0;
		const n = await invoke<number>('undo_cover_run', { stamp: this.lastRun });
		this.undone = true;
		this.lastRun = null;
		return n;
	}

	reset() {
		this.plans = [];
		this.progress = null;
		this.error = null;
		this.undone = false;
	}
}

export const coversStore = new CoversStore();
