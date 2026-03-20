<script lang="ts">
	import { libraryStore } from '$lib/stores/library.svelte';
	import { playlistsStore } from '$lib/stores/playlists.svelte';
	import { playerStore } from '$lib/stores/player.svelte';
	import { formatDuration } from '$lib/types';
	import type { Track, SortColumn, ColumnDef } from '$lib/types';
	import { invoke } from '@tauri-apps/api/core';
	import type { ContextMenuItem } from './ContextMenu.svelte';
	import ContextMenu from './ContextMenu.svelte';
	import SearchBar from './SearchBar.svelte';

	const ROW_HEIGHT = 26;
	const BUFFER = 10;

	let container: HTMLDivElement | undefined = $state();
	let wrapperEl: HTMLDivElement | undefined = $state();
	let scrollTop = $state(0);
	let containerHeight = $state(600);

	// Context menu state
	let contextMenu = $state<{ items: ContextMenuItem[]; x: number; y: number } | null>(null);

	// Drag state
	let dragImageEl: HTMLDivElement | undefined = $state();

	let tracks = $derived(
		playlistsStore.activePlaylistId ? playlistsStore.playlistTracks : libraryStore.filteredTracks
	);

	let visibleColumns = $derived(libraryStore.visibleColumns);

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

	function handleTrackClick(track: Track, event: MouseEvent, idx: number) {
		libraryStore.selectTrack(track.id, event.ctrlKey || event.metaKey, event.shiftKey, idx);
	}

	function handleTrackDblClick(track: Track) {
		playerStore.playTrack(track);
	}

	function handleMiddleClick(track: Track, event: MouseEvent) {
		if (event.button === 1) {
			event.preventDefault();
			playerStore.enqueueTracks([track.id]);
		}
	}

	function handleTrackContextMenu(track: Track, event: MouseEvent) {
		event.preventDefault();
		// If right-clicked track is not in selection, select it alone
		if (!libraryStore.selectedTrackIds.has(track.id)) {
			libraryStore.selectTrack(track.id);
		}

		const selectedIds = [...libraryStore.selectedTrackIds];
		const inQueue = selectedIds.length === 1 && playerStore.isInQueue(track.id);
		const count = selectedIds.length;

		const items: ContextMenuItem[] = [];

		if (inQueue) {
			items.push({
				label: 'Remove from queue',
				action: () => playerStore.dequeueTracks(selectedIds)
			});
		} else {
			items.push({
				label: count > 1 ? `Add ${count} tracks to queue` : 'Add to queue',
				action: () => playerStore.enqueueTracks(selectedIds)
			});
		}

		if (count === 1) {
			items.push({ separator: true });
			items.push({
				label: 'Open containing folder',
				action: () => invoke('open_containing_folder', { path: track.path })
			});
		}

		contextMenu = { items, x: event.clientX, y: event.clientY };
	}

	function buildHeaderMenuItems(): ContextMenuItem[] {
		return libraryStore.columns.map((c) => ({
			label: c.label,
			checked: c.visible,
			action: () => {
				libraryStore.toggleColumn(c.id);
				if (contextMenu) {
					contextMenu = { ...contextMenu, items: buildHeaderMenuItems() };
				}
			}
		}));
	}

	function handleHeaderContextMenu(event: MouseEvent) {
		event.preventDefault();
		contextMenu = { items: buildHeaderMenuItems(), x: event.clientX, y: event.clientY };
	}

	function sortIndicator(column: SortColumn): string {
		if (libraryStore.sortConfig.column !== column) return '';
		return libraryStore.sortConfig.direction === 'asc' ? ' ▲' : ' ▼';
	}

	function getCellValue(track: Track, col: ColumnDef): string {
		if (col.format) return col.format(track);
		const val = track[col.field as keyof Track];
		if (val === null || val === undefined) return '';
		if (col.field === 'duration_ms') return formatDuration(val as number);
		return String(val);
	}

	function getColumnStyle(col: ColumnDef): string {
		const parts: string[] = [];
		if (col.width) {
			parts.push(`width: ${col.width}; min-width: ${col.width}`);
		} else if (col.flex) {
			parts.push(`flex: ${col.flex}`);
			if (col.minWidth) parts.push(`min-width: ${col.minWidth}`);
		}
		if (col.align === 'right') parts.push('text-align: right');
		else if (col.align === 'center') parts.push('text-align: center');
		return parts.join('; ');
	}

	function handleKeydown(e: KeyboardEvent) {
		if ((e.ctrlKey || e.metaKey) && e.key === 'a') {
			e.preventDefault();
			libraryStore.selectAll();
		}
	}

	// Drag to playlist
	function handleDragStart(e: DragEvent, track: Track) {
		if (!libraryStore.selectedTrackIds.has(track.id)) {
			libraryStore.selectTrack(track.id);
		}
		const ids = [...libraryStore.selectedTrackIds];
		e.dataTransfer!.setData('application/x-tunante-tracks', JSON.stringify(ids));
		e.dataTransfer!.effectAllowed = 'copy';

		// Custom drag image
		if (dragImageEl) {
			dragImageEl.textContent = `♫ ${ids.length} track${ids.length > 1 ? 's' : ''}`;
			dragImageEl.style.display = 'block';
			e.dataTransfer!.setDragImage(dragImageEl, 0, 0);
			requestAnimationFrame(() => {
				if (dragImageEl) dragImageEl.style.display = 'none';
			});
		}
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

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="tracklist-wrapper" bind:this={wrapperEl} onkeydown={handleKeydown} tabindex="-1">
	<SearchBar />
	<!-- svelte-ignore a11y_no_static_element_interactions -->
	<div class="tracklist-header" oncontextmenu={handleHeaderContextMenu}>
		<div class="col col-status">
			<svg width="10" height="10" viewBox="0 0 16 16" fill="currentColor"><polygon points="3,1 15,8 3,15"/></svg>
		</div>
		{#each visibleColumns as col (col.id)}
			{#if col.sortable}
				<button class="col" style={getColumnStyle(col)} onclick={() => handleSort(col.field)}>
					{col.label}{sortIndicator(col.field)}
				</button>
			{:else}
				<div class="col" style={getColumnStyle(col)}>{col.label}</div>
			{/if}
		{/each}
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
						onclick={(e) => handleTrackClick(track, e, idx)}
						ondblclick={() => handleTrackDblClick(track)}
						onauxclick={(e) => handleMiddleClick(track, e)}
						oncontextmenu={(e) => handleTrackContextMenu(track, e)}
						draggable={libraryStore.selectedTrackIds.has(track.id)}
						ondragstart={(e) => handleDragStart(e, track)}
					>
						<div class="col col-status">
							{#if playerStore.currentTrack?.id === track.id && playerStore.isPlaying}
								<span class="playing-icon">▶</span>
							{:else if playerStore.isInQueue(track.id)}
								<span class="queue-pos">{playerStore.queuePosition(track.id)}</span>
							{/if}
						</div>
						{#each visibleColumns as col (col.id)}
							<div class="col" style={getColumnStyle(col)}>
								<span class="cell-text" title={getCellValue(track, col)}>{getCellValue(track, col)}</span>
							</div>
						{/each}
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

	<!-- Drag image element (hidden) -->
	<div class="drag-image" bind:this={dragImageEl}></div>
</div>

{#if contextMenu}
	<ContextMenu
		items={contextMenu.items}
		x={contextMenu.x}
		y={contextMenu.y}
		onclose={() => (contextMenu = null)}
	/>
{/if}

<style>
	.tracklist-wrapper {
		flex: 1;
		display: flex;
		flex-direction: column;
		min-width: 0;
		overflow: hidden;
		outline: none;
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

	.col-status {
		width: 28px;
		min-width: 28px;
		text-align: center;
		display: flex;
		align-items: center;
		justify-content: center;
		flex-shrink: 0;
		padding: 0 4px;
	}

	.tracklist-header .col-status {
		color: var(--color-text-muted);
	}

	.playing-icon {
		color: var(--color-accent);
		font-size: 10px;
	}

	.queue-pos {
		font-size: 10px;
		font-weight: 700;
		color: var(--color-accent);
	}

	.cell-text {
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
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

	.drag-image {
		position: fixed;
		top: -100px;
		left: -100px;
		display: none;
		background-color: var(--color-accent);
		color: white;
		padding: 4px 10px;
		border-radius: 12px;
		font-size: 12px;
		font-weight: 600;
		white-space: nowrap;
		pointer-events: none;
		z-index: 9999;
	}
</style>
