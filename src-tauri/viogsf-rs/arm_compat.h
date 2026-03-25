/* arm_compat.h — included before all sources on non-x86 platforms */
/* Override GBAcpu.h's regparm definition which is x86-only */
#define INSN_REGPARM /*nothing*/
/* Prevent GBAcpu.h from redefining it */
#define GBACPU_H
struct GBASystem;
extern int armExecute(GBASystem *);
extern int thumbExecute(GBASystem *);
#ifdef __GNUC__
# define LIKELY(x) __builtin_expect(!!(x),1)
# define UNLIKELY(x) __builtin_expect(!!(x),0)
#else
# define LIKELY(x) (x)
# define UNLIKELY(x) (x)
#endif
