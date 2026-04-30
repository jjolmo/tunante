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
	showFaved = $state(true);
	showPlaylists = $state(true);
	showConsoles = $state(true);
	showFiles = $state(false);
	autoUpdateOnStart = $state(false);
	checkUpdatesOnStart = $state(true);
	autoDownloadCoverArt = $state(false);
	storeCoversInFolder = $state(false);
	consoleGroupByFolder = $state(false);
	fastScan = $state(false);
	continueFromQueue = $state(true);
	fadeOnTrackChange = $state(false);
	fadeSeconds = $state(2);
	trayMiddleClickAction = $state<'none' | 'play_pause' | 'stop' | 'next_track' | 'next_track_with_fade'>('play_pause');

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

		const showFaved = this._settingsCache.get('show_faved');
		if (showFaved !== undefined) this.showFaved = showFaved === 'true';

		const showPlaylists = this._settingsCache.get('show_playlists');
		if (showPlaylists !== undefined) this.showPlaylists = showPlaylists === 'true';

		const showConsoles = this._settingsCache.get('show_consoles');
		if (showConsoles !== undefined) this.showConsoles = showConsoles === 'true';

		const showFiles = this._settingsCache.get('show_files');
		if (showFiles !== undefined) this.showFiles = showFiles === 'true';

		const autoUpdate = this._settingsCache.get('auto_update_on_start');
		if (autoUpdate !== undefined) this.autoUpdateOnStart = autoUpdate === 'true';

		const checkUpdates = this._settingsCache.get('check_updates_on_start');
		if (checkUpdates !== undefined) this.checkUpdatesOnStart = checkUpdates === 'true';

		const dlCover = this._settingsCache.get('auto_download_cover_art');
		if (dlCover !== undefined) this.autoDownloadCoverArt = dlCover === 'true';

		const storeCover = this._settingsCache.get('store_covers_in_folder');
		if (storeCover !== undefined) this.storeCoversInFolder = storeCover === 'true';

		const groupByFolder = this._settingsCache.get('console_group_by_folder');
		if (groupByFolder !== undefined) this.consoleGroupByFolder = groupByFolder === 'true';

		const fastScan = this._settingsCache.get('fast_scan');
		if (fastScan !== undefined) this.fastScan = fastScan === 'true';

		const continueFromQueue = this._settingsCache.get('continue_from_queue');
		if (continueFromQueue !== undefined) this.continueFromQueue = continueFromQueue === 'true';

		const fadeOnTrackChange = this._settingsCache.get('fade_on_track_change');
		if (fadeOnTrackChange !== undefined) this.fadeOnTrackChange = fadeOnTrackChange === 'true';

		const fadeSeconds = this._settingsCache.get('fade_seconds');
		if (fadeSeconds !== undefined) {
			const parsed = parseFloat(fadeSeconds);
			if (!isNaN(parsed)) this.fadeSeconds = Math.max(0, Math.min(10, parsed));
		}

		const trayMid = this._settingsCache.get('tray_middle_click_action');
		if (
			trayMid === 'none' ||
			trayMid === 'play_pause' ||
			trayMid === 'stop' ||
			trayMid === 'next_track' ||
			trayMid === 'next_track_with_fade'
		) {
			this.trayMiddleClickAction = trayMid;
		}

		// Sync continue_from_queue to the backend queue
		invoke('set_continue_from_queue', { enabled: this.continueFromQueue }).catch(() => {});

		// Sync fade settings to the backend audio engine
		invoke('set_fade_on_track_change', { enabled: this.fadeOnTrackChange }).catch(() => {});
		invoke('set_fade_seconds', { seconds: this.fadeSeconds }).catch(() => {});
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

	async setShowFaved(enabled: boolean) {
		this.showFaved = enabled;
		try {
			await invoke('set_setting', { key: 'show_faved', value: String(enabled) });
		} catch (e) {
			console.error('Failed to save faved setting:', e);
		}
	}

	async setShowPlaylists(enabled: boolean) {
		this.showPlaylists = enabled;
		try {
			await invoke('set_setting', { key: 'show_playlists', value: String(enabled) });
		} catch (e) {
			console.error('Failed to save playlists setting:', e);
		}
	}

	async setShowFiles(enabled: boolean) {
		this.showFiles = enabled;
		try {
			await invoke('set_setting', { key: 'show_files', value: String(enabled) });
		} catch (e) {
			console.error('Failed to save files setting:', e);
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
		// If download is disabled, also disable store in folder
		if (!enabled && this.storeCoversInFolder) {
			await this.setStoreCoversInFolder(false);
		}
		try {
			await invoke('set_setting', { key: 'auto_download_cover_art', value: String(enabled) });
		} catch (e) {
			console.error('Failed to save auto-download cover art setting:', e);
		}
	}

	async setStoreCoversInFolder(enabled: boolean) {
		this.storeCoversInFolder = enabled;
		try {
			await invoke('set_setting', { key: 'store_covers_in_folder', value: String(enabled) });
		} catch (e) {
			console.error('Failed to save store covers setting:', e);
		}
	}

	async setFastScan(enabled: boolean) {
		this.fastScan = enabled;
		try {
			await invoke('set_setting', { key: 'fast_scan', value: String(enabled) });
		} catch (e) {
			console.error('Failed to save fast scan setting:', e);
		}
	}

	async setContinueFromQueue(enabled: boolean) {
		this.continueFromQueue = enabled;
		try {
			await invoke('set_setting', { key: 'continue_from_queue', value: String(enabled) });
			await invoke('set_continue_from_queue', { enabled });
		} catch (e) {
			console.error('Failed to save continue from queue setting:', e);
		}
	}

	async setConsoleGroupByFolder(enabled: boolean) {
		this.consoleGroupByFolder = enabled;
		try {
			await invoke('set_setting', { key: 'console_group_by_folder', value: String(enabled) });
		} catch (e) {
			console.error('Failed to save console group by folder setting:', e);
		}
	}

	async setFadeOnTrackChange(enabled: boolean) {
		this.fadeOnTrackChange = enabled;
		try {
			await invoke('set_setting', { key: 'fade_on_track_change', value: String(enabled) });
			await invoke('set_fade_on_track_change', { enabled });
		} catch (e) {
			console.error('Failed to save fade-on-track-change setting:', e);
		}
	}

	async setTrayMiddleClickAction(
		action: 'none' | 'play_pause' | 'stop' | 'next_track' | 'next_track_with_fade'
	) {
		this.trayMiddleClickAction = action;
		try {
			await invoke('set_setting', { key: 'tray_middle_click_action', value: action });
		} catch (e) {
			console.error('Failed to save tray middle-click action:', e);
		}
	}

	async setFadeSeconds(seconds: number) {
		const clamped = Math.max(0, Math.min(10, seconds));
		this.fadeSeconds = clamped;
		try {
			await invoke('set_setting', { key: 'fade_seconds', value: String(clamped) });
			await invoke('set_fade_seconds', { seconds: clamped });
		} catch (e) {
			console.error('Failed to save fade-seconds setting:', e);
		}
	}
}

export const settingsStore = new SettingsStore();
