<script lang="ts">
	import { libraryStore } from '$lib/stores/library.svelte';
</script>

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
</style>
