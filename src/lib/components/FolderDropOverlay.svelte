<script lang="ts">
	import { onMount, onDestroy } from 'svelte';
	import { invoke } from '@tauri-apps/api/core';
	import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
	import { playlistsStore } from '$lib/stores/playlists.svelte';
	import { libraryStore } from '$lib/stores/library.svelte';
	import { listen } from '@tauri-apps/api/event';

	let isDraggingOver = $state(false);
	let showNameDialog = $state(false);
	let playlistName = $state('');
	let droppedPath = $state('');
	let isCreating = $state(false);
	let nameInput: HTMLInputElement | undefined = $state();

	let unlisten: (() => void) | null = null;
	let unlistenPlaylistCreated: (() => void) | null = null;

	onMount(async () => {
		try {
			const appWindow = getCurrentWebviewWindow();
			unlisten = await appWindow.onDragDropEvent(async (event) => {
				if (event.payload.type === 'enter' || event.payload.type === 'over') {
					// Only show overlay if not already in name dialog
					if (!showNameDialog && !isCreating) {
						isDraggingOver = true;
					}
				} else if (event.payload.type === 'leave') {
					isDraggingOver = false;
				} else if (event.payload.type === 'drop') {
					isDraggingOver = false;
					const paths = event.payload.paths;
					if (paths.length > 0) {
						const firstPath = paths[0];
						const isDir = await invoke<boolean>('is_directory', { path: firstPath });
						if (isDir) {
							// Extract folder name for pre-fill
							const parts = firstPath.replace(/\\/g, '/').split('/');
							const folderName = parts[parts.length - 1] || parts[parts.length - 2] || 'New Playlist';
							playlistName = folderName;
							droppedPath = firstPath;
							showNameDialog = true;
							// Focus the input after dialog renders
							requestAnimationFrame(() => {
								nameInput?.focus();
								nameInput?.select();
							});
						}
					}
				}
			});
		} catch (e) {
			console.warn('Drag-drop events not available:', e);
		}

		// Listen for playlist-created event to reload playlists
		unlistenPlaylistCreated = await listen<{ id: string; track_count: number }>(
			'playlist-created',
			async (event) => {
				isCreating = false;
				playlistsStore.scanningPlaylistId = null;
				await playlistsStore.loadPlaylists();
				await libraryStore.loadTracks();
				// Select the new playlist
				playlistsStore.selectPlaylist(event.payload.id);
			}
		);
	});

	onDestroy(() => {
		unlisten?.();
		unlistenPlaylistCreated?.();
	});

	async function handleConfirm() {
		if (!playlistName.trim() || !droppedPath) return;
		showNameDialog = false;
		isCreating = true;
		try {
			// 1. Create the playlist immediately so it shows in the sidebar
			const playlistId = await invoke<string>('create_playlist', {
				name: playlistName.trim(),
			});
			await playlistsStore.loadPlaylists();

			// 2. Mark it as scanning (shows spinner in sidebar)
			playlistsStore.scanningPlaylistId = playlistId;

			// 3. Scan folder in background — populates the playlist with tracks
			await invoke('create_playlist_from_folder', {
				path: droppedPath,
				playlistId,
			});
		} catch (e) {
			console.error('Failed to create playlist from folder:', e);
			isCreating = false;
			playlistsStore.scanningPlaylistId = null;
		}
	}

	function handleCancel() {
		showNameDialog = false;
		playlistName = '';
		droppedPath = '';
	}

	function handleKeydown(e: KeyboardEvent) {
		if (e.key === 'Enter') handleConfirm();
		else if (e.key === 'Escape') handleCancel();
	}
</script>

{#if isDraggingOver}
	<div class="drop-overlay">
		<div class="drop-content">
			<svg width="48" height="48" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
				<path d="M3 7v10a2 2 0 002 2h14a2 2 0 002-2V9a2 2 0 00-2-2h-6l-2-2H5a2 2 0 00-2 2z" />
				<path d="M12 11v6M9 14h6" />
			</svg>
			<p class="drop-text">Drop folder to create playlist</p>
		</div>
	</div>
{/if}

{#if showNameDialog}
	<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
	<div class="dialog-overlay" role="dialog" onkeydown={handleKeydown}>
		<div class="dialog-box">
			<h3 class="dialog-title">Create Playlist from Folder</h3>
			<p class="dialog-path">{droppedPath}</p>
			<label class="dialog-label">
				Playlist name
				<input
					bind:this={nameInput}
					type="text"
					class="dialog-input"
					bind:value={playlistName}
				/>
			</label>
			<div class="dialog-buttons">
				<button class="btn primary" onclick={handleConfirm} disabled={!playlistName.trim()}>
					Create
				</button>
				<button class="btn" onclick={handleCancel}>Cancel</button>
			</div>
		</div>
	</div>
{/if}

<style>
	.drop-overlay {
		position: fixed;
		inset: 0;
		z-index: 250;
		background-color: rgba(0, 100, 200, 0.15);
		border: 3px dashed var(--color-accent);
		display: flex;
		align-items: center;
		justify-content: center;
		pointer-events: none;
	}

	.drop-content {
		display: flex;
		flex-direction: column;
		align-items: center;
		gap: 12px;
		color: var(--color-accent);
	}

	.drop-text {
		font-size: 18px;
		font-weight: 600;
		margin: 0;
	}

	.dialog-overlay {
		position: fixed;
		inset: 0;
		z-index: 300;
		background-color: rgba(0, 0, 0, 0.6);
		display: flex;
		align-items: center;
		justify-content: center;
	}

	.dialog-box {
		background-color: var(--color-bg-primary);
		border: 1px solid var(--color-border);
		border-radius: 8px;
		padding: 24px;
		width: 450px;
		max-width: 90vw;
		box-shadow: 0 8px 32px rgba(0, 0, 0, 0.5);
	}

	.dialog-title {
		margin: 0 0 8px;
		font-size: 16px;
		font-weight: 600;
		color: var(--color-text-primary);
	}

	.dialog-path {
		margin: 0 0 16px;
		font-size: 11px;
		color: var(--color-text-muted);
		word-break: break-all;
	}

	.dialog-label {
		display: flex;
		flex-direction: column;
		gap: 6px;
		font-size: 13px;
		color: var(--color-text-secondary);
		margin-bottom: 16px;
	}

	.dialog-input {
		padding: 8px 12px;
		border: 1px solid var(--color-border);
		border-radius: 4px;
		background-color: var(--color-bg-secondary);
		color: var(--color-text-primary);
		font-size: 14px;
		outline: none;
	}

	.dialog-input:focus {
		border-color: var(--color-accent);
	}

	.dialog-buttons {
		display: flex;
		gap: 8px;
	}

	.btn {
		padding: 6px 16px;
		border: 1px solid var(--color-border);
		border-radius: 4px;
		background: none;
		color: var(--color-text-primary);
		font-size: 13px;
		cursor: pointer;
	}

	.btn:hover {
		background-color: var(--color-bg-hover);
	}

	.btn:disabled {
		opacity: 0.4;
		cursor: not-allowed;
	}

	.btn.primary {
		background-color: var(--color-accent);
		border-color: var(--color-accent);
		color: white;
	}

	.btn.primary:hover:not(:disabled) {
		opacity: 0.9;
	}
</style>
