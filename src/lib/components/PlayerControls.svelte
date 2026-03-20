<script lang="ts">
	import { playerStore } from '$lib/stores/player.svelte';

	let isSeeking = $state(false);
	let seekValue = $state(0);

	function handleSeekStart() {
		isSeeking = true;
		seekValue = playerStore.positionMs;
	}

	function handleSeekEnd() {
		isSeeking = false;
		playerStore.seek(seekValue);
	}

	function handleSeekInput(e: Event) {
		seekValue = Number((e.target as HTMLInputElement).value);
	}

	function handleVolumeChange(e: Event) {
		playerStore.setVolume(Number((e.target as HTMLInputElement).value));
	}

	let displayPosition = $derived(isSeeking ? seekValue : playerStore.positionMs);

	function formatTime(ms: number): string {
		const totalSeconds = Math.floor(ms / 1000);
		const minutes = Math.floor(totalSeconds / 60);
		const seconds = totalSeconds % 60;
		return `${minutes}:${seconds.toString().padStart(2, '0')}`;
	}
</script>

<div class="player-controls">
	<div class="now-playing">
		{#if playerStore.currentTrack}
			<div class="track-info">
				<span class="track-title">{playerStore.currentTrack.title || 'Unknown'}</span>
				<span class="track-artist">{playerStore.currentTrack.artist || 'Unknown Artist'}</span>
			</div>
		{:else}
			<div class="track-info">
				<span class="track-title idle">Tunante</span>
			</div>
		{/if}
	</div>

	<div class="controls-center">
		<div class="transport-buttons">
			<button
				class="ctrl-btn"
				onclick={() => playerStore.toggleShuffle()}
				class:active={playerStore.shuffle}
				title="Shuffle"
			>
				<svg width="16" height="16" viewBox="0 0 16 16" fill="currentColor">
					<path
						d="M13.151 8L14 8.849l-.849.849L12.303 8.849 13.151 8zM14 4.849L13.151 4l-.849.849.849.849L14 4.849zM11.5 13h1v-2.5L8.964 7H5.5V5h-3v3h3V6h2.964L11.5 9.5V13zM5.5 11h-3v-3h3v3zM12.5 3h-1v2.5L8.036 9H5.5v2h-3V8h3v2h2.964L11.5 6.5V3h1z"
					/>
				</svg>
			</button>

			<button class="ctrl-btn" onclick={() => playerStore.prevTrack()} title="Previous">
				<svg width="16" height="16" viewBox="0 0 16 16" fill="currentColor">
					<path d="M4 3v10h1V3H4zm8.03 5L6 3v10l6.03-5z" />
				</svg>
			</button>

			<button
				class="ctrl-btn play-btn"
				onclick={() => playerStore.togglePlayPause()}
				title={playerStore.isPlaying ? 'Pause' : 'Play'}
			>
				{#if playerStore.isPlaying}
					<svg width="20" height="20" viewBox="0 0 16 16" fill="currentColor">
						<path d="M4.5 3H7v10H4.5V3zm4.5 0h2.5v10H9V3z" />
					</svg>
				{:else}
					<svg width="20" height="20" viewBox="0 0 16 16" fill="currentColor">
						<path d="M4 3v10l9-5-9-5z" />
					</svg>
				{/if}
			</button>

			<button class="ctrl-btn" onclick={() => playerStore.stop()} title="Stop">
				<svg width="16" height="16" viewBox="0 0 16 16" fill="currentColor">
					<path d="M3.5 3.5h9v9h-9z" />
				</svg>
			</button>

			<button class="ctrl-btn" onclick={() => playerStore.nextTrack()} title="Next">
				<svg width="16" height="16" viewBox="0 0 16 16" fill="currentColor">
					<path d="M11 3v10h1V3h-1zM3.97 8L10 13V3L3.97 8z" />
				</svg>
			</button>

			<button
				class="ctrl-btn"
				onclick={() => playerStore.cycleRepeat()}
				class:active={playerStore.repeat !== 'off'}
				title="Repeat: {playerStore.repeat}"
			>
				<svg width="16" height="16" viewBox="0 0 16 16" fill="currentColor">
					<path
						d="M5.5 2l-3 3 3 3V6h6.5v2h1V5.5l-.5-.5H5.5V2zm5 11l3-3-3-3v2H4V7H3v3.5l.5.5h7V13z"
					/>
				</svg>
				{#if playerStore.repeat === 'one'}
					<span class="repeat-one">1</span>
				{/if}
			</button>
		</div>

		<div class="seek-bar">
			<span class="time">{formatTime(displayPosition)}</span>
			<input
				type="range"
				min="0"
				max={playerStore.durationMs || 100}
				value={displayPosition}
				oninput={handleSeekInput}
				onmousedown={handleSeekStart}
				onmouseup={handleSeekEnd}
				class="seek-slider"
			/>
			<span class="time">{formatTime(playerStore.durationMs)}</span>
		</div>
	</div>

	<div class="controls-right">
		<div class="volume-control">
			<button
				class="ctrl-btn"
				onclick={() => playerStore.setVolume(playerStore.volume > 0 ? 0 : 0.8)}
				title="Volume"
			>
				<svg width="16" height="16" viewBox="0 0 16 16" fill="currentColor">
					{#if playerStore.volume === 0}
						<path d="M7.56 2L4 5.43H1v5.14h3L7.56 14V2zM9 6.5l1.5 1.5L9 9.5v1.41L11.21 8.5 9 6.09V6.5z" />
					{:else if playerStore.volume < 0.5}
						<path d="M7.56 2L4 5.43H1v5.14h3L7.56 14V2zm2.88 3.17a3.5 3.5 0 010 5.66l-.7-.72a2.5 2.5 0 000-4.22l.7-.72z" />
					{:else}
						<path
							d="M7.56 2L4 5.43H1v5.14h3L7.56 14V2zm4.75.92l-.71.71a6 6 0 010 8.74l.71.71a7 7 0 000-10.16zm-1.77 1.77l-.7.7a3.77 3.77 0 010 5.22l.7.7a4.77 4.77 0 000-6.62z"
						/>
					{/if}
				</svg>
			</button>
			<input
				type="range"
				min="0"
				max="1"
				step="0.01"
				value={playerStore.volume}
				oninput={handleVolumeChange}
				class="volume-slider"
			/>
		</div>
	</div>
</div>

<style>
	.player-controls {
		display: flex;
		align-items: center;
		height: 72px;
		padding: 0 16px;
		background-color: var(--color-bg-secondary);
		border-top: 1px solid var(--color-border);
		gap: 16px;
		flex-shrink: 0;
	}

	.now-playing {
		width: 200px;
		min-width: 150px;
		overflow: hidden;
	}

	.track-info {
		display: flex;
		flex-direction: column;
		gap: 2px;
	}

	.track-title {
		font-size: 13px;
		font-weight: 500;
		color: var(--color-text-primary);
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}

	.track-title.idle {
		color: var(--color-text-muted);
	}

	.track-artist {
		font-size: 11px;
		color: var(--color-text-secondary);
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}

	.controls-center {
		flex: 1;
		display: flex;
		flex-direction: column;
		align-items: center;
		gap: 4px;
		max-width: 600px;
	}

	.transport-buttons {
		display: flex;
		align-items: center;
		gap: 8px;
	}

	.ctrl-btn {
		background: none;
		border: none;
		color: var(--color-text-secondary);
		cursor: pointer;
		padding: 4px;
		border-radius: 50%;
		display: flex;
		align-items: center;
		justify-content: center;
		position: relative;
	}

	.ctrl-btn:hover {
		color: var(--color-text-primary);
	}

	.ctrl-btn.active {
		color: var(--color-accent);
	}

	.ctrl-btn.play-btn {
		background-color: var(--color-text-primary);
		color: var(--color-bg-primary);
		width: 32px;
		height: 32px;
	}

	.ctrl-btn.play-btn:hover {
		background-color: white;
	}

	.repeat-one {
		position: absolute;
		font-size: 8px;
		font-weight: 700;
		top: -2px;
		right: -2px;
		color: var(--color-accent);
	}

	.seek-bar {
		display: flex;
		align-items: center;
		gap: 8px;
		width: 100%;
	}

	.time {
		font-size: 11px;
		color: var(--color-text-muted);
		font-family: var(--font-mono);
		min-width: 40px;
		text-align: center;
	}

	.seek-slider,
	.volume-slider {
		-webkit-appearance: none;
		appearance: none;
		height: 4px;
		background: var(--color-bg-tertiary);
		border-radius: 2px;
		outline: none;
		cursor: pointer;
	}

	.seek-slider {
		flex: 1;
	}

	.seek-slider::-webkit-slider-thumb,
	.volume-slider::-webkit-slider-thumb {
		-webkit-appearance: none;
		appearance: none;
		width: 12px;
		height: 12px;
		background: var(--color-text-primary);
		border-radius: 50%;
		cursor: pointer;
		opacity: 0;
		transition: opacity 0.15s;
	}

	.seek-slider:hover::-webkit-slider-thumb,
	.volume-slider:hover::-webkit-slider-thumb {
		opacity: 1;
	}

	.controls-right {
		width: 200px;
		min-width: 150px;
		display: flex;
		justify-content: flex-end;
	}

	.volume-control {
		display: flex;
		align-items: center;
		gap: 4px;
	}

	.volume-slider {
		width: 90px;
	}
</style>
