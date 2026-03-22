import { libraryStore } from '$lib/stores/library.svelte';
import type { Track } from '$lib/types';

export interface ConsoleDefinition {
	id: string;
	name: string;
	codecs: string[];
	icon: string; // SVG path d attribute
}

// Console definitions with codec mappings and SVG icon paths (16x16 viewBox)
export const CONSOLE_DEFINITIONS: ConsoleDefinition[] = [
	{
		id: 'nes',
		name: 'NES',
		codecs: ['NSF', 'NSFE'],
		// NES controller silhouette
		icon: 'M2 5h12a1 1 0 011 1v4a1 1 0 01-1 1H2a1 1 0 01-1-1V6a1 1 0 011-1zm2 2v2h2V7H4zm6 0a1 1 0 100 2 1 1 0 000-2zm2.5.5a.5.5 0 100 1 .5.5 0 000-1z'
	},
	{
		id: 'snes',
		name: 'SNES',
		codecs: ['SPC'],
		// SNES controller (rounded with buttons)
		icon: 'M1 6a2 2 0 012-2h2l1 1h4l1-1h2a2 2 0 012 2v4a2 2 0 01-2 2H3a2 2 0 01-2-2V6zm3 0v1H3v1h1v1h1V8h1V7H5V6H4zm7 0a.5.5 0 100 1 .5.5 0 000-1zm1 1a.5.5 0 100 1 .5.5 0 000-1zm1-1a.5.5 0 100 1 .5.5 0 000-1zm-1-1a.5.5 0 100 1 .5.5 0 000-1z'
	},
	{
		id: 'gameboy',
		name: 'Game Boy',
		codecs: ['GBS'],
		// Game Boy handheld
		icon: 'M4 1h8a1 1 0 011 1v12a1 1 0 01-1 1H4a1 1 0 01-1-1V2a1 1 0 011-1zm1 2v4h6V3H5zm1 6v1H5v1h1v1h1v-1h1V10H7V9H6zm4 .5a.75.75 0 100 1.5.75.75 0 000-1.5zm2 0a.75.75 0 100 1.5.75.75 0 000-1.5z'
	},
	{
		id: 'genesis',
		name: 'Sega Genesis',
		codecs: ['VGM', 'VGZ', 'GYM'],
		// Genesis 3-button controller
		icon: 'M1 7c0-2 1-3 3-3h1l1.5 1h3L11 4h1c2 0 3 1 3 3v2c0 2-1 3-3 3H4c-2 0-3-1-3-3V7zm3 0v1h1V7H4zm1.5-1h1v1h-1V6zm4 1a.75.75 0 100 1.5.75.75 0 000-1.5zm2 0a.75.75 0 100 1.5.75.75 0 000-1.5zm2 0a.75.75 0 100 1.5.75.75 0 000-1.5z'
	},
	{
		id: 'tg16',
		name: 'TurboGrafx-16',
		codecs: ['HES'],
		// TG-16 elongated controller
		icon: 'M0 7a2 2 0 012-2h12a2 2 0 012 2v2a2 2 0 01-2 2H2a2 2 0 01-2-2V7zm3 0v1H2v1h1v1h1V9h1V8H4V7H3zm7.5.5a1 1 0 100 2 1 1 0 000-2zm3 0a1 1 0 100 2 1 1 0 000-2z'
	},
	{
		id: 'msx',
		name: 'MSX',
		codecs: ['KSS'],
		// Small computer/keyboard
		icon: 'M2 3h12a1 1 0 011 1v7a1 1 0 01-1 1h-1l-.5 1h-9L3 12H2a1 1 0 01-1-1V4a1 1 0 011-1zm1 2v3h10V5H3zm0 4h1v1H3V9zm2 0h1v1H5V9zm2 0h2v1H7V9zm3 0h1v1h-1V9zm2 0h1v1h-1V9z'
	},
	{
		id: 'spectrum',
		name: 'ZX Spectrum',
		codecs: ['AY'],
		// Spectrum keyboard shape
		icon: 'M1 4h14a1 1 0 011 1v6a1 1 0 01-1 1H1a1 1 0 01-1-1V5a1 1 0 011-1zm1 2v1h1V6H2zm2 0v1h1V6H4zm2 0v1h1V6H6zm2 0v1h1V6H8zm2 0v1h1V6h-1zm2 0v1h1V6h-1zm-9 2v1h1V8H3zm2 0v1h6V8H5zm7 0v1h1V8h-1z'
	},
	{
		id: 'atari',
		name: 'Atari',
		codecs: ['SAP'],
		// Atari joystick
		icon: 'M7 2h2v6h2.5a2.5 2.5 0 010 5h-7a2.5 2.5 0 010-5H7V2zm-2.5 7.5a1 1 0 100 2 1 1 0 000-2zm7 0a1 1 0 100 2 1 1 0 000-2z'
	},
	{
		id: 'gba',
		name: 'GB Advance',
		codecs: ['GSF', 'MINIGSF'],
		// GBA wide handheld
		icon: 'M1 5a2 2 0 012-2h10a2 2 0 012 2v6a2 2 0 01-2 2H3a2 2 0 01-2-2V5zm4 0H4v4h4V5H5zm-2 1v1H2V6h1zm1.5 3h1v1h-1V9zm-1 0v1H3V9h1.5zm8-3a.75.75 0 100 1.5.75.75 0 000-1.5zm-1.5 1.5a.75.75 0 100 1.5.75.75 0 000-1.5z'
	},
	{
		id: 'nds',
		name: 'Nintendo DS',
		codecs: ['2SF', 'MINI2SF'],
		// DS two screens stacked
		icon: 'M3 1h10a1 1 0 011 1v5H2V2a1 1 0 011-1zm-1 7h12v1H2V8zm0 1h12v5a1 1 0 01-1 1H3a1 1 0 01-1-1V9zm2 1v3h8v-3H4z'
	},
	{
		id: 'ps1',
		name: 'PlayStation',
		codecs: ['PSF', 'MINIPSF'],
		// PlayStation controller shape
		icon: 'M1 6.5C1 5.67 1.67 5 2.5 5h2L6 4h4l1.5 1h2c.83 0 1.5.67 1.5 1.5v3c0 .83-.67 1.5-1.5 1.5h-11C1.67 11 1 10.33 1 9.5v-3zM4 7v1H3v1h1v1h1V9h1V8H5V7H4zm6.5.25l-.75.75.75.75.75-.75-.75-.75zm0 1.5l-.75.75.75.75.75-.75-.75-.75zm-.75.75l-.75-.75-.75.75.75.75.75-.75zm1.5 0l-.75-.75-.75.75.75.75.75-.75z'
	},
	{
		id: 'ps2',
		name: 'PlayStation 2',
		codecs: ['PSF2', 'MINIPSF2'],
		// PS2 console standing
		icon: 'M5 1h6a1 1 0 011 1v12a1 1 0 01-1 1H5a1 1 0 01-1-1V2a1 1 0 011-1zm.5 1.5v2h5v-2h-5zM7 6h2v1H7V6zm-1 7h4v1H6v-1z'
	},
	{
		id: 'n64',
		name: 'Nintendo 64',
		codecs: ['USF', 'MINIUSF'],
		// N64 three-prong controller
		icon: 'M2 4a1 1 0 011-1h3l1.5 1h1L10 3h3a1 1 0 011 1v4a1 1 0 01-1 1h-2v2a1 1 0 01-1 1H6a1 1 0 01-1-1V9H3a1 1 0 01-1-1V4zm5 3v4h2V7H7zM4 5v1H3v1h1v1h1V7h1V6H5V5H4zm6 .5a.75.75 0 100 1.5.75.75 0 000-1.5zm2 0a.75.75 0 100 1.5.75.75 0 000-1.5z'
	},
	{
		id: 'saturn',
		name: 'Sega Saturn',
		codecs: ['SSF', 'MINISSF'],
		// Saturn 6-button controller
		icon: 'M0 7c0-2 1.5-3 3-3h1.5L6 3h4l1.5 1H13c1.5 0 3 1 3 3v2c0 2-1.5 3-3 3H3c-1.5 0-3-1-3-3V7zm3.5 0v1H3v1h.5v1h1V9H5V8h-.5V7h-1zM9 6.5a.6.6 0 100 1.2.6.6 0 000-1.2zm1.5.5a.6.6 0 100 1.2.6.6 0 000-1.2zm1.5-.5a.6.6 0 100 1.2.6.6 0 000-1.2zM9 8.5a.6.6 0 100 1.2.6.6 0 000-1.2zm1.5.5a.6.6 0 100 1.2.6.6 0 000-1.2zm1.5-.5a.6.6 0 100 1.2.6.6 0 000-1.2z'
	},
	{
		id: 'dreamcast',
		name: 'Sega Dreamcast',
		codecs: ['DSF', 'MINIDSF'],
		// Dreamcast controller with VMU
		icon: 'M2 5a2 2 0 012-2h1l1 1h4l1-1h1a2 2 0 012 2v5a2 2 0 01-2 2H4a2 2 0 01-2-2V5zm4-1v3h4V4H6zm-2 5v1H3V9h1zM5 8v1H4V8h1zm5.5-2a1.5 1.5 0 100 3 1.5 1.5 0 000-3z'
	}
];

