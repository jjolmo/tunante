<script lang="ts">
	import { libraryStore } from '$lib/stores/library.svelte';
	import { playlistsStore } from '$lib/stores/playlists.svelte';
	import { consolesStore } from '$lib/stores/consoles.svelte';
	import { filesStore } from '$lib/stores/files.svelte';
	import { playerStore } from '$lib/stores/player.svelte';
	import { formatDuration } from '$lib/types';
	import type { Track, SortColumn, ColumnDef } from '$lib/types';
	import { invoke } from '@tauri-apps/api/core';
	import type { ContextMenuItem } from './ContextMenu.svelte';
	import ContextMenu from './ContextMenu.svelte';
	import MetadataDialog from './MetadataDialog.svelte';
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

	// Metadata dialog state
	let metadataDialogTracks = $state<Track[]>([]);

	// Column resize state
	let resizingCol = $state<string | null>(null);
	let resizeStartX = $state(0);
	let resizeStartWidth = $state(0);

	// Column drag reorder state
	let draggingColId = $state<string | null>(null);
	let dragOverColId = $state<string | null>(null);

	let tracks = $derived.by(() => {
		let result =
			playlistsStore.isFavedView ? playlistsStore.favedTracks :
			playlistsStore.activePlaylistId ? playlistsStore.playlistTracks :
			consolesStore.activeConsoleId ? consolesStore.consoleTracks :
			filesStore.activeFolder ? filesStore.folderTracks :
			libraryStore.filteredTracks;

		// Apply short track filter to non-library views (library view already filters in filteredTracks)
		const isLibraryView = result === libraryStore.filteredTracks;
		if (!isLibraryView && libraryStore.shortFilterEnabled && libraryStore.shortFilterThresholdSec > 0) {
			const thresholdMs = libraryStore.shortFilterThresholdSec * 1000;
			result = result.filter((t) => t.duration_ms >= thresholdMs);
		}

		// Apply search filter to all views (not just All Tracks)
		if (libraryStore.activeSearchQuery.trim() && !consolesStore.activeConsoleId && !filesStore.activeFolder && !isLibraryView) {
			const q = libraryStore.activeSearchQuery.toLowerCase();
			result = result.filter(
				(t) =>
					t.title.toLowerCase().includes(q) ||
					t.artist.toLowerCase().includes(q) ||
					t.album.toLowerCase().includes(q)
			);
		}

		// Apply sorting to non-library views (library view is already sorted in filteredTracks)
		if (!isLibraryView) {
			const { column, direction } = libraryStore.sortConfig;
			const dir = direction === 'asc' ? 1 : -1;
			result = [...result].sort((a, b) => {
				const va = a[column] ?? '';
				const vb = b[column] ?? '';
				let cmp: number;
				if (typeof va === 'number' && typeof vb === 'number') {
					cmp = (va - vb) * dir;
				} else {
					cmp = String(va).localeCompare(String(vb)) * dir;
				}
				if (cmp === 0 && column !== 'path') {
					return (a.path ?? '').localeCompare(b.path ?? '');
				}
				return cmp;
			});
		}
		return result;
	});

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

	// Unified column header interaction: click-to-sort + drag-to-reorder.
	// Uses raw mouse events instead of HTML5 Drag & Drop because Tauri's
	// native drag handler intercepts DOM drag events on macOS, preventing
	// dragstart from ever firing.
	let colMouseDown: { colId: string; field: SortColumn | null; x: number; y: number; dragging: boolean } | null = null;

	function handleColMouseDown(e: MouseEvent, colId: string, field: SortColumn | null) {
		if (e.button !== 0) return;
		colMouseDown = { colId, field, x: e.clientX, y: e.clientY, dragging: false };
		window.addEventListener('mousemove', handleColMouseMove);
		window.addEventListener('mouseup', handleColMouseUpGlobal);
	}

	function handleColMouseMove(e: MouseEvent) {
		if (!colMouseDown) return;
		const dx = e.clientX - colMouseDown.x;
		const dy = e.clientY - colMouseDown.y;
		if (!colMouseDown.dragging && dx * dx + dy * dy > 25) {
			colMouseDown.dragging = true;
			draggingColId = colMouseDown.colId;
		}
		if (colMouseDown.dragging) {
			// Find which column header the mouse is over
			const el = document.elementFromPoint(e.clientX, e.clientY);
			const colEl = el?.closest('[data-col-id]') as HTMLElement | null;
			const overId = colEl?.dataset.colId ?? null;
			dragOverColId = overId && overId !== draggingColId ? overId : null;
		}
	}

	function handleColMouseUpGlobal() {
		window.removeEventListener('mousemove', handleColMouseMove);
		window.removeEventListener('mouseup', handleColMouseUpGlobal);
		if (!colMouseDown) return;
		if (colMouseDown.dragging && draggingColId && dragOverColId) {
			libraryStore.moveColumn(draggingColId, dragOverColId);
		} else if (!colMouseDown.dragging && colMouseDown.field) {
			// Didn't drag — treat as a click → sort
			handleSort(colMouseDown.field);
		}
		draggingColId = null;
		dragOverColId = null;
		colMouseDown = null;
	}

	function handleTrackClick(track: Track, event: MouseEvent, idx: number) {
		libraryStore.selectTrack(track.id, event.ctrlKey || event.metaKey, event.shiftKey, idx, tracks);
	}

	function handleTrackDblClick(track: Track) {
		// Pass current view's track IDs as queue context for context-aware auto-advance
		const contextIds = tracks.map((t) => t.id);
		playerStore.playTrack(track, contextIds);
	}

	function handleMiddleClick(track: Track, event: MouseEvent) {
		if (event.button === 1) {
			event.preventDefault();
			playerStore.enqueueTracks([track.id]);
		}
	}

	function handleTrackContextMenu(track: Track, event: MouseEvent) {
		event.preventDefault();
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

		// Remove from playlist (only in playlist view)
		if (playlistsStore.activePlaylistId) {
			items.push({
				label: count > 1 ? `Remove ${count} tracks from playlist` : 'Remove from playlist',
				action: () => {
					playlistsStore.removeTracksFromPlaylist(playlistsStore.activePlaylistId!, selectedIds);
					libraryStore.clearSelection();
				}
			});
		}

		items.push({ separator: true, label: '', action: () => {} });

		// View metadata
		items.push({
			label: count > 1 ? `View metadata (${count} tracks)` : 'View metadata',
			action: () => {
				const selectedTracks = libraryStore.tracks.filter((t) => selectedIds.includes(t.id));
				metadataDialogTracks = selectedTracks;
			}
		});

		if (count === 1) {
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
		const tag = (e.target as HTMLElement)?.tagName;
		if (tag === 'INPUT' || tag === 'TEXTAREA') return;

		if ((e.ctrlKey || e.metaKey) && e.key === 'a') {
			e.preventDefault();
			libraryStore.selectAll(tracks);
		}

		// Arrow keys: move selection up/down
		if (e.key === 'ArrowUp' || e.key === 'ArrowDown') {
			e.preventDefault();
			const direction = e.key === 'ArrowDown' ? 1 : -1;

			// Find current index from last clicked or first selected
			let currentIdx = libraryStore.lastClickedIndex ?? -1;
			if (currentIdx < 0 && libraryStore.selectedTrackIds.size > 0) {
				const firstId = libraryStore.selectedTrackIds.values().next().value;
				currentIdx = tracks.findIndex((t) => t.id === firstId);
			}

			const newIdx = Math.max(0, Math.min(tracks.length - 1, currentIdx + direction));
			if (newIdx >= 0 && newIdx < tracks.length) {
				libraryStore.selectTrack(tracks[newIdx].id, false, e.shiftKey, newIdx, tracks);

				// Auto-scroll to keep selection visible
				if (container) {
					const rowTop = newIdx * ROW_HEIGHT;
					const rowBottom = rowTop + ROW_HEIGHT;
					if (rowTop < container.scrollTop) {
						container.scrollTop = rowTop;
					} else if (rowBottom > container.scrollTop + containerHeight) {
						container.scrollTop = rowBottom - containerHeight;
					}
				}
			}
		}

		// Enter key: play selected track
		if (e.key === 'Enter') {
			const selectedIds = [...libraryStore.selectedTrackIds];
			if (selectedIds.length === 1) {
				const track = tracks.find((t) => t.id === selectedIds[0]);
				if (track) {
					e.preventDefault();
					const contextIds = tracks.map((t) => t.id);
					playerStore.playTrack(track, contextIds);
				}
			}
		}

		// Delete key: remove selected tracks from active playlist
		if (e.key === 'Delete' && playlistsStore.activePlaylistId) {
			const selectedIds = [...libraryStore.selectedTrackIds];
			if (selectedIds.length > 0) {
				e.preventDefault();
				playlistsStore.removeTracksFromPlaylist(playlistsStore.activePlaylistId, selectedIds);
				libraryStore.clearSelection();
			}
		}
	}

	// Column resize
	function handleResizeStart(e: MouseEvent, colId: string) {
		e.preventDefault();
		e.stopPropagation();
		resizingCol = colId;
		resizeStartX = e.clientX;
		const col = libraryStore.columns.find((c) => c.id === colId);
		if (col) {
			// Get current computed width from the header element
			const headerEl = document.querySelector(`[data-col-id="${colId}"]`) as HTMLElement;
			resizeStartWidth = headerEl ? headerEl.getBoundingClientRect().width : 100;
		}
		window.addEventListener('mousemove', handleResizeMove);
		window.addEventListener('mouseup', handleResizeEnd);
	}

	function handleResizeMove(e: MouseEvent) {
		if (!resizingCol) return;
		const diff = e.clientX - resizeStartX;
		const newWidth = Math.max(40, resizeStartWidth + diff);
		libraryStore.setColumnWidth(resizingCol, `${Math.round(newWidth)}px`);
	}

	function handleResizeEnd() {
		if (resizingCol) {
			libraryStore.saveColumnConfig();
		}
		resizingCol = null;
		window.removeEventListener('mousemove', handleResizeMove);
		window.removeEventListener('mouseup', handleResizeEnd);
	}


	// Drag to playlist
	function handleDragStart(e: DragEvent, track: Track) {
		if (!libraryStore.selectedTrackIds.has(track.id)) {
			libraryStore.selectTrack(track.id);
		}
		const ids = [...libraryStore.selectedTrackIds];
		e.dataTransfer!.setData('application/x-tunante-tracks', JSON.stringify(ids));
		e.dataTransfer!.effectAllowed = 'copy';

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

	// Scroll to track when requested (e.g. clicking now-playing info)
	$effect(() => {
		const targetId = libraryStore.scrollToTrackId;
		if (!targetId || !container) return;

		const idx = tracks.findIndex((t) => t.id === targetId);
		if (idx >= 0) {
			const targetTop = idx * ROW_HEIGHT;
			container.scrollTop = targetTop - containerHeight / 2 + ROW_HEIGHT / 2;
			libraryStore.selectTrack(targetId, false, false, idx);
		}
		libraryStore.scrollToTrackId = null;
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
		<!-- Column headers use custom mouse-based drag instead of HTML5 Drag & Drop
		     because Tauri's native drag handler intercepts DOM drag events on macOS. -->
		{#each visibleColumns as col (col.id)}
			<!-- svelte-ignore a11y_no_static_element_interactions -->
			<div
				class="col"
				class:sortable={col.sortable}
				class:dragging-col={draggingColId === col.id}
				class:drag-over-col={dragOverColId === col.id}
				style={getColumnStyle(col)}
				data-col-id={col.id}
				role={col.sortable ? 'button' : undefined}
				tabindex={col.sortable ? 0 : undefined}
				onmousedown={(e) => handleColMouseDown(e, col.id, col.sortable ? col.field : null)}
			>
				{col.sortable ? sortIndicator(col.field) : ''}{col.label}
				<!-- svelte-ignore a11y_no_static_element_interactions -->
				<span
					class="col-resize-handle"
					onmousedown={(e) => handleResizeStart(e, col.id)}
				></span>
			</div>
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
								<svg class="playing-icon" width="10" height="10" viewBox="0 0 16 16" fill="currentColor"><polygon points="3,1 15,8 3,15"/></svg>
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

{#if metadataDialogTracks.length > 0}
	<MetadataDialog
		tracks={metadataDialogTracks}
		onclose={() => (metadataDialogTracks = [])}
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

	.tracklist-header .col.sortable {
		cursor: pointer;
	}

	.tracklist-header .col.sortable:hover {
		color: var(--color-text-primary);
	}

	.tracklist-header .col {
		position: relative;
	}

	.tracklist-header .dragging-col {
		opacity: 0.5;
	}

	.tracklist-header .drag-over-col {
		border-left: 2px solid var(--color-accent);
	}

	.col-resize-handle {
		position: absolute;
		top: 0;
		right: 0;
		width: 5px;
		height: 100%;
		cursor: col-resize;
		z-index: 1;
	}

	.col-resize-handle:hover {
		background-color: var(--color-accent);
		opacity: 0.5;
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
		color: var(--color-text-muted);
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
