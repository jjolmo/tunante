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

**What this repo does about it:** `tunante` holds a logind sleep inhibitor
while it is playing (`apps/tunante/src/inhibit.rs`), so the app
no longer causes the suspend that triggers this. That is a way of not stepping
on the rake, not a repair.

---

## The library lists tracks it cannot play

**Where it belongs:** here, not upstream. It is an asymmetry of our own making.

Seen on Android and on the desktop, identically — so this is not a port
problem.

`/storage/emulated/0/Music/Samsung/Over_the_Horizon.m4a`, a ringtone Samsung
ships, is an `mp42` container with a `dby1` brand: Dolby, 768 kbps. The scan
accepts it and the player cannot open it:

```
probe --fast   ok = true, codec = M4A, has_artwork = true
play           Decoder error: The format of the data has not been recognized.
```

The two answers come from different code. Metadata is read with **lofty**, which
parses tags and never decodes; playback goes through **symphonia**, which has no
AC-4. So the track lands in the library with its title, album, duration and
cover art, looks completely ordinary, and fails the moment it is pressed.

Making the scan agree would mean decoding a frame of every file, and `probe
--fast` exists precisely because a library scan cannot afford that — it is the
difference between 4 ms and over a second per file (see
`tunante-helper::scan`).

The cheap answer is the other way round: when `play` fails, remember that this
path could not be opened and draw it differently, so the library learns from the
one moment it does find out. Not done.
