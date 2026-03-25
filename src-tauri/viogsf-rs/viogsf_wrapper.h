/* viogsf_wrapper.h - C API for viogsf (VBA-M based GSF decoder) */
#ifndef VIOGSF_WRAPPER_H
#define VIOGSF_WRAPPER_H

#include <stdint.h>
#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct viogsf_state viogsf_state_t;

/* Create a new GSF decoder state */
viogsf_state_t* viogsf_create(uint32_t sample_rate);

/* Load a GSF ROM into the decoder. data/size is the assembled ROM.
 * entry_point is the GBA entry point address from the GSF header. */
int viogsf_load_rom(viogsf_state_t* state, const uint8_t* data, uint32_t size, uint32_t entry_point);

/* Render count stereo frames of audio into buf (interleaved i16 L,R,L,R...).
 * buf must have space for count*2 int16_t values.
 * Returns 0 on success. */
int viogsf_render(viogsf_state_t* state, int16_t* buf, size_t count);

/* Reset playback to the beginning */
void viogsf_restart(viogsf_state_t* state);

/* Free all resources */
void viogsf_destroy(viogsf_state_t* state);

#ifdef __cplusplus
}
#endif

#endif /* VIOGSF_WRAPPER_H */
