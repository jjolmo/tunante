<script lang="ts">
	import '../app.css';
	import { onMount } from 'svelte';
	import { getCurrentWindow } from '@tauri-apps/api/window';
	import { invoke } from '@tauri-apps/api/core';
	import { playerStore } from '$lib/stores/player.svelte';
	import { libraryStore } from '$lib/stores/library.svelte';
	import { playlistsStore } from '$lib/stores/playlists.svelte';
	import { settingsStore } from '$lib/stores/settings.svelte';

	const APP_TITLE = 'Tunante';

	let { children } = $props();
	let sessionRestored = $state(false);

	// Debounced setting saver to avoid hammering IPC
	const pendingSaves = new Map<string, string>();
	let saveTimer: ReturnType<typeof setTimeout> | null = null;
	function saveSetting(key: string, value: string) {
		pendingSaves.set(key, value);
		if (saveTimer) return;
		saveTimer = setTimeout(() => {
			saveTimer = null;
			for (const [k, v] of pendingSaves) {
				invoke('set_setting', { key: k, value: v }).catch(() => {});
			}
			pendingSaves.clear();
		}, 500);
	}

	$effect(() => {
		// Settings must load first (batch IPC), then other stores can use the cache
		settingsStore.init().then(() => {
			playerStore.init();
			// Wait for BOTH library and playlists to finish before restoring session
			const libReady = libraryStore.init((key) => settingsStore.getSetting(key));
			const plReady = playlistsStore.init();
			Promise.all([libReady, plReady]).then(() => restoreSession());
		});
	});

	function restoreSession() {
		const gs = (k: string) => settingsStore.getSetting(k);

		// Restore sort
		const sortCol = gs('session_sort_column');
		const sortDir = gs('session_sort_direction');
		if (sortCol) {
			libraryStore.sortConfig = {
				column: sortCol as any,
				direction: (sortDir === 'desc' ? 'desc' : 'asc')
			};
		}

		// Search query is already restored by libraryStore.init() from 'search_query' setting

		// Restore volume
		const vol = gs('session_volume');
		if (vol) playerStore.setVolume(parseFloat(vol));

		// Restore shuffle/repeat
		const shuffle = gs('session_shuffle');
		if (shuffle === 'true' && !playerStore.shuffle) playerStore.toggleShuffle();
		const repeat = gs('session_repeat');
		if (repeat === 'all' || repeat === 'one') {
			while (playerStore.repeat !== repeat) playerStore.cycleRepeat();
		}

		// Restore active view
		const view = gs('session_view');
		const viewId = gs('session_view_id');
		if (view === 'faved') {
			playlistsStore.selectFaved();
		} else if (view === 'playlist' && viewId) {
			playlistsStore.selectPlaylist(viewId);
		} else if (view === 'console' && viewId) {
			// Dynamic import avoids circular dependency that breaks Tailwind CSS plugin
			import('$lib/stores/consoles.svelte').then(({ consolesStore }) => {
				playlistsStore.selectPlaylist(null);
				consolesStore.selectConsole(viewId);
			});
		}

		// Restore last track (show in player bar but don't auto-play)
		const lastTrackId = gs('session_last_track_id');
		if (lastTrackId) {
			const track = libraryStore.tracks.find((t) => t.id === lastTrackId);
			if (track) {
				playerStore.currentTrack = track;
			}
		}

		sessionRestored = true;
	}

	// Persist session state on changes (only after initial restore)
	$effect(() => {
		if (!sessionRestored) return;
		const { column, direction } = libraryStore.sortConfig;
		saveSetting('session_sort_column', column);
		saveSetting('session_sort_direction', direction);
	});

	$effect(() => {
		if (!sessionRestored) return;
		saveSetting('session_volume', String(playerStore.volume));
	});

	$effect(() => {
		if (!sessionRestored) return;
		saveSetting('session_shuffle', String(playerStore.shuffle));
	});

	$effect(() => {
		if (!sessionRestored) return;
		saveSetting('session_repeat', playerStore.repeat);
	});

	$effect(() => {
		if (!sessionRestored) return;
		const track = playerStore.currentTrack;
		if (track) {
			saveSetting('session_last_track_id', track.id);
		}
	});

	$effect(() => {
		if (!sessionRestored) return;
		// Save playlist/faved/all view (console view is saved by Sidebar)
		if (playlistsStore.isFavedView) {
			saveSetting('session_view', 'faved');
			saveSetting('session_view_id', '');
		} else if (playlistsStore.activePlaylistId) {
			saveSetting('session_view', 'playlist');
			saveSetting('session_view_id', playlistsStore.activePlaylistId);
		} else if (!playlistsStore.isFavedView && !playlistsStore.activePlaylistId) {
			// "All tracks" or console (console saves its own view in Sidebar)
			// Don't overwrite console view - only save "all" if no console is active
			// We can't check consolesStore here (circular import), so Sidebar handles console saves
		}
	});

	// Show window after first render — prevents white flash on startup
	onMount(() => {
		getCurrentWindow().show();

		// Flush pending saves before window closes so session state is never lost
		const flushSaves = () => {
			if (saveTimer) {
				clearTimeout(saveTimer);
				saveTimer = null;
			}
			for (const [k, v] of pendingSaves) {
				invoke('set_setting', { key: k, value: v }).catch(() => {});
			}
			pendingSaves.clear();
		};
		window.addEventListener('beforeunload', flushSaves);
		return () => window.removeEventListener('beforeunload', flushSaves);
	});

	$effect(() => {
		const track = playerStore.currentTrack;
		const show = settingsStore.showTrackInTitlebar;
		let title = APP_TITLE;
		if (show && track) {
			const parts = [track.title, track.artist].filter(Boolean);
			if (parts.length > 0) {
				title = `${parts.join(' - ')} — ${APP_TITLE}`;
			}
		}
		// Set document.title synchronously as fallback
		document.title = title;
		// Also set the native window titlebar (async)
		getCurrentWindow().setTitle(title).catch((err) => {
			console.warn('Failed to set window title:', err);
		});
	});
</script>

{@render children()}
