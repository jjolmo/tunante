<script lang="ts">
	import { invoke } from '@tauri-apps/api/core';
	import { listen } from '@tauri-apps/api/event';
	import { open } from '@tauri-apps/plugin-dialog';
	import { onMount } from 'svelte';
	import { settingsStore, RATING_SOURCE_LABELS } from '$lib/stores/settings.svelte';
	import { libraryStore } from '$lib/stores/library.svelte';

	let isResyncing = $state(false);
	let addError = $state<string | null>(null);

	function formatMinutes(total: number): string {
		const m = Math.floor(total / 60);
		const s = total % 60;
		return m > 0 ? `${m}:${String(s).padStart(2, '0')}` : `${s}s`;
	}

	onMount(() => {
		const unlisten = listen('scan-complete', () => {
			isResyncing = false;
		});
		return () => {
			unlisten.then((fn) => fn());
		};
	});

	async function handleAddFolder() {
		const selected = await open({ directory: true, multiple: false });
		if (selected) {
			addError = null;
			try {
				await settingsStore.addMonitoredFolder(selected as string);
			} catch (e) {
				addError = typeof e === 'string' ? e : 'Failed to add folder';
			}
		}
	}

	async function handleResync() {
		isResyncing = true;
		libraryStore.isScanning = true;
		try {
			await invoke('resync_library');
		} catch (e) {
			console.error('Failed to resync library:', e);
			isResyncing = false;
			libraryStore.isScanning = false;
		}
	}
</script>

