/*
 * sexypsf_wrapper.c - Thread-based push-to-pull bridge for sexypsf
 *
 * sexypsf uses a callback (push) model for audio output via sexyd_update(),
 * but rodio needs a pull model (render N samples into my buffer). This wrapper
 * uses a background thread running sexy_execute() with mutex/condvar
 * synchronization to transfer audio data between the emulator and the caller.
 *
 * Previous approach using setjmp/longjmp was unreliable — the longjmp target's
 * stack frames could be overwritten between renders, causing SIGSEGV.
 *
 * Copyright (C) 2026 - GPL-2.0 (same as sexypsf)
 */

#include <string.h>
#include <stdint.h>
#include <stdlib.h>
#include <stdarg.h>

/* ========================================================================= */
/* Platform-specific threading abstraction                                    */
/* ========================================================================= */

#ifdef _WIN32
#define WIN32_LEAN_AND_MEAN
#include <windows.h>

typedef CRITICAL_SECTION   wrapper_mutex_t;
typedef CONDITION_VARIABLE wrapper_cond_t;
typedef HANDLE             wrapper_thread_t;

static void wrapper_mutex_init(wrapper_mutex_t *m)   { InitializeCriticalSection(m); }
static void wrapper_mutex_destroy(wrapper_mutex_t *m) { DeleteCriticalSection(m); }
static void wrapper_mutex_lock(wrapper_mutex_t *m)    { EnterCriticalSection(m); }
static void wrapper_mutex_unlock(wrapper_mutex_t *m)  { LeaveCriticalSection(m); }

static void wrapper_cond_init(wrapper_cond_t *c)      { InitializeConditionVariable(c); }
static void wrapper_cond_destroy(wrapper_cond_t *c)    { (void)c; /* no-op on Windows */ }
static void wrapper_cond_signal(wrapper_cond_t *c)     { WakeConditionVariable(c); }
static void wrapper_cond_wait(wrapper_cond_t *c, wrapper_mutex_t *m) {
    SleepConditionVariableCS(c, m, INFINITE);
}

static DWORD WINAPI emu_thread_func_win(LPVOID arg);

static int wrapper_thread_create(wrapper_thread_t *t) {
    *t = CreateThread(NULL, 0, emu_thread_func_win, NULL, 0, NULL);
    return (*t == NULL) ? -1 : 0;
}
static void wrapper_thread_join(wrapper_thread_t t) {
    WaitForSingleObject(t, INFINITE);
    CloseHandle(t);
}

#else /* POSIX */
#include <pthread.h>

typedef pthread_mutex_t wrapper_mutex_t;
typedef pthread_cond_t  wrapper_cond_t;
typedef pthread_t       wrapper_thread_t;

static void wrapper_mutex_init(wrapper_mutex_t *m)   { pthread_mutex_init(m, NULL); }
static void wrapper_mutex_destroy(wrapper_mutex_t *m) { pthread_mutex_destroy(m); }
static void wrapper_mutex_lock(wrapper_mutex_t *m)    { pthread_mutex_lock(m); }
static void wrapper_mutex_unlock(wrapper_mutex_t *m)  { pthread_mutex_unlock(m); }

static void wrapper_cond_init(wrapper_cond_t *c)      { pthread_cond_init(c, NULL); }
static void wrapper_cond_destroy(wrapper_cond_t *c)    { pthread_cond_destroy(c); }
static void wrapper_cond_signal(wrapper_cond_t *c)     { pthread_cond_signal(c); }
static void wrapper_cond_wait(wrapper_cond_t *c, wrapper_mutex_t *m) {
    pthread_cond_wait(c, m);
}

static void *emu_thread_func_posix(void *arg);

static int wrapper_thread_create(wrapper_thread_t *t) {
    return pthread_create(t, NULL, emu_thread_func_posix, NULL);
}
static void wrapper_thread_join(wrapper_thread_t t) {
    pthread_join(t, NULL);
}

#endif

#include "driver.h"

/* Forward declarations for sexypsf internal functions */
extern void SPUsetlength(int32_t stop, int32_t fade);

/* Forward declarations for our own functions */
void sexypsf_close(void);

