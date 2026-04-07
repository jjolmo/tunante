<script lang="ts">
	import { playerStore } from '$lib/stores/player.svelte';
	import { settingsStore } from '$lib/stores/settings.svelte';
	import { libraryStore } from '$lib/stores/library.svelte';
	import { playlistsStore } from '$lib/stores/playlists.svelte';
	import { invoke } from '@tauri-apps/api/core';

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

	// Rating: operates on the selected track (not the playing track)
	let selectedTrack = $derived(libraryStore.selectedTrack);
	let selectedTrackRating = $derived(selectedTrack?.rating ?? 0);

	async function toggleRating() {
		if (!selectedTrack) return;
		const newRating = selectedTrackRating > 0 ? 0 : 5;
		try {
			await invoke('set_track_rating', {
				trackId: selectedTrack.id,
				rating: newRating,
				writeToFile: settingsStore.keepFavsInMetadata
			});
			libraryStore.updateTrackRating(selectedTrack.id, newRating);
			playlistsStore.updateTrackRating(selectedTrack.id, newRating);
			// Refresh faved view if active
			if (playlistsStore.isFavedView) {
				await playlistsStore.loadFavedTracks();
			}
		} catch (e) {
			console.error('Failed to set rating:', e);
		}
	}

	async function handleNowPlayingClick() {
		const track = playerStore.currentTrack;
		if (!track) return;

		const { consolesStore } = await import('$lib/stores/consoles.svelte');
		const { filesStore } = await import('$lib/stores/files.svelte');

		// Determine which tracks are actually visible in the current view
		// (must match TrackList's logic: faved → playlist → console → files → all)
		const visibleTracks =
			playlistsStore.isFavedView ? playlistsStore.favedTracks :
			playlistsStore.activePlaylistId ? playlistsStore.playlistTracks :
			consolesStore.activeConsoleId ? consolesStore.consoleTracks :
			filesStore.activeFolder ? filesStore.folderTracks :
			libraryStore.filteredTracks;

		// If the track is already visible, just scroll to it
		if (visibleTracks.some((t) => t.id === track.id)) {
			libraryStore.requestScrollTo(track.id);
			return;
		}

		// Try to find which console contains this track (direct codec or folder inference)
		const trackConsoleId = consolesStore.getTrackConsole(track);
		if (trackConsoleId) {
			playlistsStore.selectPlaylist(null);
			filesStore.selectFolder(null);
			consolesStore.selectConsole(trackConsoleId);
			setTimeout(() => libraryStore.requestScrollTo(track.id), 50);
			return;
		}

		// Fallback: navigate to track's parent folder if files browser is enabled
		if (settingsStore.showFiles) {
			const trackDir = track.path.substring(0, track.path.lastIndexOf('/'));
			playlistsStore.selectPlaylist(null);
			consolesStore.selectConsole(null);
			filesStore.selectFolder(trackDir);
			setTimeout(() => libraryStore.requestScrollTo(track.id), 50);
			return;
		}

		// Last resort: switch to All Tracks
		playlistsStore.selectAllTracks();
		consolesStore.selectConsole(null);
		filesStore.selectFolder(null);
		setTimeout(() => libraryStore.requestScrollTo(track.id), 50);
	}

	function formatTime(ms: number): string {
		const totalSeconds = Math.floor(ms / 1000);
		const minutes = Math.floor(totalSeconds / 60);
		const seconds = totalSeconds % 60;
		return `${minutes}:${seconds.toString().padStart(2, '0')}`;
	}

	// In-app shortcut handler for bare keys (no modifiers — only work when window focused)
	async function handleGlobalKeydown(e: KeyboardEvent) {
		// Always handle Ctrl+P for settings
		if ((e.ctrlKey || e.metaKey) && e.key === 'p') {
			e.preventDefault();
			settingsStore.openSettings();
			return;
		}

		// Don't intercept when typing in inputs or settings is open
		const tag = (e.target as HTMLElement)?.tagName;
		if (tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT') return;
		if (settingsStore.isSettingsOpen) return;

		// Check if this key matches any in-app shortcut (bare keys without modifiers)
		const shortcuts = await invoke<Record<string, string>>('get_shortcuts');
		const normalizedKey = normalizeKeyForMatch(e);

		for (const [actionId, keys] of Object.entries(shortcuts)) {
			if (!keys || keys.includes('Mouse')) continue;
			if (keys === normalizedKey) {
				e.preventDefault();
				executeInAppAction(actionId);
				return;
			}
		}
	}

	function normalizeKeyForMatch(e: KeyboardEvent): string {
		const parts: string[] = [];
		if (e.ctrlKey || e.metaKey) parts.push('Ctrl');
		if (e.shiftKey) parts.push('Shift');
		if (e.altKey) parts.push('Alt');

		let key = e.key;
		if (key === ' ') key = 'Space';
		else if (key === 'ArrowUp') key = 'Up';
		else if (key === 'ArrowDown') key = 'Down';
		else if (key === 'ArrowLeft') key = 'Left';
		else if (key === 'ArrowRight') key = 'Right';
		else if (key.length === 1) key = key.toUpperCase();

		if (!['Control', 'Shift', 'Alt', 'Meta'].includes(e.key)) {
			parts.push(key);
		}

		return parts.join('+');
	}

	function executeInAppAction(actionId: string) {
		switch (actionId) {
			case 'play_pause': playerStore.togglePlayPause(); break;
			case 'stop': playerStore.stop(); break;
			case 'next_track': playerStore.nextTrack(); break;
			case 'prev_track': playerStore.prevTrack(); break;
			case 'volume_up': playerStore.setVolume(Math.min(1, playerStore.volume + 0.05)); break;
			case 'volume_down': playerStore.setVolume(Math.max(0, playerStore.volume - 0.05)); break;
			case 'mute': playerStore.setVolume(playerStore.volume > 0 ? 0 : 0.8); break;
			case 'toggle_shuffle': playerStore.toggleShuffle(); break;
			case 'cycle_repeat': playerStore.cycleRepeat(); break;
			case 'focus_search': {
				const input = document.querySelector('.search-bar input') as HTMLInputElement;
				if (input) input.focus();
				break;
			}
			case 'toggle_fav': toggleRating(); break;
		}
	}

	// Listen for shortcut actions from backend (for frontend-only actions like focus_search)
	import { listen } from '@tauri-apps/api/event';
	import { onMount } from 'svelte';

	onMount(() => {
		const unlisten = listen<string>('shortcut-action', (event) => {
			executeInAppAction(event.payload);
		});
		return () => { unlisten.then(fn => fn()); };
	});
</script>

<svelte:window onkeydown={handleGlobalKeydown} />

<div class="player-controls">
	<div class="seek-row">
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

	<div class="controls-row">
		<!-- svelte-ignore a11y_click_events_have_key_events -->
		<!-- svelte-ignore a11y_no_static_element_interactions -->
		<div class="now-playing" onclick={handleNowPlayingClick} class:clickable={!!playerStore.currentTrack}>
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

		<div class="transport-buttons">
			<button
				class="ctrl-btn"
				onclick={() => playerStore.toggleShuffle()}
				class:active={playerStore.shuffle}
				title="Shuffle"
			>
				<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round">
					<polyline points="16 3 21 3 21 8" />
					<line x1="4" y1="20" x2="21" y2="3" />
					<polyline points="21 16 21 21 16 21" />
					<line x1="15" y1="15" x2="21" y2="21" />
					<line x1="4" y1="4" x2="9" y2="9" />
				</svg>
			</button>

			<button class="ctrl-btn" onclick={() => playerStore.prevTrack()} title="Previous">
				<svg width="20" height="20" viewBox="0 0 16 16" fill="currentColor">
					<path d="M11 3v10h1V3h-1zM3.97 8L10 13V3L3.97 8z" />
				</svg>
			</button>

			<button
				class="ctrl-btn play-btn"
				onclick={() => {
					if (!playerStore.isPlaying && !playerStore.currentTrack) {
						const selected = libraryStore.selectedTrack;
						if (selected) {
							const contextIds = libraryStore.filteredTracks.map((t) => t.id);
							playerStore.playTrack(selected, contextIds);
							return;
						}
					}
					playerStore.togglePlayPause();
				}}
				title={playerStore.isPlaying ? 'Pause' : 'Play'}
			>
				{#if playerStore.isPlaying}
					<svg width="24" height="24" viewBox="0 0 16 16" fill="currentColor">
						<path d="M4.5 3H7v10H4.5V3zm4.5 0h2.5v10H9V3z" />
					</svg>
				{:else}
					<svg width="24" height="24" viewBox="0 0 16 16" fill="currentColor">
						<path d="M4 3v10l9-5-9-5z" />
					</svg>
				{/if}
			</button>

			<button class="ctrl-btn" onclick={() => playerStore.stop()} title="Stop">
				<svg width="20" height="20" viewBox="0 0 16 16" fill="currentColor">
					<path d="M3.5 3.5h9v9h-9z" />
				</svg>
			</button>

			<button class="ctrl-btn" onclick={() => playerStore.nextTrack()} title="Next">
				<svg width="20" height="20" viewBox="0 0 16 16" fill="currentColor">
					<path d="M4 3v10h1V3H4zm8.03 5L6 3v10l6.03-5z" />
				</svg>
			</button>

			<button
				class="ctrl-btn"
				onclick={() => playerStore.cycleRepeat()}
				class:active={playerStore.repeat !== 'off'}
				title="Repeat: {playerStore.repeat}"
			>
				<svg width="20" height="20" viewBox="0 0 16 16" fill="currentColor">
					<path
						d="M5.5 2l-3 3 3 3V6h6.5v2h1V5.5l-.5-.5H5.5V2zm5 11l3-3-3-3v2H4V7H3v3.5l.5.5h7V13z"
					/>
				</svg>
				{#if playerStore.repeat === 'one'}
					<span class="repeat-one">1</span>
				{/if}
			</button>

			<button
				class="ctrl-btn fav-btn"
				class:active={selectedTrackRating > 0}
				onclick={toggleRating}
				disabled={!selectedTrack}
				title={selectedTrack ? (selectedTrackRating > 0 ? 'Remove from favorites' : 'Add to favorites') : 'Select a track first'}
			>
				<svg width="20" height="20" viewBox="0 0 16 16" fill="currentColor">
					{#if selectedTrackRating > 0}
						<path d="M8 1.23l2.18 4.41 4.87.71-3.52 3.43.83 4.85L8 12.26l-4.36 2.37.83-4.85L1 6.35l4.87-.71L8 1.23z" />
					{:else}
						<path d="M8 2.5l1.55 3.14 3.47.5-2.51 2.45.59 3.46L8 10.26l-3.1 1.79.59-3.46L3 6.14l3.47-.5L8 2.5zM8 1.23l-2.18 4.41-4.87.71 3.52 3.43-.83 4.85L8 12.26l4.36 2.37-.83-4.85 3.52-3.43-4.87-.71L8 1.23z" />
					{/if}
				</svg>
			</button>
		</div>

		<div class="controls-right">
			<button
				class="ctrl-btn"
				onclick={() => settingsStore.openSettings()}
				title="Settings (Ctrl+P)"
			>
				<svg width="18" height="18" viewBox="0 0 16 16" fill="currentColor">
					<path
						d="M9.1 4.4L8.6 2H7.4l-.5 2.4-.7.3-2-1.3-.9.8 1.3 2-.2.7-2.4.5v1.2l2.4.5.3.7-1.3 2 .8.8 2-1.3.7.3.5 2.4h1.2l.5-2.4.7-.3 2 1.3.8-.8-1.3-2 .3-.7 2.4-.5V7.4l-2.4-.5-.3-.7 1.3-2-.8-.8-2 1.3-.7-.3zM8 10a2 2 0 110-4 2 2 0 010 4z"
					/>
				</svg>
			</button>
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
</div>

<style>
	.player-controls {
		display: flex;
		flex-direction: column;
		background-color: var(--color-bg-secondary);
		border-top: 1px solid var(--color-border);
		flex-shrink: 0;
	}

	.seek-row {
		display: flex;
		align-items: center;
		gap: 8px;
		padding: 6px 16px 0;
	}

	.controls-row {
		display: flex;
		align-items: center;
		padding: 4px 16px 8px;
		gap: 16px;
	}

	.now-playing {
		flex: 1;
		min-width: 150px;
		overflow: hidden;
	}

	.now-playing.clickable {
		cursor: pointer;
		border-radius: 4px;
		padding: 2px 4px;
		margin: -2px -4px;
	}

	.now-playing.clickable:hover {
		background-color: transparent;
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

	.ctrl-btn:disabled {
		opacity: 0.3;
		cursor: default;
	}

	.ctrl-btn:disabled:hover {
		color: var(--color-text-secondary);
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

	.fav-btn.active {
		color: #f5c518;
	}

	.fav-btn.active:hover {
		color: #d4a810;
	}

	.repeat-one {
		position: absolute;
		font-size: 8px;
		font-weight: 700;
		top: -2px;
		right: -2px;
		color: var(--color-accent);
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
	}

	.controls-right {
		display: flex;
		align-items: center;
		gap: 4px;
		margin-left: auto;
	}

	.volume-control {
		display: flex;
		align-items: center;
		gap: 6px;
	}

	.volume-slider {
		width: 100px;
	}
</style>
