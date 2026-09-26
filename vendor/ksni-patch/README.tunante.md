# ksni, patched

Upstream ksni 0.3.6, with one addition: the `ProvideXdgActivationToken`
method of `org.kde.StatusNotifierItem`, surfaced as
`Tray::provide_xdg_activation_token`.

KDE Plasma calls it right before `Activate` with a token from the compositor.
Without it, a window the tray click shows or raises is held back by KWin's
focus-stealing prevention and opens behind whatever was on top — which is
what "clicking the tray icon sometimes does nothing" was.

Every change is marked `Tunante patch` (`src/lib.rs`, `src/dbus_interface.rs`,
`src/service.rs`). Drop this copy once upstream grows the method.

Trimmed to what a path dependency needs: no tests, examples or
dev-dependencies (they would land in the workspace's `Cargo.lock`) — so the
lib's own unit test modules, which need them, are cut out too — and
`default = ["async-io"]` rather than `["tokio"]`: the tray runs on async-io, and a
workspace-wide check would otherwise build tokio for nothing.
