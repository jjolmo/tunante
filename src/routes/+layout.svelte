<script lang="ts">
	import '../app.css';
	import { playerStore } from '$lib/stores/player.svelte';
	import { libraryStore } from '$lib/stores/library.svelte';
	import { playlistsStore } from '$lib/stores/playlists.svelte';

	let { children } = $props();

	$effect(() => {
		playerStore.init();
		libraryStore.init();
		playlistsStore.init();
	});
</script>

{@render children()}
