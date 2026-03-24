<script lang="ts">
	import { open } from '@tauri-apps/plugin-dialog';
	import { invoke } from '@tauri-apps/api/core';
	import { libraryStore } from '$lib/stores/library.svelte';
	import { playlistsStore } from '$lib/stores/playlists.svelte';
	import { consolesStore, CODEC_TO_CONSOLE, CONSOLE_DEFINITIONS } from '$lib/stores/consoles.svelte';
	import { filesStore } from '$lib/stores/files.svelte';
	import { playerStore } from '$lib/stores/player.svelte';
	import { settingsStore } from '$lib/stores/settings.svelte';
	import FilesBrowser from './FilesBrowser.svelte';
	import type { ContextMenuItem } from './ContextMenu.svelte';
	import ContextMenu from './ContextMenu.svelte';

	let newPlaylistName = $state('');
	let showNewPlaylistInput = $state(false);
	let dragOverPlaylistId = $state<string | null>(null);
	let dragOverCreatePlaylist = $state(false);
	let pendingCreateTrackIds = $state<string[]>([]);
	let artworkSrc = $state<string | null>(null);
	let lastArtworkTrackPath = $state<string | null>(null);

	// Playlist reorder drag state
	let draggingPlaylistId = $state<string | null>(null);
	let reorderDragOverId = $state<string | null>(null);

	// Playlist context menu
	let contextMenu = $state<{ items: ContextMenuItem[]; x: number; y: number } | null>(null);

	// Rename state
	let renamingPlaylistId = $state<string | null>(null);
	let renameValue = $state('');

	// Fetch artwork when the current track changes
	$effect(() => {
		const track = playerStore.currentTrack;
		if (!track) {
			artworkSrc = null;
			lastArtworkTrackPath = null;
			return;
		}
		if (track.path === lastArtworkTrackPath) return;
		lastArtworkTrackPath = track.path;
		// Immediately reset to placeholder while loading
		artworkSrc = null;
		invoke<string | null>('get_artwork', { trackPath: track.path })
			.then(async (data) => {
				if (data) {
					artworkSrc = data;
				} else if (settingsStore.autoDownloadCoverArt) {
					// No local artwork — try downloading
					try {
						let downloaded: string | null = null;
						const consoleId = CODEC_TO_CONSOLE.get(track.codec);
						if (consoleId && track.album) {
							// VGM track: use Wikipedia game cover scraper
							const consoleDef = CONSOLE_DEFINITIONS.find((d) => d.id === consoleId);
							downloaded = await invoke<string | null>('fetch_vgm_cover_art', {
								gameName: track.album,
								consoleName: consoleDef?.name || '',
							});
						} else if (track.album || track.artist) {
							// Standard track: use iTunes scraper
							downloaded = await invoke<string | null>('fetch_cover_art', {
								album: track.album || '',
								artist: track.artist || '',
							});
						}
						// Only update if still on the same track
						if (lastArtworkTrackPath === track.path) {
							artworkSrc = downloaded;
						}
					} catch {
						artworkSrc = null;
					}
				} else {
					artworkSrc = null;
				}
			})
			.catch(() => {
				artworkSrc = null;
			});
	});

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
						'mp3', 'flac', 'ogg', 'wav', 'aac', 'aiff', 'wma', 'm4a', 'opus',
						'nsf', 'nsfe', 'spc', 'gbs', 'vgm', 'vgz', 'hes', 'kss', 'ay', 'sap', 'gym',
						'bcstm', 'bfstm', 'brstm', 'bcwav', 'bfwav', 'brwav',
						'adx', 'hca', 'dsp', 'idsp', 'fsb', 'wem', 'xma', 'at3', 'at9',
						'nus3bank', 'lopus', 'acb', 'awb', 'ktss',
						'gsf', 'minigsf', 'psf', 'minipsf', 'psf2', 'minipsf2',
						'usf', 'miniusf', '2sf', 'mini2sf',
						'ssf', 'minissf', 'dsf', 'minidsf'
					]
				}
			]
		});
		if (selected) {
			await libraryStore.addFiles(selected as string[]);
		}
	}

	function handleSelectPlaylist(id: string) {
		consolesStore.selectConsole(null);
		filesStore.selectFolder(null);
		playlistsStore.selectPlaylist(id);
	}

	function handleSelectConsole(id: string) {
		playlistsStore.selectPlaylist(null);
		filesStore.selectFolder(null);
		consolesStore.selectConsole(id);
		invoke('set_setting', { key: 'session_view', value: 'console' }).catch(() => {});
		invoke('set_setting', { key: 'session_view_id', value: id }).catch(() => {});
	}

	function handleSelectAllTracks() {
		consolesStore.selectConsole(null);
		filesStore.selectFolder(null);
		playlistsStore.selectPlaylist(null);
		invoke('set_setting', { key: 'session_view', value: 'all' }).catch(() => {});
		invoke('set_setting', { key: 'session_view_id', value: '' }).catch(() => {});
	}

	async function handleCreatePlaylist() {
		if (pendingCreateTrackIds.length > 0) {
			await handleCreatePlaylistWithTracks();
			return;
		}
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

	function handleRenameKeydown(e: KeyboardEvent) {
		if (e.key === 'Enter') {
			finishRename();
		}
		if (e.key === 'Escape') {
			renamingPlaylistId = null;
			renameValue = '';
		}
	}

	async function finishRename() {
		if (renamingPlaylistId && renameValue.trim()) {
			await playlistsStore.renamePlaylist(renamingPlaylistId, renameValue.trim());
		}
		renamingPlaylistId = null;
		renameValue = '';
	}

	function handlePlaylistContextMenu(e: MouseEvent, playlist: { id: string; name: string; track_count: number }) {
		e.preventDefault();
		const items: ContextMenuItem[] = [
			{
				label: `Enqueue all (${playlist.track_count} tracks)`,
				action: async () => {
					// Get all track IDs from this playlist and enqueue them
					try {
						const tracks = await invoke<{ id: string }[]>('get_playlist_tracks', { playlistId: playlist.id });
						const ids = tracks.map((t) => t.id);
						if (ids.length > 0) {
							await playerStore.enqueueTracks(ids);
						}
					} catch (err) {
						console.error('Failed to enqueue playlist:', err);
					}
				}
			},
			{ separator: true, label: '', action: () => {} },
			{
				label: 'Rename',
				action: () => {
					renamingPlaylistId = playlist.id;
					renameValue = playlist.name;
				}
			},
			{
				label: 'Delete',
				action: () => {
					playlistsStore.deletePlaylist(playlist.id);
				}
			}
		];
		contextMenu = { items, x: e.clientX, y: e.clientY };
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

	// Drag-to-create-playlist on section header
	function handleSectionDragOver(e: DragEvent) {
		if (e.dataTransfer?.types.includes('application/x-tunante-tracks')) {
			e.preventDefault();
			e.dataTransfer.dropEffect = 'copy';
			dragOverCreatePlaylist = true;
		}
	}

	function handleSectionDragLeave() {
		dragOverCreatePlaylist = false;
	}

	function handleSectionDrop(e: DragEvent) {
		e.preventDefault();
		dragOverCreatePlaylist = false;
		const data = e.dataTransfer?.getData('application/x-tunante-tracks');
		if (data) {
			const trackIds: string[] = JSON.parse(data);
			pendingCreateTrackIds = trackIds;
			showNewPlaylistInput = true;
			newPlaylistName = '';
		}
	}

	// Playlist reorder drag handlers
	function handlePlaylistReorderDragStart(e: DragEvent, playlistId: string) {
		draggingPlaylistId = playlistId;
		e.dataTransfer!.effectAllowed = 'move';
		e.dataTransfer!.setData('application/x-tunante-playlist-reorder', playlistId);
	}

	function handlePlaylistReorderDragOver(e: DragEvent, playlistId: string) {
		if (e.dataTransfer?.types.includes('application/x-tunante-playlist-reorder')) {
			e.preventDefault();
			e.dataTransfer.dropEffect = 'move';
			reorderDragOverId = playlistId;
		}
	}

	function handlePlaylistReorderDragEnd() {
		draggingPlaylistId = null;
		reorderDragOverId = null;
	}

	async function handlePlaylistReorderDrop(e: DragEvent, targetId: string) {
		e.preventDefault();
		reorderDragOverId = null;
		const sourceId = e.dataTransfer?.getData('application/x-tunante-playlist-reorder');
		if (!sourceId || sourceId === targetId) {
			draggingPlaylistId = null;
			return;
		}
		const ids = playlistsStore.playlists.map((p) => p.id);
		const fromIdx = ids.indexOf(sourceId);
		const toIdx = ids.indexOf(targetId);
		if (fromIdx === -1 || toIdx === -1) {
			draggingPlaylistId = null;
			return;
		}
		ids.splice(fromIdx, 1);
		ids.splice(toIdx, 0, sourceId);
		// Optimistic reorder
		const reordered = ids.map((id) => playlistsStore.playlists.find((p) => p.id === id)!);
		playlistsStore.playlists = reordered;
		draggingPlaylistId = null;
		await playlistsStore.reorderPlaylists(ids);
	}

	async function handleCreatePlaylistWithTracks() {
		if (newPlaylistName.trim()) {
			await playlistsStore.createPlaylist(newPlaylistName.trim());
			// Find the newly created playlist (last one)
			const newPlaylist = playlistsStore.playlists[playlistsStore.playlists.length - 1];
			if (newPlaylist && pendingCreateTrackIds.length > 0) {
				await playlistsStore.addTracksToPlaylist(newPlaylist.id, pendingCreateTrackIds);
			}
			newPlaylistName = '';
			showNewPlaylistInput = false;
			pendingCreateTrackIds = [];
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
			class:active={playlistsStore.activePlaylistId === null && !playlistsStore.isFavedView && consolesStore.activeConsoleId === null && filesStore.activeFolder === null}
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

		<button
			class="sidebar-item"
			class:active={playlistsStore.isFavedView}
			onclick={() => { consolesStore.selectConsole(null); playlistsStore.selectFaved(); }}
		>
			<svg width="14" height="14" viewBox="0 0 16 16" fill="currentColor">
				<path d="M8 1.23l2.18 4.41 4.87.71-3.52 3.43.83 4.85L8 12.26l-4.36 2.37.83-4.85L1 6.35l4.87-.71L8 1.23z" />
			</svg>
			<span>Faved</span>
			<span class="track-count">{libraryStore.favedCount}</span>
		</button>

		{#if settingsStore.showPlaylists}
		<!-- svelte-ignore a11y_no_static_element_interactions -->
		<div class="sidebar-section">
			<div
				class="section-header"
				class:drag-over-create={dragOverCreatePlaylist}
				ondragover={handleSectionDragOver}
				ondragleave={handleSectionDragLeave}
				ondrop={handleSectionDrop}
			>
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
				{#if renamingPlaylistId === playlist.id}
					<div class="new-playlist-input">
						<input
							type="text"
							bind:value={renameValue}
							onkeydown={handleRenameKeydown}
							onblur={finishRename}
						/>
					</div>
				{:else}
					<button
						class="sidebar-item"
						class:active={playlistsStore.activePlaylistId === playlist.id}
						class:drag-over={dragOverPlaylistId === playlist.id}
						class:reorder-over={reorderDragOverId === playlist.id}
						class:dragging={draggingPlaylistId === playlist.id}
						draggable="true"
						onclick={() => handleSelectPlaylist(playlist.id)}
						oncontextmenu={(e) => handlePlaylistContextMenu(e, playlist)}
						ondragstart={(e) => handlePlaylistReorderDragStart(e, playlist.id)}
						ondragover={(e) => { handlePlaylistDragOver(e, playlist.id); handlePlaylistReorderDragOver(e, playlist.id); }}
						ondragleave={() => { handlePlaylistDragLeave(); reorderDragOverId = null; }}
						ondrop={(e) => { handlePlaylistDrop(e, playlist.id); handlePlaylistReorderDrop(e, playlist.id); }}
						ondragend={handlePlaylistReorderDragEnd}
					>
						<svg width="14" height="14" viewBox="0 0 16 16" fill="currentColor">
							<path d="M14 1H3v1h10.5l.5.5V13h1V1.5L14 1zM1 3.5l.5-.5h10l.5.5v11l-.5.5h-10l-.5-.5v-11zM2 4v10h9V4H2z" />
						</svg>
						<span>{playlist.name}</span>
						{#if playlistsStore.scanningPlaylistId === playlist.id}
							<span class="scanning-icon" title="Scanning...">
								<svg width="14" height="14" viewBox="0 0 16 16" fill="currentColor" class="spin">
									<path d="M8 1a7 7 0 00-7 7h2a5 5 0 015-5V1z" />
								</svg>
							</span>
						{:else}
							<span class="track-count">{playlist.track_count}</span>
						{/if}
					</button>
				{/if}
			{/each}
		</div>
		{/if}

		{#if settingsStore.showConsoles && consolesStore.consolesWithCounts.length > 0}
			<div class="sidebar-section">
				<div class="section-header">
					<span>Consoles</span>
				</div>

				{#each consolesStore.consolesWithCounts as console (console.id)}
					<button
						class="sidebar-item"
						class:active={consolesStore.activeConsoleId === console.id}
						onclick={() => handleSelectConsole(console.id)}
					>
						<svg width="14" height="14" viewBox="0 0 16 16" fill="currentColor">
							<path d={console.icon} />
						</svg>
						<span>{console.name}</span>
						<span class="track-count">{console.trackCount}</span>
					</button>
				{/each}
			</div>
		{/if}

		{#if settingsStore.showFiles && libraryStore.tracks.length > 0}
			<FilesBrowser />
		{/if}
	</div>

	{#if settingsStore.showCoverArt}
		<div class="sidebar-artwork">
			{#if playerStore.currentTrack}
				<div class="artwork-container">
					{#if artworkSrc}
						<img src={artworkSrc} alt="Album art" class="artwork-image" />
					{:else}
						<div class="artwork-placeholder">
							<svg width="40" height="40" viewBox="0 0 24 24" fill="currentColor" opacity="0.3">
								<path
									d="M12 3v10.55c-.59-.34-1.27-.55-2-.55C7.79 13 6 14.79 6 17s1.79 4 4 4 4-1.79 4-4V7h4V3h-6z"
								/>
							</svg>
						</div>
					{/if}
				</div>
			{/if}
		</div>
	{/if}

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

{#if contextMenu}
	<ContextMenu
		items={contextMenu.items}
		x={contextMenu.x}
		y={contextMenu.y}
		onclose={() => (contextMenu = null)}
	/>
{/if}

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

	.sidebar-item.reorder-over {
		border-top: 2px solid var(--color-accent);
	}

	.sidebar-item.dragging {
		opacity: 0.4;
	}

	.sidebar-item .track-count {
		margin-left: auto;
		color: var(--color-text-muted);
		font-size: 11px;
	}

	.sidebar-item .scanning-icon {
		margin-left: auto;
		color: var(--color-accent);
		display: flex;
		align-items: center;
	}

	.spin {
		animation: spin 1s linear infinite;
	}

	@keyframes spin {
		from { transform: rotate(0deg); }
		to { transform: rotate(360deg); }
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

	.section-header.drag-over-create {
		border: 2px dashed var(--color-accent);
		border-radius: 4px;
		background-color: rgba(var(--color-accent-rgb, 0, 120, 212), 0.1);
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

	.sidebar-artwork {
		flex-shrink: 0;
	}

	.artwork-container {
		aspect-ratio: 1;
		background-color: var(--color-bg-tertiary);
		display: flex;
		align-items: center;
		justify-content: center;
		overflow: hidden;
		border-top: 1px solid var(--color-border);
	}

	.artwork-image {
		width: 100%;
		height: 100%;
		object-fit: cover;
	}

	.artwork-placeholder {
		display: flex;
		align-items: center;
		justify-content: center;
		width: 100%;
		height: 100%;
		color: var(--color-text-muted);
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
