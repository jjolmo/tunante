export interface Track {
	id: string;
	path: string;
	title: string;
	artist: string;
	album: string;
	/**
	 * The game named by the file's own header, where the format has such a
	 * field — `game=` in a PSF tag, GD3 for VGM, ID666 for SPC. Empty for
	 * anything that does not, which is every MP3 and everything vgmstream reads.
	 *
	 * Distinct from `album` on purpose: an album is the name of a release, and a
	 * soundtrack release is frequently not named after its game.
	 */
	header_game: string;
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
	rating: number;
	/**
	 * Which machine this came from and which game it belongs to, resolved in
	 * Rust by `tunante_core::classify` and stamped onto the row by the database.
	 * Empty string when unknown — an honest blank, not a guess.
	 */
	console_id: string;
	game: string;
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

export type SortColumn =
	| 'title'
	| 'artist'
	| 'album'
	| 'album_artist'
	| 'duration_ms'
	| 'codec'
	| 'track_number'
	| 'disc_number'
	| 'sample_rate'
	| 'channels'
	| 'bitrate'
	| 'file_size'
	| 'rating'
	| 'game'
	| 'album_game'
	| 'console_id'
	| 'path';
export type SortDirection = 'asc' | 'desc';

export interface ColumnDef {
	id: string;
	label: string;
	field: SortColumn;
	width?: string;
	flex?: number;
	minWidth?: string;
	align?: 'left' | 'right' | 'center';
	sortable: boolean;
	visible: boolean;
	format?: (track: Track) => string;
}

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

export interface PinnedFolder {
	id: string;
	path: string;
	added_at: number;
}

/// Which of the two the combined "Album / Game" column shows first.
export type AlbumGamePreference = 'album' | 'game';

export type Theme = 'dark' | 'light' | 'system';

/// How a cover is fitted into the square it is drawn in.
///
/// Game box art is not square and is not one shape either: a SNES box is
/// almost square, a PS1 jewel case is portrait, a Mega Drive box is wide. One
/// rule cannot flatter all of them, which is why this is a setting.
/// Which face the system tray wears.
///
/// `system` is not "the same as symbolic": it means letting each platform do
/// what it does natively, which on macOS is a template image the OS inverts
/// itself — including while the menu is open, a state no manual swap can
/// reproduce.
export type TrayIconStyle = 'system' | 'symbolic' | 'logo';

export type CoverFit = 'cover' | 'contain' | 'blur' | 'fill' | 'none';
