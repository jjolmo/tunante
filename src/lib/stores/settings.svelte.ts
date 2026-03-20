import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { MonitoredFolder, Theme } from '$lib/types';
import { libraryStore } from '$lib/stores/library.svelte';

class SettingsStore {
	theme = $state<Theme>('dark');
	monitoredFolders = $state<MonitoredFolder[]>([]);
	isSettingsOpen = $state(false);
	activeCategory = $state('general');
	showTrackInTitlebar = $state(true);

	async init() {
		try {
			const theme = await invoke<string | null>('get_setting', { key: 'theme' });
			if (theme === 'light' || theme === 'dark') {
				this.theme = theme;
			}
			this.applyTheme(this.theme);
		} catch (e) {
			console.error('Failed to load theme setting:', e);
		}

		try {
			const val = await invoke<string | null>('get_setting', { key: 'show_track_in_titlebar' });
			if (val !== null) {
				this.showTrackInTitlebar = val === 'true';
			}
		} catch (e) {
			console.error('Failed to load titlebar setting:', e);
		}

		await this.loadMonitoredFolders();
	}

	applyTheme(theme: Theme) {
		const html = document.documentElement;
		html.classList.remove('dark', 'light');
		html.classList.add(theme);
		this.theme = theme;
	}

	async setTheme(theme: Theme) {
		this.applyTheme(theme);
		try {
			await invoke('set_setting', { key: 'theme', value: theme });
		} catch (e) {
			console.error('Failed to save theme setting:', e);
		}
	}

	async loadMonitoredFolders() {
		try {
			this.monitoredFolders = await invoke<MonitoredFolder[]>('get_monitored_folders');
		} catch (e) {
			console.error('Failed to load monitored folders:', e);
		}
	}

	async addMonitoredFolder(path: string) {
		try {
			libraryStore.isScanning = true;
			const folder = await invoke<MonitoredFolder>('add_monitored_folder', { path });
			this.monitoredFolders = [...this.monitoredFolders, folder];
		} catch (e) {
			console.error('Failed to add monitored folder:', e);
			libraryStore.isScanning = false;
		}
	}

	async removeMonitoredFolder(id: string) {
		try {
			await invoke('remove_monitored_folder', { id });
			this.monitoredFolders = this.monitoredFolders.filter((f) => f.id !== id);
		} catch (e) {
			console.error('Failed to remove monitored folder:', e);
		}
	}

	async toggleFolderWatching(id: string, enabled: boolean) {
		try {
			await invoke('toggle_folder_watching', { id, enabled });
			this.monitoredFolders = this.monitoredFolders.map((f) =>
				f.id === id ? { ...f, watching_enabled: enabled } : f
			);
		} catch (e) {
			console.error('Failed to toggle folder watching:', e);
		}
	}

	openSettings() {
		this.isSettingsOpen = true;
	}

	closeSettings() {
		this.isSettingsOpen = false;
	}

	async setShowTrackInTitlebar(enabled: boolean) {
		this.showTrackInTitlebar = enabled;
		try {
			await invoke('set_setting', { key: 'show_track_in_titlebar', value: String(enabled) });
		} catch (e) {
			console.error('Failed to save titlebar setting:', e);
		}
	}
}

export const settingsStore = new SettingsStore();
