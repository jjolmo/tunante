<script lang="ts">
	import '../app.css';
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
		getCurrentWindow().setTitle(title);
	});
</script>

{@render children()}
