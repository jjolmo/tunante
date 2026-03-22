#ifndef _SPSF_TYPES_H__
#define _SPSF_TYPES_H__

#include <inttypes.h>

#define INLINE inline

typedef int8_t s8;
typedef int16_t s16;
typedef int32_t s32;
typedef int64_t s64;
        
typedef uint8_t u8;
typedef uint16_t u16;
typedef uint32_t u32;
typedef uint64_t u64;



/* Struct packing attribute — all members are u32 so packing is
   effectively a no-op, but the original code requires the macro. */
#ifdef _MSC_VER
#define PACKSTRUCT
/* sexypsf headers define globals (psxRegs, psxM, etc.) without 'extern',
   which causes "multiply defined symbol" on MSVC. selectany tells the
   linker to pick one copy and discard duplicates (same as GCC -fcommon). */
#define PSX_GLOBAL __declspec(selectany)
#else
#define PACKSTRUCT	__attribute__ ((packed))
#define PSX_GLOBAL
#endif

#endif
