<script lang="ts">
</script>

<div class="about-settings">
	<div class="about-header">
		<svg width="48" height="48" viewBox="0 0 16 16" fill="var(--color-accent)">
			<path d="M8 1.5a6.5 6.5 0 100 13 6.5 6.5 0 000-13zM0 8a8 8 0 1116 0A8 8 0 010 8z" />
			<path d="M5 5.5a1 1 0 012 0v4a1 1 0 01-2 0v-4zm4 0a1 1 0 012 0v4a1 1 0 01-2 0v-4z" />
		</svg>
		<div class="about-title-block">
			<h2 class="about-title">Tunante</h2>
			<span class="about-version">v0.1.0</span>
		</div>
	</div>

	<p class="about-tagline">
		A cross-platform music player focused on video game music formats, inspired by foobar2000.
	</p>

	<div class="about-section">
		<span class="about-credit">
			Vibecoded by <strong>Cidwel</strong> to understand how much I'm replaceable by AI.
		</span>
	</div>

	<div class="about-section">
		<h4 class="about-label">Source Code</h4>
		<a class="about-link" href="https://github.com/" target="_blank" rel="noopener noreferrer">
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
</style>
