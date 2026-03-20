import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { Track, SortConfig, ScanProgress, ColumnDef } from '$lib/types';
import { formatDuration, formatFileSize } from '$lib/types';

const DEFAULT_COLUMNS: ColumnDef[] = [
	{ id: 'title', label: 'Title', field: 'title', flex: 3, minWidth: '150px', align: 'left', sortable: true, visible: true },
	{ id: 'artist', label: 'Artist', field: 'artist', flex: 2, minWidth: '100px', align: 'left', sortable: true, visible: true },
	{ id: 'album', label: 'Album', field: 'album', flex: 2, minWidth: '100px', align: 'left', sortable: true, visible: true },
	{ id: 'album_artist', label: 'Album Artist', field: 'album_artist', flex: 2, minWidth: '100px', align: 'left', sortable: true, visible: false },
	{ id: 'duration', label: 'Duration', field: 'duration_ms', width: '70px', align: 'right', sortable: true, visible: true, format: (t) => formatDuration(t.duration_ms) },
	{ id: 'codec', label: 'Codec', field: 'codec', width: '60px', align: 'center', sortable: true, visible: true },
	{ id: 'track_number', label: 'Track #', field: 'track_number', width: '55px', align: 'right', sortable: true, visible: false },
	{ id: 'disc_number', label: 'Disc #', field: 'disc_number', width: '50px', align: 'right', sortable: true, visible: false },
	{ id: 'sample_rate', label: 'Sample Rate', field: 'sample_rate', width: '85px', align: 'right', sortable: true, visible: false, format: (t) => t.sample_rate ? `${t.sample_rate} Hz` : '' },
	{ id: 'channels', label: 'Channels', field: 'channels', width: '65px', align: 'center', sortable: true, visible: false, format: (t) => t.channels ? String(t.channels) : '' },
	{ id: 'bitrate', label: 'Bitrate', field: 'bitrate', width: '75px', align: 'right', sortable: true, visible: false, format: (t) => t.bitrate ? `${Math.round(t.bitrate / 1000)} kbps` : '' },
	{ id: 'file_size', label: 'Size', field: 'file_size', width: '70px', align: 'right', sortable: true, visible: false, format: (t) => formatFileSize(t.file_size) },
	{ id: 'path', label: 'Path', field: 'path', flex: 3, minWidth: '200px', align: 'left', sortable: true, visible: false },
];

class LibraryStore {
	tracks = $state<Track[]>([]);
	searchQuery = $state('');
	sortConfig = $state<SortConfig>({ column: 'title', direction: 'asc' });
	isScanning = $state(false);
	scanProgress = $state<ScanProgress | null>(null);
	selectedTrackIds = $state<Set<string>>(new Set());
	columns = $state<ColumnDef[]>(DEFAULT_COLUMNS.map((c) => ({ ...c })));
	lastClickedIndex = $state<number | null>(null);

	get visibleColumns(): ColumnDef[] {
		return this.columns.filter((c) => c.visible);
	}

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

		await listen('library-updated', () => {
			this.loadTracks();
		});

		await this.loadColumnConfig();
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

	selectTrack(id: string, multi = false, shiftKey = false, trackIndex?: number) {
		if (shiftKey && this.lastClickedIndex !== null && trackIndex !== undefined) {
			const tracks = this.filteredTracks;
			const start = Math.min(this.lastClickedIndex, trackIndex);
			const end = Math.max(this.lastClickedIndex, trackIndex);
			const next = multi ? new Set(this.selectedTrackIds) : new Set<string>();
			for (let i = start; i <= end; i++) {
				if (tracks[i]) next.add(tracks[i].id);
			}
			this.selectedTrackIds = next;
		} else if (multi) {
			const next = new Set(this.selectedTrackIds);
			if (next.has(id)) {
				next.delete(id);
			} else {
				next.add(id);
			}
			this.selectedTrackIds = next;
			this.lastClickedIndex = trackIndex ?? null;
		} else {
			this.selectedTrackIds = new Set([id]);
			this.lastClickedIndex = trackIndex ?? null;
		}
	}

	selectAll() {
		this.selectedTrackIds = new Set(this.filteredTracks.map((t) => t.id));
	}

	clearSelection() {
		this.selectedTrackIds = new Set();
	}

	toggleColumn(id: string) {
		const col = this.columns.find((c) => c.id === id);
		if (col) {
			col.visible = !col.visible;
			this.columns = [...this.columns];
			this.saveColumnConfig();
		}
	}

	async loadColumnConfig() {
		try {
			const val = await invoke<string | null>('get_setting', { key: 'column_visibility' });
			if (val) {
				const visibility: Record<string, boolean> = JSON.parse(val);
				this.columns = this.columns.map((c) => ({
					...c,
					visible: visibility[c.id] ?? c.visible
				}));
			}
		} catch {
			// Use defaults
		}
	}

	async saveColumnConfig() {
		try {
			const visibility: Record<string, boolean> = {};
			for (const col of this.columns) {
				visibility[col.id] = col.visible;
			}
			await invoke('set_setting', { key: 'column_visibility', value: JSON.stringify(visibility) });
		} catch (e) {
			console.error('Failed to save column config:', e);
		}
	}
}

export const libraryStore = new LibraryStore();
