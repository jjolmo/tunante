import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { Track, SortConfig, ScanProgress, ColumnDef } from '$lib/types';
import { formatDuration, formatFileSize } from '$lib/types';

const DEFAULT_COLUMNS: ColumnDef[] = [
	{ id: 'title', label: 'Title', field: 'title', flex: 3, minWidth: '150px', align: 'left', sortable: true, visible: true },
	{ id: 'rating', label: '\u2605', field: 'rating', width: '36px', align: 'center', sortable: true, visible: true, format: (t) => t.rating > 0 ? '\u2605' : '' },
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
	/** Debounced query used for actual filtering — updates 150ms after typing stops */
	private _activeQuery = $state('');
	sortConfig = $state<SortConfig>({ column: 'title', direction: 'asc' });
	isScanning = $state(false);
	scanProgress = $state<ScanProgress | null>(null);
	selectedTrackIds = $state<Set<string>>(new Set());
	columns = $state<ColumnDef[]>(DEFAULT_COLUMNS.map((c) => ({ ...c })));
	lastClickedIndex = $state<number | null>(null);
	scrollToTrackId = $state<string | null>(null);

	private _searchSaveTimeout: ReturnType<typeof setTimeout> | null = null;
	private _searchFilterTimeout: ReturnType<typeof setTimeout> | null = null;

	/** The debounced search query actually used for filtering */
	get activeSearchQuery(): string {
		return this._activeQuery;
	}

	get visibleColumns(): ColumnDef[] {
		return this.columns.filter((c) => c.visible);
	}

	get favedCount(): number {
		return this.tracks.filter((t) => t.rating > 0).length;
	}

	get filteredTracks(): Track[] {
		let result = this.tracks;

		if (this._activeQuery.trim()) {
			const q = this._activeQuery.toLowerCase();
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
			let cmp: number;
			if (typeof va === 'number' && typeof vb === 'number') {
				cmp = (va - vb) * dir;
			} else {
				cmp = String(va).localeCompare(String(vb)) * dir;
			}
			// Secondary sort: when primary values are equal, sort by path
			// so tracks within the same album/artist stay in filesystem order
			if (cmp === 0 && column !== 'path') {
				return (a.path ?? '').localeCompare(b.path ?? '');
			}
			return cmp;
		});

		return result;
	}

	async init(getSetting?: (key: string) => string | null) {
		// Register all event listeners in parallel (no dependencies between them)
		await Promise.all([
			listen<ScanProgress>('scan-progress', (event) => {
				this.scanProgress = event.payload;
			}),
			listen('scan-complete', () => {
				this.isScanning = false;
				this.scanProgress = null;
				this.loadTracks();
			}),
			listen('library-updated', () => {
				this.loadTracks();
			}),
		]);

		// Restore settings from cache (no IPC needed — settingsStore already loaded all)
		if (getSetting) {
			const savedQuery = getSetting('search_query');
			if (savedQuery) {
				this.searchQuery = savedQuery;
				this._activeQuery = savedQuery;
			}
			this.loadColumnConfigFromCache(getSetting);
		}

		await this.loadTracks();
	}

	setSearchQuery(query: string) {
		this.searchQuery = query; // Immediate — keeps input responsive

		// Debounce the actual filtering (expensive with 16k+ tracks)
		// Clear is instant (no reason to delay showing all tracks)
		if (this._searchFilterTimeout) clearTimeout(this._searchFilterTimeout);
		if (!query.trim()) {
			this._activeQuery = '';
		} else {
			this._searchFilterTimeout = setTimeout(() => {
				this._activeQuery = query;
			}, 150);
		}

		// Debounce the DB save
		if (this._searchSaveTimeout) clearTimeout(this._searchSaveTimeout);
		this._searchSaveTimeout = setTimeout(async () => {
			try {
				await invoke('set_setting', { key: 'search_query', value: query });
			} catch {
				// ignore
			}
		}, 500);
	}

	requestScrollTo(trackId: string) {
		this.scrollToTrackId = trackId;
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

	selectTrack(id: string, multi = false, shiftKey = false, trackIndex?: number, contextTracks?: Track[]) {
		if (shiftKey && this.lastClickedIndex !== null && trackIndex !== undefined) {
			const tracks = contextTracks ?? this.filteredTracks;
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

	selectAll(contextTracks?: Track[]) {
		const tracks = contextTracks ?? this.filteredTracks;
		this.selectedTrackIds = new Set(tracks.map((t) => t.id));
	}

	clearSelection() {
		this.selectedTrackIds = new Set();
	}

	/** Get the first selected track object */
	get selectedTrack(): Track | null {
		if (this.selectedTrackIds.size === 0) return null;
		const firstId = this.selectedTrackIds.values().next().value;
		return this.tracks.find((t) => t.id === firstId) ?? null;
	}

	/** Update a track's rating locally (already saved to DB by caller) */
	updateTrackRating(trackId: string, rating: number) {
		const track = this.tracks.find((t) => t.id === trackId);
		if (track) {
			track.rating = rating;
			this.tracks = [...this.tracks];
		}
	}

	toggleColumn(id: string) {
		const col = this.columns.find((c) => c.id === id);
		if (col) {
			col.visible = !col.visible;
			this.columns = [...this.columns];
			this.saveColumnConfig();
		}
	}

	setColumnWidth(id: string, width: string) {
		const col = this.columns.find((c) => c.id === id);
		if (col) {
			col.width = width;
			col.flex = undefined;
			col.minWidth = undefined;
			this.columns = [...this.columns];
			// Don't save on every pixel — debounce via the mouseup in TrackList
		}
	}

	moveColumn(fromId: string, toId: string) {
		const cols = [...this.columns];
		const fromIdx = cols.findIndex((c) => c.id === fromId);
		const toIdx = cols.findIndex((c) => c.id === toId);
		if (fromIdx === -1 || toIdx === -1) return;
		const [moved] = cols.splice(fromIdx, 1);
		cols.splice(toIdx, 0, moved);
		this.columns = cols;
		this.saveColumnConfig();
	}

	/** Load column config synchronously from settings cache */
	private loadColumnConfigFromCache(getSetting: (key: string) => string | null) {
		try {
			const val = getSetting('column_config');
			if (val) {
				const config: Record<string, { visible: boolean; width?: string; flex?: number; order?: number }> = JSON.parse(val);
				this.columns = this.columns.map((c) => {
					const cfg = config[c.id];
					if (!cfg) return c;
					return {
						...c,
						visible: cfg.visible ?? c.visible,
						width: cfg.width ?? c.width,
						flex: cfg.width ? undefined : (cfg.flex ?? c.flex),
						minWidth: cfg.width ? undefined : c.minWidth,
					};
				});
				const hasOrder = Object.values(config).some((c) => c.order !== undefined);
				if (hasOrder) {
					this.columns.sort((a, b) => {
						const oa = config[a.id]?.order ?? 999;
						const ob = config[b.id]?.order ?? 999;
						return oa - ob;
					});
				}
			} else {
				// Try legacy visibility-only config
				const legacyVal = getSetting('column_visibility');
				if (legacyVal) {
					const visibility: Record<string, boolean> = JSON.parse(legacyVal);
					this.columns = this.columns.map((c) => ({
						...c,
						visible: visibility[c.id] ?? c.visible
					}));
				}
			}
		} catch {
			// Use defaults
		}
	}

	async saveColumnConfig() {
		try {
			const config: Record<string, { visible: boolean; width?: string; flex?: number; order: number }> = {};
			for (let i = 0; i < this.columns.length; i++) {
				const col = this.columns[i];
				config[col.id] = {
					visible: col.visible,
					width: col.width,
					flex: col.flex,
					order: i,
				};
			}
			await invoke('set_setting', { key: 'column_config', value: JSON.stringify(config) });
		} catch (e) {
			console.error('Failed to save column config:', e);
		}
	}
}

export const libraryStore = new LibraryStore();
