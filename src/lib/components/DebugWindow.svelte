<script lang="ts">
	import { invoke } from '@tauri-apps/api/core';
	import { onMount } from 'svelte';

	interface LogEntry {
		timestamp: string;
		level: string;
		target: string;
		message: string;
	}

	let { onclose }: { onclose: () => void } = $props();

	let logs = $state<LogEntry[]>([]);
	let filterLevel = $state('all');
	let filterText = $state('');
	let autoScroll = $state(true);
	let logContainer: HTMLElement | undefined = $state();
	let refreshInterval: ReturnType<typeof setInterval> | undefined;

	const levelColors: Record<string, string> = {
		ERROR: '#ff5555',
		WARN: '#ffb86c',
		INFO: '#8be9fd',
		DEBUG: '#6272a4',
		TRACE: '#44475a',
	};

	let filteredLogs = $derived(() => {
		let result = logs;
		if (filterLevel !== 'all') {
			result = result.filter((l) => l.level === filterLevel);
		}
		if (filterText.trim()) {
			const q = filterText.toLowerCase();
			result = result.filter(
				(l) =>
					l.message.toLowerCase().includes(q) ||
					l.target.toLowerCase().includes(q)
			);
		}
		return result;
	});

	async function loadLogs() {
		try {
			logs = await invoke<LogEntry[]>('get_debug_logs');
			if (autoScroll && logContainer) {
				requestAnimationFrame(() => {
					if (logContainer) {
						logContainer.scrollTop = logContainer.scrollHeight;
					}
				});
			}
		} catch (e) {
			console.error('Failed to load debug logs:', e);
		}
	}

	async function clearLogs() {
		try {
			await invoke('clear_debug_logs');
			logs = [];
		} catch (e) {
			console.error('Failed to clear debug logs:', e);
		}
	}

	function handleKeydown(e: KeyboardEvent) {
		if (e.key === 'Escape') {
			onclose();
		}
	}

	async function copyLogs() {
		const text = filteredLogs()
			.map((e) => `${e.timestamp} ${e.level} [${e.target}] ${e.message}`)
			.join('\n');
		try {
			await navigator.clipboard.writeText(text);
		} catch {
			// Fallback: select all in container
			if (logContainer) {
				const range = document.createRange();
				range.selectNodeContents(logContainer);
				const sel = window.getSelection();
				sel?.removeAllRanges();
				sel?.addRange(range);
			}
		}
	}

	onMount(() => {
		loadLogs();
		// Auto-refresh every 2 seconds
		refreshInterval = setInterval(loadLogs, 2000);
		return () => {
			if (refreshInterval) clearInterval(refreshInterval);
		};
	});
</script>

<svelte:window onkeydown={handleKeydown} />

