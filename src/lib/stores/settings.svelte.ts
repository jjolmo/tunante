import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { MonitoredFolder, Theme } from '$lib/types';

class SettingsStore {
	theme = $state<Theme>('dark');
	monitoredFolders = $state<MonitoredFolder[]>([]);
	isSettingsOpen = $state(false);
	activeCategory = $state('library');

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
			const folder = await invoke<MonitoredFolder>('add_monitored_folder', { path });
			this.monitoredFolders = [...this.monitoredFolders, folder];
		} catch (e) {
			console.error('Failed to add monitored folder:', e);
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
}

export const settingsStore = new SettingsStore();
