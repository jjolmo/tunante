import type { Track } from '$lib/types';

/**
 * Normalise a string for searching.
 *
 * Lowercases, and treats `-` and `_` as spaces because game-music tags and rip
 * filenames use them interchangeably with a real space — the same album shows up as
 * "La-Mulana 2", "La Mulana MSX" and "sword_of_mana" depending on who ripped it.
 * Typing "la mulana" should find all of them.
 *
 * Runs of separators collapse into one space, so "La  -  Mulana" and "La-Mulana" match
 * the same query. Applied to both sides of the comparison, which is what makes it
 * symmetric: searching "la-mulana" finds "La Mulana" just as "la mulana" finds
 * "La-Mulana".
 */
export function normalizeForSearch(value: string): string {
	return value.toLowerCase().replace(/[-_\s]+/g, ' ').trim();
}

/**
 * Whether a track matches an already-normalised query (see {@link normalizeForSearch}).
 *
 * The query is normalised once by the caller rather than per track — this runs over the
 * whole library on every search.
 */
export function trackMatchesSearch(track: Track, normalizedQuery: string): boolean {
	if (!normalizedQuery) return true;
	return (
		normalizeForSearch(track.title).includes(normalizedQuery) ||
		normalizeForSearch(track.artist).includes(normalizedQuery) ||
		normalizeForSearch(track.album).includes(normalizedQuery)
	);
}
