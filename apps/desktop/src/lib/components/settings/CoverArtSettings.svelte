<script lang="ts">
	import { onMount, onDestroy } from 'svelte';
	import { coversStore } from '$lib/stores/covers.svelte';
	import { settingsStore } from '$lib/stores/settings.svelte';
	import type { CoverFit } from '$lib/types';

	// Ordered by how often they are the right answer, not alphabetically.
	const FITS: { value: CoverFit; label: string; hint: string }[] = [
		{ value: 'cover', label: 'Fill the square', hint: 'Crops the edges. Nothing is letterboxed.' },
		{ value: 'contain', label: 'Show the whole cover', hint: 'Never cropped, bars on the short side.' },
		{ value: 'blur', label: 'Whole cover on a blurred backdrop', hint: 'Never cropped, and the bars are the cover itself.' },
		{ value: 'none', label: 'Original size, centred', hint: 'No scaling at all — keeps pixel art sharp.' },
		{ value: 'fill', label: 'Stretch to fit', hint: 'Ignores the aspect ratio. It will look wrong.' }
	];

	let undoneCount = $state<number | null>(null);

	onMount(() => coversStore.listen());
	onDestroy(() => coversStore.stopListening());

	/// "about 6 minutes", not "371s". Nobody reading a progress bar is doing
	/// arithmetic, and a number that precise is a promise this cannot keep.
	function humanEta(s: number | null): string {
		if (s === null) return 'estimating…';
		if (s < 45) return 'less than a minute left';
		const m = Math.round(s / 60);
		if (m < 60) return `about ${m} minute${m === 1 ? '' : 's'} left`;
		const h = Math.floor(m / 60);
		return `about ${h}h ${m % 60}m left`;
	}

	async function doUndo() {
		undoneCount = await coversStore.undo();
	}
</script>

<div class="settings-section">
	<h3>Cover art</h3>

	<div class="setting-block">
		<button
			class="btn primary big"
			onclick={() => coversStore.apply('library', '', false)}
			disabled={coversStore.running}
		>
			{coversStore.running ? 'Downloading…' : 'Download missing covers'}
		</button>
		<span class="hint">
			Goes through the whole library and fetches what is missing. A folder that already has
			an image is left alone — yours is never overwritten.
		</span>
		<span class="hint">
			It takes its time on purpose: one request at a time to each service, four games at
			once, and it backs off when a server asks it to. The archives it draws on are free and
			shared, and hammering them is how an address stops answering for everybody.
		</span>
	</div>

	{#if coversStore.running && coversStore.progress}
		{@const p = coversStore.progress}
		<div class="setting-block progress-block">
			<div class="bar">
				<div class="fill" style="width:{p.total ? (p.done / p.total) * 100 : 0}%"></div>
			</div>
			<div class="row between">
				<span class="counts">
					<strong>{p.done}</strong> of {p.total} · {p.found} found · {p.written} saved
				</span>
				<span class="eta">{humanEta(coversStore.etaSeconds)}</span>
			</div>
			{#if p.current}<span class="current" title={p.current}>{p.current}</span>{/if}
			<div class="row">
				<button class="btn" onclick={() => coversStore.cancel()}>Stop</button>
				<span class="hint inline">Everything already saved stays where it is.</span>
			</div>
		</div>
	{/if}

	{#if coversStore.error}
		<p class="err">{coversStore.error}</p>
	{/if}

	{#if !coversStore.running && coversStore.lastRun && !coversStore.undone}
		<div class="setting-block">
			<div class="row">
				<button class="btn" onclick={doUndo}>Undo the last run</button>
				<span class="hint inline">
					Removes only the files that run created, never one of yours.
				</span>
			</div>
			{#if undoneCount !== null}
				<span class="hint">Removed {undoneCount} file{undoneCount === 1 ? '' : 's'}.</span>
			{/if}
		</div>
	{/if}

	<!--
		Box art is not square and is not one shape either: a SNES box is nearly
		square, a PS1 jewel case is portrait, a Mega Drive box is wide. Cropping
		flatters some and beheads others, so this is a choice rather than a
		default someone has to live with.
	-->
	<div class="setting-block">
		<div class="row">
			<label class="lbl" for="cover-fit">Fit covers by</label>
			<select
				id="cover-fit"
				value={settingsStore.coverFit}
				onchange={(e) => settingsStore.setCoverFit(e.currentTarget.value as CoverFit)}
			>
				{#each FITS as f (f.value)}
					<option value={f.value}>{f.label}</option>
				{/each}
			</select>
		</div>
		<span class="hint">{FITS.find((f) => f.value === settingsStore.coverFit)?.hint}</span>
	</div>

</div>

<style>
	.settings-section {
		display: flex;
		flex-direction: column;
		gap: 12px;
	}

	h3 {
		font-size: 13px;
		font-weight: 600;
		color: var(--color-text-primary);
		margin: 8px 0 0;
	}

	.setting-block {
		display: flex;
		flex-direction: column;
		gap: 8px;
	}

	.row {
		display: flex;
		align-items: center;
		gap: 8px;
		flex-wrap: wrap;
	}

	.lbl {
		font-size: 12px;
		color: var(--color-text-primary);
	}

	select:disabled {
		opacity: 0.45;
		cursor: default;
	}

	.btn {
		background: var(--color-bg-tertiary);
		color: var(--color-text-primary);
		border: 1px solid var(--color-border);
		border-radius: 3px;
		padding: 4px 10px;
		font-size: 12px;
		cursor: pointer;
	}

	.btn:disabled {
		opacity: 0.4;
		cursor: default;
	}

	.hint {
		font-size: 11px;
		color: var(--color-text-secondary);
		line-height: 1.4;
	}

	.err {
		font-size: 11px;
		color: #e05252;
	}

	.big {
		font-size: 13px;
		padding: 7px 14px;
	}
	.progress-block {
		gap: 8px;
	}
	.bar {
		height: 6px;
		border-radius: 3px;
		background-color: var(--color-bg-primary);
		overflow: hidden;
	}
	.fill {
		height: 100%;
		background-color: var(--color-text-secondary);
		transition: width 200ms ease;
	}
	.between {
		justify-content: space-between;
	}
	.counts {
		font-size: 12px;
		color: var(--color-text-secondary);
	}
	.eta {
		font-size: 12px;
		color: var(--color-text-muted);
	}
	/* One line, always. The name of whatever is being looked up changes several
	   times a second, and a block that reflows with it drags the Stop button
	   around under the pointer. */
	.current {
		font-size: 11px;
		color: var(--color-text-muted);
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.hint.inline {
		margin: 0;
	}
</style>
