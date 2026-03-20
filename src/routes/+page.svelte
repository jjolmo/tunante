<script lang="ts">
	import Sidebar from '$lib/components/Sidebar.svelte';
	import TrackList from '$lib/components/TrackList.svelte';
	import ArtworkPanel from '$lib/components/ArtworkPanel.svelte';
	import PlayerControls from '$lib/components/PlayerControls.svelte';
</script>

<div class="app-layout">
	<div class="app-main">
		<Sidebar />
		<TrackList />
		<ArtworkPanel />
	</div>
	<PlayerControls />
</div>

<style>
	.app-layout {
		display: flex;
		flex-direction: column;
		height: 100vh;
		background-color: var(--color-bg-primary);
	}

	.app-main {
		display: flex;
		flex: 1;
		min-height: 0;
		overflow: hidden;
	}
</style>
