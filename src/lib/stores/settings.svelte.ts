import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { MonitoredFolder, Theme } from '$lib/types';
import { libraryStore } from '$lib/stores/library.svelte';

class SettingsStore {
	theme = $state<Theme>('system');
	monitoredFolders = $state<MonitoredFolder[]>([]);
	isSettingsOpen = $state(false);
	activeCategory = $state('general');
	showTrackInTitlebar = $state(true);
	keepFavsInMetadata = $state(true);
	showInTray = $state(false);
	closeToTray = $state(false);

	private _mediaQueryListener: ((e: MediaQueryListEvent) => void) | null = null;
	private _mediaQuery: MediaQueryList | null = null;

	async init() {
		try {
			const theme = await invoke<string | null>('get_setting', { key: 'theme' });
			if (theme === 'light' || theme === 'dark' || theme === 'system') {
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

		try {
			const val = await invoke<string | null>('get_setting', { key: 'keep_favs_in_metadata' });
			if (val !== null) {
				this.keepFavsInMetadata = val === 'true';
			}
		} catch (e) {
			console.error('Failed to load keep favs setting:', e);
		}

		try {
			const val = await invoke<string | null>('get_setting', { key: 'show_in_tray' });
			if (val !== null) {
				this.showInTray = val === 'true';
			}
		} catch (e) {
			console.error('Failed to load show in tray setting:', e);
		}

		try {
			const val = await invoke<string | null>('get_setting', { key: 'close_to_tray' });
			if (val !== null) {
				this.closeToTray = val === 'true';
			}
		} catch (e) {
			console.error('Failed to load close to tray setting:', e);
		}

		await this.loadMonitoredFolders();
	}

	private _teardownMediaListener() {
		if (this._mediaQuery && this._mediaQueryListener) {
			this._mediaQuery.removeEventListener('change', this._mediaQueryListener);
		}
		this._mediaQueryListener = null;
		this._mediaQuery = null;
	}

	private _resolveSystemTheme(): 'dark' | 'light' {
		return window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light';
	}

	applyTheme(theme: Theme) {
		this._teardownMediaListener();
		this.theme = theme;

		const html = document.documentElement;
		html.classList.remove('dark', 'light');

		if (theme === 'system') {
			const resolved = this._resolveSystemTheme();
			html.classList.add(resolved);

			// Listen for OS theme changes
			this._mediaQuery = window.matchMedia('(prefers-color-scheme: dark)');
			this._mediaQueryListener = (e) => {
				html.classList.remove('dark', 'light');
				html.classList.add(e.matches ? 'dark' : 'light');
			};
			this._mediaQuery.addEventListener('change', this._mediaQueryListener);
		} else {
			html.classList.add(theme);
		}
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

	async setKeepFavsInMetadata(enabled: boolean) {
		this.keepFavsInMetadata = enabled;
		try {
			await invoke('set_setting', { key: 'keep_favs_in_metadata', value: String(enabled) });
		} catch (e) {
			console.error('Failed to save keep favs setting:', e);
		}
	}

	async setShowInTray(enabled: boolean) {
		this.showInTray = enabled;
		try {
			await invoke('set_setting', { key: 'show_in_tray', value: String(enabled) });
			await invoke('set_tray_visible', { visible: enabled });
		} catch (e) {
			console.error('Failed to save show in tray setting:', e);
		}

		// If tray is hidden, close-to-tray can't work — disable it
		if (!enabled && this.closeToTray) {
			await this.setCloseToTray(false);
		}
	}

	async setCloseToTray(enabled: boolean) {
		this.closeToTray = enabled;
		try {
			await invoke('set_setting', { key: 'close_to_tray', value: String(enabled) });
		} catch (e) {
			console.error('Failed to save close to tray setting:', e);
		}
	}
}

export const settingsStore = new SettingsStore();
