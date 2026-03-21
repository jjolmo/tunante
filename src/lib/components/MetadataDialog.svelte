<script lang="ts">
	import type { Track } from '$lib/types';
	import { formatDuration, formatFileSize } from '$lib/types';
	import { invoke } from '@tauri-apps/api/core';
	import { libraryStore } from '$lib/stores/library.svelte';

	let { tracks, onclose }: { tracks: Track[]; onclose: () => void } = $props();

	let isSingle = $derived(tracks.length === 1);
	let firstTrack = $derived(tracks[0]);

	// Editable fields (initialized from track data)
	let title = $state('');
	let artist = $state('');
	let album = $state('');
	let albumArtist = $state('');
	let trackNumber = $state('');
	let discNumber = $state('');

	// For multi-select: detect which fields differ
	let titleDiffers = $derived(!isSingle && new Set(tracks.map((t) => t.title)).size > 1);
	let artistDiffers = $derived(!isSingle && new Set(tracks.map((t) => t.artist)).size > 1);
	let albumDiffers = $derived(!isSingle && new Set(tracks.map((t) => t.album)).size > 1);
	let albumArtistDiffers = $derived(!isSingle && new Set(tracks.map((t) => t.album_artist)).size > 1);

	// Initialize fields from tracks on mount
	let initialized = false;
	$effect(() => {
		if (initialized || !firstTrack) return;
		initialized = true;

		if (isSingle) {
			title = firstTrack.title;
			artist = firstTrack.artist;
			album = firstTrack.album;
			albumArtist = firstTrack.album_artist;
			trackNumber = firstTrack.track_number !== null ? String(firstTrack.track_number) : '';
			discNumber = firstTrack.disc_number !== null ? String(firstTrack.disc_number) : '';
		} else {
			// For multi-select with same values, pre-fill
			if (!titleDiffers) title = tracks[0].title;
			if (!artistDiffers) artist = tracks[0].artist;
			if (!albumDiffers) album = tracks[0].album;
			if (!albumArtistDiffers) albumArtist = tracks[0].album_artist;
		}
	});

	// Track which fields have been touched by the user
	let touchedFields = $state(new Set<string>());

	function markTouched(field: string) {
		touchedFields = new Set([...touchedFields, field]);
	}

	// Read-only metadata rows for single track
	interface MetaRow {
		label: string;
		value: string;
	}

	let readOnlyRows = $derived<MetaRow[]>(
		isSingle && firstTrack
			? [
					{ label: 'Path', value: firstTrack.path },
					{ label: 'Duration', value: formatDuration(firstTrack.duration_ms) },
					{ label: 'Codec', value: firstTrack.codec },
					{
						label: 'Sample Rate',
						value: firstTrack.sample_rate ? `${firstTrack.sample_rate} Hz` : 'N/A'
					},
					{ label: 'Channels', value: firstTrack.channels ? String(firstTrack.channels) : 'N/A' },
					{
						label: 'Bitrate',
						value: firstTrack.bitrate ? `${Math.round(firstTrack.bitrate / 1000)} kbps` : 'N/A'
					},
					{ label: 'File Size', value: formatFileSize(firstTrack.file_size) },
					{ label: 'Rating', value: firstTrack.rating > 0 ? `${firstTrack.rating}/5` : 'None' }
				]
			: [{ label: 'Tracks', value: `${tracks.length} tracks selected` }]
	);

	let isSaving = $state(false);

	async function handleSave() {
		isSaving = true;
		try {
			const fields: Record<string, string | number | null> = {};

			if (isSingle || touchedFields.has('title')) fields.title = title;
			if (isSingle || touchedFields.has('artist')) fields.artist = artist;
			if (isSingle || touchedFields.has('album')) fields.album = album;
			if (isSingle || touchedFields.has('album_artist')) fields.album_artist = albumArtist;
			if (isSingle || touchedFields.has('track_number')) {
				fields.track_number = trackNumber ? parseInt(trackNumber) : null;
			}
			if (isSingle || touchedFields.has('disc_number')) {
				fields.disc_number = discNumber ? parseInt(discNumber) : null;
			}

			const trackIds = tracks.map((t) => t.id);
			await invoke('update_track_metadata', { trackIds, fields });
			await libraryStore.loadTracks();
			onclose();
		} catch (e) {
			console.error('Failed to update metadata:', e);
		} finally {
			isSaving = false;
		}
	}

	function handleKeydown(e: KeyboardEvent) {
		if (e.key === 'Escape') onclose();
	}

	function handleBackdropClick(e: MouseEvent) {
		if (e.target === e.currentTarget) onclose();
	}
</script>

