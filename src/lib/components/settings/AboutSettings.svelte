<script lang="ts">
	import { invoke } from '@tauri-apps/api/core';
	import { getVersion } from '@tauri-apps/api/app';

	let appVersion = $state('0.1.0');
	let updateStatus = $state<'idle' | 'checking' | 'available' | 'up-to-date' | 'downloading' | 'done' | 'done-mac' | 'done-mac-script' | 'error'>('idle');
	let updateVersion = $state('');
	let updateError = $state('');
	let downloadProgress = $state('');

	$effect(() => {
		getVersion().then(v => appVersion = v).catch(() => {});
	});

	const isMacOS = navigator.platform.startsWith('Mac');

	async function checkForUpdates() {
		updateStatus = 'checking';
		updateError = '';

		// On macOS, skip the Tauri plugin updater — it returns null (no update)
		// instead of throwing, because the builds aren't codesigned/notarized.
		// Go straight to the GitHub API fallback which always works.
		if (!isMacOS) {
			try {
				const { check } = await import('@tauri-apps/plugin-updater');
				const update = await check();
				if (update) {
					updateVersion = update.version;
					updateStatus = 'available';
					(window as any).__tauriUpdate = update;
					return;
				} else {
					updateStatus = 'up-to-date';
					return;
				}
			} catch {
				// Tauri plugin failed — fall through to custom updater
			}
		}

		// Custom updater via GitHub API (works on all platforms)
		try {
			const info = await invoke<any>('check_for_updates');
			if (info.update_available) {
				updateVersion = info.latest_version;
				updateStatus = 'available';
				(window as any).__tauriUpdate = null;
				(window as any).__customUpdateUrl = info.download_url;
			} else {
				updateStatus = 'up-to-date';
			}
		} catch (e2) {
			updateError = String(e2);
			updateStatus = 'error';
		}
	}

	async function downloadAndInstall() {
		updateStatus = 'downloading';
		try {
			const update = (window as any).__tauriUpdate;
			if (update) {
				// Tauri plugin updater (Windows/Mac)
				let totalBytes = 0;
				let downloadedBytes = 0;
				await update.downloadAndInstall((event: any) => {
					if (event.event === 'Started' && event.data?.contentLength) {
						totalBytes = event.data.contentLength;
					} else if (event.event === 'Progress') {
						downloadedBytes += event.data?.chunkLength ?? 0;
						if (totalBytes > 0) {
							const pct = Math.round((downloadedBytes / totalBytes) * 100);
							downloadProgress = `${pct}%`;
						}
					} else if (event.event === 'Finished') {
						downloadProgress = 'Done!';
					}
				});
				updateStatus = 'done';
				// Auto-relaunch after 2 seconds
				const { relaunch } = await import('@tauri-apps/plugin-process');
				setTimeout(() => relaunch(), 2000);
			} else {
				// Custom updater fallback (Linux AppImage)
				const url = (window as any).__customUpdateUrl;
				const msg = await invoke<string>('download_and_apply_update', { downloadUrl: url });
				downloadProgress = msg;
				updateStatus = isMacOS ? 'done-mac' : 'done';
			}
		} catch (e) {
			updateError = String(e);
			updateStatus = 'error';
		}
	}

	async function runMacUpdateScript() {
		try {
			await invoke('run_macos_update_script');
			updateStatus = 'done-mac-script';
		} catch (e) {
			updateError = String(e);
			updateStatus = 'error';
		}
	}
</script>

