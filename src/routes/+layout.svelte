<script lang="ts">
	import '../app.css';
	import { playerStore } from '$lib/stores/player.svelte';
	import { libraryStore } from '$lib/stores/library.svelte';
	import { playlistsStore } from '$lib/stores/playlists.svelte';
	import { settingsStore } from '$lib/stores/settings.svelte';

	let { children } = $props();

	$effect(() => {
		playerStore.init();
		libraryStore.init();
		playlistsStore.init();
		settingsStore.init();
	});
</script>

{@render children()}
