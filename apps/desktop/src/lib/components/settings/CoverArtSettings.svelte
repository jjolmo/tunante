<script lang="ts">
	import { onMount, onDestroy } from 'svelte';
	import { coversStore, isTrusted, type Scope } from '$lib/stores/covers.svelte';
	import { consolesStore, type UnclassifiedFolder } from '$lib/stores/consoles.svelte';
	import { libraryStore } from '$lib/stores/library.svelte';
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

	let scope = $state<Scope>('library');
	let target = $state('');
	let replaceExisting = $state(false);
	let unclassified = $state<UnclassifiedFolder[]>([]);
	let loadingUnclassified = $state(false);
	let flagging = $state<Record<string, string>>({});
	let undoneCount = $state<number | null>(null);

	onMount(() => {
		coversStore.listen();
		consolesStore.loadCatalog();
		loadUnclassified();
		// Arrived here from a right-click on a console in the sidebar.
		if (coversStore.requestedConsole) {
			scope = 'console';
			target = coversStore.requestedConsole;
			coversStore.requestedConsole = null;
		}
	});
	onDestroy(() => coversStore.stopListening());

	async function loadUnclassified() {
		loadingUnclassified = true;
		try {
			unclassified = await consolesStore.unclassifiedFolders();
		} finally {
			loadingUnclassified = false;
		}
	}

	function folderName(path: string): string {
		const parts = path.split('/').filter(Boolean);
		return parts.slice(-2).join('/') || path;
	}

	async function flag(folder: string) {
		const id = flagging[folder];
		if (!id) return;
		await consolesStore.flagFolder(folder, id);
		unclassified = unclassified.filter((u) => u.folder !== folder);
	}

	async function doUndo() {
		undoneCount = await coversStore.undo();
	}

</script>

