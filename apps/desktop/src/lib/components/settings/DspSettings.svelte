<script lang="ts">
	import { settingsStore, type DspConfig } from '$lib/stores/settings.svelte';

	const dsp = $derived(settingsStore.dsp);

	/**
	 * Balance sits *after* the mono downmix in the chain, so an off-centre balance
	 * re-splits what mono just collapsed. That is intentional — it lets you pan a
	 * mono signal — but a slider left a few percent off centre makes mono look
	 * completely broken, so say so instead of letting it be a puzzle.
	 */
	const monoDefeatedByBalance = $derived(dsp.mono && dsp.balance !== 0);

	/**
	 * What the audio engine reports it is running, read back after every change.
	 * The panel shows this rather than the local state so a knob that never
	 * reaches the audio thread is visible here instead of being a listening
	 * puzzle — which is exactly how the mono downmix was first reported.
	 */
	const engine = $derived(settingsStore.dspEngine);

	const engineSummary = $derived.by(() => {
		if (!engine) return 'engine not responding';
		const on: string[] = [];
		if (engine.mono) {
			const tags = [
				engine.mono_compensate && 'compensated',
				engine.mono_phase_safe && 'phase-safe'
			].filter(Boolean);
			on.push(tags.length ? `mono (${tags.join(', ')})` : 'mono');
		}
		if (engine.balance !== 0) on.push(`balance ${fmtBalance(engine.balance)}`);
		if (engine.width_enabled && engine.width !== 1) on.push(`width ${fmtWidth(engine.width)}`);
		if (
			engine.eq_enabled &&
			(engine.eq_low_db || engine.eq_mid_db || engine.eq_high_db)
		)
			on.push('eq');
		if (engine.preamp_enabled && engine.preamp_db !== 0) on.push(`preamp ${fmtDb(engine.preamp_db)}`);
		if (engine.limiter) on.push('limiter');
		return on.length ? on.join(' · ') : 'nothing active — audio is untouched';
	});

	/** The local state and the engine disagreeing means a change never landed. */
	const outOfSync = $derived(
		engine !== null &&
			(engine.mono !== dsp.mono ||
				engine.balance !== dsp.balance ||
				engine.width_enabled !== dsp.width_enabled ||
				engine.eq_enabled !== dsp.eq_enabled ||
				engine.preamp_enabled !== dsp.preamp_enabled ||
				engine.limiter !== dsp.limiter)
	);

	function fmtDb(v: number) {
		return `${v > 0 ? '+' : ''}${v.toFixed(1)} dB`;
	}

	function fmtBalance(v: number) {
		if (v === 0) return 'Centre';
		return `${v < 0 ? 'L' : 'R'} ${Math.round(Math.abs(v) * 100)}%`;
	}

	function fmtWidth(v: number) {
		if (v === 0) return 'Mono';
		return `${Math.round(v * 100)}%`;
	}

	interface SliderProps {
		label: string;
		key: keyof DspConfig;
		min: number;
		max: number;
		step: number;
		format: (v: number) => string;
		disabled?: boolean;
		desc?: string;
	}
</script>

