<script lang="ts">
	import { settingsStore } from '$lib/stores/settings.svelte';
	import GeneralSettings from './GeneralSettings.svelte';
	import LibrarySettings from './LibrarySettings.svelte';
	import ThemeSettings from './ThemeSettings.svelte';
	import ShortcutsSettings from './ShortcutsSettings.svelte';
	import AboutSettings from './AboutSettings.svelte';

	interface Category {
		id: string;
		label: string;
		icon: string;
	}

	const categories: Category[] = [
		{
			id: 'general',
			label: 'General',
			icon: 'M9.1 4.4L8.6 2H7.4l-.5 2.4-.7.3-2-1.3-.9.8 1.3 2-.2.7-2.4.5v1.2l2.4.5.3.7-1.3 2 .8.8 2-1.3.7.3.5 2.4h1.2l.5-2.4.7-.3 2 1.3.8-.8-1.3-2 .3-.7 2.4-.5V7.4l-2.4-.5-.3-.7 1.3-2-.8-.8-2 1.3-.7-.3zM8 10a2 2 0 110-4 2 2 0 010 4z'
		},
		{
			id: 'library',
			label: 'Library',
			icon: 'M14.5 3H7.71l-.85-.85L6.51 2H1.5l-.5.5v11l.5.5h13l.5-.5v-10L14.5 3zm-.51 8.49V13H2V7h5.29l.85.85.36.15H14v3.49zM2 3h4.29l.85.85.36.15H14v2H8.5l-.85-.85L7.29 5H2V3z'
		},
		{
			id: 'theme',
			label: 'Appearance',
			icon: 'M8 1a7 7 0 100 14A7 7 0 008 1zm0 13A6 6 0 018 2v12z'
		},
		{
			id: 'shortcuts',
			label: 'Shortcuts',
			icon: 'M14 3H2v10h12V3zM1 2.5l.5-.5h13l.5.5v11l-.5.5H1.5l-.5-.5v-11zM4 6h1v1H4V6zm0 2h1v3H4V8zm7 3h1V8h-1v3zm0-4h1V6h-1v1zM6 6h4v1H6V6zm0 2h1v3H6V8zm2 0h2v3H8V8z'
		},
		{
			id: 'about',
			label: 'About',
			icon: 'M8 1a7 7 0 100 14A7 7 0 008 1zm0 2a5 5 0 110 10A5 5 0 018 3zm-.5 2.5h1v1h-1v-1zm0 2h1v4h-1v-4z'
		}
	];

	function handleBackdropClick(e: MouseEvent) {
		if (e.target === e.currentTarget) {
			settingsStore.closeSettings();
		}
	}

	function handleKeydown(e: KeyboardEvent) {
		if (e.key === 'Escape') {
			settingsStore.closeSettings();
		}
	}
</script>

<svelte:window onkeydown={handleKeydown} />

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="settings-overlay" onclick={handleBackdropClick}>
	<div class="settings-dialog">
		<div class="settings-header">
			<span class="settings-title">Settings</span>
			<button
				class="close-btn"
				onclick={() => settingsStore.closeSettings()}
				aria-label="Close settings"
			>
				<svg width="16" height="16" viewBox="0 0 16 16" fill="currentColor">
					<path
						d="M8 8.707l3.646 3.647.708-.707L8.707 8l3.647-3.646-.707-.708L8 7.293 4.354 3.646l-.708.708L7.293 8l-3.647 3.646.708.708L8 8.707z"
					/>
				</svg>
			</button>
		</div>
		<div class="settings-body">
			<nav class="settings-nav">
				{#each categories as cat}
					<button
						class="nav-item"
						class:active={settingsStore.activeCategory === cat.id}
						onclick={() => (settingsStore.activeCategory = cat.id)}
					>
						<svg width="16" height="16" viewBox="0 0 16 16" fill="currentColor">
							<path d={cat.icon} />
						</svg>
						<span>{cat.label}</span>
					</button>
				{/each}
			</nav>
			<div class="settings-content">
				{#if settingsStore.activeCategory === 'general'}
					<GeneralSettings />
				{:else if settingsStore.activeCategory === 'library'}
					<LibrarySettings />
				{:else if settingsStore.activeCategory === 'theme'}
					<ThemeSettings />
				{:else if settingsStore.activeCategory === 'shortcuts'}
					<ShortcutsSettings />
				{:else if settingsStore.activeCategory === 'about'}
					<AboutSettings />
				{/if}
			</div>
		</div>
	</div>
</div>

<style>
	.settings-overlay {
		position: fixed;
		inset: 0;
		z-index: 100;
		background-color: rgba(0, 0, 0, 0.5);
		display: flex;
		align-items: center;
		justify-content: center;
	}

	.settings-dialog {
		width: 850px;
		max-width: 90vw;
		height: 550px;
		max-height: 85vh;
		background-color: var(--color-bg-primary);
		border: 1px solid var(--color-border);
		border-radius: 6px;
		display: flex;
		flex-direction: column;
		overflow: hidden;
		box-shadow: 0 8px 32px rgba(0, 0, 0, 0.4);
	}

	.settings-header {
		display: flex;
		align-items: center;
		justify-content: space-between;
		padding: 12px 16px;
		border-bottom: 1px solid var(--color-border);
		background-color: var(--color-bg-secondary);
	}

	.settings-title {
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

	.settings-body {
		display: flex;
		flex: 1;
		min-height: 0;
	}

	.settings-nav {
		width: 180px;
		min-width: 180px;
		background-color: var(--color-bg-secondary);
		border-right: 1px solid var(--color-border);
		padding: 8px 0;
		overflow-y: auto;
	}

	.nav-item {
		display: flex;
		align-items: center;
		gap: 8px;
		width: 100%;
		padding: 8px 16px;
		background: none;
		border: none;
		color: var(--color-text-primary);
		cursor: pointer;
		text-align: left;
		font-size: 13px;
	}

	.nav-item:hover {
		background-color: var(--color-bg-hover);
	}

	.nav-item.active {
		background-color: var(--color-bg-selected);
	}

	.settings-content {
		flex: 1;
		padding: 16px;
		overflow-y: auto;
	}
</style>
