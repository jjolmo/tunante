<script lang="ts">
	import { settingsStore } from '$lib/stores/settings.svelte';
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

	<label class="setting-row">
		<input
			type="checkbox"
			checked={settingsStore.showCoverArt}
			onchange={(e) =>
				settingsStore.setShowCoverArt((e.target as HTMLInputElement).checked)}
		/>
		<div class="setting-text">
			<span class="setting-label">Show cover art</span>
			<span class="setting-desc"
				>Display album artwork in the sidebar when a track is playing.</span
			>
		</div>
	</label>
</div>

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
</style>