<div class="settings-section">
	<h3>Cover art</h3>

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

	<!--
		Preview first, always. This writes files into folders the user owns and
		syncs, so the flow is look → choose → apply, and only matches the backend
		graded Exact or High are ever applied without being read.
	-->
	<div class="setting-block">
		<div class="row">
			<label class="lbl" for="cover-scope">Look for covers in</label>
			<select id="cover-scope" bind:value={scope} disabled={coversStore.running}>
				<option value="library">the whole library</option>
				<option value="console">one console</option>
			</select>
			{#if scope === 'console'}
				<select bind:value={target} disabled={coversStore.running}>
					<option value="">choose…</option>
					{#each consolesStore.consolesWithCounts as c (c.id)}
						<option value={c.id}>{c.name} ({c.trackCount})</option>
					{/each}
				</select>
			{/if}
		</div>

		<div class="row">
			<button
				class="btn"
				onclick={() => coversStore.preview(scope, target)}
				disabled={coversStore.previewing || coversStore.running || (scope === 'console' && !target)}
			>
				{coversStore.previewing ? 'Looking…' : 'See what it finds'}
			</button>

			{#if coversStore.plans.length > 0 && !coversStore.running}
				<button class="btn primary" onclick={() => coversStore.apply(scope, target, replaceExisting)}>
					Save {coversStore.trusted.length} cover{coversStore.trusted.length === 1 ? '' : 's'}
				</button>
			{/if}

			{#if coversStore.running || coversStore.previewing}
				<button class="btn" onclick={() => coversStore.cancel()}>Cancel</button>
			{/if}
		</div>

		<label class="check">
			<input type="checkbox" bind:checked={replaceExisting} disabled={coversStore.running} />
			<span>Replace covers that are already in the folder</span>
		</label>
		<span class="hint">
			Off by default: an image already sitting in a folder is one you chose, and it is never
			overwritten.
		</span>

		{#if coversStore.error}
			<p class="err">{coversStore.error}</p>
		{/if}

		{#if coversStore.progress}
			<div class="progress">
				<div class="bar">
					<div
						class="fill"
						style:width="{(coversStore.progress.done / Math.max(coversStore.progress.total, 1)) * 100}%"
					></div>
				</div>
				<span class="hint">
					{coversStore.progress.done}/{coversStore.progress.total} ·
					{coversStore.progress.written} saved · {coversStore.progress.current}
				</span>
			</div>
		{/if}

		{#if coversStore.plans.length > 0 && !coversStore.running}
			<div class="summary">
				<span><b>{coversStore.trusted.length}</b> ready</span>
				<span><b>{coversStore.needsReview.length}</b> to check</span>
				<span><b>{coversStore.missing.length}</b> not found</span>
				{#if coversStore.untouched.length > 0}
					<span><b>{coversStore.untouched.length}</b> already have art</span>
				{/if}
			</div>

			{#if coversStore.needsReview.length > 0}
				<p class="hint review-note">
					These matched, but not exactly. They are <em>not</em> saved automatically — a wrong cover
					is worse than none, because a missing one is visibly missing and a wrong one looks
					deliberate.
				</p>
				<ul class="plans">
					{#each coversStore.needsReview as p (p.game + p.console_id)}
						<li>
							<span class="game">{p.game}</span>
							<span class="arrow">→</span>
							<span class="matched">{p.matched_name}</span>
							<span class="src">{p.source}</span>
						</li>
					{/each}
				</ul>
			{/if}

			{#if coversStore.lastRun !== null && !coversStore.undone}
				<button class="btn" onclick={doUndo}>Undo the last run</button>
			{/if}
			{#if coversStore.undone && undoneCount !== null}
				<span class="hint">Removed {undoneCount} file{undoneCount === 1 ? '' : 's'}.</span>
			{/if}
		{/if}
	</div>

	<h3>Music we could not place</h3>
	<p class="hint">
		These folders hold games we could not name a console for — a franchise folder spanning several
		machines, or a remix collection. No rule gets those right, so nothing is guessed. Tell it which
		machine, and covers start working for them.
	</p>

	{#if loadingUnclassified}
		<p class="hint">Looking…</p>
	{:else if unclassified.length === 0}
		<p class="hint">Nothing unplaced. {libraryStore.tracks.length} tracks all have a console.</p>
	{:else}
		<ul class="unclassified">
			{#each unclassified.slice(0, 40) as u (u.folder)}
				<li>
					<span class="folder" title={u.folder}>{folderName(u.folder)}</span>
					<span class="count">{u.track_count}</span>
					<select bind:value={flagging[u.folder]}>
						<option value="">console…</option>
						{#each consolesStore.definitions as d (d.id)}
							<option value={d.id}>{d.name}</option>
						{/each}
					</select>
					<button class="btn small" onclick={() => flag(u.folder)} disabled={!flagging[u.folder]}>
						Set
					</button>
				</li>
			{/each}
		</ul>
		{#if unclassified.length > 40}
			<span class="hint">…and {unclassified.length - 40} more, smallest last.</span>
		{/if}
	{/if}
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
	/*
	 * `appearance: none` is not optional. A native select on Linux ignores
	 * `background` entirely and renders in the GTK theme's own colours — a white
	 * box on a dark page. The arrow has to be drawn by hand once it is gone.
	 * Same shape as the one in GeneralSettings.
	 */
	select {
		appearance: none;
		-webkit-appearance: none;
		padding: 4px 26px 4px 8px;
		background-color: var(--color-bg-primary);
		background-image: linear-gradient(45deg, transparent 50%, var(--color-text-secondary) 50%),
			linear-gradient(135deg, var(--color-text-secondary) 50%, transparent 50%);
		background-position:
			calc(100% - 13px) 50%,
			calc(100% - 8px) 50%;
		background-size:
			5px 5px,
			5px 5px;
		background-repeat: no-repeat;
		border: 1px solid var(--color-border);
		border-radius: 4px;
		color: var(--color-text-primary);
		font-size: 12px;
		cursor: pointer;
	}

	select:disabled {
		opacity: 0.45;
		cursor: default;
	}

	/* The dropdown itself is drawn by the OS, so it needs telling too. */
	select option {
		background: var(--color-bg-secondary);
		color: var(--color-text-primary);
	}

	input[type='checkbox'] {
		accent-color: var(--color-accent);
		cursor: pointer;
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
	.btn.primary {
		background: var(--color-accent, #3b82f6);
		border-color: transparent;
		color: #fff;
	}
	.btn.small {
		padding: 2px 8px;
	}
	.check {
		display: flex;
		align-items: center;
		gap: 6px;
		font-size: 12px;
		color: var(--color-text-primary);
	}
	.hint {
		font-size: 11px;
		color: var(--color-text-secondary);
		line-height: 1.4;
	}
	.review-note {
		margin: 4px 0 0;
	}
	.err {
		font-size: 11px;
		color: #e05252;
	}
	.progress .bar {
		height: 4px;
		background: var(--color-bg-tertiary);
		border-radius: 2px;
		overflow: hidden;
	}
	.progress .fill {
		height: 100%;
		background: var(--color-accent, #3b82f6);
		transition: width 120ms linear;
	}
	.summary {
		display: flex;
		gap: 14px;
		font-size: 11px;
		color: var(--color-text-secondary);
	}
	.plans,
	.unclassified {
		list-style: none;
		margin: 0;
		padding: 0;
		max-height: 240px;
		overflow-y: auto;
		display: flex;
		flex-direction: column;
		gap: 3px;
	}
	.plans li {
		display: flex;
		align-items: center;
		gap: 8px;
		font-size: 11px;
		padding: 2px 0;
	}

	/*
	 * A grid, not a flex row: with flex every line sized its own columns, so the
	 * dropdowns marched left and right down the list depending on how long each
	 * folder name happened to be.
	 */
	.unclassified li {
		display: grid;
		grid-template-columns: minmax(0, 1fr) 3.5em 11em auto;
		align-items: center;
		gap: 8px;
		font-size: 11px;
		padding: 2px 0;
	}
	.game,
	.folder {
		color: var(--color-text-primary);
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}

	.game {
		min-width: 140px;
		max-width: 220px;
	}
	.arrow,
	.count,
	.src,
	.matched {
		color: var(--color-text-secondary);
	}
	.matched {
		flex: 1;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.count {
		text-align: right;
		font-variant-numeric: tabular-nums;
	}
</style>