/* ========================================================================= */
/* Global stop flag — checked by intExecute() in PsxInterpreter.c            */
/*                                                                           */
/* When set to 1, the CPU loop in intExecute() will call psxShutdown() and   */
/* return, causing the emulator thread to exit cleanly. This is MUCH safer   */
/* than calling psxShutdown() from the main thread while the CPU loop runs.  */
/* ========================================================================= */

volatile int sexypsf_stop_flag = 0;

/* ========================================================================= */
/* Ring buffer for audio transfer between emulator thread and render caller   */
/* ========================================================================= */

#define RING_SIZE (65536)  /* 64K stereo frames — ~1.5 seconds at 44100 Hz */

static int16_t ring_buf[RING_SIZE * 2]; /* Stereo interleaved */
static volatile int ring_write;         /* Write position (emulator thread) */
static volatile int ring_read;          /* Read position (render caller)    */

static wrapper_mutex_t ring_mutex;
static wrapper_cond_t  ring_data_avail;   /* Emulator wrote data  */
static wrapper_cond_t  ring_space_avail;  /* Render read data     */
static int threading_initialized = 0;

static wrapper_thread_t emu_thread;
static volatile int emu_running;
static volatile int emu_finished;   /* Emulator thread exited naturally */

static PSFINFO *current_info;

static void ensure_threading_init(void)
{
    if (!threading_initialized) {
        wrapper_mutex_init(&ring_mutex);
        wrapper_cond_init(&ring_data_avail);
        wrapper_cond_init(&ring_space_avail);
        threading_initialized = 1;
    }
}

/* How many frames are available to read */
static int ring_available(void)
{
    int avail = ring_write - ring_read;
    if (avail < 0) avail += RING_SIZE;
    return avail;
}

/* How many frames of space available to write */
static int ring_space(void)
{
    return RING_SIZE - 1 - ring_available();
}

/* ========================================================================= */
/* sexyd_update - Audio output callback called by sexypsf's SPU              */
/*                                                                           */
/* Called from SPUendflush() during CPU execution. Writes audio data into    */
/* the ring buffer. If the ring buffer is full, waits for the render caller  */
/* to consume data.                                                          */
/* ========================================================================= */

void sexyd_update(unsigned char *buf, long len)
{
    if (buf == NULL || len == 0)
        return;

    int16_t *src = (int16_t *)buf;
    long frames = len / 4;  /* 4 bytes per stereo frame (2 x int16) */

    wrapper_mutex_lock(&ring_mutex);

    while (frames > 0) {
        /* Wait for space in ring buffer */
        while (ring_space() == 0 && emu_running) {
            wrapper_cond_wait(&ring_space_avail, &ring_mutex);
        }

        if (!emu_running) {
            wrapper_mutex_unlock(&ring_mutex);
            return;
        }

        /* Copy as much as we can */
        int space = ring_space();
        int to_copy = (int)frames;
        if (to_copy > space) to_copy = space;

        for (int i = 0; i < to_copy; i++) {
            int pos = (ring_write + i) % RING_SIZE;
            ring_buf[pos * 2]     = src[i * 2];
            ring_buf[pos * 2 + 1] = src[i * 2 + 1];
        }
        ring_write = (ring_write + to_copy) % RING_SIZE;
        src += to_copy * 2;
        frames -= to_copy;

        /* Signal that data is available */
        wrapper_cond_signal(&ring_data_avail);
    }

    wrapper_mutex_unlock(&ring_mutex);
}

/* ========================================================================= */
/* Emulator thread entry point                                               */
/* ========================================================================= */

static void emu_thread_main(void)
{
    sexy_execute();
    /* sexy_execute() returned — song ended or stop flag was set.
     * intExecute() already called psxShutdown() before returning. */
    wrapper_mutex_lock(&ring_mutex);
    emu_finished = 1;
    wrapper_cond_signal(&ring_data_avail); /* Wake up render if waiting */
    wrapper_mutex_unlock(&ring_mutex);
}

