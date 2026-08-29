import { invoke } from '@tauri-apps/api/core';
import { libraryStore } from '$lib/stores/library.svelte';
import { consoleIcon } from '$lib/data/consoleIcons';
import type { Track } from '$lib/types';

/**
 * The console table used to be maintained here as well as in Rust, and the two
 * disagreed about almost everything: whether SNES was called "SNES" or "Super
 * Nintendo", whether GameCube/Wii/3DS were one bucket or three, whether Saturn
 * existed. Worse, `libretro_system_name` on the Rust side keyed off *these
 * display strings*, so renaming a label in this file silently disabled box-art
 * lookups for that console.
 *
 * There is one table now, in `tunante_core::console`. This fetches it.
 */
export interface ConsoleDefinition {
	id: string;
	name: string;
	name_es: string;
	codecs: string[];
	libretro: string | null;
	/** SVG path `d`, from `consoleIcons.ts`. Presentation, not data. */
	icon: string;
}

export interface ClassificationOverride {
	id: string;
	scope: 'track' | 'folder';
	target: string;
	console_id: string | null;
	game_name: string | null;
	created_at: number;
}

export interface UnclassifiedFolder {
	folder: string;
	track_count: number;
	sample_path: string;
}

interface ConsoleDto {
	id: string;
	name: string;
	name_es: string;
	codecs: string[];
	libretro: string | null;
}

class ConsolesStore {
	activeConsoleId = $state<string | null>(null);
	definitions = $state<ConsoleDefinition[]>([]);

	/** Fetch the catalog once at boot. */
	async loadCatalog(): Promise<void> {
		if (this.definitions.length > 0) return;
		const dtos = await invoke<ConsoleDto[]>('get_console_catalog');
		this.definitions = dtos.map((d) => ({ ...d, icon: consoleIcon(d.id) }));
	}

	/**
	 * The console a track belongs to.
	 *
	 * Read straight off the row. It was resolved once, in Rust, against the
	 * registered library roots and the user's corrections, and cached — where
	 * this used to guess from the codec and, failing that, from whether every
	 * chiptune file sharing a grandparent folder agreed. That guess got 71% of a
	 * real library and could not see past a `Disc 1` subfolder or classify an
	 * `.mp3` at all. The stored answer gets 93%.
	 */
	getTrackConsole(track: Track): string | null {
		return track.console_id || null;
	}

	get consolesWithCounts(): (ConsoleDefinition & { trackCount: number })[] {
		const counts = new Map<string, number>();
		for (const track of libraryStore.tracks) {
			if (track.console_id) {
				counts.set(track.console_id, (counts.get(track.console_id) || 0) + 1);
			}
		}
		return this.definitions
			.filter((def) => (counts.get(def.id) || 0) > 0)
			.map((def) => ({ ...def, trackCount: counts.get(def.id) || 0 }));
	}

	get activeConsole(): ConsoleDefinition | null {
		if (!this.activeConsoleId) return null;
		return this.definitions.find((d) => d.id === this.activeConsoleId) || null;
	}

	get consoleTracks(): Track[] {
		const id = this.activeConsoleId;
		if (!id) return [];
		return libraryStore.filteredTracks.filter((t) => t.console_id === id);
	}

	selectConsole(id: string | null) {
		this.activeConsoleId = id;
	}

	// --- corrections ---

	/**
	 * Folders whose tracks nothing could classify — the worklist for flagging.
	 *
	 * These are the franchise and community folders no rule can get right:
	 * `Megaten/` spans five machines, `OCRemixes/` spans everything. Guessing
	 * would be confidently wrong, so they arrive here instead.
	 */
	async unclassifiedFolders(): Promise<UnclassifiedFolder[]> {
		return invoke<UnclassifiedFolder[]>('get_unclassified_folders');
	}

	async overrides(): Promise<ClassificationOverride[]> {
		return invoke<ClassificationOverride[]>('get_classification_overrides');
	}

	/** Flag a folder and everything under it. Pass `null` to leave a half alone. */
	async flagFolder(
		folder: string,
		consoleId: string | null,
		gameName: string | null = null
	): Promise<void> {
		await invoke('set_folder_classification', { folder, consoleId, gameName });
		await libraryStore.loadTracks();
	}

	async flagTrack(
		trackPath: string,
		consoleId: string | null,
		gameName: string | null = null
	): Promise<void> {
		await invoke('set_track_classification', { trackPath, consoleId, gameName });
		await libraryStore.loadTracks();
	}

	async clearFlag(scope: 'track' | 'folder', target: string): Promise<void> {
		await invoke('clear_classification', { scope, target });
		await libraryStore.loadTracks();
	}
}

export const consolesStore = new ConsolesStore();
