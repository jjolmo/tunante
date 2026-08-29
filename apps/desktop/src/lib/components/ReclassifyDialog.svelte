<script lang="ts">
	import { consolesStore, type ConsoleDefinition } from '$lib/stores/consoles.svelte';
	import { libraryStore } from '$lib/stores/library.svelte';
	import type { Track } from '$lib/types';

	let {
		track = null,
		folderPath = null,
		onclose
	}: { track?: Track | null; folderPath?: string | null; onclose: () => void } = $props();

	// Opened on a track, the folder is the one it sits in. Opened on a folder,
	// it is that folder. Either way a rip is a folder, so naming the console
	// once repairs every track in it.
	// $derived rather than const: reading a prop at the top level captures only
	// its first value, and Svelte warns about exactly that.
	let folder = $derived(folderPath ?? (track ? track.path.replace(/[/\\][^/\\]*$/, '') : ''));
	let folderLabel = $derived(folder.split(/[/\\]/).slice(-2).join('/'));

	// A folder has no single track to override, so that choice does not exist.
	type Scope = 'folder' | 'track';
	let scope = $state<Scope>('folder');

	// What the tracks under this folder were already classified as, which is
	// what the dialog opens showing.
	let inFolder = $derived(
		libraryStore.tracks.filter((t) => t.path.startsWith(folder + '/') || t.path.startsWith(folder + '\\'))
	);
	let guessed = $derived(track ?? inFolder[0] ?? null);

	// Pre-filled with what the classifier already worked out, so agreeing with
	// its guess about the game while correcting only the console costs nothing.
	let consoleQuery = $state('');
	let consoleId = $state<string | null>(null);
	let gameQuery = $state('');

	let seeded = false;
	$effect(() => {
		if (seeded) return;
		const g = guessed;
		if (!g && folderPath && inFolder.length === 0) return;
		seeded = true;
		consoleId = g?.console_id || null;
		gameQuery = g?.game || folder.split(/[/\\]/).pop() || '';
	});

	let consoleOpen = $state(false);
	let gameOpen = $state(false);
	let saving = $state(false);
	let error = $state<string | null>(null);

	let chosen = $derived(consolesStore.definitions.find((c) => c.id === consoleId) ?? null);

	// The id is in here on purpose ("snes", "ps1"), and so are the codecs: the
	// fastest way to say "this is a SNES rip" is to type `spc`.
	function score(c: ConsoleDefinition, q: string): number {
		const names = [c.name, c.name_es, c.id].map((x) => x.toLowerCase());
		if (names.some((h) => h === q)) return 0;
		if (names.some((h) => h.startsWith(q))) return 1;
		if (c.codecs.some((x) => x.toLowerCase() === q)) return 2;
		if (names.some((h) => h.includes(q))) return 3;
		return -1;
	}

	// Ranked, not filtered: an exact name beats a substring, so typing "ps"
	// offers PlayStation before Master System rather than in table order.
	let consoleMatches = $derived.by(() => {
		const q = consoleQuery.trim().toLowerCase();
		const all = consolesStore.definitions;
		if (!q) return all.slice(0, 12);
		return all
			.map((c: ConsoleDefinition) => ({ c, s: score(c, q) }))
			.filter((x: { c: ConsoleDefinition; s: number }) => x.s >= 0)
			.sort(
				(a: { c: ConsoleDefinition; s: number }, b: { c: ConsoleDefinition; s: number }) =>
					a.s - b.s || a.c.name.localeCompare(b.c.name)
			)
			.slice(0, 12)
			.map((x: { c: ConsoleDefinition; s: number }) => x.c);
	});

	// Every game name already in the library, so a correction lands on the name
	// the rest of the collection uses instead of inventing a near-duplicate.
	let knownGames = $derived.by(() => {
		const s = new Set<string>();
		for (const t of libraryStore.tracks) if (t.game) s.add(t.game);
		return [...s].sort((a, b) => a.localeCompare(b));
	});

	let gameMatches = $derived.by(() => {
		const q = gameQuery.trim().toLowerCase();
		if (!q) return [];
		return knownGames
			.filter((g) => g.toLowerCase().includes(q) && g.toLowerCase() !== q)
			.slice(0, 8);
	});

	async function save() {
		if (!consoleId) return;
		saving = true;
		error = null;
		try {
			const game = gameQuery.trim() || null;
			// flagFolder/flagTrack already reload the library afterwards.
			if (scope === 'folder' || !track) {
				await consolesStore.flagFolder(folder, consoleId, game);
			} else {
				await consolesStore.flagTrack(track.path, consoleId, game);
			}
			onclose();
		} catch (e) {
			error = String(e);
			saving = false;
		}
	}

	function onKeydown(e: KeyboardEvent) {
		if (e.key === 'Escape') {
			if (consoleOpen || gameOpen) {
				consoleOpen = false;
				gameOpen = false;
			} else {
				onclose();
			}
		}
	}
