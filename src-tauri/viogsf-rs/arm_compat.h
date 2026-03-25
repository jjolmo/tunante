/* arm_compat.h — Fix regparm attribute on non-x86 platforms.
 * regparm is an x86-only GCC extension. On ARM/aarch64, it doesn't exist.
 * This header is force-included before all sources to redefine regparm
 * as a no-op macro, so __attribute__((regparm(2))) becomes
 * __attribute__((/*nothing*/)) which GCC/Clang accept silently. */
#define regparm(x) /* nothing */
