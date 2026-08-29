<script lang="ts">
	export interface ContextMenuItem {
		label: string;
		action: () => void;
		checked?: boolean;
		separator?: boolean;
		disabled?: boolean;
	}

	let { items, x, y, onclose }: { items: ContextMenuItem[]; x: number; y: number; onclose: () => void } = $props();

	let menuEl: HTMLDivElement | undefined = $state();

	$effect(() => {
		if (menuEl) {
			const rect = menuEl.getBoundingClientRect();
			const vw = window.innerWidth;
			const vh = window.innerHeight;
			if (rect.right > vw) {
				menuEl.style.left = `${vw - rect.width - 4}px`;
			}
			if (rect.bottom > vh) {
				menuEl.style.top = `${vh - rect.height - 4}px`;
			}
		}
	});

	function handleBackdropClick() {
		onclose();
	}

	function handleKeydown(e: KeyboardEvent) {
		if (e.key === 'Escape') {
			onclose();
		}
	}

	function handleItemClick(item: ContextMenuItem) {
		if (item.disabled) return;
		item.action();
		if (item.checked === undefined) {
			onclose();
		}
	}
</script>

<svelte:window onkeydown={handleKeydown} />

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="context-backdrop" onmousedown={handleBackdropClick}></div>
<div class="context-menu" bind:this={menuEl} style="left: {x}px; top: {y}px;">
	{#each items as item}
		{#if item.separator}
			<div class="separator"></div>
		{:else}
			<button
				class="menu-item"
				class:disabled={item.disabled}
				onclick={() => handleItemClick(item)}
			>
				{#if item.checked !== undefined}
					<span class="check">{item.checked ? '✓' : ''}</span>
				{/if}
				<span class="label">{item.label}</span>
			</button>
		{/if}
	{/each}
</div>

<style>
	.context-backdrop {
		position: fixed;
		inset: 0;
		z-index: 999;
	}

	.context-menu {
		position: fixed;
		z-index: 1000;
		min-width: 160px;
		background-color: var(--color-bg-secondary);
		border: 1px solid var(--color-border);
		border-radius: 4px;
		padding: 4px 0;
		box-shadow: 0 4px 12px rgba(0, 0, 0, 0.4);
	}

	.menu-item {
		display: flex;
		align-items: center;
		gap: 8px;
		width: 100%;
		padding: 5px 12px;
		background: none;
		border: none;
		color: var(--color-text-primary);
		font-size: 12px;
		cursor: pointer;
		text-align: left;
		white-space: nowrap;
	}

	.menu-item:hover:not(.disabled) {
		background-color: var(--color-bg-hover);
	}

	.menu-item.disabled {
		color: var(--color-text-muted);
		cursor: default;
	}

	.check {
		width: 14px;
		font-size: 11px;
		text-align: center;
		color: var(--color-accent);
	}

	.separator {
		height: 1px;
		background-color: var(--color-border);
		margin: 4px 0;
	}
</style>
