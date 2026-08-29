<!--
  The reclassify form, with no chrome of its own.

  Two callers need it framed differently: the context menu wants a dialog, and
  the metadata dialog wants a second column beside the tags rather than a popup
  stacked on top of one. So the form lives here and each caller brings its frame.
-->
<script lang="ts">
	import { consolesStore, type ConsoleDefinition } from '$lib/stores/consoles.svelte';
	import { libraryStore } from '$lib/stores/library.svelte';
	import { invoke } from '@tauri-apps/api/core';
	import { coversStore } from '$lib/stores/covers.svelte';
	import type { Track } from '$lib/types';

	let {
		track = null,
		tracks = null,
		folderPath = null,
		onclose
	}: {
		track?: Track | null;
		/// A whole selection. `track` is the one-item case and still works.
		tracks?: Track[] | null;
		folderPath?: string | null;
		onclose: () => void;
	} = $props();

	let chosenTracks = $derived(tracks && tracks.length > 0 ? tracks : track ? [track] : []);
	let first = $derived(chosenTracks[0] ?? null);
	// The folders the selection spans. One is the ordinary case; more than one
	// means "apply to the folder" has to be honest about how many it will touch.
	let folders = $derived([
		...new Set(chosenTracks.map((t) => t.path.replace(/[/\\][^/\\]*$/, '')))
	]);

	// Opened on a track, the folder is the one it sits in. Opened on a folder,
	// it is that folder. Either way a rip is a folder, so naming the console
	// once repairs every track in it.
	// $derived rather than const: reading a prop at the top level captures only
	// its first value, and Svelte warns about exactly that.
	let folder = $derived(folderPath ?? (first ? first.path.replace(/[/\\][^/\\]*$/, '') : ''));
	let folderLabel = $derived(folder.split(/[/\\]/).slice(-2).join('/'));

	// A folder has no single track to override, so that choice does not exist.
	type Scope = 'folder' | 'track';
	let scope = $state<Scope>('folder');

	// What the tracks under this folder were already classified as, which is
	// what the dialog opens showing.
	let inFolder = $derived(
		libraryStore.tracks.filter((t) => t.path.startsWith(folder + '/') || t.path.startsWith(folder + '\\'))
	);
	let guessed = $derived(first ?? inFolder[0] ?? null);

	// Pre-filled with what the classifier already worked out, so agreeing with
	// its guess about the game while correcting only the console costs nothing.
	let consoleQuery = $state('');
	let consoleId = $state<string | null>(null);
	let gameQuery = $state('');

	/// Every name worth offering, and its provenance. The seed picks the first
	/// of these; showing the rest means a wrong guess is one click from right,
	/// instead of something to retype.
	let candidates = $derived.by(() => {
		const g = guessed;
		const out: { label: string; from: string }[] = [];
		const add = (label: string | undefined, from: string) => {
			const v = (label ?? '').trim();
			if (v && !out.some((c) => c.label.toLowerCase() === v.toLowerCase())) {
				out.push({ label: v, from });
			}
		};
		add(g?.header_game, "the file's header");
		add(g?.game, 'its current classification');
		add(folder.split(/[/\\]/).pop(), 'the folder');
		add(g?.album, 'the album tag');
		return out;
	});

	let seeded = false;
	let seededFromHeader = $state(false);
	$effect(() => {
		if (seeded) return;
		const g = guessed;
		const best = candidates[0];
		// Nothing to seed *from* yet, so nothing is seeded and this runs again.
		//
		// The previous guard only covered the folder case, which meant that if
		// this effect ever ran before the track was resolved it set `seeded` on
		// the way past and the field stayed empty for good — with no way to ask
		// for a value, because a type-ahead needs something typed before it can
		// look anything up.
		if (!g && !best) return;
		seeded = true;
		consoleId = g?.console_id || null;
		// Whatever the candidate list ranks first, which is the file's own
		// header when it has one. Taken from the same list the chips below are
		// drawn from, so the box always shows one of the options rather than a
		// value with no chip to match it.
		gameQuery = best?.label ?? '';
		seededFromHeader = best?.from === "the file's header";
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

	let localMatches = $derived.by(() => {
		const q = gameQuery.trim().toLowerCase();
		if (!q) return [];
		return knownGames
			.filter((g) => g.toLowerCase().includes(q) && g.toLowerCase() !== q)
			.slice(0, 6);
	});

	// Canonical names, from the console's Libretro archive and from Steam. The
	// point is not completeness: it is that a name picked here is a name the
	// cover downloader will later match, so confirming the game and finding its
	// artwork stop being two separate gambles.
	let remoteMatches = $state<string[]>([]);
	let searching = $state(false);
	let searchTimer: ReturnType<typeof setTimeout> | null = null;

	/// Ask the archives, without waiting for someone to type.
	///
	/// The type-ahead only reaches the network once there is a query, which is
	/// useless when the field is empty and the whole question is "what is this?"
	/// This searches with the best name available — whatever is in the box, or
	/// the folder's name, which is usually what the ripper called the game.
	async function fetchOnline() {
		const q = (gameQuery.trim() || folder.split(/[/\\]/).pop() || '').trim();
		if (!q) return;
		searching = true;
		gameOpen = true;
		gameAnchor = anchorTo(gameInput);
		try {
			remoteMatches = await invoke<string[]>('suggest_game_names', {
				consoleId: consoleId ?? '',
				query: q
			});
			if (remoteMatches.length === 0) {
				searchError = `Nothing found for "${q}".`;
			} else {
				searchError = null;
			}
		} catch (e) {
			searchError = String(e);
		} finally {
			searching = false;
		}
	}

	let searchError = $state<string | null>(null);

	/// Where a dropdown should be drawn, in viewport coordinates.
	///
	/// `position: absolute` is not enough here and no z-index rescues it: the
	/// form scrolls, and a scroll container clips its positioned descendants
	/// whatever their stacking order. The dialog around it clips again with
	/// `overflow: hidden`. So the lists are `position: fixed` and measured from
	/// the input they belong to, which takes them out of both boxes.
	type Anchor = { left: number; top: number; width: number; maxHeight: number };
	let consoleAnchor = $state<Anchor | null>(null);
	let gameAnchor = $state<Anchor | null>(null);

	function anchorTo(el: HTMLElement | null): Anchor | null {
		if (!el) return null;
		const r = el.getBoundingClientRect();
		const below = window.innerHeight - r.bottom - 8;
		return {
			left: r.left,
			top: r.bottom + 2,
			width: r.width,
			// Never taller than the room under the field. Without this the list
			// runs off the bottom of the screen and its last entries cannot be
			// reached at all.
			maxHeight: Math.max(120, Math.min(260, below))
		};
	}

	let consoleInput = $state<HTMLInputElement | null>(null);
	let gameInput = $state<HTMLInputElement | null>(null);
	let consoleList = $state<HTMLElement | null>(null);
	let gameList = $state<HTMLElement | null>(null);

	// Now that the lists float above everything, something has to dismiss them.
	// While they were clipped by the form they were merely awkward; a fixed
	// element left open sits over whatever the user does next.
	$effect(() => {
		if (!consoleOpen && !gameOpen) return;
		const away = (e: MouseEvent) => {
			const t = e.target as Node;
			const inside = (a: HTMLElement | null, b: HTMLElement | null) =>
				(a && a.contains(t)) || (b && b.contains(t));
			if (consoleOpen && !inside(consoleInput, consoleList)) consoleOpen = false;
			if (gameOpen && !inside(gameInput, gameList)) gameOpen = false;
		};
		// Capture, so a click on something that stops propagation still closes
		// them — the dialog around this stops mousedown on purpose.
		document.addEventListener('mousedown', away, true);
		return () => document.removeEventListener('mousedown', away, true);
	});

	// Re-measured while either list is open: the dialog can be moved, the window
	// resized, and the form scrolled underneath it.
	$effect(() => {
		if (!consoleOpen && !gameOpen) return;
		const remeasure = () => {
			if (consoleOpen) consoleAnchor = anchorTo(consoleInput);
			if (gameOpen) gameAnchor = anchorTo(gameInput);
		};
		remeasure();
		window.addEventListener('resize', remeasure);
		window.addEventListener('scroll', remeasure, true);
		return () => {
			window.removeEventListener('resize', remeasure);
			window.removeEventListener('scroll', remeasure, true);
		};
	});

	function searchRemote(q: string) {
		if (searchTimer) clearTimeout(searchTimer);
		if (q.trim().length < 2) {
			remoteMatches = [];
			return;
		}
		// Debounced: this reaches the network on the first keystroke of a query
		// it has not cached, and a request per character is a request per
		// character.
		searchTimer = setTimeout(async () => {
			searching = true;
			try {
				remoteMatches = await invoke<string[]>('suggest_game_names', {
					consoleId: consoleId ?? '',
					query: q
				});
			} catch (e) {
				console.error('suggest_game_names:', e);
				remoteMatches = [];
			} finally {
				searching = false;
			}
		}, 300);
	}

	let gameMatches = $derived.by(() => {
		const q = gameQuery.trim().toLowerCase();
		const seen = new Set<string>();
		const out: string[] = [];
		for (const g of [...localMatches, ...remoteMatches]) {
			const k = g.toLowerCase();
			if (k === q || seen.has(k)) continue;
			seen.add(k);
			out.push(g);
		}
		return out.slice(0, 10);
	});

	async function save() {
		if (!consoleId) return;
		saving = true;
		error = null;
		try {
			const game = gameQuery.trim() || null;
			// flagFolder/flagTrack each reload the library, so a selection of
			// forty tracks would reload it forty times. The folder case is
			// already deduplicated by `folders`.
			if (scope === 'folder' || chosenTracks.length === 0) {
				const targets = folderPath ? [folderPath] : folders;
				for (const f of targets) {
					await consolesStore.flagFolder(f, consoleId, game);
				}
			} else {
				for (const t of chosenTracks) {
					await consolesStore.flagTrack(t.path, consoleId, game);
				}
			}

			// The point of naming the game is the artwork, so fetch it now.
			//
			// Without this the whole action is invisible: it writes a
			// classification, which nothing on screen showed until the Game
			// column existed, and which only pays off the next time somebody
			// runs a cover sweep. An action whose result you cannot see is
			// indistinguishable from one that did not happen.
			const sample = (scope === 'folder' ? [chosenTracks[0]] : chosenTracks)
				.filter(Boolean)
				.slice(0, 20);
			for (const t of sample) {
				try {
					await invoke('refetch_cover', { trackPath: t.path });
				} catch (e) {
					console.error('refetch after reclassify:', e);
				}
			}
			coversStore.refreshToken++;
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

<div class="panel">
		<div class="body">
			<p class="what">
				<span class="what-name">
					{#if chosenTracks.length > 1}
						{chosenTracks.length} tracks
					{:else if first}
						{first.title || first.path.split(/[/\\]/).pop()}
					{:else}
						{folder.split(/[/\\]/).pop()}
					{/if}
				</span>
				<span class="what-path">
					{folders.length > 1 ? `${folders.length} folders` : folderLabel}{#if chosenTracks.length === 0}&nbsp;· {inFolder.length} track{inFolder.length === 1 ? '' : 's'}{/if}
				</span>
			</p>

			{#if chosenTracks.length > 0}
			<div class="field">
				<span class="lbl" id="rc-scope-label">Apply to</span>
				<div class="segmented" role="radiogroup" aria-labelledby="rc-scope-label">
					<button
						class="seg" class:on={scope === 'folder'}
						role="radio" aria-checked={scope === 'folder'}
						onclick={() => (scope = 'folder')}
					>{folders.length === 1
						? 'The whole folder'
						: `All ${folders.length} folders`}</button>
					<button
						class="seg" class:on={scope === 'track'}
						role="radio" aria-checked={scope === 'track'}
						onclick={() => (scope = 'track')}
					>{chosenTracks.length === 1
						? 'Only this track'
						: `Only these ${chosenTracks.length} tracks`}</button>
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
						bind:this={consoleInput}
						placeholder={chosen ? chosen.name : 'Type a console…'}
						bind:value={consoleQuery}
						onfocus={() => {
							consoleOpen = true;
							consoleAnchor = anchorTo(consoleInput);
						}}
						oninput={() => {
							consoleOpen = true;
							consoleAnchor = anchorTo(consoleInput);
						}}
					/>
					{#if consoleOpen && consoleMatches.length > 0 && consoleAnchor}
						<ul
							bind:this={consoleList}
							class="drop"
							style="left:{consoleAnchor.left}px; top:{consoleAnchor.top}px; width:{consoleAnchor.width}px; max-height:{consoleAnchor.maxHeight}px"
						>
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
						bind:this={gameInput}
						placeholder="Game name"
						bind:value={gameQuery}
						onfocus={() => {
							gameOpen = true;
							gameAnchor = anchorTo(gameInput);
						}}
						oninput={() => {
							gameOpen = true;
							gameAnchor = anchorTo(gameInput);
							searchRemote(gameQuery);
						}}
					/>
					{#if gameOpen && gameMatches.length > 0 && gameAnchor}
						<ul
							bind:this={gameList}
							class="drop"
							style="left:{gameAnchor.left}px; top:{gameAnchor.top}px; width:{gameAnchor.width}px; max-height:{gameAnchor.maxHeight}px"
						>
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
				<button
					class="btn fetch"
					onclick={fetchOnline}
					disabled={searching}
					title="Search the console's box-art archive and Steam"
				>
					{searching ? 'Searching…' : 'Search online'}
				</button>
			</div>

			{#if candidates.length > 0}
				<div class="candidates">
					{#each candidates as c (c.label)}
						<button
							class="chip"
							class:on={c.label === gameQuery}
							onclick={() => (gameQuery = c.label)}
							title={`From ${c.from}`}
						>
							<span class="chip-name">{c.label}</span>
							<span class="chip-from">{c.from}</span>
						</button>
					{/each}
				</div>
			{/if}

			{#if searchError}<span class="hint warn">{searchError}</span>{/if}
			<span class="hint">
				{#if seededFromHeader}
					<strong>From the file's own header</strong> — the game it names, which is not
					always what its album says.
				{:else}
					Guessed from the tags and the folder; this format carries no game field of its
					own.
				{/if}
				Suggestions come from your library and from the console's box-art archive, so a
				name picked here is one the cover downloader can find.
				<strong>This does not edit the file's tags</strong> — the Album stays as the
				ripper wrote it. Turn on the Game column to see the result.{#if searching}
					<em> Searching…</em>{/if}
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

<style>
	.panel {
		display: flex;
		flex-direction: column;
		min-height: 0;
		flex: 1;
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
	.fetch {
		flex-shrink: 0;
		white-space: nowrap;
	}
	/* Left-aligned with the fields above, not with the label column: these are
	   answers to the question the field asks. */
	.candidates {
		display: flex;
		flex-wrap: wrap;
		gap: 6px;
		margin: 6px 0 0 76px;
	}
	.chip {
		display: flex;
		flex-direction: column;
		align-items: flex-start;
		gap: 1px;
		max-width: 200px;
		text-align: left;
		background-color: var(--color-bg-primary);
		border: 1px solid var(--color-border);
		border-radius: 4px;
		padding: 3px 8px;
		cursor: pointer;
	}
	.chip:hover {
		background-color: var(--color-bg-tertiary);
	}
	.chip.on {
		border-color: var(--color-text-secondary);
	}
	.chip-name {
		font-size: 12px;
		color: var(--color-text-primary);
		max-width: 184px;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.chip-from {
		font-size: 10px;
		color: var(--color-text-muted);
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

	/* Fixed, and positioned from the input's measured rectangle. See `anchorTo`:
	   absolute positioning is clipped by the scrolling form around it and again
	   by the dialog's `overflow: hidden`, and no z-index escapes a clip. The
	   z-index still has to clear the dialog itself, which sits at 200. */
	.drop {
		position: fixed;
		z-index: 300;
		margin: 0;
		padding: 2px;
		list-style: none;
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
