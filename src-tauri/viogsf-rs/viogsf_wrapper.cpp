/* viogsf_wrapper.cpp - C wrapper around VBA-M GBA emulator for GSF playback */

#include "viogsf_wrapper.h"
#include "../viogsf/vbam/gba/GBA.h"
#include "../viogsf/vbam/gba/Sound.h"
#include <cstdio>

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

    static int write_call_count;
    void write(const void* samples, unsigned long bytes) override {
        const int16_t* src = (const int16_t*)samples;
        size_t count = bytes / sizeof(int16_t);
        if (write_call_count < 10) {
            // Check if samples have actual audio data (not silence)
            int16_t max_val = 0;
            for (size_t i = 0; i < count && i < 100; i++) {
                int16_t abs_val = src[i] < 0 ? -src[i] : src[i];
                if (abs_val > max_val) max_val = abs_val;
            }
            fprintf(stderr, "[viogsf] write: bytes=%lu samples=%zu avail=%zu max_sample=%d\n",
                    bytes, count, available, max_val);
            write_call_count++;
        }
        for (size_t i = 0; i < count; i++) {
            data[write_pos] = src[i];
            write_pos = (write_pos + 1) % CAPACITY;
        }
        available += count;
        if (available > CAPACITY) available = CAPACITY;
    }

    static int read_call_count;
    size_t read(int16_t* dst, size_t count) {
        size_t to_read = count < available ? count : available;
        for (size_t i = 0; i < to_read; i++) {
            dst[i] = data[read_pos];
            read_pos = (read_pos + 1) % CAPACITY;
        }
        if (read_call_count < 5 && to_read > 0) {
            int16_t max_val = 0;
            for (size_t i = 0; i < to_read && i < 100; i++) {
                int16_t abs_val = dst[i] < 0 ? -dst[i] : dst[i];
                if (abs_val > max_val) max_val = abs_val;
            }
            fprintf(stderr, "[viogsf] read: requested=%zu got=%zu max_out=%d\n",
                    count, to_read, max_val);
            read_call_count++;
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

int AudioBuffer::write_call_count = 0;
int AudioBuffer::read_call_count = 0;

extern "C" {

viogsf_state_t* viogsf_create(uint32_t sample_rate) {
    viogsf_state_t* state = new viogsf_state();
    state->gba = new GBASystem();
    state->audio = new AudioBuffer();
    state->sample_rate = sample_rate;
    state->loaded = false;

    /* Configure for GSF playback (no video, no BIOS) */
    state->gba->useBios = false;
    state->gba->skipBios = true;
    state->gba->cpuDisableSfx = true;
    state->gba->speedHack = false;

    return state;
}

int viogsf_load_rom(viogsf_state_t* state, const uint8_t* data, uint32_t size, uint32_t entry_point) {
    if (!state || !data || size == 0) return -1;

    /* Follow the exact init sequence from deadbeef_GSFdecoder (working reference):
     * 1. CPULoadRom — allocate memory and load ROM
     * 2. soundInit + soundSetSampleRate + soundReset — set up audio pipeline
     * 3. CPUInit — initialize CPU state (needs bios from CPULoadRom)
     * 4. CPUReset — reset emulator to start state
     */
    int result = CPULoadRom(state->gba, data, size);
    if (result == 0) return -1;

    state->gba->output = state->audio;
    soundInit(state->gba, state->audio);
    soundSetSampleRate(state->gba, state->sample_rate);
    soundReset(state->gba);

    CPUInit(state->gba);
    CPUReset(state->gba);

    fprintf(stderr, "[viogsf] load_rom OK: size=%u, stereo_buffer=%p, gb_apu=%p, sampleRate=%ld\n",
            size, (void*)state->gba->stereo_buffer, (void*)state->gba->gb_apu,
            state->gba->soundSampleRate);

    state->loaded = true;
    return 0;
}

int viogsf_render(viogsf_state_t* state, int16_t* buf, size_t count) {
    if (!state || !state->loaded || !buf) return -1;

    static int render_call_count = 0;
    if (render_call_count < 3) {
        fprintf(stderr, "[viogsf] render called: count=%zu, audio_avail=%zu, gba=%p, stereo_buffer=%p\n",
                count, state->audio->available, (void*)state->gba,
                (void*)state->gba->stereo_buffer);
        render_call_count++;
    }

    size_t samples_needed = count * 2; /* stereo */
    size_t samples_written = 0;
    int empty_frames = 0;

    while (samples_written < samples_needed) {
        /* Run emulation (~250000 ticks, matching deadbeef_GSFdecoder reference) */
        CPULoop(state->gba, 250000);

        /* Read available samples from the audio buffer */
        size_t remaining = samples_needed - samples_written;
        size_t got = state->audio->read(buf + samples_written, remaining);
        samples_written += got;

        if (got == 0) {
            empty_frames++;
            /* Give the emulator up to 120 empty frames (~2 seconds) to start
             * producing audio. GBA games need time to initialize. */
            if (empty_frames > 120) {
                memset(buf + samples_written, 0,
                       (samples_needed - samples_written) * sizeof(int16_t));
                break;
            }
        } else {
            empty_frames = 0;
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
