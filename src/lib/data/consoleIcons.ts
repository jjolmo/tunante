/**
 * Console glyphs, keyed by the stable console id from `tunante_core::console`.
 *
 * Presentation only, which is why these live here and not in the Rust table.
 * A console with no glyph drawn yet falls back to {@link FALLBACK_ICON} rather
 * than being blocked from existing — the table has thirty-odd machines and
 * drawing a silhouette for each is not a prerequisite for classifying its music.
 *
 * SVG path `d` attributes on a 16x16 viewBox.
 */
export const CONSOLE_ICONS: Record<string, string> = {
	nes: 'M2 5h12a1 1 0 011 1v4a1 1 0 01-1 1H2a1 1 0 01-1-1V6a1 1 0 011-1zm2 2v2h2V7H4zm6 0a1 1 0 100 2 1 1 0 000-2zm2.5.5a.5.5 0 100 1 .5.5 0 000-1z',
	snes: 'M1 6a2 2 0 012-2h2l1 1h4l1-1h2a2 2 0 012 2v4a2 2 0 01-2 2H3a2 2 0 01-2-2V6zm3 0v1H3v1h1v1h1V8h1V7H5V6H4zm7 0a.5.5 0 100 1 .5.5 0 000-1zm1 1a.5.5 0 100 1 .5.5 0 000-1zm1-1a.5.5 0 100 1 .5.5 0 000-1zm-1-1a.5.5 0 100 1 .5.5 0 000-1z',
	gameboy:
		'M4 1h8a1 1 0 011 1v12a1 1 0 01-1 1H4a1 1 0 01-1-1V2a1 1 0 011-1zm1 2v4h6V3H5zm1 6v1H5v1h1v1h1v-1h1V10H7V9H6zm4 .5a.75.75 0 100 1.5.75.75 0 000-1.5zm2 0a.75.75 0 100 1.5.75.75 0 000-1.5z',
	// The Color shares the Game Boy silhouette; only the shell colour differed.
	gbc: 'M4 1h8a1 1 0 011 1v12a1 1 0 01-1 1H4a1 1 0 01-1-1V2a1 1 0 011-1zm1 2v4h6V3H5zm1 6v1H5v1h1v1h1v-1h1V10H7V9H6zm4 .5a.75.75 0 100 1.5.75.75 0 000-1.5zm2 0a.75.75 0 100 1.5.75.75 0 000-1.5z',
	genesis:
		'M1 7c0-2 1-3 3-3h1l1.5 1h3L11 4h1c2 0 3 1 3 3v2c0 2-1 3-3 3H4c-2 0-3-1-3-3V7zm3 0v1h1V7H4zm1.5-1h1v1h-1V6zm4 1a.75.75 0 100 1.5.75.75 0 000-1.5zm2 0a.75.75 0 100 1.5.75.75 0 000-1.5zm2 0a.75.75 0 100 1.5.75.75 0 000-1.5z',
	tg16: 'M0 7a2 2 0 012-2h12a2 2 0 012 2v2a2 2 0 01-2 2H2a2 2 0 01-2-2V7zm3 0v1H2v1h1v1h1V9h1V8H4V7H3zm7.5.5a1 1 0 100 2 1 1 0 000-2zm3 0a1 1 0 100 2 1 1 0 000-2z',
	msx: 'M2 3h12a1 1 0 011 1v7a1 1 0 01-1 1h-1l-.5 1h-9L3 12H2a1 1 0 01-1-1V4a1 1 0 011-1zm1 2v3h10V5H3zm0 4h1v1H3V9zm2 0h1v1H5V9zm2 0h2v1H7V9zm3 0h1v1h-1V9zm2 0h1v1h-1V9z',
	spectrum:
		'M1 4h14a1 1 0 011 1v6a1 1 0 01-1 1H1a1 1 0 01-1-1V5a1 1 0 011-1zm1 2v1h1V6H2zm2 0v1h1V6H4zm2 0v1h1V6H6zm2 0v1h1V6H8zm2 0v1h1V6h-1zm2 0v1h1V6h-1zm-9 2v1h1V8H3zm2 0v1h6V8H5zm7 0v1h1V8h-1z',
	// SAP is a POKEY log — Atari 8-bit computers, not the 2600.
	atari8: 'M7 2h2v6h2.5a2.5 2.5 0 010 5h-7a2.5 2.5 0 010-5H7V2zm-2.5 7.5a1 1 0 100 2 1 1 0 000-2zm7 0a1 1 0 100 2 1 1 0 000-2z',
	gba: 'M1 5a2 2 0 012-2h10a2 2 0 012 2v6a2 2 0 01-2 2H3a2 2 0 01-2-2V5zm4 0H4v4h4V5H5zm-2 1v1H2V6h1zm1.5 3h1v1h-1V9zm-1 0v1H3V9h1.5zm8-3a.75.75 0 100 1.5.75.75 0 000-1.5zm-1.5 1.5a.75.75 0 100 1.5.75.75 0 000-1.5z',
	gamecube:
		'M2 5.5C2 4.67 2.67 4 3.5 4h2L7 3h2l1.5 1h2c.83 0 1.5.67 1.5 1.5v4c0 .83-.67 1.5-1.5 1.5h-9C2.67 11 2 10.33 2 9.5v-4zM8 5a2 2 0 100 4 2 2 0 000-4zm0 1a1 1 0 110 2 1 1 0 010-2zM4.5 6a.5.5 0 100 1 .5.5 0 000-1zm7 .5a.5.5 0 100 1 .5.5 0 000-1z',
	wii: 'M5 1h6a2 2 0 012 2v10a2 2 0 01-2 2H5a2 2 0 01-2-2V3a2 2 0 012-2zm2.5 2a1 1 0 100 2 1 1 0 000-2zM6 6h4v3H6V6zm1 5h2v1H7v-1z',
	n3ds: 'M3 1h10a1 1 0 011 1v5H2V2a1 1 0 011-1zm5 1.5a.5.5 0 100 1 .5.5 0 000-1zM2 8h12v1H2V8zm1 1h10v5a1 1 0 01-1 1H4a1 1 0 01-1-1V9zm1 1v3h8v-3H4z',
	wiiu: 'M1 4h14a1 1 0 011 1v6a1 1 0 01-1 1H1a1 1 0 01-1-1V5a1 1 0 011-1zm2 1.5a.5.5 0 100 1 .5.5 0 000-1zM5 6v4h6V6H5zm8 .5a.5.5 0 100 1 .5.5 0 000-1z',
	nds: 'M3 1h10a1 1 0 011 1v5H2V2a1 1 0 011-1zm-1 7h12v1H2V8zm0 1h12v5a1 1 0 01-1 1H3a1 1 0 01-1-1V9zm2 1v3h8v-3H4z',
	ps1: 'M1 6.5C1 5.67 1.67 5 2.5 5h2L6 4h4l1.5 1h2c.83 0 1.5.67 1.5 1.5v3c0 .83-.67 1.5-1.5 1.5h-11C1.67 11 1 10.33 1 9.5v-3zM4 7v1H3v1h1v1h1V9h1V8H5V7H4zm6.5.25l-.75.75.75.75.75-.75-.75-.75zm0 1.5l-.75.75.75.75.75-.75-.75-.75zm-.75.75l-.75-.75-.75.75.75.75.75-.75zm1.5 0l-.75-.75-.75.75.75.75.75-.75z',
	ps2: 'M5 1h6a1 1 0 011 1v12a1 1 0 01-1 1H5a1 1 0 01-1-1V2a1 1 0 011-1zm.5 1.5v2h5v-2h-5zM7 6h2v1H7V6zm-1 7h4v1H6v-1z',
	n64: 'M2 4a1 1 0 011-1h3l1.5 1h1L10 3h3a1 1 0 011 1v4a1 1 0 01-1 1h-2v2a1 1 0 01-1 1H6a1 1 0 01-1-1V9H3a1 1 0 01-1-1V4zm5 3v4h2V7H7zM4 5v1H3v1h1v1h1V7h1V6H5V5H4zm6 .5a.75.75 0 100 1.5.75.75 0 000-1.5zm2 0a.75.75 0 100 1.5.75.75 0 000-1.5z',
	saturn: 'M0 7c0-2 1.5-3 3-3h1.5L6 3h4l1.5 1H13c1.5 0 3 1 3 3v2c0 2-1.5 3-3 3H3c-1.5 0-3-1-3-3V7zm3.5 0v1H3v1h.5v1h1V9H5V8h-.5V7h-1zM9 6.5a.6.6 0 100 1.2.6.6 0 000-1.2zm1.5.5a.6.6 0 100 1.2.6.6 0 000-1.2zm1.5-.5a.6.6 0 100 1.2.6.6 0 000-1.2zM9 8.5a.6.6 0 100 1.2.6.6 0 000-1.2zm1.5.5a.6.6 0 100 1.2.6.6 0 000-1.2zm1.5-.5a.6.6 0 100 1.2.6.6 0 000-1.2z',
	dreamcast:
		'M2 5a2 2 0 012-2h1l1 1h4l1-1h1a2 2 0 012 2v5a2 2 0 01-2 2H4a2 2 0 01-2-2V5zm4-1v3h4V4H6zm-2 5v1H3V9h1zM5 8v1H4V8h1zm5.5-2a1.5 1.5 0 100 3 1.5 1.5 0 000-3z',
	// Sega's 8-bit machines reuse the Mega Drive pad; the silhouette is the same
	// at 16 pixels.
	mastersystem:
		'M1 7c0-2 1-3 3-3h1l1.5 1h3L11 4h1c2 0 3 1 3 3v2c0 2-1 3-3 3H4c-2 0-3-1-3-3V7zm3 0v1h1V7H4zm1.5-1h1v1h-1V6zm4 1a.75.75 0 100 1.5.75.75 0 000-1.5zm2 0a.75.75 0 100 1.5.75.75 0 000-1.5z',
	// A disc, for the machines that shipped on one.
	ps3: 'M8 2a6 6 0 100 12A6 6 0 008 2zm0 4.5a1.5 1.5 0 110 3 1.5 1.5 0 010-3z',
	ps4: 'M8 2a6 6 0 100 12A6 6 0 008 2zm0 4.5a1.5 1.5 0 110 3 1.5 1.5 0 010-3z',
	// Handhelds: a wide slab with a screen.
	psp: 'M1 5a1 1 0 011-1h12a1 1 0 011 1v6a1 1 0 01-1 1H2a1 1 0 01-1-1V5zm4 1v4h6V6H5zm-2.5.5a.75.75 0 100 1.5.75.75 0 000-1.5zm10 0a.75.75 0 100 1.5.75.75 0 000-1.5z',
	psvita:
		'M1 5a1 1 0 011-1h12a1 1 0 011 1v6a1 1 0 01-1 1H2a1 1 0 01-1-1V5zm4 1v4h6V6H5zm-2.5.5a.75.75 0 100 1.5.75.75 0 000-1.5zm10 0a.75.75 0 100 1.5.75.75 0 000-1.5z',
	// Switch: a screen with a rail on each side.
	switch:
		'M2 3h2v10H2a1 1 0 01-1-1V4a1 1 0 011-1zm3 0h6v10H5V3zm7 0h2a1 1 0 011 1v8a1 1 0 01-1 1h-2V3z',
	c64: 'M1 6h14a1 1 0 011 1v4a1 1 0 01-1 1H1a1 1 0 01-1-1V7a1 1 0 011-1zm1 2v1h1V8H2zm2 0v1h1V8H4zm2 0v1h4V8H6zm5 0v1h1V8h-1zm2 0v1h1V8h-1z',
	arcade:
		'M4 1h8a1 1 0 011 1v12a1 1 0 01-1 1H4a1 1 0 01-1-1V2a1 1 0 011-1zm1 2v4h6V3H5zm3 6a1 1 0 100 2 1 1 0 000-2zm-2 3h4v1H6v-1z',
	xbox: 'M8 1a7 7 0 100 14A7 7 0 008 1zM5 4l3 3 3-3 1 1-3 3 3 3-1 1-3-3-3 3-1-1 3-3-3-3 1-1z',
	x360: 'M8 1a7 7 0 100 14A7 7 0 008 1zM5 4l3 3 3-3 1 1-3 3 3 3-1 1-3-3-3 3-1-1 3-3-3-3 1-1z',
	// A desktop machine, for everything that shipped on one.
	pc: 'M2 3h12a1 1 0 011 1v6a1 1 0 01-1 1H9v1h2v1H5v-1h2v-1H2a1 1 0 01-1-1V4a1 1 0 011-1zm1 2v4h10V5H3z'
};

/** Sound bars, for a console with no silhouette of its own. */
export const FALLBACK_ICON =
	'M1 7h2v2H1V7zm3-2h2v6H4V5zm3 3h2v1H7V8zm3-2h2v4h-2V6zm3 1h2v2h-2V7z';

export function consoleIcon(id: string): string {
	return CONSOLE_ICONS[id] ?? FALLBACK_ICON;
}
