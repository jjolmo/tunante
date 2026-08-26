# Upstream bugs

Things wrong below Tunante, that Tunante can only work around. Each one is
worth reporting where it belongs rather than absorbing quietly — a workaround
in this repo is a bug someone else never hears about.

---

## The Poco X3 loses its sound card across a suspend, until it is rebooted

**Where it belongs:** postmarketOS (`surya` / sm7150), and from there the
kernel's `snd-sm8250` machine driver and the `wcd937x` codec.

**Seen on:** postmarketOS edge, kernel `7.1.0-rc3-sm7150`, Poco X3 NFC.

Suspend the phone while audio is playing and resume it. Sometimes — not always,
which is what makes it a race and not a rule — the card does not come back:

```
wcd937x_codec audio-codec: ASoC error (-16): at snd_soc_component_probe() on audio-codec
snd-sm8250 sound: ASoC: failed to instantiate card -16
```

`-16` is `EBUSY`. Afterwards:

- `/proc/asound/cards` reads `--- no soundcards ---`.
- PulseAudio's sink dies with `alsa-util.c: Got POLLNVAL from ALSA` and every
  client is moved to the `auto_null` sink that `module-always-sink` provides.
- Unbinding and rebinding the driver by hand to force a re-probe fails the same
  way: `echo sound > /sys/bus/platform/drivers/snd-sm8250/unbind` returns
  `Resource busy`.

**Only a reboot brings the speaker back.** A phone that loses its speaker until
you restart it is a bug in its own right, whatever else is going on around it.

It first appeared on the second suspend of a session, having survived the
first, so a reproduction probably needs a few cycles rather than one. The ADSP
restarting across the resume is the obvious thing to look at: the probe failing
with `EBUSY` reads like the codec is still held by something that did not let
go on the way down.

**What this repo does about it:** `tunante-mini` holds a logind sleep inhibitor
while it is playing (`src-tauri/crates/tunante-mini/src/inhibit.rs`), so the app
no longer causes the suspend that triggers this. That is a way of not stepping
on the rake, not a repair.
