/* viogsf_wrapper.cpp - C wrapper around VBA-M GBA emulator for GSF playback */

#include "viogsf_wrapper.h"
#include "../viogsf/vbam/gba/GBA.h"
#include "../viogsf/vbam/gba/Sound.h"

#include <cstring>
#include <cstdlib>
#include <vector>

/* Ring buffer to collect audio samples from the emulator callback */
struct AudioBuffer : public GBASoundOut {
    std::vector<int16_t> data;
    size_t write_pos;
    size_t read_pos;
    size_t available;
    static const size_t CAPACITY = 65536; /* in samples (L+R pairs = CAPACITY/2 frames) */

    AudioBuffer() : write_pos(0), read_pos(0), available(0) {
        data.resize(CAPACITY, 0);
    }

    void write(const void* samples, unsigned long bytes) override {
        const int16_t* src = (const int16_t*)samples;
        size_t count = bytes / sizeof(int16_t);
        for (size_t i = 0; i < count; i++) {
            data[write_pos] = src[i];
            write_pos = (write_pos + 1) % CAPACITY;
        }
        available += count;
        if (available > CAPACITY) available = CAPACITY;
    }

    size_t read(int16_t* dst, size_t count) {
        size_t to_read = count < available ? count : available;
        for (size_t i = 0; i < to_read; i++) {
            dst[i] = data[read_pos];
            read_pos = (read_pos + 1) % CAPACITY;
        }
        available -= to_read;
        return to_read;
    }
};

struct viogsf_state {
    GBASystem* gba;
    AudioBuffer* audio;
    uint32_t sample_rate;
    bool loaded;
};

extern "C" {

viogsf_state_t* viogsf_create(uint32_t sample_rate) {
    viogsf_state_t* state = new viogsf_state();
    state->gba = new GBASystem();
    state->audio = new AudioBuffer();
    state->sample_rate = sample_rate;
    state->loaded = false;

    /* Initialize the GBA system */
    CPUInit(state->gba);

    /* Set up audio */
    state->gba->output = state->audio;
    soundInit(state->gba, state->audio);
    soundSetSampleRate(state->gba, sample_rate);

    /* Configure for GSF playback (no video, no BIOS) */
    state->gba->useBios = false;
    state->gba->skipBios = true;
    state->gba->cpuDisableSfx = true;
    state->gba->speedHack = false;

    return state;
}

int viogsf_load_rom(viogsf_state_t* state, const uint8_t* data, uint32_t size) {
    if (!state || !data || size == 0) return -1;

    /* CPULoadRom expects the ROM data and its size */
    int result = CPULoadRom(state->gba, data, size);
    if (result == 0) return -1; /* CPULoadRom returns 0 on failure */

    CPUReset(state->gba);
    state->loaded = true;
    return 0;
}

int viogsf_render(viogsf_state_t* state, int16_t* buf, size_t count) {
    if (!state || !state->loaded || !buf) return -1;

    size_t samples_needed = count * 2; /* stereo */
    size_t samples_written = 0;

    while (samples_written < samples_needed) {
        /* Run one frame of GBA emulation (~280896 ticks per frame) */
        CPULoop(state->gba, 280896);

        /* Read available samples from the audio buffer */
        size_t remaining = samples_needed - samples_written;
        size_t got = state->audio->read(buf + samples_written, remaining);
        samples_written += got;

        /* Safety: if no samples produced after a frame, break to avoid infinite loop */
        if (got == 0) {
            /* Fill remainder with silence */
            memset(buf + samples_written, 0, (samples_needed - samples_written) * sizeof(int16_t));
            break;
        }
    }

    return 0;
}

void viogsf_restart(viogsf_state_t* state) {
    if (!state || !state->loaded) return;
    CPUReset(state->gba);
    state->audio->write_pos = 0;
    state->audio->read_pos = 0;
    state->audio->available = 0;
}

void viogsf_destroy(viogsf_state_t* state) {
    if (!state) return;
    if (state->gba) {
        soundShutdown(state->gba);
        CPUCleanUp(state->gba);
        delete state->gba;
    }
    if (state->audio) delete state->audio;
    delete state;
}

} /* extern "C" */