// Build a reverse lookup: codec → console id
const CODEC_TO_CONSOLE = new Map<string, string>();
for (const def of CONSOLE_DEFINITIONS) {
	for (const codec of def.codecs) {
		CODEC_TO_CONSOLE.set(codec, def.id);
	}
}

class ConsolesStore {
	activeConsoleId = $state<string | null>(null);

	get consolesWithCounts(): (ConsoleDefinition & { trackCount: number })[] {
		const counts = new Map<string, number>();
		for (const track of libraryStore.tracks) {
			const consoleId = CODEC_TO_CONSOLE.get(track.codec);
			if (consoleId) {
				counts.set(consoleId, (counts.get(consoleId) || 0) + 1);
			}
		}

		return CONSOLE_DEFINITIONS.filter((def) => (counts.get(def.id) || 0) > 0).map((def) => ({
			...def,
			trackCount: counts.get(def.id) || 0
		}));
	}

	get activeConsole(): ConsoleDefinition | null {
		if (!this.activeConsoleId) return null;
		return CONSOLE_DEFINITIONS.find((d) => d.id === this.activeConsoleId) || null;
	}

	get consoleTracks(): Track[] {
		const console = this.activeConsole;
		if (!console) return [];
		const codecSet = new Set(console.codecs);
		return libraryStore.filteredTracks.filter((t) => codecSet.has(t.codec));
	}

	selectConsole(id: string | null) {
		this.activeConsoleId = id;
	}
}

export const consolesStore = new ConsolesStore();
