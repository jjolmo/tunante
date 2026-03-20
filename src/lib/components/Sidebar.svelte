<script lang="ts">
	import { open } from '@tauri-apps/plugin-dialog';
	import { libraryStore } from '$lib/stores/library.svelte';
	import { playlistsStore } from '$lib/stores/playlists.svelte';
	import { settingsStore } from '$lib/stores/settings.svelte';

	let newPlaylistName = $state('');
	let showNewPlaylistInput = $state(false);
	let dragOverPlaylistId = $state<string | null>(null);

	async function handleAddFolder() {
		const selected = await open({ directory: true, multiple: false });
		if (selected) {
			await libraryStore.scanFolder(selected as string);
		}
	}

	async function handleAddFiles() {
		const selected = await open({
			multiple: true,
			filters: [
				{
					name: 'Audio',
					extensions: [
						'mp3',
						'flac',
						'ogg',
						'wav',
						'aac',
						'aiff',
						'wma',
						'm4a',
						'opus',
						'nsf',
						'nsfe',
						'spc',
						'gbs',
						'vgm',
						'vgz',
						'hes',
						'kss',
						'ay',
						'sap',
						'gym'
					]
				}
			]
		});
		if (selected) {
			await libraryStore.addFiles(selected as string[]);
		}
	}

	function handleSelectAllTracks() {
		playlistsStore.selectPlaylist(null);
	}

	function handleSelectPlaylist(id: string) {
		playlistsStore.selectPlaylist(id);
	}

	async function handleCreatePlaylist() {
		if (newPlaylistName.trim()) {
			await playlistsStore.createPlaylist(newPlaylistName.trim());
			newPlaylistName = '';
			showNewPlaylistInput = false;
		}
	}

	function handleKeydown(e: KeyboardEvent) {
		if (e.key === 'Enter') handleCreatePlaylist();
		if (e.key === 'Escape') {
			showNewPlaylistInput = false;
			newPlaylistName = '';
		}
	}

	function handlePlaylistDragOver(e: DragEvent, playlistId: string) {
		if (e.dataTransfer?.types.includes('application/x-tunante-tracks')) {
			e.preventDefault();
			e.dataTransfer.dropEffect = 'copy';
			dragOverPlaylistId = playlistId;
		}
	}

	function handlePlaylistDragLeave() {
		dragOverPlaylistId = null;
	}

	async function handlePlaylistDrop(e: DragEvent, playlistId: string) {
		e.preventDefault();
		dragOverPlaylistId = null;
		const data = e.dataTransfer?.getData('application/x-tunante-tracks');
		if (data) {
			const trackIds: string[] = JSON.parse(data);
			await playlistsStore.addTracksToPlaylist(playlistId, trackIds);
		}
	}
</script>

