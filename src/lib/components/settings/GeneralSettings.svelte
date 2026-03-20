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
</style>
