<script lang="ts">
	import { settingsStore } from '$lib/stores/settings.svelte';
</script>

<div class="theme-settings">
	<h3 class="section-title">Appearance</h3>
	<p class="section-desc">Choose your preferred color theme.</p>

	<div class="theme-options">
		<button
			class="theme-card"
			class:active={settingsStore.theme === 'dark'}
			onclick={() => settingsStore.setTheme('dark')}
		>
			<div class="theme-preview dark-preview">
				<div class="preview-sidebar"></div>
				<div class="preview-content">
					<div class="preview-line"></div>
					<div class="preview-line short"></div>
				</div>
			</div>
			<span class="theme-label">Dark</span>
			{#if settingsStore.theme === 'dark'}
				<span class="theme-check">&#10003;</span>
			{/if}
		</button>

		<button
			class="theme-card"
			class:active={settingsStore.theme === 'light'}
			onclick={() => settingsStore.setTheme('light')}
		>
			<div class="theme-preview light-preview">
				<div class="preview-sidebar"></div>
				<div class="preview-content">
					<div class="preview-line"></div>
					<div class="preview-line short"></div>
				</div>
			</div>
			<span class="theme-label">Light</span>
			{#if settingsStore.theme === 'light'}
				<span class="theme-check">&#10003;</span>
			{/if}
		</button>

		<button
			class="theme-card"
			class:active={settingsStore.theme === 'system'}
			onclick={() => settingsStore.setTheme('system')}
		>
			<div class="theme-preview system-preview">
				<div class="system-dark-half">
					<div class="preview-line"></div>
					<div class="preview-line short"></div>
				</div>
				<div class="system-light-half">
					<div class="preview-line"></div>
					<div class="preview-line short"></div>
				</div>
			</div>
			<span class="theme-label">System</span>
			{#if settingsStore.theme === 'system'}
				<span class="theme-check">&#10003;</span>
			{/if}
		</button>
	</div>
</div>

<style>
	.theme-settings {
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

	.theme-options {
		display: flex;
		gap: 12px;
	}

	.theme-card {
		display: flex;
		flex-direction: column;
		align-items: center;
		gap: 8px;
		padding: 12px;
		background: none;
		border: 2px solid var(--color-border);
		border-radius: 6px;
		cursor: pointer;
		width: 140px;
		position: relative;
	}

	.theme-card:hover {
		border-color: var(--color-text-muted);
	}

	.theme-card.active {
		border-color: var(--color-accent);
	}

	.theme-preview {
		width: 100%;
		height: 70px;
		border-radius: 4px;
		display: flex;
		overflow: hidden;
	}

	.dark-preview {
		background-color: #1e1e1e;
	}

	.dark-preview .preview-sidebar {
		width: 30%;
		background-color: #252526;
		border-right: 1px solid #3e3e42;
	}

	.dark-preview .preview-content {
		flex: 1;
		padding: 8px;
		display: flex;
		flex-direction: column;
		gap: 4px;
	}

	.dark-preview .preview-line {
		height: 4px;
		background-color: #3e3e42;
		border-radius: 2px;
	}

	.dark-preview .preview-line.short {
		width: 60%;
	}

	.light-preview {
		background-color: #ffffff;
	}

	.light-preview .preview-sidebar {
		width: 30%;
		background-color: #f3f3f3;
		border-right: 1px solid #d4d4d4;
	}

	.light-preview .preview-content {
		flex: 1;
		padding: 8px;
		display: flex;
		flex-direction: column;
		gap: 4px;
	}

	.light-preview .preview-line {
		height: 4px;
		background-color: #d4d4d4;
		border-radius: 2px;
	}

	.light-preview .preview-line.short {
		width: 60%;
	}

	.system-preview {
		background: linear-gradient(to right, #1e1e1e 50%, #ffffff 50%);
	}

	.system-dark-half {
		width: 50%;
		padding: 8px;
		display: flex;
		flex-direction: column;
		gap: 4px;
	}

	.system-dark-half .preview-line {
		height: 4px;
		background-color: #3e3e42;
		border-radius: 2px;
	}

	.system-dark-half .preview-line.short {
		width: 60%;
	}

	.system-light-half {
		width: 50%;
		padding: 8px;
		display: flex;
		flex-direction: column;
		gap: 4px;
	}

	.system-light-half .preview-line {
		height: 4px;
		background-color: #d4d4d4;
		border-radius: 2px;
	}

	.system-light-half .preview-line.short {
		width: 60%;
	}

	.theme-label {
		font-size: 12px;
		color: var(--color-text-primary);
	}

	.theme-check {
		position: absolute;
		top: 4px;
		right: 8px;
		color: var(--color-accent);
		font-size: 14px;
	}
</style>
