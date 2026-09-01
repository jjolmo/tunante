<!--
  Picking a cover, instead of asking for another one and hoping.

  "Re-download cover art" used to run the automatic path again. That can only
  ever produce the same kind of answer, and when the cover is wrong because the
  *name* is wrong — a folder called `ct`, a soundtrack whose album tag is not
  the game's title — it has nothing to do differently, so it looked broken. Here
  the search is visible: every archive and service is asked at once, the whole
  list is shown, and the name can be retyped.
-->
<script lang="ts">
	import { invoke } from '@tauri-apps/api/core';
	import { coversStore, type Confidence } from '$lib/stores/covers.svelte';
	import type { Track } from '$lib/types';

	interface CoverOption {
		url: string;
		source: string;
		name: string;
		confidence: Confidence;
	}

	let { track, onclose }: { track: Track; onclose: () => void } = $props();

	/// What the folder is called, which is what the automatic path would have
	/// searched for when the tags are empty.
	let folderName = $derived(track.path.replace(/[/\\][^/\\]*$/, '').split(/[/\\]/).pop() ?? '');
	let query = $state('');
	let options = $state<CoverOption[]>([]);
	let searching = $state(false);
	let error = $state<string | null>(null);
	/// The one being downloaded, so its tile can say so and the rest can stop
	/// accepting clicks.
	let applying = $state<string | null>(null);
	/// URLs whose image never arrived. A search offers what a service claims to
	/// have, and Steam in particular advertises a portrait cover for every app
	/// and serves one for perhaps half of them; a tile that will never hold a
	/// picture is worse than one row less.
	let broken = $state<string[]>([]);
	/// Which tiles have finished loading, so the rest can show that they are on
	/// their way rather than an empty square.
	let loaded = $state<string[]>([]);

	let shown = $derived(options.filter((o) => !broken.includes(o.url)));

	/// The first search asks with no name at all, which makes the backend use
	/// the same candidates the automatic download uses. So the dialog opens
	/// showing what the download would have found — including, usually, the
	/// cover that is already wrong, which is the one to compare against.
	let seeded = false;
	$effect(() => {
		if (seeded) return;
		seeded = true;
		query = track.game || track.album || folderName;
		search(null);
	});

	async function search(name: string | null) {
		searching = true;
		error = null;
		options = [];
		broken = [];
		loaded = [];
		try {
			options = await invoke<CoverOption[]>('search_cover_options', {
				trackPath: track.path,
				query: name
			});
			if (options.length === 0) {
				error = 'Nothing found. Try the game name as the shops spell it.';
			}
		} catch (e) {
			error = typeof e === 'string' ? e : 'The search failed';
		} finally {
			searching = false;
		}
	}

	function submit(e: Event) {
		e.preventDefault();
		if (searching) return;
		search(query.trim() || null);
	}

	async function choose(option: CoverOption) {
		if (applying) return;
		applying = option.url;
		error = null;
		try {
			await invoke<string>('choose_cover', { trackPath: track.path, url: option.url });
			// Everything showing artwork re-reads it: the cover for the playing
			// track may be the one that just changed, and without this it stays
			// wrong on screen until the track does.
			coversStore.refreshToken++;
			onclose();
		} catch (e) {
			error = typeof e === 'string' ? e : 'That cover could not be saved';
			applying = null;
		}
	}

	/// The old "Re-download cover art", kept as one button.
	///
	/// It still earns its place beside a grid of choices: when the automatic
	/// answer was right and only the cache was stale, this is the whole job,
	/// and it forces past the cache, past "never overwrite an existing image"
	/// and past the confidence floor a bulk run applies.
	let retrying = $state(false);
	async function retryAutomatically() {
		if (retrying || applying) return;
		retrying = true;
		error = null;
		try {
			const found = await invoke<string | null>('refetch_cover', { trackPath: track.path });
			coversStore.refreshToken++;
			if (found) {
				onclose();
			} else {
				error = 'Nothing found automatically. Pick one below, or retype the name.';
			}
		} catch (e) {
			error = typeof e === 'string' ? e : 'The download failed';
		} finally {
			retrying = false;
		}
	}

	function handleKeydown(e: KeyboardEvent) {
		if (e.key === 'Escape') onclose();
	}
</script>

<svelte:window onkeydown={handleKeydown} />

