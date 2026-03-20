<script lang="ts">
	import { open } from '@tauri-apps/plugin-dialog';
	import { settingsStore } from '$lib/stores/settings.svelte';

	async function handleAddFolder() {
		const selected = await open({ directory: true, multiple: false });
		if (selected) {
			await settingsStore.addMonitoredFolder(selected as string);
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

	<button class="add-folder-btn" onclick={handleAddFolder}>
		<svg width="14" height="14" viewBox="0 0 16 16" fill="currentColor">
			<path d="M14 7v1H8v6H7V8H1V7h6V1h1v6h6z" />
		</svg>
		Add Folder
	</button>
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
</style>
