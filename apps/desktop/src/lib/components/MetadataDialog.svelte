<script lang="ts">
	import type { Track } from '$lib/types';
	import { formatDuration, formatFileSize } from '$lib/types';
	import { invoke } from '@tauri-apps/api/core';
	import { libraryStore } from '$lib/stores/library.svelte';
	import ReclassifyPanel from './ReclassifyPanel.svelte';
	import { consolesStore } from '$lib/stores/consoles.svelte';

	let { tracks, onclose }: { tracks: Track[]; onclose: () => void } = $props();

	let isSingle = $derived(tracks.length === 1);
	let reclassifying = $state(false);
	// Bound so Apply can drive the column's half of the save. One button, both
	// halves: the tags and what game they belong to are two answers to the same
	// dialog, and two Save buttons stacked made the reader work out which one
	// owned which.
	let reclassifyPanel = $state<{
		save: () => Promise<boolean>;
		canSave: () => boolean;
		pending: () => { scope: 'folder' | 'track'; folders: number; tracks: number };
	} | null>(null);

	// The confirmation for a folder-wide change, in this footer rather than in
	// another window. A dialog that answers a question by opening a dialog is
	// the thing this column exists to avoid.
	let confirming = $state<{ folders: number; tracks: number } | null>(null);

	/// The formats that pack a whole game into one file, addressed by index.
	///
	/// Everything else is one song per file and already carries its own title,
	/// so offering to look one up would be offering nothing. NSFE is here even
	/// though its `tlbl` chunk names its tracks — a rip can leave that empty,
	/// and the listing is then still worth asking for.
	const MULTI_SUBSONG = ['gbs', 'nsf', 'nsfe', 'hes', 'kss', 'ay', 'sap'];
	let canFetchNames = $derived(
		tracks.length > 0 &&
			tracks.every((t) => MULTI_SUBSONG.includes((t.codec || '').toLowerCase()))
	);

	type Names = { file: string; subsongs: number; titles: string[]; problem: string | null };
	let names = $state<Names | null>(null);

	let askingNames = $state(false);
	let fetchingNames = $state(false);
	let namesApplied = $state<number | null>(null);

	async function fetchNames() {
		askingNames = false;
		fetchingNames = true;
		namesApplied = null;
		try {
			names = await invoke<Names>('suggest_track_names', { trackPath: firstTrack.path });
		} catch (e) {
			names = { file: '', subsongs: 0, titles: [], problem: String(e) };
		} finally {
			fetchingNames = false;
		}
	}

	/// `onlyIndex` null means the whole file. Either way the .m3u written beside
	/// it describes every subsong — a playlist with one line would leave the
	/// rest worse off than before.
	async function applyNames(onlyIndex: number | null) {
		if (!names || names.titles.length === 0) return;
		fetchingNames = true;
		try {
			namesApplied = await invoke<number>('apply_track_names', {
				file: names.file,
				titles: names.titles,
				onlyIndex
			});
			await libraryStore.loadTracks();
			names = null;
		} catch (e) {
			if (names) names = { ...names, problem: String(e) };
		} finally {
			fetchingNames = false;
		}
	}

	/// Which subsong the open dialog is showing, for the "only this one" case.
	let subsongIndex = $derived.by(() => {
		const m = firstTrack?.path.match(/#(\d+)$/);
		return m ? Number(m[1]) : null;
	});

	// Closing the column withdraws the question with it. Otherwise the footer
	// keeps asking to confirm a change that is no longer on the table, and
	// "Yes, apply" would save only the tags while claiming otherwise.
	$effect(() => {
		if (!reclassifying) confirming = null;
	});

	// What the files themselves say, when they agree. A selection spanning two
	// games has no single answer, and offering one of them would be a guess
	// wearing a fact's clothes.
	let headerGame = $derived.by(() => {
		const names = new Set(tracks.map((t) => t.header_game).filter(Boolean));
		return names.size === 1 ? [...names][0] : '';
	});
	let firstTrack = $derived(tracks[0]);

	/// Shown before the lookup, not after. The useful thing to confirm is not
	/// "are you sure" — nothing is written yet — but *what it is about to look
	/// up*: the game name comes from the classification, and if that is wrong
	/// this is the moment it is visible and one click from fixed.
	let lookupGame = $derived(firstTrack?.game?.trim() ?? '');
	let lookupConsole = $derived(
		consolesStore.definitions.find((c) => c.id === firstTrack?.console_id)?.name ?? ''
	);

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
		// A folder-wide reclassification reaches every track under the folder,
		// which is almost always more than was selected and can be hundreds. Ask
		// once, with the real numbers, before doing it.
		if (reclassifying && !confirming && reclassifyPanel?.canSave()) {
			const p = reclassifyPanel.pending();
			if (p.scope === 'folder') {
				confirming = { folders: p.folders, tracks: p.tracks };
				return;
			}
		}
		confirming = null;
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

			// The classification, if that column is open and has an answer. It
			// writes an override rather than a tag, so it cannot go through
			// `update_track_metadata` — but from here it is one Apply.
			if (reclassifying && reclassifyPanel?.canSave()) {
				await reclassifyPanel.save();
			}

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

</script>

<svelte:window onkeydown={handleKeydown} />

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="metadata-overlay" onclick={(e) => e.stopPropagation()} onmousedown={(e) => e.stopPropagation()}>
	<div class="metadata-dialog" class:wide={reclassifying}>
		<div class="columns">
		<div class="col-tags">
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

		{#if askingNames || names || namesApplied !== null}
			<!--
				A section of its own, with the tags hidden behind it. Grown inside a
				table cell this put a scrolling list of fifty names into a column two
				inches wide, beside the very field it was about to overwrite, and read
				as neither a list nor a form.
			-->
			<div class="names-panel">
				<div class="names-panel-head">
					<span class="names-panel-title">Track names</span>
					<button
						class="close-btn"
						onclick={() => {
							askingNames = false;
							names = null;
							namesApplied = null;
						}}
						aria-label="Back to the tags">✕</button
					>
				</div>
				<div class="names-panel-body">
			{#if askingNames}
				<div class="names">
					{#if lookupGame}
						<p class="names-problem">
							This file holds the whole game as numbered tracks, and their
							names are not in it. Tunante will ask
							<strong>zophar.net</strong> for the track list of
							<strong>{lookupGame}</strong>{#if lookupConsole}
								({lookupConsole}){/if}.
							<br /><br />
							Nothing is written yet — the list is shown first, and it is
							refused outright unless it has exactly as many entries as
							this file has tracks.
						</p>
						<div class="names-actions">
							<button class="btn btn-primary" onclick={fetchNames}>Look it up</button>
							<button class="btn btn-secondary" onclick={() => (askingNames = false)}
								>Cancel</button
							>
						</div>
					{:else}
						<p class="names-problem">
							Name the game first — the list is looked up by it. The
							column beside this one does that.
						</p>
						<div class="names-actions">
							<button class="btn btn-secondary" onclick={() => (askingNames = false)}
								>Close</button
							>
						</div>
					{/if}
				</div>
			{:else if names}
				<div class="names">
					{#if names.problem}
						<p class="names-problem">{names.problem}</p>
					{:else}
						<p class="names-head">
							{names.titles.length} names for this file, in order.
						</p>
						<ol class="names-list">
							{#each names.titles as t, i (i)}
								<li class:on={i === subsongIndex}>{t}</li>
							{/each}
						</ol>
						<div class="names-actions">
							{#if subsongIndex !== null}
								<button
									class="btn btn-secondary"
									onclick={() => applyNames(subsongIndex)}
									disabled={fetchingNames}
								>Only this track</button>
							{/if}
							<button
								class="btn btn-primary"
								onclick={() => applyNames(null)}
								disabled={fetchingNames}
							>All {names.titles.length}</button>
							<button class="btn btn-secondary" onclick={() => (names = null)}
								>Discard</button
							>
						</div>
					{/if}
				</div>
			{:else if namesApplied !== null}
				<p class="names-head">Renamed {namesApplied} track{namesApplied === 1 ? '' : 's'}.</p>
			{/if}
				</div>
			</div>
		{:else}
		<div class="metadata-body">
			<table class="metadata-table">
				<tbody>
					<tr>
						<td class="meta-label">Title</td>
						<td>
							<!--
								The lookup lives on this row because Title is the only
								field it writes, and this is the row you are looking at
								when a track is called "pokemon.gbs - Track 17".
							-->
							<div class="title-row">
								<input
									type="text"
									class="meta-input"
									bind:value={title}
									oninput={() => markTouched('title')}
									placeholder={titleDiffers ? '(Multiple values)' : ''}
								/>
								{#if canFetchNames}
									<button
										class="btn btn-secondary small"
										onclick={() => (askingNames = true)}
										disabled={fetchingNames || askingNames}
										title="This format holds the whole game in one file. Look the track names up."
									>
										{fetchingNames ? 'Looking…' : 'Get track names'}
									</button>
								{/if}
							</div>

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
		{/if}

		</div>

		{#if reclassifying}
			<div class="col-reclassify">
				<div class="col-head">
					<span class="col-title">Which videogame</span>
					<button
						class="close-btn"
						onclick={() => (reclassifying = false)}
						aria-label="Close the reclassify column">✕</button
					>
				</div>
				<ReclassifyPanel
					bind:this={reclassifyPanel}
					{tracks}
					embedded
					onclose={() => (reclassifying = false)}
				/>
			</div>
		{/if}
		</div>

		<div class="metadata-footer">
			{#if confirming}
				<span class="confirm-text">
					This will reclassify <strong>{confirming.tracks}</strong>
					track{confirming.tracks === 1 ? '' : 's'} across
					<strong>{confirming.folders}</strong>
					folder{confirming.folders === 1 ? '' : 's'}, not just the selection.
				</span>
				<button class="btn btn-secondary" onclick={() => (confirming = null)}>Back</button>
				<button class="btn btn-primary" onclick={handleSave} disabled={isSaving}>
					{isSaving ? 'Saving...' : 'Yes, apply'}
				</button>
			{:else}
			<!--
				On the left, away from Apply. This one does not edit the file's
				tags at all — it records what game the tracks belong to, which is
				a different thing from what is written in them, and putting it
				beside Apply would suggest otherwise.
			-->
			<button
				class="btn btn-secondary reclassify"
				onclick={() => (reclassifying = true)}
				title={headerGame
					? `The file's header says: ${headerGame}`
					: 'Say which game these tracks belong to'}
			>
				Reclassify as videogame…{#if headerGame}
					<span class="from-header">{headerGame}</span>{/if}
			</button>
			<span class="spacer"></span>
			<button class="btn btn-secondary" onclick={onclose}>Cancel</button>
			<button class="btn btn-primary" onclick={handleSave} disabled={isSaving}>
				{isSaving ? 'Saving...' : 'Apply'}
			</button>
			{/if}
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

	.metadata-dialog.wide {
		width: 1010px;
	}
	.columns {
		display: flex;
		min-height: 0;
		flex: 1;
	}
	.col-tags {
		flex: 1;
		min-width: 0;
		display: flex;
		flex-direction: column;
	}
	.col-reclassify {
		width: 460px;
		flex-shrink: 0;
		border-left: 1px solid var(--color-border);
		display: flex;
		flex-direction: column;
		min-height: 0;
	}
	.col-head {
		display: flex;
		align-items: center;
		justify-content: space-between;
		padding: 12px 16px;
		border-bottom: 1px solid var(--color-border);
		background-color: var(--color-bg-secondary);
	}
	.col-title {
		font-size: 13px;
		font-weight: 600;
		color: var(--color-text-primary);
	}
	.metadata-dialog {
		width: 550px;
		/* Animated so the column arriving reads as this dialog growing, rather
		   than as a different dialog appearing on top of it. */
		transition: width 140ms ease;
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

	.spacer {
		flex: 1;
	}
	.reclassify {
		display: inline-flex;
		align-items: baseline;
		gap: 6px;
	}
	.from-header {
		font-size: 11px;
		color: var(--color-text-muted);
		max-width: 180px;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.confirm-text {
		flex: 1;
		font-size: 12px;
		color: var(--color-text-secondary);
		line-height: 1.4;
		text-align: left;
	}
	.title-row {
		display: flex;
		gap: 8px;
		align-items: center;
	}
	.btn.small {
		font-size: 11px;
		padding: 3px 8px;
		white-space: nowrap;
		flex-shrink: 0;
	}
	.names-panel {
		display: flex;
		flex-direction: column;
		min-height: 0;
		flex: 1;
	}
	.names-panel-head {
		display: flex;
		align-items: center;
		justify-content: space-between;
		padding: 10px 16px;
		border-bottom: 1px solid var(--color-border);
	}
	.names-panel-title {
		font-size: 13px;
		font-weight: 600;
		color: var(--color-text-primary);
	}
	.names-panel-body {
		padding: 14px 16px;
		overflow-y: auto;
	}
	/* No border and no inset: this used to be a box inside a table cell, and
	   the panel around it is the box now. */
	.names {
		margin: 0;
	}
	.names-head {
		margin: 0 0 10px;
		font-size: 13px;
		color: var(--color-text-secondary);
		line-height: 1.5;
	}
	.names-problem {
		margin: 0 0 10px;
		font-size: 13px;
		color: var(--color-text-secondary);
		line-height: 1.45;
	}
	/* Scrolls rather than growing: some of these run to fifty entries, and a
	   dialog that resizes to fit one is a dialog that jumps. */
	.names-list {
		margin: 0 0 12px;
		padding-left: 26px;
		max-height: 260px;
		overflow-y: auto;
		font-size: 12px;
		color: var(--color-text-secondary);
	}
	.names-list li.on {
		color: var(--color-text-primary);
		font-weight: 600;
	}
	.names-actions {
		display: flex;
		gap: 6px;
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
