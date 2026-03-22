<script lang="ts">
	import { invoke } from '@tauri-apps/api/core';

	interface ShortcutAction {
		id: string;
		label: string;
	}

	const ACTIONS: ShortcutAction[] = [
		{ id: 'play_pause', label: 'Play / Pause' },
		{ id: 'stop', label: 'Stop' },
		{ id: 'prev_track', label: 'Previous Track' },
		{ id: 'next_track', label: 'Next Track' },
		{ id: 'volume_up', label: 'Volume Up' },
		{ id: 'volume_down', label: 'Volume Down' },
		{ id: 'mute', label: 'Mute / Unmute' },
		{ id: 'toggle_shuffle', label: 'Toggle Shuffle' },
		{ id: 'cycle_repeat', label: 'Cycle Repeat' },
		{ id: 'focus_search', label: 'Focus Search' },
		{ id: 'toggle_fav', label: 'Toggle Favorite' },
	];

	const MOUSE_BUTTONS = [
		{ value: 'MouseMiddle', label: 'Middle Click' },
		{ value: 'MouseBack', label: 'Mouse 4 (Back)' },
		{ value: 'MouseForward', label: 'Mouse 5 (Forward)' },
		{ value: 'Mouse6', label: 'Mouse 6' },
		{ value: 'Mouse7', label: 'Mouse 7' },
		{ value: 'Mouse8', label: 'Mouse 8' },
		{ value: 'Mouse9', label: 'Mouse 9' },
		{ value: 'Mouse10', label: 'Mouse 10' },
	];

	const MODIFIERS = [
		{ value: '', label: 'None' },
		{ value: 'Ctrl', label: 'Ctrl' },
		{ value: 'Alt', label: 'Alt' },
		{ value: 'Shift', label: 'Shift' },
		{ value: 'Ctrl+Shift', label: 'Ctrl+Shift' },
		{ value: 'Ctrl+Alt', label: 'Ctrl+Alt' },
		{ value: 'Alt+Shift', label: 'Alt+Shift' },
	];

	let shortcuts = $state<Record<string, string>>({});
	let recordingId = $state<string | null>(null);
	let mousePickerId = $state<string | null>(null);
	let mouseModifier = $state('');
	let mouseButton = $state('MouseMiddle');
	let loaded = $state(false);

	$effect(() => {
		if (!loaded) {
			loadShortcuts();
		}
	});

	async function loadShortcuts() {
		try {
			const data = await invoke<Record<string, string>>('get_shortcuts');
			shortcuts = data;
		} catch {
			shortcuts = {};
		}
		loaded = true;
	}

	function getBinding(actionId: string): string {
		return shortcuts[actionId] || '';
	}

	function hasModifier(keys: string): boolean {
		if (!keys) return false;
		return keys.includes('Ctrl+') || keys.includes('Shift+') || keys.includes('Alt+') || keys.includes('Meta+');
	}

	function isMouseBinding(keys: string): boolean {
		return keys.includes('Mouse');
	}

	function getScopeLabel(keys: string): string {
		if (!keys) return '';
		if (isMouseBinding(keys)) return 'Global';
		if (hasModifier(keys)) return 'Global';
		return 'App only';
	}

	function normalizeKey(key: string): string {
		switch (key) {
			case ' ': return 'Space';
			case 'ArrowUp': return 'Up';
			case 'ArrowDown': return 'Down';
			case 'ArrowLeft': return 'Left';
			case 'ArrowRight': return 'Right';
			default:
				if (key.length === 1) return key.toUpperCase();
				return key;
		}
	}

	function startRecording(actionId: string) {
		mousePickerId = null;
		recordingId = actionId;
	}

	function cancelRecording() {
		recordingId = null;
	}

	function openMousePicker(actionId: string) {
		recordingId = null;
		mouseModifier = '';
		mouseButton = 'MouseMiddle';
		mousePickerId = actionId;
	}

	function closeMousePicker() {
		mousePickerId = null;
	}

	async function confirmMouseBinding() {
		if (!mousePickerId) return;
		const keys = mouseModifier ? `${mouseModifier}+${mouseButton}` : mouseButton;
		await saveBinding(mousePickerId, keys);
		mousePickerId = null;
	}

	function handleRecordKeydown(e: KeyboardEvent) {
		if (!recordingId) return;
		e.preventDefault();
		e.stopPropagation();

		if (e.key === 'Escape') {
			cancelRecording();
			return;
		}

		if (['Control', 'Shift', 'Alt', 'Meta'].includes(e.key)) return;

		const parts: string[] = [];
		if (e.ctrlKey || e.metaKey) parts.push('Ctrl');
		if (e.shiftKey) parts.push('Shift');
		if (e.altKey) parts.push('Alt');
		parts.push(normalizeKey(e.key));

		saveBinding(recordingId, parts.join('+'));
	}

	function handleRecordMousedown(e: MouseEvent) {
		if (!recordingId) return;
		if (e.button === 0 || e.button === 2) return;
		e.preventDefault();
		e.stopPropagation();

		const btnNames: Record<number, string> = {
			1: 'MouseMiddle',
			3: 'MouseBack',
			4: 'MouseForward',
		};
		const btnName = btnNames[e.button];
		if (!btnName) return;

		saveBinding(recordingId, btnName);
	}

	async function saveBinding(actionId: string, keys: string) {
		shortcuts = { ...shortcuts, [actionId]: keys };
		recordingId = null;
		try {
			await invoke('update_shortcuts', { bindings: shortcuts });
		} catch (e) {
			console.error('Failed to save shortcut:', e);
		}
	}

	async function clearBinding(actionId: string) {
		shortcuts = { ...shortcuts, [actionId]: '' };
		try {
			await invoke('update_shortcuts', { bindings: shortcuts });
		} catch (e) {
			console.error('Failed to clear shortcut:', e);
		}
	}

	async function resetAll() {
		shortcuts = {};
		try {
			await invoke('update_shortcuts', { bindings: shortcuts });
		} catch (e) {
			console.error('Failed to reset shortcuts:', e);
		}
	}

	function formatKeyDisplay(keys: string): string {
		if (!keys) return 'Not set';
		return keys
			.replace('MouseMiddle', 'Middle Click')
			.replace('MouseBack', 'Mouse 4')
			.replace('MouseForward', 'Mouse 5');
	}
