<script lang="ts">
	import { libraryStore } from '$lib/stores/library.svelte';

	let showShortFilterPopover = $state(false);
	let popoverEl: HTMLDivElement | undefined = $state();
	let thresholdInput = $state(String(libraryStore.shortFilterThresholdSec));

	function toggleShortFilter() {
		libraryStore.setShortFilterEnabled(!libraryStore.shortFilterEnabled);
	}

	function handleThresholdContextMenu(e: MouseEvent) {
		e.preventDefault();
		showShortFilterPopover = !showShortFilterPopover;
	}

	function applyThreshold() {
		const n = parseInt(thresholdInput, 10);
		if (!isNaN(n) && n > 0) {
			libraryStore.setShortFilterThreshold(n);
		}
		showShortFilterPopover = false;
	}

	function handleThresholdKeydown(e: KeyboardEvent) {
		if (e.key === 'Enter') {
			applyThreshold();
		} else if (e.key === 'Escape') {
			showShortFilterPopover = false;
		}
	}

	// Close popover on click outside
	function handleWindowClick(e: MouseEvent) {
		if (showShortFilterPopover && popoverEl && !popoverEl.contains(e.target as Node)) {
			showShortFilterPopover = false;
		}
	}
</script>

<svelte:window onclick={handleWindowClick} />

<div class="search-bar">
	<svg class="search-icon" width="14" height="14" viewBox="0 0 16 16" fill="currentColor">
		<path
			d="M15.25 13.69l-3.77-3.77a5.54 5.54 0 10-1.56 1.56l3.77 3.77a1.1 1.1 0 001.56-1.56zM2 6.5A4.5 4.5 0 116.5 11 4.5 4.5 0 012 6.5z"
		/>
	</svg>
	<input
		type="text"
		placeholder="Search tracks..."
		value={libraryStore.searchQuery}
		oninput={(e) => libraryStore.setSearchQuery((e.target as HTMLInputElement).value)}
		class="search-input"
	/>
	{#if libraryStore.searchQuery}
		<button class="clear-btn" onclick={() => libraryStore.setSearchQuery('')} aria-label="Clear search">
			<svg width="12" height="12" viewBox="0 0 16 16" fill="currentColor">
				<path
					d="M8 8.707l3.646 3.647.708-.707L8.707 8l3.647-3.646-.707-.708L8 7.293 4.354 3.646l-.708.708L7.293 8l-3.647 3.646.708.708L8 8.707z"
				/>
			</svg>
		</button>
	{/if}

	<!-- Short track filter toggle -->
	<div class="short-filter-wrapper" bind:this={popoverEl}>
		<button
			class="short-filter-btn"
			class:active={libraryStore.shortFilterEnabled}
			onclick={toggleShortFilter}
			oncontextmenu={handleThresholdContextMenu}
			title={libraryStore.shortFilterEnabled
				? `Hiding tracks < ${libraryStore.shortFilterThresholdSec}s (right-click to configure)`
				: 'Filter short tracks (right-click to configure)'}
		>
			<!-- Clock/timer icon -->
			<svg width="13" height="13" viewBox="0 0 16 16" fill="currentColor">
				<path d="M8 1a7 7 0 110 14A7 7 0 018 1zm0 1.5a5.5 5.5 0 100 11 5.5 5.5 0 000-11zM8.5 4v4.25l2.85 1.65-.75 1.3L7.5 9V4h1z"/>
			</svg>
			{#if libraryStore.shortFilterEnabled}
				<span class="threshold-badge">{libraryStore.shortFilterThresholdSec}s</span>
			{/if}
		</button>

		{#if showShortFilterPopover}
			<!-- svelte-ignore a11y_no_static_element_interactions -->
			<div class="short-filter-popover" onclick={(e) => e.stopPropagation()}>
				<label class="popover-label" for="short-filter-threshold">Min. duration (seconds)</label>
				<div class="popover-row">
					<input
						id="short-filter-threshold"
						type="number"
						min="1"
						max="999"
						class="threshold-input"
						bind:value={thresholdInput}
						onkeydown={handleThresholdKeydown}
					/>
					<button class="popover-apply" onclick={applyThreshold}>OK</button>
				</div>
			</div>
		{/if}
	</div>
</div>

<style>
	.search-bar {
		display: flex;
		align-items: center;
		gap: 6px;
		padding: 6px 12px;
		background-color: var(--color-bg-secondary);
		border-bottom: 1px solid var(--color-border);
		flex-shrink: 0;
	}

	.search-icon {
		color: var(--color-text-muted);
		flex-shrink: 0;
	}

	.search-input {
		flex: 1;
		background: none;
		border: none;
		color: var(--color-text-primary);
		font-size: 12px;
		outline: none;
	}

	.search-input::placeholder {
		color: var(--color-text-muted);
	}

	.clear-btn {
		background: none;
		border: none;
		color: var(--color-text-muted);
		cursor: pointer;
		padding: 2px;
		border-radius: 3px;
		display: flex;
		align-items: center;
	}

	.clear-btn:hover {
		color: var(--color-text-primary);
		background-color: var(--color-bg-hover);
	}

	.short-filter-wrapper {
		position: relative;
		flex-shrink: 0;
	}

	.short-filter-btn {
		background: none;
		border: 1px solid transparent;
		color: var(--color-text-muted);
		cursor: pointer;
		padding: 2px 4px;
		border-radius: 3px;
		display: flex;
		align-items: center;
		gap: 3px;
		font-size: 10px;
		font-weight: 600;
	}

	.short-filter-btn:hover {
		color: var(--color-text-primary);
		background-color: var(--color-bg-hover);
	}

	.short-filter-btn.active {
		color: var(--color-accent);
		border-color: var(--color-accent);
		background-color: rgba(var(--color-accent-rgb, 66, 153, 225), 0.1);
	}

	.threshold-badge {
		font-size: 9px;
		line-height: 1;
	}

	.short-filter-popover {
		position: absolute;
		top: 100%;
		right: 0;
		margin-top: 4px;
		background-color: var(--color-bg-tertiary);
		border: 1px solid var(--color-border);
		border-radius: 6px;
		padding: 8px;
		z-index: 100;
		box-shadow: 0 4px 12px rgba(0, 0, 0, 0.3);
		min-width: 160px;
	}

	.popover-label {
		display: block;
		font-size: 11px;
		color: var(--color-text-secondary);
		margin-bottom: 6px;
	}

	.popover-row {
		display: flex;
		gap: 4px;
	}

	.threshold-input {
		flex: 1;
		background-color: var(--color-bg-primary);
		border: 1px solid var(--color-border);
		border-radius: 3px;
		color: var(--color-text-primary);
		font-size: 12px;
		padding: 3px 6px;
		width: 60px;
		outline: none;
	}

	.threshold-input:focus {
		border-color: var(--color-accent);
	}

	.popover-apply {
		background-color: var(--color-accent);
		border: none;
		border-radius: 3px;
		color: white;
		font-size: 11px;
		font-weight: 600;
		padding: 3px 10px;
		cursor: pointer;
	}

	.popover-apply:hover {
		opacity: 0.9;
	}
</style>
