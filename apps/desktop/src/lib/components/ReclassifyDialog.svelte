<!--
  The reclassify form as a dialog, for the context menu. The metadata dialog
  frames the same form as a column instead — see ReclassifyPanel.
-->
<script lang="ts">
	import ReclassifyPanel from './ReclassifyPanel.svelte';
	import type { Track } from '$lib/types';

	let {
		track = null,
		tracks = null,
		folderPath = null,
		onclose
	}: {
		track?: Track | null;
		tracks?: Track[] | null;
		folderPath?: string | null;
		onclose: () => void;
	} = $props();
</script>

<div class="overlay" onclick={onclose} onmousedown={(e) => e.stopPropagation()} role="presentation">
	<div class="dialog" onclick={(e) => e.stopPropagation()} role="presentation">
		<div class="header">
			<span class="title">Reclassify</span>
			<button class="close-btn" onclick={onclose} aria-label="Close">✕</button>
		</div>
		<ReclassifyPanel {track} {tracks} {folderPath} {onclose} />
	</div>
</div>

<style>
	.overlay {
		position: fixed;
		inset: 0;
		z-index: 200;
		background-color: rgba(0, 0, 0, 0.5);
		display: flex;
		align-items: center;
		justify-content: center;
	}

	.dialog {
		width: 460px;
		max-width: 92vw;
		max-height: 86vh;
		background-color: var(--color-bg-primary);
		border: 1px solid var(--color-border);
		border-radius: 6px;
		display: flex;
		flex-direction: column;
		overflow: hidden;
		box-shadow: 0 8px 32px rgba(0, 0, 0, 0.4);
	}

	.header {
		display: flex;
		align-items: center;
		justify-content: space-between;
		padding: 12px 16px;
		border-bottom: 1px solid var(--color-border);
		background-color: var(--color-bg-secondary);
	}

	.title {
		font-size: 13px;
		font-weight: 600;
		color: var(--color-text-primary);
	}

	.close-btn {
		background: none;
		border: none;
		color: var(--color-text-secondary);
		cursor: pointer;
		font-size: 13px;
		line-height: 1;
		padding: 2px 4px;
	}

	.close-btn:hover {
		color: var(--color-text-primary);
	}
</style>