</script>

<svelte:window
	onkeydown={recordingId ? handleRecordKeydown : undefined}
	onmousedown={recordingId ? handleRecordMousedown : undefined}
/>

<div class="shortcuts-settings">
	<h3 class="section-title">Shortcuts</h3>
	<p class="section-desc">
		Click the key badge to record a keyboard shortcut, or the mouse icon to assign a mouse button.
		Shortcuts with modifiers work globally, even when minimized.
	</p>

	<div class="shortcut-list">
		{#each ACTIONS as action (action.id)}
			{@const binding = getBinding(action.id)}
			<div class="shortcut-row">
				<span class="action-label">{action.label}</span>
				<div class="binding-area">
					<!-- Keyboard recording button -->
					<button
						class="key-badge"
						class:recording={recordingId === action.id}
						class:unset={!binding}
						onclick={() => startRecording(action.id)}
						title="Click to record keyboard shortcut"
					>
						{#if recordingId === action.id}
							Press key...
						{:else}
							{formatKeyDisplay(binding)}
						{/if}
					</button>

					<!-- Mouse button picker toggle -->
					<button
						class="mouse-btn"
						class:active={mousePickerId === action.id}
						onclick={() => mousePickerId === action.id ? closeMousePicker() : openMousePicker(action.id)}
						title="Assign mouse button"
					>
						<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
							<rect x="6" y="3" width="12" height="18" rx="6" />
							<line x1="12" y1="3" x2="12" y2="10" />
						</svg>
					</button>

					{#if binding}
						<span class="scope-badge" class:global={getScopeLabel(binding) === 'Global'}>
							{getScopeLabel(binding)}
						</span>
						<button class="clear-btn" onclick={() => clearBinding(action.id)} title="Clear">
							<svg width="12" height="12" viewBox="0 0 16 16" fill="currentColor">
								<path d="M8 8.707l3.646 3.647.708-.707L8.707 8l3.647-3.646-.707-.708L8 7.293 4.354 3.646l-.708.708L7.293 8l-3.647 3.646.708.708L8 8.707z" />
							</svg>
						</button>
					{/if}
				</div>
			</div>

			<!-- Mouse picker panel (inline, below the row) -->
			{#if mousePickerId === action.id}
				<div class="mouse-picker">
					<select class="mouse-select" bind:value={mouseModifier}>
						{#each MODIFIERS as mod}
							<option value={mod.value}>{mod.label}</option>
						{/each}
					</select>
					<span class="mouse-plus">+</span>
					<select class="mouse-select" bind:value={mouseButton}>
						{#each MOUSE_BUTTONS as btn}
							<option value={btn.value}>{btn.label}</option>
						{/each}
					</select>
					<button class="mouse-confirm" onclick={confirmMouseBinding}>Assign</button>
					<button class="mouse-cancel" onclick={closeMousePicker}>Cancel</button>
				</div>
			{/if}
		{/each}
	</div>

	<div class="shortcuts-footer">
		<button class="reset-btn" onclick={resetAll}>Reset all</button>
		<span class="footer-note">Media keys (Play/Pause, Next, Prev, Stop) are always active.</span>
	</div>
</div>

<style>
	.shortcuts-settings {
		display: flex;
		flex-direction: column;
		gap: 12px;
	}

	.section-title {
		font-size: 14px;
		font-weight: 600;
		color: var(--color-text-primary);
		margin: 0;
	}

	.section-desc {
		font-size: 11px;
		color: var(--color-text-secondary);
		margin: 0;
		line-height: 1.4;
	}

	.shortcut-list {
		display: flex;
		flex-direction: column;
		gap: 2px;
	}

	.shortcut-row {
		display: flex;
		align-items: center;
		justify-content: space-between;
		padding: 5px 8px;
		border-radius: 4px;
	}

	.shortcut-row:hover {
		background-color: var(--color-bg-hover);
	}

	.action-label {
		font-size: 13px;
		color: var(--color-text-primary);
		min-width: 130px;
	}

	.binding-area {
		display: flex;
		align-items: center;
		gap: 4px;
	}

	.key-badge {
		background-color: var(--color-bg-tertiary);
		border: 1px solid var(--color-border);
		border-radius: 4px;
		padding: 3px 10px;
		font-size: 11px;
		font-family: var(--font-mono, monospace);
		color: var(--color-text-primary);
		cursor: pointer;
		min-width: 70px;
		text-align: center;
		white-space: nowrap;
	}

	.key-badge:hover {
		border-color: var(--color-accent);
	}

	.key-badge.recording {
		border-color: var(--color-accent);
		background-color: rgba(var(--color-accent-rgb, 0, 120, 212), 0.15);
		animation: pulse 1s infinite;
		font-family: inherit;
		font-style: italic;
		color: var(--color-text-secondary);
	}

	.key-badge.unset {
		color: var(--color-text-muted);
		font-style: italic;
		font-family: inherit;
	}

	@keyframes pulse {
		0%, 100% { opacity: 1; }
		50% { opacity: 0.6; }
	}

	.mouse-btn {
		background: none;
		border: 1px solid var(--color-border);
		border-radius: 4px;
		color: var(--color-text-muted);
		cursor: pointer;
		padding: 3px 5px;
		display: flex;
		align-items: center;
	}

	.mouse-btn:hover, .mouse-btn.active {
		color: var(--color-accent);
		border-color: var(--color-accent);
	}

	.mouse-picker {
		display: flex;
		align-items: center;
		gap: 6px;
		padding: 6px 8px 6px 140px;
		background-color: var(--color-bg-tertiary);
		border-radius: 4px;
		margin-top: 2px;
	}

	.mouse-select {
		background-color: var(--color-bg-secondary);
		border: 1px solid var(--color-border);
		border-radius: 3px;
		color: var(--color-text-primary);
		font-size: 12px;
		padding: 4px 8px;
		cursor: pointer;
		-webkit-appearance: none;
		appearance: none;
		background-image: url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='8' height='8' viewBox='0 0 8 8'%3E%3Cpath fill='%23999' d='M0 2l4 4 4-4z'/%3E%3C/svg%3E");
		background-repeat: no-repeat;
		background-position: right 6px center;
		padding-right: 20px;
	}

	.mouse-select option {
		background-color: var(--color-bg-secondary);
		color: var(--color-text-primary);
	}

	.mouse-plus {
		color: var(--color-text-muted);
		font-size: 12px;
	}

	.mouse-confirm {
		background-color: var(--color-accent);
		border: none;
		border-radius: 3px;
		color: white;
		font-size: 11px;
		padding: 3px 10px;
		cursor: pointer;
	}

	.mouse-confirm:hover {
		opacity: 0.9;
	}

	.mouse-cancel {
		background: none;
		border: 1px solid var(--color-border);
		border-radius: 3px;
		color: var(--color-text-secondary);
		font-size: 11px;
		padding: 3px 8px;
		cursor: pointer;
	}

	.mouse-cancel:hover {
		color: var(--color-text-primary);
	}

	.scope-badge {
		font-size: 9px;
		text-transform: uppercase;
		letter-spacing: 0.5px;
		padding: 1px 5px;
		border-radius: 3px;
		background-color: var(--color-bg-tertiary);
		color: var(--color-text-muted);
		white-space: nowrap;
	}

	.scope-badge.global {
		background-color: rgba(var(--color-accent-rgb, 0, 120, 212), 0.2);
		color: var(--color-accent);
	}

	.clear-btn {
		background: none;
		border: none;
		color: var(--color-text-muted);
		cursor: pointer;
		padding: 2px;
		border-radius: 3px;
		display: flex;
		align-items: center;
	}

	.clear-btn:hover {
		color: var(--color-text-primary);
		background-color: var(--color-bg-hover);
	}

	.shortcuts-footer {
		display: flex;
		align-items: center;
		justify-content: space-between;
		padding-top: 8px;
		border-top: 1px solid var(--color-border);
	}

	.reset-btn {
		background: none;
		border: 1px solid var(--color-border);
		color: var(--color-text-secondary);
		padding: 4px 12px;
		border-radius: 4px;
		font-size: 12px;
		cursor: pointer;
	}

	.reset-btn:hover {
		color: var(--color-text-primary);
		border-color: var(--color-text-secondary);
	}

	.footer-note {
		font-size: 10px;
		color: var(--color-text-muted);
	}
</style>
