<script lang="ts">
	import { onMount } from 'svelte';
	import { invoke } from '@tauri-apps/api/core';
	import Sidebar from '$lib/components/Sidebar.svelte';
	import TrackList from '$lib/components/TrackList.svelte';
	import PlayerControls from '$lib/components/PlayerControls.svelte';
	import ErrorToast from '$lib/components/ErrorToast.svelte';
	import FolderDropOverlay from '$lib/components/FolderDropOverlay.svelte';
	import SettingsPanel from '$lib/components/settings/SettingsPanel.svelte';
	import { settingsStore } from '$lib/stores/settings.svelte';

	let sidebarWidth = $state(220);
	let isDragging = $state(false);

	onMount(() => {
		// Restore saved sidebar width
		const saved = settingsStore.getSetting('sidebar_width');
		if (saved) {
			const w = parseInt(saved, 10);
			if (w >= 150 && w <= 500) sidebarWidth = w;
		}
	});

	function startResize(e: MouseEvent) {
		e.preventDefault();
		isDragging = true;
		const startX = e.clientX;
		const startWidth = sidebarWidth;

		function onMove(e: MouseEvent) {
			const newWidth = Math.max(150, Math.min(500, startWidth + (e.clientX - startX)));
			sidebarWidth = newWidth;
		}

		function onUp() {
			isDragging = false;
			window.removeEventListener('mousemove', onMove);
			window.removeEventListener('mouseup', onUp);
			invoke('set_setting', { key: 'sidebar_width', value: String(sidebarWidth) }).catch(() => {});
		}

		window.addEventListener('mousemove', onMove);
		window.addEventListener('mouseup', onUp);
	}
</script>

<div class="app-layout">
	<div class="app-main">
		<div style="width: {sidebarWidth}px; min-width: 150px; max-width: 500px; flex-shrink: 0;">
			<Sidebar />
		</div>
		<!-- svelte-ignore a11y_no_static_element_interactions -->
		<div
			class="resize-handle"
			class:active={isDragging}
			onmousedown={startResize}
		></div>
		<TrackList />
	</div>
	<PlayerControls />
</div>

<ErrorToast />
<FolderDropOverlay />

{#if settingsStore.isSettingsOpen}
	<SettingsPanel />
{/if}

<style>
	.app-layout {
		display: flex;
		flex-direction: column;
		height: 100vh;
		background-color: var(--color-bg-primary);
	}

	.app-main {
		display: flex;
		flex: 1;
		min-height: 0;
		overflow: hidden;
	}

	.resize-handle {
		width: 4px;
		cursor: col-resize;
		background: transparent;
		flex-shrink: 0;
		position: relative;
		z-index: 10;
		margin-left: -2px;
		margin-right: -2px;
	}

	.resize-handle:hover,
	.resize-handle.active {
		background-color: var(--color-accent, #4a9eff);
		opacity: 0.5;
	}

	.resize-handle.active {
		opacity: 0.8;
	}
</style>
