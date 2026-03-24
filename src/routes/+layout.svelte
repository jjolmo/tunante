<script lang="ts">
	import '../app.css';
	import { onMount } from 'svelte';
	import { getCurrentWindow } from '@tauri-apps/api/window';
	import { invoke } from '@tauri-apps/api/core';
	import { playerStore } from '$lib/stores/player.svelte';
	import { libraryStore } from '$lib/stores/library.svelte';
	import { playlistsStore } from '$lib/stores/playlists.svelte';
	import { settingsStore } from '$lib/stores/settings.svelte';

	import UpdateDialog from '$lib/components/UpdateDialog.svelte';
	import DebugWindow from '$lib/components/DebugWindow.svelte';

	const APP_TITLE = 'Tunante';

	let { children } = $props();
	let sessionRestored = $state(false);
	let showDebugWindow = $state(false);
	let showUpdateDialog = $state(false);
	let silentUpdateReady = $state(false);
	let updateVersion = $state('');

	async function checkStartupUpdate() {
		const skippedVersion = settingsStore.getSetting('skipped_update_version');
		try {
			const info = await invoke<any>('check_for_updates');
			if (info.update_available && info.latest_version !== skippedVersion) {
				updateVersion = info.latest_version;
				showUpdateDialog = true;
			}
		} catch {}
	}

	async function silentAutoUpdate() {
		try {
			const info = await invoke<any>('check_for_updates');
			if (!info.update_available) return;

			// 1. Try Tauri plugin updater (works on macOS/Windows with signed artifacts)
			try {
				const { check } = await import('@tauri-apps/plugin-updater');
				const update = await check();
				if (update) {
					await update.downloadAndInstall();
					const { relaunch } = await import('@tauri-apps/plugin-process');
					setTimeout(() => relaunch(), 1000);
					return; // Success — app will relaunch
				}
			} catch {
				// Plugin failed — fall through to platform-specific fallback
			}

			// 2. Linux AppImage: download and self-replace, then prompt restart
			const result = await invoke<string>('download_and_apply_update', { downloadUrl: info.download_url });
			if (result.includes('applied')) {
				// Linux: AppImage was replaced — show restart toast
				updateVersion = info.latest_version;
				silentUpdateReady = true;
			}
			// macOS/Windows fallback: browser was opened silently, no toast needed
		} catch {}
	}

	function handleSkipVersion(version: string) {
		invoke('set_setting', { key: 'skipped_update_version', value: version }).catch(() => {});
		showUpdateDialog = false;
	}

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
			Promise.all([libReady, plReady]).then(() => {
				restoreSession();
				// Update on startup (skip on macOS — no codesigning yet)
				const isMacOS = navigator.platform.startsWith('Mac');
				if (!isMacOS) {
					if (settingsStore.autoUpdateOnStart) {
						silentAutoUpdate();
					} else if (settingsStore.checkUpdatesOnStart) {
						checkStartupUpdate();
					}
				}
			});
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
		} else if (view === 'files' && viewId) {
			import('$lib/stores/files.svelte').then(({ filesStore }) => {
				playlistsStore.selectPlaylist(null);
				filesStore.selectFolder(viewId);
				filesStore.restoreFromCache((k) => settingsStore.getSetting(k));
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

		// Ctrl+Alt+D: toggle debug window
		function handleDebugShortcut(e: KeyboardEvent) {
			if (e.ctrlKey && e.altKey && e.key.toLowerCase() === 'd') {
				e.preventDefault();
				showDebugWindow = !showDebugWindow;
			}
		}
		window.addEventListener('keydown', handleDebugShortcut);

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
		return () => {
			window.removeEventListener('beforeunload', flushSaves);
			window.removeEventListener('keydown', handleDebugShortcut);
		};
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

{#if showDebugWindow}
	<DebugWindow onclose={() => showDebugWindow = false} />
{/if}

{#if showUpdateDialog}
	<UpdateDialog
		version={updateVersion}
		onupdate={() => { showUpdateDialog = false; }}
		oncancel={() => { showUpdateDialog = false; }}
		onskip={handleSkipVersion}
	/>
{/if}

{#if silentUpdateReady}
	<div class="update-toast">
		<span>v{updateVersion} downloaded.</span>
		<button class="toast-btn" onclick={async () => {
			try {
				const { relaunch } = await import('@tauri-apps/plugin-process');
				await relaunch();
			} catch {
				silentUpdateReady = false;
			}
		}}>Restart</button>
		<button class="toast-dismiss" onclick={() => silentUpdateReady = false}>✕</button>
	</div>
{/if}

<style>
	.update-toast {
		position: fixed;
		bottom: 48px;
		right: 16px;
		z-index: 200;
		display: flex;
		align-items: center;
		gap: 10px;
		padding: 8px 14px;
		background-color: var(--color-bg-secondary, #2a2a2a);
		border: 1px solid var(--color-accent, #4a9eff);
		border-radius: 6px;
		font-size: 13px;
		color: var(--color-text-primary, #eee);
		box-shadow: 0 4px 16px rgba(0,0,0,0.4);
	}

	.toast-btn {
		padding: 4px 12px;
		border: none;
		border-radius: 4px;
		background-color: var(--color-accent, #4a9eff);
		color: white;
		font-size: 12px;
		cursor: pointer;
	}

	.toast-btn:hover { opacity: 0.9; }

	.toast-dismiss {
		background: none;
		border: none;
		color: var(--color-text-muted, #888);
		cursor: pointer;
		font-size: 14px;
		padding: 0 2px;
	}

	.toast-dismiss:hover { color: var(--color-text-primary, #eee); }
</style>
