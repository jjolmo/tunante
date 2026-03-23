import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { MonitoredFolder, Setting, Theme } from '$lib/types';
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
	showCoverArt = $state(true);
	showConsoles = $state(true);
	autoUpdateOnStart = $state(false);
	checkUpdatesOnStart = $state(true);
	autoDownloadCoverArt = $state(false);

	private _mediaQueryListener: ((e: MediaQueryListEvent) => void) | null = null;
	private _mediaQuery: MediaQueryList | null = null;
	private _settingsCache = new Map<string, string>();

	/** Synchronous getter for cached settings (available after init) */
	getSetting(key: string): string | null {
		return this._settingsCache.get(key) ?? null;
	}

	async init() {
		// Load all settings in a single batch IPC call + monitored folders in parallel
		const [settings] = await Promise.all([
			invoke<Setting[]>('get_settings').catch((e) => {
				console.error('Failed to load settings:', e);
				return [] as Setting[];
			}),
			this.loadMonitoredFolders(),
		]);

		// Build cache for other stores to use
		for (const s of settings) {
			this._settingsCache.set(s.key, s.value);
		}

		// Apply settings from batch
		const theme = this._settingsCache.get('theme');
		if (theme === 'light' || theme === 'dark' || theme === 'system') {
			this.theme = theme;
		}
		this.applyTheme(this.theme);

		const titlebar = this._settingsCache.get('show_track_in_titlebar');
		if (titlebar !== undefined) this.showTrackInTitlebar = titlebar === 'true';

		const keepFavs = this._settingsCache.get('keep_favs_in_metadata');
		if (keepFavs !== undefined) this.keepFavsInMetadata = keepFavs === 'true';

		const showTray = this._settingsCache.get('show_in_tray');
		if (showTray !== undefined) this.showInTray = showTray === 'true';

		const closeTray = this._settingsCache.get('close_to_tray');
		if (closeTray !== undefined) this.closeToTray = closeTray === 'true';

		const showArt = this._settingsCache.get('show_cover_art');
		if (showArt !== undefined) this.showCoverArt = showArt === 'true';

		const showConsoles = this._settingsCache.get('show_consoles');
		if (showConsoles !== undefined) this.showConsoles = showConsoles === 'true';

		const autoUpdate = this._settingsCache.get('auto_update_on_start');
		if (autoUpdate !== undefined) this.autoUpdateOnStart = autoUpdate === 'true';

		const checkUpdates = this._settingsCache.get('check_updates_on_start');
		if (checkUpdates !== undefined) this.checkUpdatesOnStart = checkUpdates === 'true';

		const dlCover = this._settingsCache.get('auto_download_cover_art');
		if (dlCover !== undefined) this.autoDownloadCoverArt = dlCover === 'true';
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

	async setShowCoverArt(enabled: boolean) {
		this.showCoverArt = enabled;
		try {
			await invoke('set_setting', { key: 'show_cover_art', value: String(enabled) });
		} catch (e) {
			console.error('Failed to save cover art setting:', e);
		}
	}

	async setShowConsoles(enabled: boolean) {
		this.showConsoles = enabled;
		try {
			await invoke('set_setting', { key: 'show_consoles', value: String(enabled) });
		} catch (e) {
			console.error('Failed to save consoles setting:', e);
		}
	}

	async setAutoUpdateOnStart(enabled: boolean) {
		this.autoUpdateOnStart = enabled;
		// If auto-update is on, disable the ask dialog
		if (enabled) {
			this.checkUpdatesOnStart = false;
			await invoke('set_setting', { key: 'check_updates_on_start', value: 'false' }).catch(() => {});
		}
		try {
			await invoke('set_setting', { key: 'auto_update_on_start', value: String(enabled) });
		} catch (e) {
			console.error('Failed to save auto-update setting:', e);
		}
	}

	async setCheckUpdatesOnStart(enabled: boolean) {
		this.checkUpdatesOnStart = enabled;
		try {
			await invoke('set_setting', { key: 'check_updates_on_start', value: String(enabled) });
		} catch (e) {
			console.error('Failed to save update check setting:', e);
		}
	}
	async setAutoDownloadCoverArt(enabled: boolean) {
		this.autoDownloadCoverArt = enabled;
		try {
			await invoke('set_setting', { key: 'auto_download_cover_art', value: String(enabled) });
		} catch (e) {
			console.error('Failed to save auto-download cover art setting:', e);
		}
	}
}

export const settingsStore = new SettingsStore();