<div class="library-settings">
	<h3 class="section-title">Monitored Folders</h3>
	<p class="section-desc">
		Folders listed below will be scanned for audio files. Enable watching to auto-detect changes.
	</p>

	<div class="folders-list">
		{#each settingsStore.monitoredFolders as folder}
			<div class="folder-item">
				<div class="folder-info">
					<span class="folder-path" title={folder.path}>{folder.path}</span>
					<span class="folder-meta">
						{#if folder.watching_enabled}
							<span class="watch-badge active">Watching</span>
						{:else}
							<span class="watch-badge">Not watching</span>
						{/if}
					</span>
				</div>
				<div class="folder-actions">
					<label class="toggle-label">
						<input
							type="checkbox"
							checked={folder.watching_enabled}
							onchange={() =>
								settingsStore.toggleFolderWatching(folder.id, !folder.watching_enabled)}
						/>
						<span class="toggle-text">Watch</span>
					</label>
					<button
						class="remove-btn"
						onclick={() => settingsStore.removeMonitoredFolder(folder.id)}
						title="Remove folder"
						aria-label="Remove folder"
					>
						<svg width="14" height="14" viewBox="0 0 16 16" fill="currentColor">
							<path
								d="M8 8.707l3.646 3.647.708-.707L8.707 8l3.647-3.646-.707-.708L8 7.293 4.354 3.646l-.708.708L7.293 8l-3.647 3.646.708.708L8 8.707z"
							/>
						</svg>
					</button>
				</div>
			</div>
		{:else}
			<div class="empty-folders">
				<p>No monitored folders configured.</p>
			</div>
		{/each}
	</div>

	{#if addError}
		<p class="add-error">{addError}</p>
	{/if}

	<button class="add-folder-btn" onclick={handleAddFolder}>
		<svg width="14" height="14" viewBox="0 0 16 16" fill="currentColor">
			<path d="M14 7v1H8v6H7V8H1V7h6V1h1v6h6z" />
		</svg>
		Add Folder
	</button>

	<label class="setting-row">
		<input
			type="checkbox"
			checked={settingsStore.fastScan}
			onchange={(e) =>
				settingsStore.setFastScan((e.target as HTMLInputElement).checked)}
		/>
		<div class="setting-text">
			<span class="setting-label">Fast scan</span>
			<span class="setting-desc">Skip silence detection for chiptune tracks without duration info. Faster scan but shows default 2:40 for SFX/jingles.</span>
		</div>
	</label>

	<div class="loop-section">
		<h3 class="section-title">Endless tracks</h3>
		<p class="section-desc">
			How long to play tracks that loop forever and never end on their own. Tracks with a
			real length &mdash; from their own tags, or from a length in the folder&rsquo;s
			<code>.m3u</code> &mdash; keep it and ignore this.
		</p>
		<label class="loop-row">
			<input
				type="number"
				min="5"
				max="3600"
				step="5"
				value={settingsStore.loopMaxSeconds}
				onchange={(e) =>
					settingsStore.setLoopMaxSeconds(Number((e.target as HTMLInputElement).value))}
			/>
			<span class="loop-unit">seconds ({formatMinutes(settingsStore.loopMaxSeconds)})</span>
		</label>
		<p class="section-desc loop-note">
			Applied while scanning, so tracks already in the library keep their current length
			until you resync below. A 10 s fade-out is added on top.
		</p>
	</div>

	<div class="rating-section">
		<h3 class="section-title">Rating storage priority</h3>
		<p class="section-desc">
			Order used to <strong>read</strong> and <strong>save</strong> ratings. The first
			destination wins; if it can't hold the rating (an NSF has no writable tag area, for
			instance) the next one is used.
		</p>
		<ol class="rating-list">
			{#each settingsStore.ratingPriority as source, i (source)}
				<li class="rating-item">
					<span class="rating-pos">{i + 1}</span>
					<span class="rating-label">{RATING_SOURCE_LABELS[source]}</span>
					<span class="rating-actions">
						<button
							class="move-btn"
							title="Move up"
							disabled={i === 0}
							onclick={() => settingsStore.moveRatingSourceUp(i)}>▲</button
						>
						<button
							class="move-btn"
							title="Move down"
							disabled={i === settingsStore.ratingPriority.length - 1}
							onclick={() => settingsStore.moveRatingSourceDown(i)}>▼</button
						>
					</span>
				</li>
			{/each}
		</ol>
		<p class="section-desc rating-note">
			Putting <em>the file itself</em> or <em>the folder file</em> first makes listing slower:
			the app has to read from disk instead of the database.
		</p>
	</div>

	<div class="resync-section">
		<h3 class="section-title">Resync Library</h3>
		<p class="section-desc">
			Clear the database and rescan all monitored folders. Use this if new formats aren't showing up.
		</p>
		<button
			class="resync-btn"
			onclick={handleResync}
			disabled={isResyncing || libraryStore.isScanning}
		>
			<svg width="14" height="14" viewBox="0 0 16 16" fill="currentColor">
				<path d="M13.451 5.609l-.579-.939-1.068.812-.076.094c-.335.415-.927 1.146-1.26 1.468a4.07 4.07 0 00-2.466-.817 4.07 4.07 0 00-4.076 4.076 4.07 4.07 0 004.076 4.076 4.07 4.07 0 003.98-3.226h-1.15a2.94 2.94 0 01-2.83 2.076 2.94 2.94 0 01-2.926-2.926 2.94 2.94 0 012.926-2.926c.896 0 1.694.41 2.228 1.05L9.478 9.09l-.074.1-.035.16.01.17.09.15.15.09.17.01h4.036l.16-.04.1-.09.09-.15.01-.17V5.289l-.04-.16-.09-.1-.15-.09-.17-.01-.16.04-.1.074-.463.57z" />
			</svg>
			{isResyncing ? 'Resyncing...' : 'Resync Library'}
		</button>
	</div>
</div>

<style>
	.library-settings {
		display: flex;
		flex-direction: column;
		gap: 12px;
	}

	.section-title {
		font-size: 14px;
		font-weight: 600;
		color: var(--color-text-primary);
		margin: 0;
	}

	.section-desc {
		font-size: 12px;
		color: var(--color-text-secondary);
		margin: 0;
	}

	.folders-list {
		display: flex;
		flex-direction: column;
		border: 1px solid var(--color-border);
		border-radius: 4px;
		overflow: hidden;
	}

	.folder-item {
		display: flex;
		align-items: center;
		justify-content: space-between;
		padding: 8px 12px;
		background-color: var(--color-bg-secondary);
		border-bottom: 1px solid var(--color-border);
	}

	.folder-item:last-child {
		border-bottom: none;
	}

	.folder-info {
		display: flex;
		flex-direction: column;
		gap: 2px;
		min-width: 0;
		flex: 1;
	}

	.folder-path {
		font-size: 12px;
		color: var(--color-text-primary);
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}

	.folder-meta {
		font-size: 11px;
	}

	.watch-badge {
		color: var(--color-text-muted);
		font-size: 10px;
		text-transform: uppercase;
		letter-spacing: 0.5px;
	}

	.watch-badge.active {
		color: var(--color-accent);
	}

	.folder-actions {
		display: flex;
		align-items: center;
		gap: 8px;
		flex-shrink: 0;
	}

	.toggle-label {
		display: flex;
		align-items: center;
		gap: 4px;
		font-size: 12px;
		color: var(--color-text-secondary);
		cursor: pointer;
	}

	.toggle-text {
		font-size: 11px;
	}

	.remove-btn {
		background: none;
		border: none;
		color: var(--color-text-muted);
		cursor: pointer;
		padding: 4px;
		border-radius: 3px;
		display: flex;
		align-items: center;
	}

	.remove-btn:hover {
		color: #e74c3c;
		background-color: var(--color-bg-hover);
	}

	.empty-folders {
		padding: 16px;
		text-align: center;
		color: var(--color-text-muted);
		font-size: 12px;
	}

	.add-folder-btn {
		display: flex;
		align-items: center;
		gap: 6px;
		padding: 8px 12px;
		background-color: var(--color-bg-tertiary);
		border: 1px solid var(--color-border);
		border-radius: 4px;
		color: var(--color-text-primary);
		cursor: pointer;
		font-size: 12px;
		align-self: flex-start;
	}

	.add-folder-btn:hover {
		background-color: var(--color-bg-hover);
	}

	.add-error {
		font-size: 12px;
		color: #e74c3c;
		margin: 0;
	}

	.resync-section {
		margin-top: 16px;
		padding-top: 16px;
		border-top: 1px solid var(--color-border);
		display: flex;
		flex-direction: column;
		gap: 8px;
	}

	.resync-btn {
		display: flex;
		align-items: center;
		gap: 6px;
		padding: 8px 12px;
		background-color: var(--color-bg-tertiary);
		border: 1px solid var(--color-border);
		border-radius: 4px;
		color: var(--color-text-primary);
		cursor: pointer;
		font-size: 12px;
		align-self: flex-start;
	}

	.resync-btn:hover:not(:disabled) {
		background-color: var(--color-bg-hover);
	}

	.resync-btn:disabled {
		opacity: 0.5;
		cursor: not-allowed;
	}

	.setting-row {
		display: flex;
		align-items: flex-start;
		gap: 10px;
		padding: 6px 8px;
		border-radius: 4px;
	}

	.setting-row:hover {
		background-color: var(--color-bg-hover);
	}

	.setting-row input[type='checkbox'] {
		margin-top: 2px;
		accent-color: var(--color-accent);
		cursor: pointer;
	}

	.setting-text {
		display: flex;
		flex-direction: column;
		gap: 2px;
	}

	.loop-section {
		margin-top: 16px;
	}

	.loop-row {
		display: flex;
		align-items: center;
		gap: 8px;
		margin-top: 8px;
	}

	.loop-row input {
		width: 80px;
		padding: 6px 8px;
		background-color: var(--color-bg-secondary);
		border: 1px solid var(--color-border);
		border-radius: 4px;
		color: var(--color-text);
	}

	.loop-unit {
		color: var(--color-text-secondary);
		font-size: 12px;
	}

	.loop-note {
		margin-top: 6px;
	}

	.rating-section {
		margin-top: 16px;
	}

	.rating-list {
		list-style: none;
		margin: 8px 0 0;
		padding: 0;
		display: flex;
		flex-direction: column;
		gap: 4px;
	}

	.rating-item {
		display: flex;
		align-items: center;
		gap: 8px;
		padding: 6px 8px;
		border: 1px solid var(--color-border);
		border-radius: 4px;
		background-color: var(--color-bg-secondary);
	}

	.rating-pos {
		flex: 0 0 18px;
		text-align: center;
		font-weight: bold;
		color: var(--color-accent);
	}

	.rating-label {
		flex: 1;
	}

	.rating-actions {
		display: flex;
		gap: 2px;
	}

	.move-btn {
		background: none;
		border: 1px solid var(--color-border);
		border-radius: 3px;
		color: var(--color-text);
		cursor: pointer;
		width: 22px;
		height: 22px;
		line-height: 1;
		font-size: 10px;
	}

	.move-btn:hover:not(:disabled) {
		background-color: var(--color-bg-hover);
	}

	.move-btn:disabled {
		opacity: 0.3;
		cursor: default;
	}

	.rating-note {
		margin-top: 6px;
	}

	.setting-label {
		font-size: 13px;
		color: var(--color-text-primary);
	}

	.setting-desc {
		font-size: 11px;
		color: var(--color-text-secondary);
	}
</style>
