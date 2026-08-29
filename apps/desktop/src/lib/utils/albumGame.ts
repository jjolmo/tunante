import type { Track } from '$lib/types';
import { settingsStore } from '$lib/stores/settings.svelte';

/**
 * The one value for the combined "Album / Game" column.
 *
 * Two different facts live here and most of the time you only want to see one:
 * `album` is what the ripper wrote in the file, `game` is what the library
 * worked out the file belongs to. For a console rip they are usually the same
 * string; for a soundtrack release they are not, and which one is useful
 * depends on what the library is mostly made of.
 *
 * Falls back to the other rather than showing nothing. A blank cell where a
 * name should be is a worse answer than the name you did not ask for — and a
 * format with no game field of its own, or a track nobody has classified, has
 * exactly one of the two.
 */
export function albumOrGame(t: Track): string {
	const prefersGame = settingsStore.albumGamePrefers === 'game';
	const first = prefersGame ? t.game : t.album;
	const second = prefersGame ? t.album : t.game;
	return (first || '').trim() || (second || '').trim();
}
