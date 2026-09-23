/* The MGS/BGM/OPX/MPK/MBM converters, stubbed.
 *
 * `KSS_bin2kss` dispatches on the file's shape and links against all of them,
 * but the five real converters need the `kss-drivers` blobs, which libkss's own
 * LICENSE.md says do not comply with its licence — so they are not vendored and
 * those files are not compiled. See ../libkss/README.upstream.md.
 *
 * Only `.kss` is claimed here, and a `.kss` never reaches these paths: every
 * detector answers "not my format" and every converter declines, which sends
 * `KSS_bin2kss` down the `KSS_kss2kss` branch that is compiled for real. */
#include "kss.h"

int KSS_isMGSdata(uint8_t *d, uint32_t n) { (void)d; (void)n; return 0; }
int KSS_isBGMdata(uint8_t *d, uint32_t n) { (void)d; (void)n; return 0; }
int KSS_isMPK106data(uint8_t *d, uint32_t n) { (void)d; (void)n; return 0; }
int KSS_isMPK103data(uint8_t *d, uint32_t n) { (void)d; (void)n; return 0; }
int KSS_isOPXdata(uint8_t *d, uint32_t n) { (void)d; (void)n; return 0; }

KSS *KSS_mgs2kss(uint8_t *d, uint32_t n) { (void)d; (void)n; return 0; }
KSS *KSS_bgm2kss(uint8_t *d, uint32_t n) { (void)d; (void)n; return 0; }
KSS *KSS_opx2kss(uint8_t *d, uint32_t n) { (void)d; (void)n; return 0; }
KSS *KSS_mpk1032kss(uint8_t *d, uint32_t n) { (void)d; (void)n; return 0; }
KSS *KSS_mpk1062kss(uint8_t *d, uint32_t n) { (void)d; (void)n; return 0; }
KSS *KSS_mbm2kss(const uint8_t *d, uint32_t n) { (void)d; (void)n; return 0; }

void KSS_get_info_mgsdata(KSS *k, uint8_t *d, uint32_t n) { (void)k; (void)d; (void)n; }
void KSS_get_info_bgmdata(KSS *k, uint8_t *d, uint32_t n) { (void)k; (void)d; (void)n; }
void KSS_get_info_mpkdata(KSS *k, uint8_t *d, uint32_t n) { (void)k; (void)d; (void)n; }
void KSS_get_info_opxdata(KSS *k, uint8_t *d, uint32_t n) { (void)k; (void)d; (void)n; }
void KSS_get_info_mbmdata(KSS *k, uint8_t *d, uint32_t n) { (void)k; (void)d; (void)n; }
int KSS_autoload_mbk(const char *a, const char *b, const char *c) { (void)a; (void)b; (void)c; return 0; }
