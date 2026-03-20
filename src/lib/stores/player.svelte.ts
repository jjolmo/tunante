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
	private _initialized = false;

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
	}

	async playTrack(track: Track) {
		try {
			await invoke('play_file', { path: track.path });
			this.currentTrack = track;
			this.isPlaying = true;
		} catch (e) {
			console.error('Failed to play track:', e);
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
		try {
			await invoke('seek', { positionMs });
			this.positionMs = positionMs;
		} catch (e) {
			console.error('Failed to seek:', e);
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
	}

	cycleRepeat() {
		const modes: RepeatMode[] = ['off', 'all', 'one'];
		const idx = modes.indexOf(this.repeat);
		this.repeat = modes[(idx + 1) % modes.length];
	}
}

export const playerStore = new PlayerStore();
