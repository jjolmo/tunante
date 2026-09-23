/* Getters for the few `KSS` fields the Rust side reads.
 *
 * Through functions rather than by declaring the struct in Rust: the layout is
 * upstream's business, and a field added there would silently shift every
 * offset in a hand-written mirror. */
#include "kss.h"

int tunante_kss_track_min(const KSS *kss) { return kss ? kss->trk_min : 0; }
int tunante_kss_track_max(const KSS *kss) { return kss ? kss->trk_max : 0; }
int tunante_kss_is_kssx(const KSS *kss) { return kss ? kss->kssx : 0; }
