<script lang="ts">
	import { filesStore, type FolderNode } from '$lib/stores/files.svelte';
	import { playlistsStore } from '$lib/stores/playlists.svelte';
	import { invoke } from '@tauri-apps/api/core';

	let expandSaveTimer: ReturnType<typeof setTimeout> | null = null;

	function handleFolderClick(node: FolderNode) {
		// Batch all state changes: clear other views first, then set files view
		// Using queueMicrotask to avoid Svelte 5 reactivity cascade crashes
		playlistsStore.isFavedView = false;
		playlistsStore.activePlaylistId = null;
		playlistsStore.playlistTracks = [];
		import('$lib/stores/consoles.svelte').then(({ consolesStore }) => {
			consolesStore.selectConsole(null);
		});
		filesStore.selectFolder(node.fullPath);
		invoke('set_setting', { key: 'session_view', value: 'files' }).catch(() => {});
		invoke('set_setting', { key: 'session_view_id', value: node.fullPath }).catch(() => {});
	}

	function handleTreeToggle(e: Event, path: string) {
		e.stopPropagation();
		filesStore.toggleExpanded(path);
		// Debounced save of expanded folders
		if (expandSaveTimer) clearTimeout(expandSaveTimer);
		expandSaveTimer = setTimeout(() => filesStore.saveExpandedFolders(), 500);
	}

	function handleBreadcrumbNav(path: string | null) {
		playlistsStore.selectPlaylist(null);
		import('$lib/stores/consoles.svelte').then(({ consolesStore }) => {
			consolesStore.selectConsole(null);
		});
		filesStore.navigateTo(path);
		invoke('set_setting', { key: 'session_view', value: 'files' }).catch(() => {});
		if (path) invoke('set_setting', { key: 'session_view_id', value: path }).catch(() => {});
	}

	function handleBreadcrumbFolderClick(node: FolderNode) {
		if (node.children.length > 0) {
			// Has children: navigate into it
			handleBreadcrumbNav(node.fullPath);
		} else {
			// Leaf folder: just select it
			handleFolderClick(node);
		}
	}

	function toggleViewMode() {
		const next = filesStore.viewMode === 'tree' ? 'breadcrumb' : 'tree';
		filesStore.setViewMode(next);
		// Sync currentPath with activeFolder when switching to breadcrumb
		if (next === 'breadcrumb' && filesStore.activeFolder) {
			filesStore.currentPath = filesStore.activeFolder;
		}
	}
</script>

