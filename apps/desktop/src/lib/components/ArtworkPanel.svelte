<script lang="ts">
	import { playerStore } from '$lib/stores/player.svelte';
	import { formatDuration } from '$lib/types';
</script>

<aside class="artwork-panel">
	<div class="artwork-container">
		{#if playerStore.currentTrack?.has_artwork}
			<img
				src="data:image/png;base64,"
				alt="Album art"
				class="artwork-image"
			/>
		{:else}
			<div class="artwork-placeholder">
				<svg width="64" height="64" viewBox="0 0 24 24" fill="currentColor" opacity="0.3">
					<path
						d="M12 3v10.55c-.59-.34-1.27-.55-2-.55C7.79 13 6 14.79 6 17s1.79 4 4 4 4-1.79 4-4V7h4V3h-6z"
					/>
				</svg>
			</div>
		{/if}
	</div>

	{#if playerStore.currentTrack}
		<div class="track-details">
			<div class="detail-row title">{playerStore.currentTrack.title || 'Unknown'}</div>
			<div class="detail-row artist">{playerStore.currentTrack.artist || 'Unknown Artist'}</div>
			<div class="detail-row album">{playerStore.currentTrack.album || 'Unknown Album'}</div>

			<div class="metadata-grid">
				<div class="meta-item">
					<span class="meta-label">Duration</span>
					<span class="meta-value"
						>{formatDuration(playerStore.currentTrack.duration_ms)}</span
					>
				</div>
				<div class="meta-item">
					<span class="meta-label">Codec</span>
					<span class="meta-value">{playerStore.currentTrack.codec}</span>
				</div>
				{#if playerStore.currentTrack.sample_rate}
					<div class="meta-item">
						<span class="meta-label">Sample Rate</span>
						<span class="meta-value"
							>{(playerStore.currentTrack.sample_rate / 1000).toFixed(1)} kHz</span
						>
					</div>
				{/if}
				{#if playerStore.currentTrack.bitrate}
					<div class="meta-item">
						<span class="meta-label">Bitrate</span>
						<span class="meta-value">{playerStore.currentTrack.bitrate} kbps</span>
					</div>
				{/if}
				{#if playerStore.currentTrack.channels}
					<div class="meta-item">
						<span class="meta-label">Channels</span>
						<span class="meta-value"
							>{playerStore.currentTrack.channels === 1 ? 'Mono' : playerStore.currentTrack.channels === 2 ? 'Stereo' : `${playerStore.currentTrack.channels}ch`}</span
						>
					</div>
				{/if}
			</div>
		</div>
	{/if}
</aside>

<style>
	.artwork-panel {
		width: 250px;
		min-width: 200px;
		background-color: var(--color-bg-secondary);
		border-left: 1px solid var(--color-border);
		display: flex;
		flex-direction: column;
		overflow-y: auto;
	}

	.artwork-container {
		aspect-ratio: 1;
		background-color: var(--color-bg-tertiary);
		display: flex;
		align-items: center;
		justify-content: center;
		overflow: hidden;
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

	.track-details {
		padding: 12px;
		display: flex;
		flex-direction: column;
		gap: 4px;
	}

	.detail-row {
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}

	.detail-row.title {
		font-size: 14px;
		font-weight: 600;
		color: var(--color-text-primary);
	}

	.detail-row.artist {
		font-size: 12px;
		color: var(--color-text-secondary);
	}

	.detail-row.album {
		font-size: 12px;
		color: var(--color-text-muted);
		margin-bottom: 8px;
	}

	.metadata-grid {
		display: flex;
		flex-direction: column;
		gap: 6px;
		padding-top: 8px;
		border-top: 1px solid var(--color-border);
	}

	.meta-item {
		display: flex;
		justify-content: space-between;
		font-size: 11px;
	}

	.meta-label {
		color: var(--color-text-muted);
	}

	.meta-value {
		color: var(--color-text-secondary);
		font-family: var(--font-mono);
	}
</style>
