<script lang="ts">
	import { settingsStore } from '$lib/stores/settings.svelte';
	import { invoke } from '@tauri-apps/api/core';

	let desktopEntryPath = $state('');
	let showDesktopModal = $state(false);
	let desktopResult = $state('');
	let isLinux = $state(false);
	const isMacOS = navigator.platform.startsWith('Mac');

	// Check if we're on Linux and get the .desktop path
	$effect(() => {
		invoke<string>('get_desktop_entry_path').then((path) => {
			if (path) {
				desktopEntryPath = path;
				isLinux = true;
			}
		}).catch(() => {});
	});

	async function handleCreateDesktopEntry() {
		try {
			const path = await invoke<string>('create_desktop_entry');
			desktopResult = `Desktop entry created at ${path}`;
			showDesktopModal = false;
		} catch (e) {
			desktopResult = `Error: ${e}`;
			showDesktopModal = false;
		}
	}
</script>

<div class="general-settings">
	<h3 class="section-title">General</h3>

	<label class="setting-row">
		<input
			type="checkbox"
			checked={settingsStore.showTrackInTitlebar}
			onchange={(e) =>
				settingsStore.setShowTrackInTitlebar((e.target as HTMLInputElement).checked)}
		/>
		<div class="setting-text">
			<span class="setting-label">Show track in main titlebar</span>
			<span class="setting-desc"
				>Display the current playing track name in the window title bar.</span
			>
		</div>
	</label>

	<label class="setting-row">
		<input
			type="checkbox"
			checked={settingsStore.keepFavsInMetadata}
			onchange={(e) =>
				settingsStore.setKeepFavsInMetadata((e.target as HTMLInputElement).checked)}
		/>
		<div class="setting-text">
			<span class="setting-label">Keep song favs in metadata</span>
			<span class="setting-desc"
				>Write rating changes to the audio file's metadata tags. When off, ratings are only saved in the local database.</span
			>
		</div>
	</label>

	<label class="setting-row">
		<input
			type="checkbox"
			checked={settingsStore.showInTray}
			onchange={(e) =>
				settingsStore.setShowInTray((e.target as HTMLInputElement).checked)}
		/>
		<div class="setting-text">
			<span class="setting-label">Show in system tray</span>
			<span class="setting-desc"
				>Display the Tunante icon in the system tray / notification area.</span
			>
		</div>
	</label>

	<label class="setting-row" class:disabled={!settingsStore.showInTray}>
		<input
			type="checkbox"
			checked={settingsStore.closeToTray}
			disabled={!settingsStore.showInTray}
			onchange={(e) =>
				settingsStore.setCloseToTray((e.target as HTMLInputElement).checked)}
		/>
		<div class="setting-text">
			<span class="setting-label">Close to tray</span>
			<span class="setting-desc"
				>Minimize to system tray when closing the window instead of quitting the application.</span
			>
		</div>
	</label>

	{#if !isMacOS}
	<label class="setting-row">
		<input
			type="checkbox"
			checked={settingsStore.autoUpdateOnStart}
			onchange={(e) =>
				settingsStore.setAutoUpdateOnStart((e.target as HTMLInputElement).checked)}
		/>
		<div class="setting-text">
			<span class="setting-label">Auto-update on startup</span>
			<span class="setting-desc"
				>Automatically download and install updates when the app starts. No dialog shown.</span
			>
		</div>
	</label>

	<label class="setting-row" class:disabled={settingsStore.autoUpdateOnStart}>
		<input
			type="checkbox"
			checked={settingsStore.checkUpdatesOnStart}
			disabled={settingsStore.autoUpdateOnStart}
			onchange={(e) =>
				settingsStore.setCheckUpdatesOnStart((e.target as HTMLInputElement).checked)}
		/>
		<div class="setting-text">
			<span class="setting-label">Ask for updates on startup</span>
			<span class="setting-desc"
				>Show a dialog when a new version is available. You can skip specific versions. Disabled when auto-update is on.</span
			>
		</div>
	</label>
	{/if}

	<label class="setting-row">
		<input
			type="checkbox"
			checked={settingsStore.autoDownloadCoverArt}
			onchange={(e) =>
				settingsStore.setAutoDownloadCoverArt((e.target as HTMLInputElement).checked)}
		/>
		<div class="setting-text">
			<span class="setting-label">Auto-download missing cover art</span>
			<span class="setting-desc"
				>Search iTunes for album artwork when no local cover is found. Downloaded covers are cached locally.</span
			>
		</div>
	</label>

	<label class="setting-row" class:disabled={!settingsStore.autoDownloadCoverArt}>
		<input
			type="checkbox"
			checked={settingsStore.storeCoversInFolder}
			disabled={!settingsStore.autoDownloadCoverArt}
			onchange={(e) =>
				settingsStore.setStoreCoversInFolder((e.target as HTMLInputElement).checked)}
		/>
		<div class="setting-text">
			<span class="setting-label">Store covers in folder</span>
			<span class="setting-desc"
				>Save downloaded cover art as cover.jpg in the track's folder for future offline access.</span
			>
		</div>
	</label>

	{#if isLinux}
		<div class="setting-action">
			<button class="action-btn" onclick={() => { desktopResult = ''; showDesktopModal = true; }}>
				Create .desktop entry
			</button>
			{#if desktopResult}
				<span class="action-result" class:error={desktopResult.startsWith('Error')}>{desktopResult}</span>
			{/if}
		</div>
	{/if}
</div>

{#if showDesktopModal}
	<div class="modal-overlay" role="dialog">
		<div class="modal-dialog">
			<h3 class="modal-title">Create Desktop Entry</h3>
			<p class="modal-text">A desktop entry will be created in:</p>
			<code class="modal-path">{desktopEntryPath}</code>
			<div class="modal-buttons">
				<button class="btn primary" onclick={handleCreateDesktopEntry}>Continue</button>
				<button class="btn" onclick={() => showDesktopModal = false}>Cancel</button>
			</div>
		</div>
	</div>
{/if}

<style>
	.general-settings {
		display: flex;
		flex-direction: column;
		gap: 16px;
	}

	.section-title {
		font-size: 14px;
		font-weight: 600;
		color: var(--color-text-primary);
		margin: 0;
	}

	.setting-row {
		display: flex;
		align-items: flex-start;
		gap: 10px;
		cursor: pointer;
		padding: 8px;
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

	.setting-label {
		font-size: 13px;
		color: var(--color-text-primary);
	}

	.setting-desc {
		font-size: 11px;
		color: var(--color-text-secondary);
	}

	.setting-row.disabled {
		opacity: 0.45;
		cursor: not-allowed;
	}

	.setting-row.disabled input[type='checkbox'] {
		cursor: not-allowed;
	}

	.setting-action {
		display: flex;
		align-items: center;
		gap: 12px;
		padding: 8px;
	}

	.action-btn {
		padding: 6px 16px;
		border: 1px solid var(--color-border);
		border-radius: 4px;
		background: none;
		color: var(--color-text-primary);
		font-size: 13px;
		cursor: pointer;
	}

	.action-btn:hover {
		background-color: var(--color-bg-hover);
	}

	.action-result {
		font-size: 11px;
		color: var(--color-accent);
	}

	.action-result.error {
		color: #f44336;
	}

	.modal-overlay {
		position: fixed;
		inset: 0;
		z-index: 300;
		background-color: rgba(0, 0, 0, 0.6);
		display: flex;
		align-items: center;
		justify-content: center;
	}

	.modal-dialog {
		background-color: var(--color-bg-primary);
		border: 1px solid var(--color-border);
		border-radius: 8px;
		padding: 24px;
		width: 420px;
		max-width: 90vw;
		box-shadow: 0 8px 32px rgba(0, 0, 0, 0.5);
	}

	.modal-title {
		margin: 0 0 8px;
		font-size: 16px;
		font-weight: 600;
		color: var(--color-text-primary);
	}

	.modal-text {
		margin: 0 0 8px;
		font-size: 13px;
		color: var(--color-text-secondary);
	}

	.modal-path {
		display: block;
		margin: 0 0 16px;
		padding: 8px 12px;
		background-color: var(--color-bg-secondary);
		border-radius: 4px;
		font-size: 12px;
		color: var(--color-text-primary);
		word-break: break-all;
	}

	.modal-buttons {
		display: flex;
		gap: 8px;
	}

	.btn {
		padding: 6px 16px;
		border: 1px solid var(--color-border);
		border-radius: 4px;
		background: none;
		color: var(--color-text-primary);
		font-size: 13px;
		cursor: pointer;
	}

	.btn:hover {
		background-color: var(--color-bg-hover);
	}

	.btn.primary {
		background-color: var(--color-accent);
		border-color: var(--color-accent);
		color: white;
	}

	.btn.primary:hover {
		opacity: 0.9;
	}
</style>