<div class="debug-overlay" role="dialog" aria-label="Debug Logs">
	<div class="debug-window">
		<div class="debug-header">
			<h2>Debug Logs</h2>
			<div class="debug-controls">
				<select bind:value={filterLevel} class="level-filter">
					<option value="all">All Levels</option>
					<option value="ERROR">Error</option>
					<option value="WARN">Warn</option>
					<option value="INFO">Info</option>
					<option value="DEBUG">Debug</option>
				</select>
				<input
					type="text"
					bind:value={filterText}
					placeholder="Filter..."
					class="filter-input"
				/>
				<label class="auto-scroll-label">
					<input type="checkbox" bind:checked={autoScroll} />
					Auto-scroll
				</label>
				<button class="btn-small" onclick={loadLogs}>Refresh</button>
				<button class="btn-small" onclick={copyLogs}>Copy</button>
				<button class="btn-small btn-danger" onclick={clearLogs}>Clear</button>
				<button class="btn-close" onclick={onclose}>&#x2715;</button>
			</div>
		</div>
		<div class="log-container" bind:this={logContainer}>
			{#each filteredLogs() as entry}
				<div class="log-entry" style="--level-color: {levelColors[entry.level] || '#888'}">
					<span class="log-time">{entry.timestamp}</span>
					<span class="log-level" style="color: {levelColors[entry.level] || '#888'}">{entry.level.padEnd(5)}</span>
					<span class="log-target">[{entry.target}]</span>
					<span class="log-msg">{entry.message}</span>
				</div>
			{:else}
				<div class="log-empty">No log entries{filterLevel !== 'all' || filterText ? ' matching filter' : ''}</div>
			{/each}
		</div>
		<div class="debug-footer">
			<span>{filteredLogs().length} / {logs.length} entries</span>
		</div>
	</div>
</div>

<style>
	.debug-overlay {
		position: fixed;
		inset: 0;
		z-index: 300;
		background: rgba(0, 0, 0, 0.6);
		display: flex;
		align-items: center;
		justify-content: center;
	}

	.debug-window {
		width: 90vw;
		height: 80vh;
		max-width: 1200px;
		background: #1a1a2e;
		border: 1px solid #333;
		border-radius: 8px;
		display: flex;
		flex-direction: column;
		box-shadow: 0 8px 32px rgba(0, 0, 0, 0.6);
	}

	.debug-header {
		display: flex;
		align-items: center;
		justify-content: space-between;
		padding: 10px 14px;
		border-bottom: 1px solid #333;
		flex-shrink: 0;
	}

	.debug-header h2 {
		margin: 0;
		font-size: 14px;
		font-weight: 600;
		color: #eee;
	}

	.debug-controls {
		display: flex;
		align-items: center;
		gap: 8px;
	}

	.level-filter,
	.filter-input {
		background: #16213e;
		border: 1px solid #444;
		border-radius: 4px;
		color: #ccc;
		font-size: 12px;
		padding: 4px 8px;
	}

	.filter-input {
		width: 150px;
	}

	.auto-scroll-label {
		display: flex;
		align-items: center;
		gap: 4px;
		font-size: 12px;
		color: #999;
		cursor: pointer;
		white-space: nowrap;
	}

	.btn-small {
		padding: 4px 10px;
		border: 1px solid #444;
		border-radius: 4px;
		background: #16213e;
		color: #ccc;
		font-size: 12px;
		cursor: pointer;
	}

	.btn-small:hover {
		background: #1a3359;
	}

	.btn-danger {
		border-color: #663333;
	}

	.btn-danger:hover {
		background: #4a1a1a;
	}

	.btn-close {
		background: none;
		border: none;
		color: #888;
		font-size: 18px;
		cursor: pointer;
		padding: 0 4px;
		line-height: 1;
	}

	.btn-close:hover {
		color: #fff;
	}

	.log-container {
		flex: 1;
		overflow-y: auto;
		font-family: 'JetBrains Mono', 'Fira Code', 'Cascadia Code', monospace;
		font-size: 12px;
		line-height: 1.5;
		padding: 4px 0;
	}

	.log-entry {
		display: flex;
		gap: 8px;
		user-select: text;
		cursor: text;
		padding: 1px 12px;
		white-space: nowrap;
	}

	.log-entry:hover {
		background: rgba(255, 255, 255, 0.04);
	}

	.log-time {
		color: #666;
		flex-shrink: 0;
	}

	.log-level {
		flex-shrink: 0;
		font-weight: 600;
		width: 42px;
	}

	.log-target {
		color: #888;
		flex-shrink: 0;
		max-width: 200px;
		overflow: hidden;
		text-overflow: ellipsis;
	}

	.log-msg {
		color: #ddd;
		white-space: pre-wrap;
		word-break: break-all;
		text-overflow: ellipsis;
	}

	.log-empty {
		padding: 20px;
		text-align: center;
		color: #666;
	}

	.debug-footer {
		padding: 6px 14px;
		border-top: 1px solid #333;
		font-size: 11px;
		color: #666;
		flex-shrink: 0;
	}
</style>