#ifdef _WIN32
static DWORD WINAPI emu_thread_func_win(LPVOID arg)
{
    (void)arg;
    emu_thread_main();
    return 0;
}
#else
static void *emu_thread_func_posix(void *arg)
{
    (void)arg;
    emu_thread_main();
    return NULL;
}
#endif

/* ========================================================================= */
/* __Log - Required by sexypsf's PsxCommon.h (debug logging, suppressed)     */
/* ========================================================================= */

void __Log(char *fmt, ...)
{
    (void)fmt;
}

/* ========================================================================= */
/* sexypsf_open - Load a PSF file and prepare for chunk-based decoding       */
/* ========================================================================= */

PSFINFO *sexypsf_open(const char *path)
{
    /* Close any existing session first */
    sexypsf_close();

    ensure_threading_init();

    ring_write = 0;
    ring_read = 0;
    emu_running = 0;
    emu_finished = 0;
    sexypsf_stop_flag = 0;

    current_info = sexy_load((char *)path);
    if (current_info) {
        SPUsetlength(~(int32_t)0, 0);

        /* Start emulator thread */
        emu_running = 1;
        if (wrapper_thread_create(&emu_thread) != 0) {
            emu_running = 0;
            sexy_freepsfinfo(current_info);
            current_info = NULL;
            return NULL;
        }
    }

    return current_info;
}

/* ========================================================================= */
/* sexypsf_render - Read stereo frames from the ring buffer                  */
/*                                                                           */
/* Fills `buf` with up to `count` stereo frames. Returns the number of      */
/* frames actually written. Returns 0 if the song has ended.                 */
/* ========================================================================= */

int sexypsf_render(int16_t *buf, int count)
{
    if (current_info == NULL)
        return 0;

    int written = 0;

    wrapper_mutex_lock(&ring_mutex);

    while (written < count) {
        /* Wait for data in ring buffer */
        while (ring_available() == 0 && !emu_finished && emu_running) {
            wrapper_cond_wait(&ring_data_avail, &ring_mutex);
        }

        int avail = ring_available();
        if (avail == 0) {
            /* No data and emulator finished — song ended */
            break;
        }

        int to_read = count - written;
        if (to_read > avail) to_read = avail;

        for (int i = 0; i < to_read; i++) {
            int pos = (ring_read + i) % RING_SIZE;
            buf[(written + i) * 2]     = ring_buf[pos * 2];
            buf[(written + i) * 2 + 1] = ring_buf[pos * 2 + 1];
        }
        ring_read = (ring_read + to_read) % RING_SIZE;
        written += to_read;

        /* Signal that space is available */
        wrapper_cond_signal(&ring_space_avail);
    }

    wrapper_mutex_unlock(&ring_mutex);
    return written;
}

/* ========================================================================= */
/* sexypsf_close - Stop the emulator and free all resources                  */
/*                                                                           */
/* Sets the stop flag so that intExecute() exits cleanly (calling            */
/* psxShutdown itself), then joins the thread. We do NOT call psxShutdown    */
/* from this thread — that would race with the emulator thread.              */
/* ========================================================================= */

void sexypsf_close(void)
{
    if (emu_running) {
        /* Tell intExecute to exit on next iteration */
        sexypsf_stop_flag = 1;

        /* Wake emulator if it's blocked in sexyd_update waiting for ring space */
        wrapper_mutex_lock(&ring_mutex);
        emu_running = 0;
        wrapper_cond_signal(&ring_space_avail);
        wrapper_mutex_unlock(&ring_mutex);

        /* Wait for the emulator thread to exit.
         * intExecute() will see sexypsf_stop_flag, call psxShutdown(), and return. */
        wrapper_thread_join(emu_thread);
    }

    if (current_info) {
        sexy_freepsfinfo(current_info);
        current_info = NULL;
    }

    ring_write = 0;
    ring_read = 0;
    emu_running = 0;
    emu_finished = 0;
    sexypsf_stop_flag = 0;
}

/* ========================================================================= */
/* sexypsf_getinfo - Read PSF metadata without initializing the emulator     */
/* ========================================================================= */

PSFINFO *sexypsf_getinfo(const char *path)
{
    return sexy_getpsfinfo((char *)path);
}
