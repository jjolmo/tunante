import { invoke } from '@tauri-apps/api/core';
import type { Playlist, Track } from '$lib/types';

class PlaylistsStore {
	playlists = $state<Playlist[]>([]);
	activePlaylistId = $state<string | null>(null);
	playlistTracks = $state<Track[]>([]);
	isFavedView = $state(false);
	favedTracks = $state<Track[]>([]);
	scanningPlaylistId = $state<string | null>(null);

	get activePlaylist(): Playlist | null {
		return this.playlists.find((p) => p.id === this.activePlaylistId) ?? null;
	}

	async init() {
		await this.loadPlaylists();
	}

	async loadPlaylists() {
		try {
			this.playlists = await invoke<Playlist[]>('get_playlists');
		} catch (e) {
			console.error('Failed to load playlists:', e);
		}
	}

	async selectPlaylist(id: string | null) {
		this.isFavedView = false;
		this.activePlaylistId = id;
		if (id) {
			try {
				this.playlistTracks = await invoke<Track[]>('get_playlist_tracks', {
					playlistId: id
				});
			} catch (e) {
				console.error('Failed to load playlist tracks:', e);
			}
		} else {
			this.playlistTracks = [];
		}
	}

	async selectFaved() {
		this.activePlaylistId = null;
		this.playlistTracks = [];
		this.isFavedView = true;
		await this.loadFavedTracks();
	}

	async loadFavedTracks() {
		try {
			this.favedTracks = await invoke<Track[]>('get_faved_tracks');
		} catch (e) {
			console.error('Failed to load faved tracks:', e);
		}
	}

	async createPlaylist(name: string) {
		try {
			await invoke('create_playlist', { name });
			await this.loadPlaylists();
		} catch (e) {
			console.error('Failed to create playlist:', e);
		}
	}

	async deletePlaylist(id: string) {
		try {
			await invoke('delete_playlist', { id });
			if (this.activePlaylistId === id) {
				this.activePlaylistId = null;
				this.playlistTracks = [];
			}
			await this.loadPlaylists();
		} catch (e) {
			console.error('Failed to delete playlist:', e);
		}
	}

	async addTracksToPlaylist(playlistId: string, trackIds: string[]) {
		try {
			await invoke('add_tracks_to_playlist', { playlistId, trackIds });
			if (this.activePlaylistId === playlistId) {
				await this.selectPlaylist(playlistId);
			}
			await this.loadPlaylists();
		} catch (e) {
			console.error('Failed to add tracks to playlist:', e);
		}
	}

	async removeTrackFromPlaylist(playlistId: string, trackId: string) {
		try {
			await invoke('remove_track_from_playlist', { playlistId, trackId });
			if (this.activePlaylistId === playlistId) {
				await this.selectPlaylist(playlistId);
			}
			await this.loadPlaylists();
		} catch (e) {
			console.error('Failed to remove track from playlist:', e);
		}
	}

	async removeTracksFromPlaylist(playlistId: string, trackIds: string[]) {
		try {
			for (const trackId of trackIds) {
				await invoke('remove_track_from_playlist', { playlistId, trackId });
			}
			if (this.activePlaylistId === playlistId) {
				await this.selectPlaylist(playlistId);
			}
			await this.loadPlaylists();
		} catch (e) {
			console.error('Failed to remove tracks from playlist:', e);
		}
	}

	selectAllTracks() {
		this.isFavedView = false;
		this.activePlaylistId = null;
		this.playlistTracks = [];
	}

	async renamePlaylist(id: string, name: string) {
		try {
			await invoke('rename_playlist', { id, name });
			await this.loadPlaylists();
		} catch (e) {
			console.error('Failed to rename playlist:', e);
		}
	}
}

export const playlistsStore = new PlaylistsStore();
