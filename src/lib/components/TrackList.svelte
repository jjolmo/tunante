<script lang="ts">
	import { libraryStore } from '$lib/stores/library.svelte';
	import { playlistsStore } from '$lib/stores/playlists.svelte';
	import { playerStore } from '$lib/stores/player.svelte';
	import { formatDuration } from '$lib/types';
	import type { Track, SortColumn } from '$lib/types';
	import SearchBar from './SearchBar.svelte';

	const ROW_HEIGHT = 26;
	const BUFFER = 10;

	let container: HTMLDivElement | undefined = $state();
	let scrollTop = $state(0);
	let containerHeight = $state(600);

	let tracks = $derived(
		playlistsStore.activePlaylistId ? playlistsStore.playlistTracks : libraryStore.filteredTracks
	);

	let totalHeight = $derived(tracks.length * ROW_HEIGHT);
	let startIndex = $derived(Math.max(0, Math.floor(scrollTop / ROW_HEIGHT) - BUFFER));
	let endIndex = $derived(
		Math.min(tracks.length, Math.ceil((scrollTop + containerHeight) / ROW_HEIGHT) + BUFFER)
	);
	let visibleTracks = $derived(tracks.slice(startIndex, endIndex));
	let offsetY = $derived(startIndex * ROW_HEIGHT);

	function handleScroll() {
		if (container) {
			scrollTop = container.scrollTop;
		}
	}

	function handleSort(column: SortColumn) {
		libraryStore.setSort(column);
	}

	function handleTrackClick(track: Track, event: MouseEvent) {
		libraryStore.selectTrack(track.id, event.ctrlKey || event.metaKey);
	}

	function handleTrackDblClick(track: Track) {
		playerStore.playTrack(track);
	}

	function sortIndicator(column: SortColumn): string {
		if (libraryStore.sortConfig.column !== column) return '';
		return libraryStore.sortConfig.direction === 'asc' ? ' ▲' : ' ▼';
	}

	$effect(() => {
		if (container) {
			const observer = new ResizeObserver((entries) => {
				containerHeight = entries[0].contentRect.height;
			});
			observer.observe(container);
			return () => observer.disconnect();
		}
	});
</script>

<div class="tracklist-wrapper">
	<SearchBar />
	<div class="tracklist-header">
		<div class="col col-num">#</div>
		<button class="col col-title" onclick={() => handleSort('title')}>
			Title{sortIndicator('title')}
		</button>
		<button class="col col-artist" onclick={() => handleSort('artist')}>
			Artist{sortIndicator('artist')}
		</button>
		<button class="col col-album" onclick={() => handleSort('album')}>
			Album{sortIndicator('album')}
		</button>
		<button class="col col-duration" onclick={() => handleSort('duration_ms')}>
			Duration{sortIndicator('duration_ms')}
		</button>
		<button class="col col-codec" onclick={() => handleSort('codec')}>
			Codec{sortIndicator('codec')}
		</button>
	</div>

	<div class="tracklist-body" bind:this={container} onscroll={handleScroll}>
		<div style="height: {totalHeight}px; position: relative;">
			<div style="transform: translateY({offsetY}px);">
				{#each visibleTracks as track, i (track.id)}
					{@const idx = startIndex + i}
					<button
						class="track-row"
						class:selected={libraryStore.selectedTrackIds.has(track.id)}
						class:playing={playerStore.currentTrack?.id === track.id}
						onclick={(e) => handleTrackClick(track, e)}
						ondblclick={() => handleTrackDblClick(track)}
					>
						<div class="col col-num">
							{#if playerStore.currentTrack?.id === track.id && playerStore.isPlaying}
								<span class="playing-icon">▶</span>
							{:else}
								{idx + 1}
							{/if}
						</div>
						<div class="col col-title" title={track.title}>{track.title || 'Unknown'}</div>
						<div class="col col-artist" title={track.artist}>
							{track.artist || 'Unknown Artist'}
						</div>
						<div class="col col-album" title={track.album}>{track.album || 'Unknown Album'}</div>
						<div class="col col-duration">{formatDuration(track.duration_ms)}</div>
						<div class="col col-codec">{track.codec}</div>
					</button>
				{/each}
			</div>
		</div>
	</div>

	{#if tracks.length === 0}
		<div class="empty-state">
			{#if libraryStore.searchQuery}
				<p>No tracks match your search.</p>
			{:else}
				<p>No tracks in library.</p>
				<p class="hint">Use the sidebar to add folders or files.</p>
			{/if}
		</div>
	{/if}
</div>

<style>
	.tracklist-wrapper {
		flex: 1;
		display: flex;
		flex-direction: column;
		min-width: 0;
		overflow: hidden;
	}

	.tracklist-header {
		display: flex;
		align-items: center;
		height: 28px;
		background-color: var(--color-bg-tertiary);
		border-bottom: 1px solid var(--color-border);
		font-size: 11px;
		font-weight: 600;
		color: var(--color-text-secondary);
		text-transform: uppercase;
		letter-spacing: 0.3px;
		flex-shrink: 0;
	}

	.tracklist-header button {
		background: none;
		border: none;
		color: inherit;
		font: inherit;
		cursor: pointer;
		text-align: left;
		letter-spacing: inherit;
		text-transform: inherit;
	}

	.tracklist-header button:hover {
		color: var(--color-text-primary);
	}

	.tracklist-body {
		flex: 1;
		overflow-y: auto;
		overflow-x: hidden;
	}

	.col {
		padding: 0 8px;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}

	.col-num {
		width: 45px;
		min-width: 45px;
		text-align: right;
		color: var(--color-text-muted);
	}

	.col-title {
		flex: 3;
		min-width: 150px;
	}

	.col-artist {
		flex: 2;
		min-width: 100px;
	}

	.col-album {
		flex: 2;
		min-width: 100px;
	}

	.col-duration {
		width: 70px;
		min-width: 70px;
		text-align: right;
		font-family: var(--font-mono);
	}

	.col-codec {
		width: 60px;
		min-width: 60px;
		text-align: center;
		font-size: 11px;
	}

	.track-row {
		display: flex;
		align-items: center;
		height: 26px;
		font-size: 12px;
		cursor: default;
		width: 100%;
		background: none;
		border: none;
		color: var(--color-text-primary);
		text-align: left;
		font-family: inherit;
	}

	.track-row:nth-child(even) {
		background-color: rgba(255, 255, 255, 0.02);
	}

	.track-row:hover {
		background-color: var(--color-bg-hover);
	}

	.track-row.selected {
		background-color: var(--color-bg-selected);
	}

	.track-row.playing {
		color: var(--color-accent-hover);
	}

	.playing-icon {
		color: var(--color-accent);
		font-size: 10px;
	}

	.empty-state {
		display: flex;
		flex-direction: column;
		align-items: center;
		justify-content: center;
		padding: 40px;
		color: var(--color-text-muted);
		position: absolute;
		inset: 0;
		pointer-events: none;
	}

	.empty-state .hint {
		font-size: 12px;
		margin-top: 8px;
	}
</style>