<div class="overlay" onclick={onclose} onmousedown={(e) => e.stopPropagation()} role="presentation">
	<div class="dialog" onclick={(e) => e.stopPropagation()} role="presentation">
		<div class="header">
			<div class="titles">
				<span class="title">Cover art</span>
				<span class="subtitle">{track.game || folderName}</span>
			</div>
			<button class="close-btn" onclick={onclose} aria-label="Close">✕</button>
		</div>

		<form class="search" onsubmit={submit}>
			<input
				type="text"
				bind:value={query}
				placeholder="Game name"
				spellcheck="false"
				autocomplete="off"
			/>
			<button type="submit" class="search-btn" disabled={searching}>
				{searching ? 'Searching…' : 'Search'}
			</button>
		</form>

		{#if error}
			<div class="message">{error}</div>
		{/if}

		<div class="grid-scroll">
			{#if searching}
				<div class="searching">
					<svg width="20" height="20" viewBox="0 0 16 16" fill="currentColor" class="spin">
						<path d="M8 1a7 7 0 00-7 7h2a5 5 0 015-5V1z" />
					</svg>
					<span>Asking every archive and shop…</span>
				</div>
			{:else}
				<div class="grid">
					{#each shown as option (option.url)}
						<button
							class="tile"
							class:applying={applying === option.url}
							disabled={applying !== null}
							onclick={() => choose(option)}
							title="{option.name} — {option.source}"
						>
							<div class="thumb">
								<img
									src={option.url}
									alt={option.name}
									loading="lazy"
									onload={() => (loaded = [...loaded, option.url])}
									onerror={() => (broken = [...broken, option.url])}
								/>
								{#if !loaded.includes(option.url)}
									<div class="thumb-loading">
										<svg width="18" height="18" viewBox="0 0 16 16" fill="currentColor" class="spin">
											<path d="M8 1a7 7 0 00-7 7h2a5 5 0 015-5V1z" />
										</svg>
									</div>
								{/if}
								{#if applying === option.url}
									<div class="thumb-loading applying-overlay">
										<svg width="22" height="22" viewBox="0 0 16 16" fill="currentColor" class="spin">
											<path d="M8 1a7 7 0 00-7 7h2a5 5 0 015-5V1z" />
										</svg>
									</div>
								{/if}
							</div>
							<div class="caption">
								<span class="name">{option.name}</span>
								<span class="source" class:sure={option.confidence === 'exact' || option.confidence === 'high'}>
									{option.source}
								</span>
							</div>
						</button>
					{/each}
				</div>
			{/if}
		</div>

		<div class="footer">
			<button
				class="cancel-btn"
				onclick={retryAutomatically}
				disabled={retrying || applying !== null}
			>
				{retrying ? 'Downloading…' : 'Download automatically'}
			</button>
			<span class="count">
				{#if !searching && shown.length > 0}{shown.length} covers{/if}
			</span>
			<button class="cancel-btn" onclick={onclose}>Cancel</button>
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
		width: 720px;
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

	.titles {
		display: flex;
		flex-direction: column;
		gap: 2px;
		min-width: 0;
	}

	.title {
		font-size: 13px;
		font-weight: 600;
		color: var(--color-text-primary);
	}

	.subtitle {
		font-size: 11px;
		color: var(--color-text-muted);
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
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

	.search {
		display: flex;
		gap: 8px;
		padding: 12px 16px;
		border-bottom: 1px solid var(--color-border);
	}

	.search input {
		flex: 1;
		background-color: var(--color-bg-tertiary);
		border: 1px solid var(--color-border);
		border-radius: 3px;
		color: var(--color-text-primary);
		font-size: 12px;
		padding: 6px 8px;
		outline: none;
	}

	.search input:focus {
		border-color: var(--color-accent);
	}

	.search-btn,
	.cancel-btn {
		background-color: var(--color-bg-tertiary);
		border: 1px solid var(--color-border);
		border-radius: 3px;
		color: var(--color-text-primary);
		cursor: pointer;
		font-size: 12px;
		padding: 6px 12px;
	}

	.search-btn:hover:not(:disabled),
	.cancel-btn:hover {
		border-color: var(--color-accent);
	}

	.cancel-btn:disabled {
		opacity: 0.5;
		cursor: default;
	}

	.search-btn:disabled {
		opacity: 0.5;
		cursor: default;
	}

	.message {
		padding: 8px 16px;
		font-size: 11px;
		color: var(--color-text-muted);
		border-bottom: 1px solid var(--color-border);
	}

	.grid-scroll {
		flex: 1;
		overflow-y: auto;
		padding: 12px 16px;
		min-height: 200px;
	}

	.searching {
		display: flex;
		align-items: center;
		justify-content: center;
		gap: 8px;
		height: 180px;
		color: var(--color-text-muted);
		font-size: 12px;
	}

	.grid {
		display: grid;
		grid-template-columns: repeat(auto-fill, minmax(120px, 1fr));
		gap: 12px;
	}

	.tile {
		background: none;
		border: 1px solid transparent;
		border-radius: 4px;
		padding: 4px;
		cursor: pointer;
		display: flex;
		flex-direction: column;
		gap: 4px;
		text-align: left;
		min-width: 0;
	}

	.tile:hover:not(:disabled) {
		border-color: var(--color-accent);
		background-color: var(--color-bg-secondary);
	}

	.tile:disabled {
		cursor: default;
	}

	.tile:disabled:not(.applying) {
		opacity: 0.45;
	}

	.thumb {
		position: relative;
		aspect-ratio: 1;
		background-color: var(--color-bg-tertiary);
		border-radius: 3px;
		overflow: hidden;
	}

	.thumb img {
		width: 100%;
		height: 100%;
		object-fit: contain;
	}

	.thumb-loading {
		position: absolute;
		inset: 0;
		display: flex;
		align-items: center;
		justify-content: center;
		color: var(--color-text-muted);
		background-color: var(--color-bg-tertiary);
	}

	/* Over the image rather than instead of it: the cover being saved stays
	   visible, so it is obvious which one was picked. */
	.applying-overlay {
		background-color: rgba(0, 0, 0, 0.55);
		color: var(--color-accent);
	}

	.caption {
		display: flex;
		flex-direction: column;
		gap: 1px;
		min-width: 0;
	}

	.name {
		font-size: 11px;
		color: var(--color-text-secondary);
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}

	.source {
		font-size: 10px;
		color: var(--color-text-muted);
		font-family: var(--font-mono);
	}

	/* The rows where the name matched outright, which are the ones worth
	   looking at first. */
	.source.sure {
		color: var(--color-accent);
	}

	.footer {
		display: flex;
		align-items: center;
		justify-content: space-between;
		padding: 10px 16px;
		border-top: 1px solid var(--color-border);
		background-color: var(--color-bg-secondary);
	}

	.count {
		font-size: 11px;
		color: var(--color-text-muted);
	}

	.spin {
		animation: spin 1s linear infinite;
	}

	@keyframes spin {
		from {
			transform: rotate(0deg);
		}
		to {
			transform: rotate(360deg);
		}
	}
</style>
