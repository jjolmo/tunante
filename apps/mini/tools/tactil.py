#!/usr/bin/env python3
"""A virtual touchscreen over uinput, to drive gestures without a finger.

Creates a fake touchscreen the size of the real panel and plays one gesture
back. It answers "does the drag scroll the list?" without asking anyone to
touch the phone, and it answers the same way every time.

    tactil.py flick X Y1 Y2 [ms]                  drag vertically
    tactil.py drag  X1 Y1 X2 Y2 [ms]              drag in any direction
    tactil.py tap   X Y
    tactil.py hold  X1 Y1 X2 Y2 [ms] [hold_ms]    press, wait, then drag

`hold` exists because Slint's Flickable only claims a gesture when the movement
starts within 500 ms of the press. Hold for longer than that and the drag
reaches the item underneath instead of scrolling the list.

Coordinates are the device's own only in portrait. Rotated right, the mapping
is device(dx, dy) -> screen(dy, 1080 - dx).

The virtual panel is the phone's by default. Set TACTIL_W/TACTIL_H to drive a
desktop build instead, where the panel has to span the whole virtual desktop: a
compositor maps an absolute pointing device across every output at once, so a
two-monitor session needs their combined extent, not one screen's.
"""

import fcntl, os, struct, sys, time

W = int(os.environ.get("TACTIL_W", 1080))
H = int(os.environ.get("TACTIL_H", 2400))

EV_SYN, EV_KEY, EV_ABS = 0x00, 0x01, 0x03
SYN_REPORT = 0
BTN_TOUCH = 0x14a
ABS_MT_SLOT = 0x2f
ABS_MT_POSITION_X = 0x35
ABS_MT_POSITION_Y = 0x36
ABS_MT_TRACKING_ID = 0x39
ABS_X, ABS_Y = 0x00, 0x01
INPUT_PROP_DIRECT = 0x01

UI_DEV_CREATE = 0x5501
UI_DEV_DESTROY = 0x5502
UI_SET_EVBIT = 0x40045564
UI_SET_KEYBIT = 0x40045565
UI_SET_ABSBIT = 0x40045567
UI_SET_PROPBIT = 0x4004556e


def main():
    fd = os.open("/dev/uinput", os.O_WRONLY | os.O_NONBLOCK)

    for ev in (EV_KEY, EV_ABS, EV_SYN):
        fcntl.ioctl(fd, UI_SET_EVBIT, ev)
    fcntl.ioctl(fd, UI_SET_KEYBIT, BTN_TOUCH)
    fcntl.ioctl(fd, UI_SET_PROPBIT, INPUT_PROP_DIRECT)
    for axis in (ABS_X, ABS_Y, ABS_MT_SLOT, ABS_MT_POSITION_X,
                 ABS_MT_POSITION_Y, ABS_MT_TRACKING_ID):
        fcntl.ioctl(fd, UI_SET_ABSBIT, axis)

    # struct uinput_user_dev: name[80], input_id (4x u16), ff_effects_max,
    # then absmax/absmin/absfuzz/absflat, 64 ints each.
    absmax = [0] * 64
    absmin = [0] * 64
    absmax[ABS_X] = absmax[ABS_MT_POSITION_X] = W - 1
    absmax[ABS_Y] = absmax[ABS_MT_POSITION_Y] = H - 1
    absmax[ABS_MT_SLOT] = 9
    absmax[ABS_MT_TRACKING_ID] = 65535

    dev = struct.pack(
        "80sHHHHi" + "i" * 64 * 4,
        b"tunante-test-touch",
        0x03, 0x1234, 0x5678, 1,   # BUS_USB, vendor, product, version
        0,
        *absmax, *absmin, *([0] * 64), *([0] * 64),
    )
    os.write(fd, dev)
    fcntl.ioctl(fd, UI_DEV_CREATE)

    # Give the compositor a moment to notice a new touchscreen and map it to
    # an output. Without this the first gesture lands nowhere.
    time.sleep(1.5)

    def emit(t, c, v):
        os.write(fd, struct.pack("qqHHi", 0, 0, t, c, v))

    def syn():
        emit(EV_SYN, SYN_REPORT, 0)

    def down(x, y):
        emit(EV_ABS, ABS_MT_SLOT, 0)
        emit(EV_ABS, ABS_MT_TRACKING_ID, 1)
        emit(EV_ABS, ABS_MT_POSITION_X, x)
        emit(EV_ABS, ABS_MT_POSITION_Y, y)
        emit(EV_KEY, BTN_TOUCH, 1)
        emit(EV_ABS, ABS_X, x)
        emit(EV_ABS, ABS_Y, y)
        syn()

    def move(x, y):
        emit(EV_ABS, ABS_MT_SLOT, 0)
        emit(EV_ABS, ABS_MT_POSITION_X, x)
        emit(EV_ABS, ABS_MT_POSITION_Y, y)
        emit(EV_ABS, ABS_X, x)
        emit(EV_ABS, ABS_Y, y)
        syn()

    def up():
        emit(EV_ABS, ABS_MT_SLOT, 0)
        emit(EV_ABS, ABS_MT_TRACKING_ID, -1)
        emit(EV_KEY, BTN_TOUCH, 0)
        syn()

    cmd = sys.argv[1]
    if cmd == "tap":
        x, y = int(sys.argv[2]), int(sys.argv[3])
        down(x, y)
        time.sleep(0.05)
        up()
        print(f"tap at {x},{y}")
    elif cmd == "hold":
        x1, y1, x2, y2 = (int(v) for v in sys.argv[2:6])
        ms = int(sys.argv[6]) if len(sys.argv) > 6 else 500
        hold_ms = int(sys.argv[7]) if len(sys.argv) > 7 else 800
        steps = 25
        down(x1, y1)
        time.sleep(hold_ms / 1000)         # stay still, or the Flickable takes it
        for i in range(1, steps + 1):
            move(int(x1 + (x2 - x1) * i / steps), int(y1 + (y2 - y1) * i / steps))
            time.sleep(ms / 1000 / steps)
        up()
        print(f"hold {hold_ms}ms then ({x1},{y1}) -> ({x2},{y2})")
    elif cmd in ("flick", "drag"):
        if cmd == "flick":
            x1 = x2 = int(sys.argv[2])
            y1, y2 = int(sys.argv[3]), int(sys.argv[4])
            ms = int(sys.argv[5]) if len(sys.argv) > 5 else 250
        else:
            x1, y1, x2, y2 = (int(v) for v in sys.argv[2:6])
            ms = int(sys.argv[6]) if len(sys.argv) > 6 else 250
        steps = 25
        down(x1, y1)
        for i in range(1, steps + 1):
            move(int(x1 + (x2 - x1) * i / steps), int(y1 + (y2 - y1) * i / steps))
            time.sleep(ms / 1000 / steps)
        up()
        print(f"{cmd} ({x1},{y1}) -> ({x2},{y2}) in {ms}ms")

    time.sleep(0.3)
    fcntl.ioctl(fd, UI_DEV_DESTROY)
    os.close(fd)


main()
