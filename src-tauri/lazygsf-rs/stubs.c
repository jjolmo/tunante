// Stub implementations for mGBA functions not needed for audio playback.
// These satisfy the linker without including the full cheats/gbp modules
// which have cross-platform compilation issues (MSVC, Apple Clang).

#include <stddef.h>
#include <stdint.h>

// Cheat system stubs
size_t mCheatSetsSize(const void* sets) { (void)sets; return 0; }
void* mCheatSetsGetPointer(const void* sets, size_t i) { (void)sets; (void)i; return NULL; }
void mCheatRefresh(void* device, void* sets) { (void)device; (void)sets; }
void mCheatDeviceDestroy(void* device) { (void)device; }

// GBA cheat device stub
void* GBACheatDeviceCreate(void) { return NULL; }

// GBA SIO Player (Game Boy Player) stubs
void GBASIOPlayerInit(void* sio) { (void)sio; }
void GBASIOPlayerReset(void* sio) { (void)sio; }
void GBASIOPlayerUpdate(void* gba) { (void)gba; }
