<script lang="ts">
	import '../app.css';
	import { onMount } from 'svelte';
	import { getCurrentWindow } from '@tauri-apps/api/window';
	import { playerStore } from '$lib/stores/player.svelte';
	import { libraryStore } from '$lib/stores/library.svelte';
	import { playlistsStore } from '$lib/stores/playlists.svelte';
	import { settingsStore } from '$lib/stores/settings.svelte';

	const APP_TITLE = 'Tunante';

	let { children } = $props();

	$effect(() => {
		playerStore.init();
		libraryStore.init();
		playlistsStore.init();
		settingsStore.init();
	});

	// Show window after first render — prevents white flash on startup
	onMount(() => {
		getCurrentWindow().show();
	});

	$effect(() => {
		const track = playerStore.currentTrack;
		const show = settingsStore.showTrackInTitlebar;
		let title = APP_TITLE;
		if (show && track) {
			const parts = [track.title, track.artist].filter(Boolean);
			if (parts.length > 0) {
				title = `${parts.join(' - ')} — ${APP_TITLE}`;
			}
		}
		// Set document.title synchronously as fallback
		document.title = title;
		// Also set the native window titlebar (async)
		getCurrentWindow().setTitle(title).catch((err) => {
			console.warn('Failed to set window title:', err);
		});
	});
</script>

{@render children()}
