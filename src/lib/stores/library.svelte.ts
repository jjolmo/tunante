import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { Track, SortConfig, ScanProgress } from '$lib/types';

class LibraryStore {
	tracks = $state<Track[]>([]);
	searchQuery = $state('');
	sortConfig = $state<SortConfig>({ column: 'title', direction: 'asc' });
	isScanning = $state(false);
	scanProgress = $state<ScanProgress | null>(null);
	selectedTrackIds = $state<Set<string>>(new Set());

	get filteredTracks(): Track[] {
		let result = this.tracks;

		if (this.searchQuery.trim()) {
			const q = this.searchQuery.toLowerCase();
			result = result.filter(
				(t) =>
					t.title.toLowerCase().includes(q) ||
					t.artist.toLowerCase().includes(q) ||
					t.album.toLowerCase().includes(q)
			);
		}

		const { column, direction } = this.sortConfig;
		const dir = direction === 'asc' ? 1 : -1;
		result = [...result].sort((a, b) => {
			const va = a[column] ?? '';
			const vb = b[column] ?? '';
			if (typeof va === 'number' && typeof vb === 'number') {
				return (va - vb) * dir;
			}
			return String(va).localeCompare(String(vb)) * dir;
		});

		return result;
	}

	async init() {
		await listen<ScanProgress>('scan-progress', (event) => {
			this.scanProgress = event.payload;
		});

		await listen('scan-complete', () => {
			this.isScanning = false;
			this.scanProgress = null;
			this.loadTracks();
		});

		await this.loadTracks();
	}

	async loadTracks() {
		try {
			this.tracks = await invoke<Track[]>('get_all_tracks');
		} catch (e) {
			console.error('Failed to load tracks:', e);
		}
	}

	async scanFolder(path: string) {
		this.isScanning = true;
		try {
			await invoke('scan_folder', { path });
		} catch (e) {
			console.error('Failed to scan folder:', e);
			this.isScanning = false;
		}
	}

	async addFiles(paths: string[]) {
		try {
			await invoke('add_files', { paths });
			await this.loadTracks();
		} catch (e) {
			console.error('Failed to add files:', e);
		}
	}

	setSort(column: SortConfig['column']) {
		if (this.sortConfig.column === column) {
			this.sortConfig = {
				column,
				direction: this.sortConfig.direction === 'asc' ? 'desc' : 'asc'
			};
		} else {
			this.sortConfig = { column, direction: 'asc' };
		}
	}

	selectTrack(id: string, multi = false) {
		if (multi) {
			const next = new Set(this.selectedTrackIds);
			if (next.has(id)) {
				next.delete(id);
			} else {
				next.add(id);
			}
			this.selectedTrackIds = next;
		} else {
			this.selectedTrackIds = new Set([id]);
		}
	}

	clearSelection() {
		this.selectedTrackIds = new Set();
	}
}

export const libraryStore = new LibraryStore();