<svelte:window onkeydown={handleKeydown} />

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="metadata-overlay" onclick={handleBackdropClick}>
	<div class="metadata-dialog">
		<div class="metadata-header">
			<span class="metadata-title">
				{isSingle ? 'Track Properties' : `Properties (${tracks.length} tracks)`}
			</span>
			<button class="close-btn" onclick={onclose} aria-label="Close">
				<svg width="16" height="16" viewBox="0 0 16 16" fill="currentColor">
					<path
						d="M8 8.707l3.646 3.647.708-.707L8.707 8l3.647-3.646-.707-.708L8 7.293 4.354 3.646l-.708.708L7.293 8l-3.647 3.646.708.708L8 8.707z"
					/>
				</svg>
			</button>
		</div>

		<div class="metadata-body">
			<table class="metadata-table">
				<tbody>
					<tr>
						<td class="meta-label">Title</td>
						<td>
							<input
								type="text"
								class="meta-input"
								bind:value={title}
								oninput={() => markTouched('title')}
								placeholder={titleDiffers ? '(Multiple values)' : ''}
							/>
						</td>
					</tr>
					<tr>
						<td class="meta-label">Artist</td>
						<td>
							<input
								type="text"
								class="meta-input"
								bind:value={artist}
								oninput={() => markTouched('artist')}
								placeholder={artistDiffers ? '(Multiple values)' : ''}
							/>
						</td>
					</tr>
					<tr>
						<td class="meta-label">Album</td>
						<td>
							<input
								type="text"
								class="meta-input"
								bind:value={album}
								oninput={() => markTouched('album')}
								placeholder={albumDiffers ? '(Multiple values)' : ''}
							/>
						</td>
					</tr>
					<tr>
						<td class="meta-label">Album Artist</td>
						<td>
							<input
								type="text"
								class="meta-input"
								bind:value={albumArtist}
								oninput={() => markTouched('album_artist')}
								placeholder={albumArtistDiffers ? '(Multiple values)' : ''}
							/>
						</td>
					</tr>
					<tr>
						<td class="meta-label">Track #</td>
						<td>
							<input
								type="text"
								class="meta-input small"
								bind:value={trackNumber}
								oninput={() => markTouched('track_number')}
								placeholder={!isSingle ? '(Multiple values)' : ''}
							/>
						</td>
					</tr>
					<tr>
						<td class="meta-label">Disc #</td>
						<td>
							<input
								type="text"
								class="meta-input small"
								bind:value={discNumber}
								oninput={() => markTouched('disc_number')}
								placeholder={!isSingle ? '(Multiple values)' : ''}
							/>
						</td>
					</tr>

					{#each readOnlyRows as row}
						<tr>
							<td class="meta-label">{row.label}</td>
							<td class="meta-value">{row.value}</td>
						</tr>
					{/each}
				</tbody>
			</table>
		</div>

		<div class="metadata-footer">
			<button class="btn btn-secondary" onclick={onclose}>Cancel</button>
			<button class="btn btn-primary" onclick={handleSave} disabled={isSaving}>
				{isSaving ? 'Saving...' : 'Apply'}
			</button>
		</div>
	</div>
</div>

<style>
	.metadata-overlay {
		position: fixed;
		inset: 0;
		z-index: 200;
		background-color: rgba(0, 0, 0, 0.5);
		display: flex;
		align-items: center;
		justify-content: center;
	}

	.metadata-dialog {
		width: 550px;
		max-width: 90vw;
		max-height: 80vh;
		background-color: var(--color-bg-primary);
		border: 1px solid var(--color-border);
		border-radius: 6px;
		display: flex;
		flex-direction: column;
		overflow: hidden;
		box-shadow: 0 8px 32px rgba(0, 0, 0, 0.4);
	}

	.metadata-header {
		display: flex;
		align-items: center;
		justify-content: space-between;
		padding: 12px 16px;
		border-bottom: 1px solid var(--color-border);
		background-color: var(--color-bg-secondary);
	}

	.metadata-title {
		font-size: 14px;
		font-weight: 600;
		color: var(--color-text-primary);
	}

	.close-btn {
		background: none;
		border: none;
		color: var(--color-text-secondary);
		cursor: pointer;
		padding: 4px;
		border-radius: 3px;
		display: flex;
		align-items: center;
	}

	.close-btn:hover {
		color: var(--color-text-primary);
		background-color: var(--color-bg-hover);
	}

	.metadata-body {
		flex: 1;
		overflow-y: auto;
		padding: 16px;
	}

	.metadata-table {
		width: 100%;
		border-collapse: collapse;
	}

	.metadata-table tr {
		border-bottom: 1px solid var(--color-border);
	}

	.metadata-table tr:last-child {
		border-bottom: none;
	}

	.metadata-table td {
		padding: 6px 8px;
		font-size: 12px;
		vertical-align: middle;
	}

	.meta-label {
		width: 100px;
		color: var(--color-text-secondary);
		font-weight: 500;
		white-space: nowrap;
	}

	.meta-value {
		color: var(--color-text-primary);
		word-break: break-all;
	}

	.meta-input {
		width: 100%;
		padding: 4px 8px;
		background-color: var(--color-bg-tertiary);
		border: 1px solid var(--color-border);
		border-radius: 3px;
		color: var(--color-text-primary);
		font-size: 12px;
		outline: none;
	}

	.meta-input:focus {
		border-color: var(--color-accent);
	}

	.meta-input.small {
		width: 80px;
	}

	.meta-input::placeholder {
		color: var(--color-text-muted);
		font-style: italic;
	}

	.metadata-footer {
		display: flex;
		justify-content: flex-end;
		gap: 8px;
		padding: 12px 16px;
		border-top: 1px solid var(--color-border);
		background-color: var(--color-bg-secondary);
	}

	.btn {
		padding: 6px 16px;
		border-radius: 4px;
		font-size: 12px;
		cursor: pointer;
		border: 1px solid var(--color-border);
	}

	.btn:disabled {
		opacity: 0.5;
		cursor: default;
	}

	.btn-secondary {
		background: none;
		color: var(--color-text-primary);
	}

	.btn-secondary:hover:not(:disabled) {
		background-color: var(--color-bg-hover);
	}

	.btn-primary {
		background-color: var(--color-accent);
		color: white;
		border-color: var(--color-accent);
	}

	.btn-primary:hover:not(:disabled) {
		background-color: var(--color-accent-hover);
	}
</style>