<div class="about-settings">
	<div class="about-header">
		<svg width="48" height="48" viewBox="0 0 16 16" fill="var(--color-accent)">
			<path d="M8 1.5a6.5 6.5 0 100 13 6.5 6.5 0 000-13zM0 8a8 8 0 1116 0A8 8 0 010 8z" />
			<path d="M5 5.5a1 1 0 012 0v4a1 1 0 01-2 0v-4zm4 0a1 1 0 012 0v4a1 1 0 01-2 0v-4z" />
		</svg>
		<div class="about-title-block">
			<h2 class="about-title">Tunante</h2>
			<span class="about-version">v{appVersion}</span>
		</div>
	</div>

	<p class="about-tagline">
		A cross-platform music player focused on video game music formats, inspired by foobar2000.
	</p>

	<!-- Update section -->
	<div class="about-section update-section">
		<h4 class="about-label">Updates</h4>

		{#if updateStatus === 'idle'}
			<button class="update-btn" onclick={checkForUpdates}>Check for updates</button>
		{:else if updateStatus === 'checking'}
			<span class="update-status">Checking for updates...</span>
		{:else if updateStatus === 'up-to-date'}
			<span class="update-status success">You're on the latest version</span>
			<button class="update-btn small" onclick={checkForUpdates}>Check again</button>
		{:else if updateStatus === 'available'}
			<div class="update-available">
				<span class="update-new">New version available: <strong>v{updateVersion}</strong></span>
				<div class="update-actions">
					{#if isMacOS}
						<button class="update-btn primary" onclick={runMacUpdateScript}>
							Download & Install
						</button>
						<button class="update-btn small" onclick={downloadAndInstall}>
							Open in browser
						</button>
					{:else}
						<button class="update-btn primary" onclick={downloadAndInstall}>
							Download & Install
						</button>
					{/if}
				</div>
			</div>
		{:else if updateStatus === 'downloading'}
			<span class="update-status">Downloading update... {downloadProgress}</span>
		{:else if updateStatus === 'done'}
			<span class="update-status success">Update installed! Restarting...</span>
		{:else if updateStatus === 'done-mac'}
			<div class="update-mac-instructions">
				<span class="update-status success">Download started in your browser.</span>
				<p class="mac-hint">After installing, open Terminal and run:</p>
				<code class="mac-command">xattr -cr /Applications/Tunante.app</code>
			</div>
		{:else if updateStatus === 'done-mac-script'}
			<span class="update-status success">Update script opened in Terminal. Follow the instructions there.</span>
		{:else if updateStatus === 'error'}
			<span class="update-status error">{updateError}</span>
			<button class="update-btn small" onclick={checkForUpdates}>Retry</button>
		{/if}
	</div>

	<div class="about-section">
		<span class="about-credit">
			Vibecoded by <strong>jjolmo</strong> to understand how much I'm replaceable by AI.
		</span>
	</div>

	<div class="about-section">
		<h4 class="about-label">Source Code</h4>
		<a class="about-link" href="https://github.com/jjolmo/tunante" target="_blank" rel="noopener noreferrer">
			<svg width="14" height="14" viewBox="0 0 16 16" fill="currentColor">
				<path d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82.64-.18 1.32-.27 2-.27.68 0 1.36.09 2 .27 1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.013 8.013 0 0016 8c0-4.42-3.58-8-8-8z" />
			</svg>
			<span>GitHub Repository</span>
		</a>
	</div>

	<div class="about-section">
		<h4 class="about-label">Built with</h4>
		<div class="about-tech">
			<span class="tech-badge">Tauri v2</span>
			<span class="tech-badge">SvelteKit 2</span>
			<span class="tech-badge">Svelte 5</span>
			<span class="tech-badge">Rust</span>
			<span class="tech-badge">SQLite</span>
			<span class="tech-badge">rodio</span>
			<span class="tech-badge">symphonia</span>
			<span class="tech-badge">lofty</span>
		</div>
	</div>
</div>

<style>
	.about-settings {
		display: flex;
		flex-direction: column;
		gap: 16px;
	}

	.about-header {
		display: flex;
		align-items: center;
		gap: 16px;
	}

	.about-title-block {
		display: flex;
		flex-direction: column;
	}

	.about-title {
		font-size: 24px;
		font-weight: 700;
		color: var(--color-text-primary);
		margin: 0;
	}

	.about-version {
		font-size: 12px;
		color: var(--color-text-secondary);
	}

	.about-tagline {
		font-size: 13px;
		color: var(--color-text-secondary);
		margin: 0;
		line-height: 1.4;
	}

	.about-section {
		display: flex;
		flex-direction: column;
		gap: 6px;
	}

	.about-credit {
		font-size: 13px;
		color: var(--color-text-primary);
		line-height: 1.4;
	}

	.about-label {
		font-size: 12px;
		font-weight: 600;
		color: var(--color-text-secondary);
		margin: 0;
		text-transform: uppercase;
		letter-spacing: 0.5px;
	}

	.about-link {
		display: inline-flex;
		align-items: center;
		gap: 6px;
		color: var(--color-accent);
		font-size: 13px;
		text-decoration: none;
	}

	.about-link:hover {
		text-decoration: underline;
	}

	.about-tech {
		display: flex;
		flex-wrap: wrap;
		gap: 6px;
	}

	.tech-badge {
		font-size: 11px;
		color: var(--color-text-secondary);
		background-color: var(--color-bg-tertiary);
		padding: 3px 8px;
		border-radius: 3px;
		border: 1px solid var(--color-border);
	}

	.update-section {
		padding: 10px;
		background-color: var(--color-bg-tertiary);
		border-radius: 6px;
		border: 1px solid var(--color-border);
	}

	.update-btn {
		display: inline-flex;
		align-items: center;
		gap: 4px;
		padding: 5px 14px;
		border: 1px solid var(--color-border);
		border-radius: 4px;
		background: none;
		color: var(--color-text-primary);
		font-size: 12px;
		cursor: pointer;
		text-decoration: none;
	}

	.update-btn:hover {
		background-color: var(--color-bg-hover);
	}

	.update-btn.primary {
		background-color: var(--color-accent);
		border-color: var(--color-accent);
		color: white;
	}

	.update-btn.primary:hover {
		opacity: 0.9;
	}

	.update-btn.small {
		padding: 3px 10px;
		font-size: 11px;
	}

	.update-status {
		font-size: 12px;
		color: var(--color-text-secondary);
	}

	.update-status.success {
		color: #4caf50;
	}

	.update-status.error {
		color: #f44336;
		font-size: 11px;
	}

	.update-available {
		display: flex;
		flex-direction: column;
		gap: 6px;
	}

	.update-new {
		font-size: 13px;
		color: var(--color-accent);
	}

	.update-asset {
		font-size: 11px;
		color: var(--color-text-muted);
	}

	.update-actions {
		display: flex;
		gap: 8px;
		margin-top: 4px;
	}

	.update-mac-instructions {
		display: flex;
		flex-direction: column;
		gap: 4px;
	}

	.mac-hint {
		font-size: 11px;
		color: var(--color-text-secondary);
		margin: 4px 0 0;
	}

	.mac-command {
		font-family: 'JetBrains Mono', 'Fira Code', monospace;
		font-size: 11px;
		background-color: var(--color-bg-primary);
		border: 1px solid var(--color-border);
		border-radius: 4px;
		padding: 6px 10px;
		color: var(--color-accent);
		user-select: all;
		cursor: text;
	}
</style>