{#snippet treeNode(node: FolderNode, depth: number)}
	<button
		class="sidebar-item tree-item"
		class:active={filesStore.activeFolder === node.fullPath}
		style="padding-left: {12 + depth * 16}px"
		onclick={() => handleFolderClick(node)}
		title={node.fullPath}
	>
		{#if node.children.length > 0}
			<svg
				class="expand-arrow"
				class:expanded={filesStore.expandedFolders.has(node.fullPath)}
				width="10"
				height="10"
				viewBox="0 0 16 16"
				fill="currentColor"
				onclick={(e) => handleTreeToggle(e, node.fullPath)}
				role="button"
				tabindex="-1"
			>
				<path d="M6 4l4 4-4 4" />
			</svg>
		{:else}
			<span class="expand-spacer"></span>
		{/if}
		<svg class="folder-icon" width="14" height="14" viewBox="0 0 16 16" fill="currentColor">
			<path d="M14.5 3H7.71l-.85-.85L6.51 2H1.5l-.5.5v11l.5.5h13l.5-.5v-10L14.5 3zm-.51 8.49V7H7.99l-.85-.85L6.79 6H2V3h4.29l.85.85.35.15H14v7.49z" />
		</svg>
		<span class="folder-name">{node.name}</span>
		<span class="track-count">{node.trackCount}</span>
	</button>
	{#if filesStore.expandedFolders.has(node.fullPath)}
		{#each node.children as child (child.fullPath)}
			{@render treeNode(child, depth + 1)}
		{/each}
	{/if}
{/snippet}

<div class="sidebar-section">
	<div class="section-header">
		<span>Files</span>
		<button
			class="mode-toggle"
			onclick={toggleViewMode}
			title={filesStore.viewMode === 'tree' ? 'Switch to breadcrumb view' : 'Switch to tree view'}
		>
			{#if filesStore.viewMode === 'tree'}
				<svg width="14" height="14" viewBox="0 0 16 16" fill="currentColor">
					<path d="M1 3h14v1H1V3zm0 4h10v1H1V7zm0 4h12v1H1v-1z" />
				</svg>
			{:else}
				<svg width="14" height="14" viewBox="0 0 16 16" fill="currentColor">
					<path d="M1.5 1h3l.5.5v3l-.5.5h-3l-.5-.5v-3l.5-.5zm0 6h3l.5.5v3l-.5.5h-3l-.5-.5v-3l.5-.5zm6-6h7v1h-7V1zm0 3h5v1h-5V4zm0 3h7v1h-7V7zm0 3h5v1h-5v-1zm-6 0h3l.5.5v3l-.5.5h-3l-.5-.5v-3l.5-.5z" />
				</svg>
			{/if}
		</button>
	</div>

	<!-- Folder search -->
	<div class="folder-search">
		<svg class="search-icon-small" width="11" height="11" viewBox="0 0 16 16" fill="currentColor">
			<path d="M15.25 13.69l-3.77-3.77a5.54 5.54 0 10-1.56 1.56l3.77 3.77a1.1 1.1 0 001.56-1.56zM2 6.5A4.5 4.5 0 116.5 11 4.5 4.5 0 012 6.5z" />
		</svg>
		<input
			type="text"
			placeholder="Find folder..."
			value={filesStore.folderSearch}
			oninput={(e) => filesStore.folderSearch = (e.target as HTMLInputElement).value}
			class="folder-search-input"
		/>
		{#if filesStore.folderSearch}
			<button class="folder-search-clear" onclick={() => filesStore.folderSearch = ''}>✕</button>
		{/if}
	</div>

	{#if filesStore.folderSearch.trim()}
		<!-- Search results: flat list of matching folders -->
		{#each filesStore.folderSearchResults as node (node.fullPath)}
			<button
				class="sidebar-item"
				class:active={filesStore.activeFolder === node.fullPath}
				onclick={() => handleFolderClick(node)}
				title={node.fullPath}
			>
				<svg class="folder-icon" width="14" height="14" viewBox="0 0 16 16" fill="currentColor">
					<path d="M14.5 3H7.71l-.85-.85L6.51 2H1.5l-.5.5v11l.5.5h13l.5-.5v-10L14.5 3zm-.51 8.49V7H7.99l-.85-.85L6.79 6H2V3h4.29l.85.85.35.15H14v7.49z" />
				</svg>
				<span class="folder-name">{node.name}</span>
				<span class="track-count">{node.trackCount}</span>
			</button>
		{:else}
			<div class="empty-hint">No folders match</div>
		{/each}
	{:else if filesStore.viewMode === 'tree'}
		<!-- Tree mode -->
		{#each filesStore.folderTree as rootNode (rootNode.fullPath)}
			{@render treeNode(rootNode, 0)}
		{/each}
	{:else}
		<!-- Breadcrumb mode -->
		{#if filesStore.currentPath}
			<div class="breadcrumb-bar">
				<button class="breadcrumb-up" onclick={() => filesStore.navigateUp()} title="Go up">
					<svg width="12" height="12" viewBox="0 0 16 16" fill="currentColor">
						<path d="M11 8.5L7 4.5 3 8.5h2.5V13h3V8.5H11z" />
					</svg>
				</button>
				{#each filesStore.breadcrumbs as crumb, i (crumb.path)}
					{#if i > 0}<span class="breadcrumb-sep">/</span>{/if}
					<button
						class="breadcrumb-segment"
						class:current={i === filesStore.breadcrumbs.length - 1}
						onclick={() => handleBreadcrumbNav(crumb.path)}
						title={crumb.path}
					>
						{crumb.name}
					</button>
				{/each}
			</div>
		{/if}
		{#each filesStore.currentFolderChildren as child (child.fullPath)}
			<button
				class="sidebar-item"
				class:active={filesStore.activeFolder === child.fullPath}
				onclick={() => handleBreadcrumbFolderClick(child)}
				title={child.fullPath}
			>
				<svg class="folder-icon" width="14" height="14" viewBox="0 0 16 16" fill="currentColor">
					<path d="M14.5 3H7.71l-.85-.85L6.51 2H1.5l-.5.5v11l.5.5h13l.5-.5v-10L14.5 3zm-.51 8.49V7H7.99l-.85-.85L6.79 6H2V3h4.29l.85.85.35.15H14v7.49z" />
				</svg>
				<span class="folder-name">{child.name}</span>
				{#if child.children.length > 0}
					<svg class="nav-arrow" width="10" height="10" viewBox="0 0 16 16" fill="currentColor">
						<path d="M6 4l4 4-4 4" />
					</svg>
				{/if}
				<span class="track-count">{child.trackCount}</span>
			</button>
		{/each}
		{#if filesStore.currentFolderChildren.length === 0 && !filesStore.currentPath}
			<div class="empty-hint">No monitored folders</div>
		{/if}
	{/if}
</div>

<style>
	.sidebar-section {
		display: flex;
		flex-direction: column;
	}

	.folder-search {
		display: flex;
		align-items: center;
		gap: 4px;
		padding: 3px 10px;
		margin: 0 6px 4px;
		background-color: var(--color-bg-secondary);
		border: 1px solid var(--color-border);
		border-radius: 4px;
	}

	.search-icon-small {
		color: var(--color-text-muted);
		flex-shrink: 0;
	}

	.folder-search-input {
		flex: 1;
		background: none;
		border: none;
		color: var(--color-text-primary);
		font-size: 11px;
		outline: none;
		padding: 2px 0;
		min-width: 0;
	}

	.folder-search-input::placeholder {
		color: var(--color-text-muted);
	}

	.folder-search-clear {
		background: none;
		border: none;
		color: var(--color-text-muted);
		cursor: pointer;
		font-size: 10px;
		padding: 0 2px;
		line-height: 1;
	}

	.folder-search-clear:hover {
		color: var(--color-text-primary);
	}

	.section-header {
		display: flex;
		align-items: center;
		justify-content: space-between;
		padding: 4px 12px;
		font-size: 11px;
		font-weight: 700;
		text-transform: uppercase;
		letter-spacing: 0.5px;
		color: var(--color-text-muted);
	}

	.mode-toggle {
		background: none;
		border: none;
		cursor: pointer;
		color: var(--color-text-muted);
		padding: 2px;
		border-radius: 3px;
		display: flex;
		align-items: center;
	}

	.mode-toggle:hover {
		color: var(--color-text-primary);
		background-color: var(--color-bg-hover);
	}

	.sidebar-item {
		display: flex;
		align-items: center;
		gap: 6px;
		width: 100%;
		padding: 5px 12px;
		border: none;
		background: none;
		color: var(--color-text-primary);
		font-size: 13px;
		text-align: left;
		cursor: pointer;
		white-space: nowrap;
		overflow: hidden;
	}

	.sidebar-item:hover {
		background-color: var(--color-bg-hover);
	}

	.sidebar-item.active {
		background-color: var(--color-bg-selected);
	}

	.tree-item {
		padding-right: 8px;
	}

	.expand-arrow {
		flex-shrink: 0;
		transition: transform 0.15s ease;
		color: var(--color-text-muted);
		cursor: pointer;
	}

	.expand-arrow.expanded {
		transform: rotate(90deg);
	}

	.expand-spacer {
		width: 10px;
		flex-shrink: 0;
	}

	.folder-icon {
		flex-shrink: 0;
		color: var(--color-text-secondary);
	}

	.folder-name {
		flex: 1;
		overflow: hidden;
		text-overflow: ellipsis;
	}

	.track-count {
		flex-shrink: 0;
		font-size: 11px;
		color: var(--color-text-muted);
		margin-left: auto;
	}

	.nav-arrow {
		flex-shrink: 0;
		color: var(--color-text-muted);
	}

	/* Breadcrumb bar */
	.breadcrumb-bar {
		display: flex;
		align-items: center;
		gap: 2px;
		padding: 4px 12px;
		overflow-x: auto;
		white-space: nowrap;
		font-size: 11px;
		color: var(--color-text-muted);
		scrollbar-width: none;
	}

	.breadcrumb-bar::-webkit-scrollbar {
		display: none;
	}

	.breadcrumb-up {
		background: none;
		border: none;
		cursor: pointer;
		color: var(--color-text-muted);
		padding: 2px;
		border-radius: 3px;
		display: flex;
		align-items: center;
		flex-shrink: 0;
	}

	.breadcrumb-up:hover {
		color: var(--color-text-primary);
		background-color: var(--color-bg-hover);
	}

	.breadcrumb-sep {
		color: var(--color-text-muted);
		flex-shrink: 0;
	}

	.breadcrumb-segment {
		background: none;
		border: none;
		cursor: pointer;
		color: var(--color-text-secondary);
		font-size: 11px;
		padding: 1px 4px;
		border-radius: 3px;
		max-width: 80px;
		overflow: hidden;
		text-overflow: ellipsis;
	}

	.breadcrumb-segment:hover {
		color: var(--color-text-primary);
		background-color: var(--color-bg-hover);
	}

	.breadcrumb-segment.current {
		color: var(--color-text-primary);
		font-weight: 600;
	}

	.empty-hint {
		padding: 8px 12px;
		font-size: 12px;
		color: var(--color-text-muted);
		font-style: italic;
	}
</style>
