import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { Track, RepeatMode } from '$lib/types';

class PlayerStore {
	isPlaying = $state(false);
	currentTrack = $state<Track | null>(null);
	positionMs = $state(0);
	durationMs = $state(0);
	volume = $state(0.8);
	shuffle = $state(false);
	repeat = $state<RepeatMode>('off');
	userQueue = $state<Track[]>([]);
	errorMessage = $state<string | null>(null);
	private _queuedIds = $state<Set<string>>(new Set());
	private _initialized = false;
	private _errorTimeout: ReturnType<typeof setTimeout> | null = null;

	get progress(): number {
		return this.durationMs > 0 ? this.positionMs / this.durationMs : 0;
	}

	get positionFormatted(): string {
		return this.formatTime(this.positionMs);
	}

	get durationFormatted(): string {
		return this.formatTime(this.durationMs);
	}

	private formatTime(ms: number): string {
		const totalSeconds = Math.floor(ms / 1000);
		const minutes = Math.floor(totalSeconds / 60);
		const seconds = totalSeconds % 60;
		return `${minutes}:${seconds.toString().padStart(2, '0')}`;
	}

	async init() {
		if (this._initialized) return;
		this._initialized = true;

		await listen<{
			is_playing: boolean;
			position_ms: number;
			duration_ms: number;
			volume: number;
		}>('player-state-update', (event) => {
			this.isPlaying = event.payload.is_playing;
			this.positionMs = event.payload.position_ms;
			this.durationMs = event.payload.duration_ms;
			this.volume = event.payload.volume;
		});

		await listen<number>('volume-scrolled', (event) => {
			this.volume = event.payload;
		});

		await listen<Track>('track-changed', (event) => {
			this.currentTrack = event.payload;
			this.positionMs = 0;
		});

		await listen('playback-stopped', () => {
			this.isPlaying = false;
			this.currentTrack = null;
			this.positionMs = 0;
			this.durationMs = 0;
		});

		await listen<{ message: string; path: string }>('playback-error', (event) => {
			const filename = event.payload.path.split('/').pop() || event.payload.path;
			this.showError(`Failed to play "${filename}": ${event.payload.message}`);
		});
	}

	showError(message: string) {
		this.errorMessage = message;
		if (this._errorTimeout) clearTimeout(this._errorTimeout);
		this._errorTimeout = setTimeout(() => {
			this.errorMessage = null;
		}, 8000);
	}

	dismissError() {
		this.errorMessage = null;
		if (this._errorTimeout) {
			clearTimeout(this._errorTimeout);
			this._errorTimeout = null;
		}
	}

	async playTrack(track: Track, contextTrackIds?: string[]) {
		try {
			await invoke('play_file', {
				path: track.path,
				trackIds: contextTrackIds ?? null,
			});
			this.currentTrack = track;
			this.isPlaying = true;
		} catch (e) {
			const msg = e instanceof Error ? e.message : String(e);
			this.showError(`Failed to play "${track.title || track.path}": ${msg}`);
		}
	}

	async togglePlayPause() {
		try {
			if (this.isPlaying) {
				await invoke('pause');
				this.isPlaying = false;
			} else {
				await invoke('resume');
				this.isPlaying = true;
			}
		} catch (e) {
			console.error('Failed to toggle play/pause:', e);
		}
	}

	async stop() {
		try {
			await invoke('stop');
			this.isPlaying = false;
			this.positionMs = 0;
		} catch (e) {
			console.error('Failed to stop:', e);
		}
	}

	async seek(positionMs: number) {
		// Optimistic update — the seek command returns immediately (non-blocking).
		// The actual seek happens on a background thread. Errors arrive via
		// the 'playback-error' event (handled by init()).
		this.positionMs = positionMs;
		try {
			await invoke('seek', { positionMs });
		} catch (e) {
			const msg = e instanceof Error ? e.message : String(e);
			this.showError(`Seek failed: ${msg}`);
		}
	}

	async setVolume(volume: number) {
		const clamped = Math.max(0, Math.min(1, volume));
		try {
			await invoke('set_volume', { volume: clamped });
			this.volume = clamped;
		} catch (e) {
			console.error('Failed to set volume:', e);
		}
	}

	async nextTrack() {
		try {
			await invoke('next_track');
		} catch (e) {
			console.error('Failed to next track:', e);
		}
	}

	async prevTrack() {
		try {
			await invoke('prev_track');
		} catch (e) {
			console.error('Failed to prev track:', e);
		}
	}

	toggleShuffle() {
		this.shuffle = !this.shuffle;
		invoke('set_shuffle', { enabled: this.shuffle }).catch((e) => {
			console.error('Failed to sync shuffle:', e);
		});
	}

	cycleRepeat() {
		const modes: RepeatMode[] = ['off', 'all', 'one'];
		const idx = modes.indexOf(this.repeat);
		this.repeat = modes[(idx + 1) % modes.length];
		invoke('set_repeat', { mode: this.repeat }).catch((e) => {
			console.error('Failed to sync repeat:', e);
		});
	}

	isInQueue(trackId: string): boolean {
		return this._queuedIds.has(trackId);
	}

	queuePosition(trackId: string): number {
		const idx = this.userQueue.findIndex((t) => t.id === trackId);
		return idx === -1 ? 0 : idx + 1;
	}

	async enqueueTracks(trackIds: string[]) {
		try {
			await invoke('enqueue_tracks', { trackIds });
			await this.loadQueue();
		} catch (e) {
			console.error('Failed to enqueue tracks:', e);
		}
	}

	async dequeueTracks(trackIds: string[]) {
		try {
			await invoke('dequeue_tracks', { trackIds });
			await this.loadQueue();
		} catch (e) {
			console.error('Failed to dequeue tracks:', e);
		}
	}

	async loadQueue() {
		try {
			this.userQueue = await invoke<Track[]>('get_queue');
			this._queuedIds = new Set(this.userQueue.map((t) => t.id));
		} catch (e) {
			console.error('Failed to load queue:', e);
		}
	}
}

export const playerStore = new PlayerStore();