</script>

<svelte:window onkeydown={onKeydown} />

<div
	class="overlay"
	onclick={onclose}
	onmousedown={(e) => e.stopPropagation()}
	role="presentation"
>
	<div class="dialog" onclick={(e) => e.stopPropagation()} role="presentation">
		<div class="header">
			<span class="title">Reclassify</span>
			<button class="close-btn" onclick={onclose} aria-label="Close">✕</button>
		</div>

		<div class="body">
			<p class="what">
				<span class="what-name">
					{track ? track.title || track.path.split(/[/\\]/).pop() : folder.split(/[/\\]/).pop()}
				</span>
				<span class="what-path">
					{folderLabel}{#if !track}&nbsp;· {inFolder.length} track{inFolder.length === 1 ? '' : 's'}{/if}
				</span>
			</p>

			{#if track}
			<div class="field">
				<span class="lbl" id="rc-scope-label">Apply to</span>
				<div class="segmented" role="radiogroup" aria-labelledby="rc-scope-label">
					<button
						class="seg" class:on={scope === 'folder'}
						role="radio" aria-checked={scope === 'folder'}
						onclick={() => (scope = 'folder')}
					>The whole folder</button>
					<button
						class="seg" class:on={scope === 'track'}
						role="radio" aria-checked={scope === 'track'}
						onclick={() => (scope = 'track')}
					>Only this track</button>
				</div>
			</div>
			<span class="hint">
				A rip is a folder, so the folder is usually what you mean. Either one survives a
				rescan.
			</span>
			{/if}

			<div class="field">
				<label class="lbl" for="rc-console">Console</label>
				<div class="type-ahead">
					<input
						id="rc-console"
						type="text"
						autocomplete="off"
						placeholder={chosen ? chosen.name : 'Type a console…'}
						bind:value={consoleQuery}
						onfocus={() => (consoleOpen = true)}
						oninput={() => (consoleOpen = true)}
					/>
					{#if consoleOpen && consoleMatches.length > 0}
						<ul class="drop">
							{#each consoleMatches as c (c.id)}
								<li>
									<button
										class="opt" class:sel={c.id === consoleId}
										onclick={() => {
											consoleId = c.id;
											consoleQuery = '';
											consoleOpen = false;
										}}
									>
										<span class="opt-name">{c.name}</span>
										{#if c.name_es && c.name_es !== c.name}
											<span class="opt-alt">{c.name_es}</span>
										{/if}
									</button>
								</li>
							{/each}
						</ul>
					{/if}
				</div>
			</div>
			{#if chosen}
				<span class="hint">Filing under <strong>{chosen.name}</strong>.</span>
			{:else}
				<span class="hint warn">Pick a console — that is the part nothing could guess.</span>
			{/if}

			<div class="field">
				<label class="lbl" for="rc-game">Game</label>
				<div class="type-ahead">
					<input
						id="rc-game"
						type="text"
						autocomplete="off"
						placeholder="Game name"
						bind:value={gameQuery}
						onfocus={() => (gameOpen = true)}
						oninput={() => (gameOpen = true)}
					/>
					{#if gameOpen && gameMatches.length > 0}
						<ul class="drop">
							{#each gameMatches as g (g)}
								<li>
									<button
										class="opt"
										onclick={() => {
											gameQuery = g;
											gameOpen = false;
										}}
									>{g}</button>
								</li>
							{/each}
						</ul>
					{/if}
				</div>
			</div>
			<span class="hint">
				Guessed from the tags and the folder. The suggestions are names already used
				elsewhere in your library, so a correction does not create a near-duplicate.
			</span>

			{#if error}<p class="err">{error}</p>{/if}
		</div>

		<div class="footer">
			<button class="btn" onclick={onclose}>Cancel</button>
			<button class="btn primary" disabled={!consoleId || saving} onclick={save}>
				{saving ? 'Saving…' : 'Save'}
			</button>
		</div>
	</div>
</div>

<style>
	.overlay {
		position: fixed;
		inset: 0;
		z-index: 200;
		background-color: rgba(0, 0, 0, 0.5);
		display: flex;
		align-items: center;
		justify-content: center;
	}
	.dialog {
		width: 460px;
		max-width: 92vw;
		max-height: 86vh;
		background-color: var(--color-bg-primary);
		border: 1px solid var(--color-border);
		border-radius: 6px;
		display: flex;
		flex-direction: column;
		overflow: hidden;
		box-shadow: 0 8px 32px rgba(0, 0, 0, 0.4);
	}
	.header {
		display: flex;
		align-items: center;
		justify-content: space-between;
		padding: 12px 16px;
		border-bottom: 1px solid var(--color-border);
		background-color: var(--color-bg-secondary);
	}
	.title {
		font-size: 13px;
		font-weight: 600;
		color: var(--color-text-primary);
	}
	.close-btn {
		background: none;
		border: none;
		color: var(--color-text-secondary);
		cursor: pointer;
		font-size: 13px;
		line-height: 1;
		padding: 2px 4px;
	}
	.close-btn:hover {
		color: var(--color-text-primary);
	}
	.body {
		padding: 14px 16px;
		overflow-y: auto;
		display: flex;
		flex-direction: column;
		gap: 4px;
	}
	.what {
		margin: 0 0 10px;
		display: flex;
		flex-direction: column;
		gap: 2px;
		min-width: 0;
	}
	.what-name {
		font-size: 13px;
		color: var(--color-text-primary);
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.what-path {
		font-size: 11px;
		color: var(--color-text-muted);
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.field {
		display: flex;
		align-items: center;
		gap: 10px;
		margin-top: 10px;
	}
	.lbl {
		font-size: 12px;
		color: var(--color-text-secondary);
		width: 66px;
		flex-shrink: 0;
	}
	.hint {
		font-size: 11px;
		color: var(--color-text-muted);
		margin-left: 76px;
		line-height: 1.4;
	}
	.hint.warn {
		color: var(--color-accent, #d09030);
	}
	.err {
		font-size: 12px;
		color: #d05555;
		margin: 8px 0 0;
	}
	.segmented {
		display: flex;
		border: 1px solid var(--color-border);
		border-radius: 4px;
		overflow: hidden;
	}
	.seg {
		background: var(--color-bg-primary);
		border: none;
		color: var(--color-text-secondary);
		font-size: 12px;
		padding: 5px 10px;
		cursor: pointer;
	}
	.seg + .seg {
		border-left: 1px solid var(--color-border);
	}
	.seg.on {
		background: var(--color-bg-tertiary);
		color: var(--color-text-primary);
	}
	/* The dropdown is absolutely positioned against this, not against the
	   dialog: the two fields would otherwise share one coordinate space and the
	   second list would open over the first. */
	.type-ahead {
		position: relative;
		flex: 1;
		min-width: 0;
	}
	.type-ahead input {
		width: 100%;
		box-sizing: border-box;
		background-color: var(--color-bg-primary);
		border: 1px solid var(--color-border);
		border-radius: 4px;
		color: var(--color-text-primary);
		font-size: 12px;
		padding: 5px 8px;
	}
	.type-ahead input:focus {
		outline: none;
		border-color: var(--color-text-secondary);
	}
	.drop {
		position: absolute;
		top: calc(100% + 2px);
		left: 0;
		right: 0;
		z-index: 10;
		margin: 0;
		padding: 2px;
		list-style: none;
		max-height: 210px;
		overflow-y: auto;
		background-color: var(--color-bg-secondary);
		border: 1px solid var(--color-border);
		border-radius: 4px;
		box-shadow: 0 4px 16px rgba(0, 0, 0, 0.4);
	}
	.opt {
		display: flex;
		align-items: baseline;
		gap: 8px;
		width: 100%;
		text-align: left;
		background: none;
		border: none;
		border-radius: 3px;
		color: var(--color-text-primary);
		font-size: 12px;
		padding: 5px 8px;
		cursor: pointer;
	}
	.opt:hover,
	.opt:focus-visible {
		background-color: var(--color-bg-tertiary);
		outline: none;
	}
	.opt.sel {
		color: var(--color-text-primary);
		font-weight: 600;
	}
	.opt-alt {
		font-size: 11px;
		color: var(--color-text-muted);
	}
	.footer {
		display: flex;
		justify-content: flex-end;
		gap: 8px;
		padding: 10px 16px;
		border-top: 1px solid var(--color-border);
		background-color: var(--color-bg-secondary);
	}
	.btn {
		background-color: var(--color-bg-primary);
		border: 1px solid var(--color-border);
		border-radius: 4px;
		color: var(--color-text-primary);
		font-size: 12px;
		padding: 5px 12px;
		cursor: pointer;
	}
	.btn:hover:not(:disabled) {
		background-color: var(--color-bg-tertiary);
	}
	.btn:disabled {
		opacity: 0.5;
		cursor: default;
	}
	.btn.primary {
		border-color: var(--color-text-secondary);
	}
</style>
