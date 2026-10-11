# smithay-clipboard, patched

Upstream smithay-clipboard 0.7.3, with one addition: file drops.
`set_drop_handler` installs a process-wide callback, and the data device's
`enter`/`leave`/`drop_performed` (empty upstream) accept `text/uri-list` drags
and hand the dropped URI list to it.

Why here: winit 0.30 has no Wayland drag-and-drop, and KWin delivers a drag only
to the *first* `wl_data_device` a client created (`dropHandlerForSurface` takes
`list.first()`). In a Slint app that is this crate's, created by the winit
backend before any app code runs, so a data device of the app's own never sees
a drag. Without a handler installed drags are refused, as before.

Every change is marked `Tunante patch` (`src/lib.rs`, `src/state.rs`,
`src/drop.rs`). Drop this copy once winit or Slint grow Wayland DnD.

Trimmed to what a path dependency needs: no examples or dev-dependencies.
