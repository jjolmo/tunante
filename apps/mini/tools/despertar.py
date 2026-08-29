#!/usr/bin/env python3
"""Press the power button, over uinput, to wake the panel.

With the screen off the compositor stops drawing, so every screenshot is the
last frame it painted — a stale clock and no sign of whatever the app is doing
now. Touch does not help: the panel is suspended too, and `tactil.py` events go
nowhere. A key event from a fresh uinput device is seen as real user activity,
which is what both logind and the compositor are waiting for.
"""

import fcntl, os, struct, sys, time

EV_SYN, EV_KEY = 0x00, 0x01
SYN_REPORT = 0
KEY_POWER = 116

UI_DEV_CREATE, UI_DEV_DESTROY = 0x5501, 0x5502
UI_SET_EVBIT = 0x40045564
UI_SET_KEYBIT = 0x40045565

key = int(sys.argv[1]) if len(sys.argv) > 1 else KEY_POWER

fd = os.open("/dev/uinput", os.O_WRONLY | os.O_NONBLOCK)
for ev in (EV_KEY, EV_SYN):
    fcntl.ioctl(fd, UI_SET_EVBIT, ev)
fcntl.ioctl(fd, UI_SET_KEYBIT, key)

dev = struct.pack("80sHHHHi" + "i" * 64 * 4, b"tunante-test-power",
                  0x03, 0x1234, 0x5679, 1, 0,
                  *([0] * 64), *([0] * 64), *([0] * 64), *([0] * 64))
os.write(fd, dev)
fcntl.ioctl(fd, UI_DEV_CREATE)
# The compositor has to notice a new keyboard before it will route to it.
time.sleep(1.5)


def emit(t, c, v):
    os.write(fd, struct.pack("qqHHi", 0, 0, t, c, v))


emit(EV_KEY, key, 1)
emit(EV_SYN, SYN_REPORT, 0)
time.sleep(0.06)
emit(EV_KEY, key, 0)
emit(EV_SYN, SYN_REPORT, 0)
time.sleep(0.4)

print(f"key {key} pressed")
fcntl.ioctl(fd, UI_DEV_DESTROY)
os.close(fd)
