<script lang="ts">
	import { invoke } from '@tauri-apps/api/core';

	interface Props {
		version: string;
		onupdate: () => void;
		oncancel: () => void;
		onskip: (version: string) => void;
	}

	let { version, onupdate, oncancel, onskip }: Props = $props();
	let downloading = $state(false);
	let downloadComplete = $state(false);
	let downloadProgress = $state('');
	let error = $state('');

	async function handleRestart() {
		try {
			const { relaunch } = await import('@tauri-apps/plugin-process');
			await relaunch();
		} catch {
			// Fallback: just tell user to restart manually
			downloadProgress = 'Please restart the app manually to apply the update.';
		}
	}

	async function handleUpdate() {
		downloading = true;
		error = '';
		try {
			// Try Tauri plugin updater first
			try {
				const { check } = await import('@tauri-apps/plugin-updater');
				const update = await check();
				if (update) {
					let total = 0, downloaded = 0;
					await update.downloadAndInstall((event: any) => {
						if (event.event === 'Started' && event.data?.contentLength) total = event.data.contentLength;
						else if (event.event === 'Progress') {
							downloaded += event.data?.chunkLength ?? 0;
							if (total > 0) downloadProgress = `${Math.round((downloaded / total) * 100)}%`;
						}
					});
					const { relaunch } = await import('@tauri-apps/plugin-process');
					setTimeout(() => relaunch(), 1000);
					return;
				}
			} catch {
				// Fallback to custom updater
				const info = await invoke<any>('check_for_updates');
				if (info.update_available) {
					const result = await invoke<string>('download_and_apply_update', { downloadUrl: info.download_url });
					if (result.includes('applied')) {
						// Linux: AppImage was replaced — show Restart button
						downloadComplete = true;
						downloadProgress = 'Download complete.';
					} else {
						// macOS/Windows: browser opened with download page
						downloadProgress = 'Download opened in browser. Install manually and restart.';
						downloading = false;
					}
				}
			}
		} catch (e) {
			error = String(e);
			downloading = false;
		}
	}
</script>

<div class="update-overlay">
	<div class="update-dialog">
		<h3 class="update-title">Update Available</h3>
		<p class="update-text">
			A new version <strong>v{version}</strong> is available. Would you like to update now?
		</p>

		{#if downloading && !downloadComplete}
			<p class="update-progress">Downloading... {downloadProgress}</p>
		{:else if downloadComplete}
			<p class="update-progress">{downloadProgress} Restart to apply update.</p>
		{:else if error}
			<p class="update-error">{error}</p>
		{/if}

		<div class="update-buttons">
			{#if downloadComplete}
				<button class="btn primary" onclick={handleRestart}>Restart Now</button>
				<button class="btn" onclick={oncancel}>Later</button>
			{:else if !downloading}
				<button class="btn primary" onclick={handleUpdate}>Update Now</button>
				<button class="btn" onclick={oncancel}>Later</button>
				<button class="btn skip" onclick={() => onskip(version)}>Skip v{version}</button>
			{/if}
		</div>
	</div>
</div>

<style>
	.update-overlay {
		position: fixed;
		inset: 0;
		z-index: 200;
		background-color: rgba(0, 0, 0, 0.6);
		display: flex;
		align-items: center;
		justify-content: center;
	}

	.update-dialog {
		background-color: var(--color-bg-primary);
		border: 1px solid var(--color-border);
		border-radius: 8px;
		padding: 24px;
		width: 400px;
		max-width: 90vw;
		box-shadow: 0 8px 32px rgba(0, 0, 0, 0.5);
	}

	.update-title {
		margin: 0 0 8px;
		font-size: 16px;
		font-weight: 600;
		color: var(--color-text-primary);
	}

	.update-text {
		margin: 0 0 16px;
		font-size: 13px;
		color: var(--color-text-secondary);
		line-height: 1.4;
	}

	.update-progress {
		font-size: 12px;
		color: var(--color-accent);
		margin: 0 0 12px;
	}

	.update-error {
		font-size: 12px;
		color: #f44336;
		margin: 0 0 12px;
	}

	.update-buttons {
		display: flex;
		gap: 8px;
	}

	.btn {
		padding: 6px 16px;
		border: 1px solid var(--color-border);
		border-radius: 4px;
		background: none;
		color: var(--color-text-primary);
		font-size: 13px;
		cursor: pointer;
	}

	.btn:hover {
		background-color: var(--color-bg-hover);
	}

	.btn.primary {
		background-color: var(--color-accent);
		border-color: var(--color-accent);
		color: white;
	}

	.btn.primary:hover {
		opacity: 0.9;
	}

	.btn.skip {
		margin-left: auto;
		font-size: 11px;
		color: var(--color-text-muted);
		border-color: transparent;
	}

	.btn.skip:hover {
		color: var(--color-text-secondary);
	}
</style>
