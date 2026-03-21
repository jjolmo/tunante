<script lang="ts">
	import Sidebar from '$lib/components/Sidebar.svelte';
	import TrackList from '$lib/components/TrackList.svelte';
	import PlayerControls from '$lib/components/PlayerControls.svelte';
	import ErrorToast from '$lib/components/ErrorToast.svelte';
	import SettingsPanel from '$lib/components/settings/SettingsPanel.svelte';
	import { settingsStore } from '$lib/stores/settings.svelte';
</script>

<div class="app-layout">
	<div class="app-main">
		<Sidebar />
		<TrackList />
	</div>
	<PlayerControls />
</div>

<ErrorToast />

{#if settingsStore.isSettingsOpen}
	<SettingsPanel />
{/if}

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