<aside class="sidebar">
	<div class="sidebar-header">
		<span class="sidebar-title">Library</span>
		<div class="sidebar-actions">
			<button class="icon-btn" onclick={handleAddFolder} title="Add folder">
				<svg width="16" height="16" viewBox="0 0 16 16" fill="currentColor">
					<path
						d="M14.5 3H7.71l-.85-.85L6.51 2H1.5l-.5.5v11l.5.5h13l.5-.5v-10L14.5 3zm-.51 8.49V13H2V7h5.29l.85.85.36.15H14v3.49zM2 3h4.29l.85.85.36.15H14v2H8.5l-.85-.85L7.29 5H2V3z"
					/>
				</svg>
			</button>
			<button class="icon-btn" onclick={handleAddFiles} title="Add files">
				<svg width="16" height="16" viewBox="0 0 16 16" fill="currentColor">
					<path
						d="M8 1v6.5H1.5V9H8v6.5h1.5V9H16V7.5H9.5V1H8z"
						transform="translate(-0.5, -0.5) scale(0.85)"
					/>
				</svg>
			</button>
		</div>
	</div>

	<div class="sidebar-content">
		<button
			class="sidebar-item"
			class:active={playlistsStore.activePlaylistId === null}
			onclick={handleSelectAllTracks}
		>
			<svg width="14" height="14" viewBox="0 0 16 16" fill="currentColor">
				<path
					d="M13.5 1h-12l-.5.5v12l.5.5h12l.5-.5v-12l-.5-.5zM13 13H2V2h11v11z M4 5h7v1H4V5zm0 3h7v1H4V8zm0 3h5v1H4v-1z"
				/>
			</svg>
			<span>All Tracks</span>
			<span class="track-count">{libraryStore.tracks.length}</span>
		</button>

		<div class="sidebar-section">
			<div class="section-header">
				<span>Playlists</span>
				<button
					class="icon-btn small"
					onclick={() => (showNewPlaylistInput = true)}
					title="New playlist"
				>
					<svg width="12" height="12" viewBox="0 0 16 16" fill="currentColor">
						<path d="M14 7v1H8v6H7V8H1V7h6V1h1v6h6z" />
					</svg>
				</button>
			</div>

			{#if showNewPlaylistInput}
				<div class="new-playlist-input">
					<input
						type="text"
						bind:value={newPlaylistName}
						placeholder="Playlist name..."
						onkeydown={handleKeydown}
					/>
				</div>
			{/if}

			{#each playlistsStore.playlists as playlist}
				<button
					class="sidebar-item"
					class:active={playlistsStore.activePlaylistId === playlist.id}
					class:drag-over={dragOverPlaylistId === playlist.id}
					onclick={() => handleSelectPlaylist(playlist.id)}
					ondragover={(e) => handlePlaylistDragOver(e, playlist.id)}
					ondragleave={handlePlaylistDragLeave}
					ondrop={(e) => handlePlaylistDrop(e, playlist.id)}
				>
					<svg width="14" height="14" viewBox="0 0 16 16" fill="currentColor">
						<path d="M14 1H3v1h10.5l.5.5V13h1V1.5L14 1zM1 3.5l.5-.5h10l.5.5v11l-.5.5h-10l-.5-.5v-11zM2 4v10h9V4H2z" />
					</svg>
					<span>{playlist.name}</span>
					<span class="track-count">{playlist.track_count}</span>
				</button>
			{/each}
		</div>
	</div>

	<div class="sidebar-footer">
		<button
			class="icon-btn settings-btn"
			onclick={() => settingsStore.openSettings()}
			title="Settings"
		>
			<svg width="16" height="16" viewBox="0 0 16 16" fill="currentColor">
				<path
					d="M9.1 4.4L8.6 2H7.4l-.5 2.4-.7.3-2-1.3-.9.8 1.3 2-.2.7-2.4.5v1.2l2.4.5.3.7-1.3 2 .8.8 2-1.3.7.3.5 2.4h1.2l.5-2.4.7-.3 2 1.3.8-.8-1.3-2 .3-.7 2.4-.5V7.4l-2.4-.5-.3-.7 1.3-2-.8-.8-2 1.3-.7-.3zM8 10a2 2 0 110-4 2 2 0 010 4z"
				/>
			</svg>
		</button>
	</div>

	{#if libraryStore.isScanning && libraryStore.scanProgress}
		<div class="scan-progress">
			<div class="scan-text">
				Scanning... {libraryStore.scanProgress.scanned}/{libraryStore.scanProgress.total}
			</div>
			<div class="scan-bar">
				<div
					class="scan-bar-fill"
					style="width: {libraryStore.scanProgress.total > 0
						? (libraryStore.scanProgress.scanned / libraryStore.scanProgress.total) * 100
						: 0}%"
				></div>
			</div>
		</div>
	{/if}
</aside>

<style>
	.sidebar {
		width: 220px;
		min-width: 180px;
		background-color: var(--color-bg-secondary);
		border-right: 1px solid var(--color-border);
		display: flex;
		flex-direction: column;
		overflow: hidden;
	}

	.sidebar-header {
		display: flex;
		align-items: center;
		justify-content: space-between;
		padding: 8px 12px;
		border-bottom: 1px solid var(--color-border);
	}

	.sidebar-title {
		font-weight: 600;
		font-size: 12px;
		text-transform: uppercase;
		color: var(--color-text-secondary);
		letter-spacing: 0.5px;
	}

	.sidebar-actions {
		display: flex;
		gap: 2px;
	}

	.icon-btn {
		background: none;
		border: none;
		color: var(--color-text-secondary);
		cursor: pointer;
		padding: 4px;
		border-radius: 3px;
		display: flex;
		align-items: center;
		justify-content: center;
	}

	.icon-btn:hover {
		color: var(--color-text-primary);
		background-color: var(--color-bg-hover);
	}

	.icon-btn.small {
		padding: 2px;
	}

	.sidebar-content {
		flex: 1;
		overflow-y: auto;
		padding: 4px 0;
	}

	.sidebar-item {
		display: flex;
		align-items: center;
		gap: 8px;
		width: 100%;
		padding: 6px 12px;
		background: none;
		border: none;
		color: var(--color-text-primary);
		cursor: pointer;
		text-align: left;
		font-size: 13px;
	}

	.sidebar-item:hover {
		background-color: var(--color-bg-hover);
	}

	.sidebar-item.active {
		background-color: var(--color-bg-selected);
	}

	.sidebar-item.drag-over {
		background-color: var(--color-accent);
		color: white;
		outline: 2px solid var(--color-accent);
		outline-offset: -2px;
	}

	.sidebar-item .track-count {
		margin-left: auto;
		color: var(--color-text-muted);
		font-size: 11px;
	}

	.sidebar-section {
		margin-top: 8px;
	}

	.section-header {
		display: flex;
		align-items: center;
		justify-content: space-between;
		padding: 4px 12px;
		font-size: 11px;
		font-weight: 600;
		text-transform: uppercase;
		color: var(--color-text-muted);
		letter-spacing: 0.5px;
	}

	.new-playlist-input {
		padding: 4px 12px;
	}

	.new-playlist-input input {
		width: 100%;
		padding: 4px 8px;
		background-color: var(--color-bg-tertiary);
		border: 1px solid var(--color-accent);
		border-radius: 3px;
		color: var(--color-text-primary);
		font-size: 12px;
		outline: none;
	}

	.sidebar-footer {
		padding: 4px 8px;
		border-top: 1px solid var(--color-border);
		display: flex;
		justify-content: flex-end;
	}

	.settings-btn {
		padding: 6px;
	}

	.scan-progress {
		padding: 8px 12px;
		border-top: 1px solid var(--color-border);
	}

	.scan-text {
		font-size: 11px;
		color: var(--color-text-secondary);
		margin-bottom: 4px;
	}

	.scan-bar {
		height: 3px;
		background-color: var(--color-bg-tertiary);
		border-radius: 2px;
		overflow: hidden;
	}

	.scan-bar-fill {
		height: 100%;
		background-color: var(--color-accent);
		transition: width 0.2s ease;
	}
</style>
