<script lang="ts">
	import { settingsStore } from '$lib/stores/settings.svelte';

	const dsp = $derived(settingsStore.dsp);

	function fmtDb(v: number) {
		return `${v > 0 ? '+' : ''}${v.toFixed(1)} dB`;
	}

	function fmtBalance(v: number) {
		if (Math.abs(v) < 0.005) return 'Centre';
		const side = v < 0 ? 'L' : 'R';
		return `${side} ${Math.round(Math.abs(v) * 100)}%`;
	}

	function fmtWidth(v: number) {
		if (Math.abs(v) < 0.005) return 'Mono';
		return `${Math.round(v * 100)}%`;
	}
</script>

<div class="settings-section">
	<h3 class="section-title">DSP</h3>
	<p class="section-desc">
		Effects applied inside tunante, before the audio reaches the system mixer — so they work the
		same on Linux, Windows and macOS, and apply to every supported format. Changes take effect on
		the track that is already playing.
	</p>

	<label class="setting-row">
		<input
			type="checkbox"
			checked={dsp.mono}
			onchange={(e) => settingsStore.applyDsp({ mono: (e.target as HTMLInputElement).checked })}
		/>
		<div class="setting-text">
			<span class="setting-label">Mono downmix</span>
			<span class="setting-desc"
				>Average all channels into one. Useful for hard-panned chiptune rips, and for
				listening on a single speaker.</span
			>
		</div>
	</label>

	<div class="setting-row slider-row">
		<div class="setting-text">
			<span class="setting-label">Balance</span>
			<span class="setting-desc">Pans between left and right. Attenuates only, never boosts.</span>
		</div>
		<div class="slider-group">
			<input
				type="range"
				min="-1"
				max="1"
				step="0.01"
				value={dsp.balance}
				oninput={(e) =>
					settingsStore.applyDsp({ balance: parseFloat((e.target as HTMLInputElement).value) })}
			/>
			<span class="slider-value">{fmtBalance(dsp.balance)}</span>
		</div>
	</div>

	<label class="setting-row">
		<input
			type="checkbox"
			checked={dsp.width_enabled}
			onchange={(e) =>
				settingsStore.applyDsp({ width_enabled: (e.target as HTMLInputElement).checked })}
		/>
		<div class="setting-text">
			<span class="setting-label">Stereo width</span>
			<span class="setting-desc"
				>Narrows or widens the stereo image. NES and SNES rips are often hard-panned per
				channel, which narrows well for headphones.</span
			>
		</div>
	</label>

	<div class="setting-row slider-row" class:disabled={!dsp.width_enabled}>
		<span class="slider-label">Width</span>
		<div class="slider-group">
			<input
				type="range"
				min="0"
				max="2"
				step="0.01"
				value={dsp.width}
				disabled={!dsp.width_enabled}
				oninput={(e) =>
					settingsStore.applyDsp({ width: parseFloat((e.target as HTMLInputElement).value) })}
			/>
			<span class="slider-value">{fmtWidth(dsp.width)}</span>
		</div>
	</div>

	<label class="setting-row">
		<input
			type="checkbox"
			checked={dsp.eq_enabled}
			onchange={(e) =>
				settingsStore.applyDsp({ eq_enabled: (e.target as HTMLInputElement).checked })}
		/>
		<div class="setting-text">
			<span class="setting-label">Equalizer</span>
			<span class="setting-desc">Three bands: low shelf at 200 Hz, peak at 1 kHz, high shelf at 4 kHz.</span>
		</div>
	</label>

	{#each [{ key: 'eq_low_db' as const, label: 'Bass (200 Hz)' }, { key: 'eq_mid_db' as const, label: 'Mid (1 kHz)' }, { key: 'eq_high_db' as const, label: 'Treble (4 kHz)' }] as band}
		<div class="setting-row slider-row" class:disabled={!dsp.eq_enabled}>
			<span class="slider-label">{band.label}</span>
			<div class="slider-group">
				<input
					type="range"
					min="-20"
					max="20"
					step="0.5"
					value={dsp[band.key]}
					disabled={!dsp.eq_enabled}
					oninput={(e) =>
						settingsStore.applyDsp({
							[band.key]: parseFloat((e.target as HTMLInputElement).value)
						})}
				/>
				<span class="slider-value">{fmtDb(dsp[band.key])}</span>
			</div>
		</div>
	{/each}

	<label class="setting-row">
		<input
			type="checkbox"
			checked={dsp.preamp_enabled}
			onchange={(e) =>
				settingsStore.applyDsp({ preamp_enabled: (e.target as HTMLInputElement).checked })}
		/>
		<div class="setting-text">
			<span class="setting-label">Preamp</span>
			<span class="setting-desc"
				>Flat gain, to even out rips that sit at wildly different levels. Turn the limiter on
				if you push this up.</span
			>
		</div>
	</label>

	<div class="setting-row slider-row" class:disabled={!dsp.preamp_enabled}>
		<span class="slider-label">Gain</span>
		<div class="slider-group">
			<input
				type="range"
				min="-20"
				max="20"
				step="0.5"
				value={dsp.preamp_db}
				disabled={!dsp.preamp_enabled}
				oninput={(e) =>
					settingsStore.applyDsp({ preamp_db: parseFloat((e.target as HTMLInputElement).value) })}
			/>
			<span class="slider-value">{fmtDb(dsp.preamp_db)}</span>
		</div>
	</div>

	<label class="setting-row">
		<input
			type="checkbox"
			checked={dsp.limiter}
			onchange={(e) => settingsStore.applyDsp({ limiter: (e.target as HTMLInputElement).checked })}
		/>
		<div class="setting-text">
			<span class="setting-label">Limiter</span>
			<span class="setting-desc"
				>Catches peaks above full scale instead of letting them clip. Sits last in the chain,
				so it protects the preamp and the equalizer.</span
			>
		</div>
	</label>

	<div class="setting-row">
		<button class="reset-btn" onclick={() => settingsStore.resetDsp()}>Reset all effects</button>
	</div>

	<p class="chain-note">
		Chain order: equalizer → stereo width → mono → balance → preamp → limiter.
	</p>
</div>

<style>
	.settings-section {
		display: flex;
		flex-direction: column;
		gap: 16px;
	}

	.section-title {
		font-size: 14px;
		font-weight: 600;
		color: var(--color-text-primary);
		margin: 0;
	}

	.section-desc,
	.chain-note {
		font-size: 11px;
		color: var(--color-text-secondary);
		margin: -8px 0 0 0;
	}

	.chain-note {
		margin: 0;
		font-style: italic;
	}

	.setting-row {
		display: flex;
		align-items: flex-start;
		gap: 10px;
		cursor: pointer;
		padding: 8px;
		border-radius: 4px;
	}

	.setting-row:hover {
		background-color: var(--color-bg-hover);
	}

	.setting-row input[type='checkbox'] {
		margin-top: 2px;
		accent-color: var(--color-accent);
		cursor: pointer;
	}

	.setting-text {
		display: flex;
		flex-direction: column;
		gap: 2px;
	}

	.setting-label {
		font-size: 13px;
		color: var(--color-text-primary);
	}

	.setting-desc {
		font-size: 11px;
		color: var(--color-text-secondary);
	}

	.setting-row.disabled {
		opacity: 0.45;
		cursor: not-allowed;
	}

	.setting-row.disabled input[type='range'] {
		cursor: not-allowed;
	}

	.slider-row {
		padding-left: 36px;
		justify-content: space-between;
		align-items: center;
		cursor: default;
	}

	.slider-label {
		font-size: 13px;
		color: var(--color-text-primary);
	}

	.slider-group {
		display: flex;
		align-items: center;
		gap: 10px;
		flex-shrink: 0;
	}

	.slider-group input[type='range'] {
		width: 200px;
		accent-color: var(--color-accent);
		cursor: pointer;
	}

	.slider-value {
		font-size: 12px;
		color: var(--color-text-secondary);
		width: 64px;
		text-align: right;
		font-variant-numeric: tabular-nums;
	}

	.reset-btn {
		padding: 5px 12px;
		background-color: var(--color-bg-primary);
		border: 1px solid var(--color-border);
		border-radius: 4px;
		color: var(--color-text-primary);
		font-size: 13px;
		cursor: pointer;
	}

	.reset-btn:hover {
		background-color: var(--color-bg-hover);
	}
</style>