{#snippet slider(p: SliderProps)}
	<div class="setting-row slider-row" class:disabled={p.disabled}>
		<div class="setting-text">
			<span class="setting-label">{p.label}</span>
			{#if p.desc}<span class="setting-desc">{p.desc}</span>{/if}
		</div>
		<div class="slider-group">
			<input
				type="range"
				min={p.min}
				max={p.max}
				step={p.step}
				value={dsp[p.key] as number}
				disabled={p.disabled}
				oninput={(e) =>
					settingsStore.applyDsp({
						[p.key]: parseFloat((e.target as HTMLInputElement).value)
					} as Partial<DspConfig>)}
				ondblclick={() => settingsStore.resetDspKey(p.key)}
				title="Double-click to reset"
			/>
			<span class="slider-value">{p.format(dsp[p.key] as number)}</span>
			<button
				class="reset-knob"
				disabled={p.disabled}
				onclick={() => settingsStore.resetDspKey(p.key)}
				title="Reset to default"
				aria-label="Reset {p.label}"
			>
				<svg width="13" height="13" viewBox="0 0 16 16" fill="currentColor">
					<path
						d="M8 3a5 5 0 104.546 2.914l-.94.437A4 4 0 118 4v2.5l3-2.75L8 1v2z"
					/>
				</svg>
			</button>
		</div>
	</div>
{/snippet}

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

	<label class="setting-row sub-row" class:disabled={!dsp.mono}>
		<input
			type="checkbox"
			checked={dsp.mono_compensate}
			disabled={!dsp.mono}
			onchange={(e) =>
				settingsStore.applyDsp({ mono_compensate: (e.target as HTMLInputElement).checked })}
		/>
		<div class="setting-text">
			<span class="setting-label">Compensate level</span>
			<span class="setting-desc"
				>Summing to mono costs level, and how much depends on the material: identical
				channels lose nothing, independent ones lose 3 dB, and out-of-phase ones cancel and
				lose more. SNES rips are a bad case — the SPC700's volume registers are signed, so
				games pan voices out of phase, and Seiken Densetsu 3 measures at −4.6 to −6.8 dB.
				This restores the gap so the downmix lands where the track was.</span
			>
		</div>
	</label>

	<label class="setting-row sub-row" class:disabled={!dsp.mono}>
		<input
			type="checkbox"
			checked={dsp.mono_phase_safe}
			disabled={!dsp.mono}
			onchange={(e) =>
				settingsStore.applyDsp({ mono_phase_safe: (e.target as HTMLInputElement).checked })}
		/>
		<div class="setting-text">
			<span class="setting-label">Phase-safe downmix</span>
			<span class="setting-desc"
				>Turn this on if instruments <em>disappear</em> when you go mono. A plain downmix
				destroys anything that only exists as a difference between the channels — a drum
				panned as +d left and −d right sums to exactly zero. This inverts the right channel
				while the two are anti-phase so those parts reinforce instead of cancelling.
				Measured on Seiken Densetsu 3 SPCs it recovers +2.2 to +5.9 dB of content. It is a
				real alteration of the signal, not a neutral mix, which is why it is off by
				default.</span
			>
		</div>
	</label>

	{#if monoDefeatedByBalance}
		<div class="warn-row">
			<span
				>Balance is at <strong>{fmtBalance(dsp.balance)}</strong> and runs after the downmix,
				so the output is still uneven. Centre it to hear true mono.</span
			>
			<button class="warn-btn" onclick={() => settingsStore.resetDspKey('balance')}
				>Centre balance</button
			>
		</div>
	{/if}

	{@render slider({
		label: 'Balance',
		key: 'balance',
		min: -1,
		max: 1,
		step: 0.01,
		format: fmtBalance,
		desc: 'Pans between left and right. Attenuates only, never boosts.'
	})}

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

	{@render slider({
		label: 'Width',
		key: 'width',
		min: 0,
		max: 2,
		step: 0.01,
		format: fmtWidth,
		disabled: !dsp.width_enabled
	})}

	<label class="setting-row">
		<input
			type="checkbox"
			checked={dsp.eq_enabled}
			onchange={(e) =>
				settingsStore.applyDsp({ eq_enabled: (e.target as HTMLInputElement).checked })}
		/>
		<div class="setting-text">
			<span class="setting-label">Equalizer</span>
			<span class="setting-desc"
				>Three bands: low shelf at 200 Hz, peak at 1 kHz, high shelf at 4 kHz.</span
			>
		</div>
	</label>

	{@render slider({
		label: 'Bass (200 Hz)',
		key: 'eq_low_db',
		min: -20,
		max: 20,
		step: 0.5,
		format: fmtDb,
		disabled: !dsp.eq_enabled
	})}
	{@render slider({
		label: 'Mid (1 kHz)',
		key: 'eq_mid_db',
		min: -20,
		max: 20,
		step: 0.5,
		format: fmtDb,
		disabled: !dsp.eq_enabled
	})}
	{@render slider({
		label: 'Treble (4 kHz)',
		key: 'eq_high_db',
		min: -20,
		max: 20,
		step: 0.5,
		format: fmtDb,
		disabled: !dsp.eq_enabled
	})}

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
				>Flat gain, to even out rips that sit at wildly different levels. Summing a
				hard-panned rip to mono can cost several dB, so this pairs well with it. Turn the
				limiter on if you push it up.</span
			>
		</div>
	</label>

	{@render slider({
		label: 'Gain',
		key: 'preamp_db',
		min: -20,
		max: 20,
		step: 0.5,
		format: fmtDb,
		disabled: !dsp.preamp_enabled
	})}

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

	<div class="engine-row" class:out-of-sync={outOfSync}>
		<span class="engine-dot" class:live={engine !== null && !outOfSync}></span>
		<span
			>Running in the audio engine: <strong>{engineSummary}</strong>{#if outOfSync}
				— this does not match the controls above, so a change did not reach the audio thread.
			{/if}</span
		>
	</div>

	<p class="chain-note">
		Chain order: equalizer → stereo width → mono → balance → preamp → limiter. Each slider resets
		with its button or a double-click.
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

	.sub-row {
		margin-left: 28px;
	}

	.setting-row.disabled input[type='checkbox'] {
		cursor: not-allowed;
	}

	.slider-row {
		padding-left: 36px;
		justify-content: space-between;
		align-items: center;
		cursor: default;
		gap: 16px;
	}

	.slider-group {
		display: flex;
		align-items: center;
		gap: 10px;
		flex-shrink: 0;
	}

	.slider-group input[type='range'] {
		width: 190px;
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

	.reset-knob {
		display: flex;
		align-items: center;
		justify-content: center;
		padding: 3px;
		background: none;
		border: none;
		border-radius: 3px;
		color: var(--color-text-secondary);
		cursor: pointer;
	}

	.reset-knob:hover:not(:disabled) {
		color: var(--color-text-primary);
		background-color: var(--color-bg-hover);
	}

	.reset-knob:disabled {
		opacity: 0.4;
		cursor: not-allowed;
	}

	.warn-row {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: 12px;
		margin: -10px 8px 0 36px;
		padding: 8px 10px;
		border-radius: 4px;
		border: 1px solid var(--color-border);
		background-color: var(--color-bg-secondary);
		font-size: 11px;
		color: var(--color-text-secondary);
	}

	.warn-btn {
		flex-shrink: 0;
		padding: 3px 10px;
		background-color: var(--color-bg-primary);
		border: 1px solid var(--color-border);
		border-radius: 4px;
		color: var(--color-text-primary);
		font-size: 11px;
		cursor: pointer;
	}

	.warn-btn:hover {
		background-color: var(--color-bg-hover);
	}

	.engine-row {
		display: flex;
		align-items: center;
		gap: 8px;
		padding: 8px 10px;
		border-radius: 4px;
		border: 1px solid var(--color-border);
		background-color: var(--color-bg-secondary);
		font-size: 11px;
		color: var(--color-text-secondary);
	}

	.engine-row.out-of-sync {
		border-color: #b4544a;
	}

	.engine-dot {
		flex-shrink: 0;
		width: 7px;
		height: 7px;
		border-radius: 50%;
		background-color: #b4544a;
	}

	.engine-dot.live {
		background-color: #5aa469;
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
