export interface Track {
	id: string;
	path: string;
	title: string;
	artist: string;
	album: string;
	album_artist: string;
	track_number: number | null;
	disc_number: number | null;
	duration_ms: number;
	sample_rate: number | null;
	channels: number | null;
	bitrate: number | null;
	codec: string;
	file_size: number;
	has_artwork: boolean;
}

export interface Playlist {
	id: string;
	name: string;
	track_count: number;
	created_at: number;
	updated_at: number;
}

export interface PlayerState {
	is_playing: boolean;
	current_track: Track | null;
	position_ms: number;
	duration_ms: number;
	volume: number;
	shuffle: boolean;
	repeat: RepeatMode;
}

export type RepeatMode = 'off' | 'all' | 'one';

export type SortColumn = 'title' | 'artist' | 'album' | 'duration_ms' | 'codec' | 'track_number';
export type SortDirection = 'asc' | 'desc';

export interface SortConfig {
	column: SortColumn;
	direction: SortDirection;
}

export interface ScanProgress {
	scanned: number;
	total: number;
	current_path: string;
}

export function formatDuration(ms: number): string {
	const totalSeconds = Math.floor(ms / 1000);
	const minutes = Math.floor(totalSeconds / 60);
	const seconds = totalSeconds % 60;
	return `${minutes}:${seconds.toString().padStart(2, '0')}`;
}

export function formatFileSize(bytes: number): string {
	if (bytes < 1024) return `${bytes} B`;
	if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
	return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

export interface Setting {
	key: string;
	value: string;
}

export interface MonitoredFolder {
	id: string;
	path: string;
	watching_enabled: boolean;
	last_scanned_at: number;
	added_at: number;
}

export type Theme = 'dark' | 'light';
