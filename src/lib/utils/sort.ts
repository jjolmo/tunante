import type { Track, SortColumn, SortDirection } from '$lib/types';

/**
 * One collator for the whole app, built once.
 *
 * `a.localeCompare(b, undefined, opts)` is specified as equivalent to
 * `new Intl.Collator(undefined, opts).compare(a, b)` — but called that way it builds a
 * fresh collator on *every* comparison. Sorting a 30k-track library is ~440k
 * comparisons, so that construction cost dominated everything else: measured against a
 * real 29,541-track library in JavaScriptCore (the engine WebKitGTK runs, so what the
 * app actually uses), sorting by title took 1281ms per-call versus 40ms reusing a
 * collator. That second and a half was blocking the UI thread on every view change,
 * every sort-column click and every time a search was cleared.
 *
 * Same locale and options as before, so the sort order is unchanged.
 */
const collator = new Intl.Collator(undefined, { numeric: true, sensitivity: 'base' });

/** Natural text ordering — numeric-aware, case- and accent-insensitive. */
export const compareText = collator.compare;

/**
 * Order two tracks by `column`. Numeric columns compare numerically; everything else
 * uses natural (numeric-aware, case- and accent-insensitive) text ordering.
 *
 * Ties fall back to `path`, so tracks from the same album keep filesystem order. That
 * tie-break is always ascending — reversing it would scramble album order when sorting
 * descending by, say, artist.
 */
export function compareTracks(a: Track, b: Track, column: SortColumn, dir: 1 | -1): number {
	const va = a[column] ?? '';
	const vb = b[column] ?? '';

	let cmp: number;
	if (typeof va === 'number' && typeof vb === 'number') {
		cmp = (va - vb) * dir;
	} else {
		cmp = compareText(String(va), String(vb)) * dir;
	}

	if (cmp === 0 && column !== 'path') {
		return compareText(a.path ?? '', b.path ?? '');
	}
	return cmp;
}

/** Sorted copy of `tracks` (the input is left untouched — callers share these arrays). */
export function sortTracks(tracks: Track[], column: SortColumn, direction: SortDirection): Track[] {
	const dir: 1 | -1 = direction === 'asc' ? 1 : -1;
	return [...tracks].sort((a, b) => compareTracks(a, b, column, dir));
}
