//! Custom global shortcuts, through the XDG GlobalShortcuts portal.
//!
//! Spike 4's verdict, executed: the portal is the one way to get keys
//! while another window has focus that works on Wayland, needs no
//! permissions, and — the part evdev could never offer — lets the user
//! rebind everything in the desktop's own settings (KDE files these under
//! System Settings → Shortcuts, per application). The compositor shows
//! its own binding dialog on the first `BindShortcuts`, which is why the
//! session is only created when the user turns the row on: an app must
//! not open a system dialog uninvited.
//!
//! Same threading as [`crate::mpris`]: zbus wants an executor, Slint's
//! event loop is not one, so the portal conversation lives on its own
//! thread and everything crosses through one channel.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;

/// What a shortcut asks of the player, or what the portal says about them.
#[derive(Debug, Clone)]
pub enum Msg {
    /// The row's label: "sí", or why not.
    Status(String),
    PlayPause,
    Next,
    Prev,
}

/// The identities registered with the portal. The ids are permanent — the
/// desktop remembers bindings by (app, id), so renaming one would silently
/// orphan whatever the user assigned to it.
const SHORTCUTS: &[(&str, &str, &str)] = &[
    ("play-pause", "Reproducir/Pausa", "CTRL+ALT+SPACE"),
    ("next", "Siguiente pista", "CTRL+ALT+n"),
    ("prev", "Pista anterior", "CTRL+ALT+b"),
];

/// Start the portal session on its own thread.
///
/// `forward` gates delivery: turning the row off silences the shortcuts
/// without tearing the session down, and turning it back on is free.
#[cfg(target_os = "linux")]
pub fn spawn(forward: Arc<AtomicBool>) -> Receiver<Msg> {
    let (tx, rx) = std::sync::mpsc::channel::<Msg>();
    std::thread::Builder::new()
        .name("shortcuts".into())
        // Same reason as the MPRIS thread: musl's default stack is 128 KB
        // and zbus's async machinery wants more.
        .stack_size(1024 * 1024)
        .spawn(move || {
            if let Err(e) = run(&tx, forward) {
                let _ = tx.send(Msg::Status(
                    tunante_core::i18n::tr("no disponible ({})").replace("{}", &e.to_string()),
                ));
            }
        })
        .ok();
    rx
}

#[cfg(not(target_os = "linux"))]
pub fn spawn(_forward: Arc<AtomicBool>) -> Receiver<Msg> {
    let (tx, rx) = std::sync::mpsc::channel::<Msg>();
    let _ = tx.send(Msg::Status(tunante_core::i18n::tr("no disponible aquí")));
    rx
}

#[cfg(target_os = "linux")]
fn run(tx: &Sender<Msg>, forward: Arc<AtomicBool>) -> zbus::Result<()> {
    use futures_lite::StreamExt;
    use std::collections::HashMap;
    use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};

    async_io::block_on(async move {
        let conn = zbus::Connection::session().await?;

        // A host app has no identity unless it claims one, and this portal
        // refuses anonymous callers — it files bindings by app id. Register
        // must be this connection's first portal call, and it validates
        // against installed .desktop files, which is why enabling the row
        // writes the desktop entry first. Best-effort: launched from the
        // menu the identity may already be there.
        let registry = zbus::Proxy::new(
            &conn,
            "org.freedesktop.portal.Desktop",
            "/org/freedesktop/portal/desktop",
            "org.freedesktop.host.portal.Registry",
        )
        .await?;
        let empty: HashMap<&str, Value> = HashMap::new();
        if let Err(e) = registry.call_method("Register", &("tunante", empty)).await {
            log::warn!("shortcuts: el registro de identidad falló ({e}); sigo por si ya la hay");
        }

        let portal = zbus::Proxy::new(
            &conn,
            "org.freedesktop.portal.Desktop",
            "/org/freedesktop/portal/desktop",
            "org.freedesktop.portal.GlobalShortcuts",
        )
        .await?;

        // The portal answers method calls through a Response signal on a
        // Request object whose path is derived from our unique name and a
        // token we choose — so the listener can exist before the call does,
        // which is the whole point: no race with a fast portal.
        let unique = conn
            .unique_name()
            .map(|n| n.trim_start_matches(':').replace('.', "_"))
            .unwrap_or_default();
        let response_of = |token: &str| {
            format!("/org/freedesktop/portal/desktop/request/{unique}/{token}")
        };

        // --- CreateSession ------------------------------------------------
        let req = zbus::Proxy::new(
            &conn,
            "org.freedesktop.portal.Desktop",
            response_of("tunante_gs1"),
            "org.freedesktop.portal.Request",
        )
        .await?;
        let mut responses = req.receive_signal("Response").await?;

        let mut opts: HashMap<&str, Value> = HashMap::new();
        opts.insert("handle_token", Value::from("tunante_gs1"));
        opts.insert("session_handle_token", Value::from("tunante"));
        portal.call_method("CreateSession", &(opts,)).await?;

        let msg = responses
            .next()
            .await
            .ok_or_else(|| zbus::Error::Failure(tunante_core::i18n::tr("el portal colgó sin contestar")))?;
        let (code, results): (u32, HashMap<String, OwnedValue>) = msg.body().deserialize()?;
        if code != 0 {
            return Err(zbus::Error::Failure(
                tunante_core::i18n::tr("el portal rechazó la sesión (código {})")
                    .replace("{}", &code.to_string()),
            ));
        }
        // The spec files session_handle as a string; some portals have
        // shipped it as an object path. Take either.
        let session = results
            .get("session_handle")
            .and_then(|v| {
                String::try_from(v.clone()).ok().or_else(|| {
                    OwnedObjectPath::try_from(v.clone())
                        .ok()
                        .map(|p| p.to_string())
                })
            })
            .ok_or_else(|| zbus::Error::Failure("respuesta sin session_handle".into()))?;
        let session = zbus::zvariant::ObjectPath::try_from(session)
            .map_err(|e| zbus::Error::Failure(
                    tunante_core::i18n::tr("session_handle ilegible: {}").replace("{}", &e.to_string()),
                ))?;

        // --- BindShortcuts ------------------------------------------------
        //
        // KDE shows its binding dialog the first time and remembers the
        // result per (app, id) afterwards, so later launches bind silently.
        let req2 = zbus::Proxy::new(
            &conn,
            "org.freedesktop.portal.Desktop",
            response_of("tunante_gs2"),
            "org.freedesktop.portal.Request",
        )
        .await?;
        let mut responses2 = req2.receive_signal("Response").await?;

        let shortcuts: Vec<(&str, HashMap<&str, Value>)> = SHORTCUTS
            .iter()
            .map(|(id, desc, trigger)| {
                let mut m: HashMap<&str, Value> = HashMap::new();
                // The desktop's binding dialog shows this: the user's language.
                m.insert("description", Value::from(tunante_core::i18n::tr(desc)));
                m.insert("preferred_trigger", Value::from(*trigger));
                (*id, m)
            })
            .collect();
        let mut opts2: HashMap<&str, Value> = HashMap::new();
        opts2.insert("handle_token", Value::from("tunante_gs2"));
        portal
            .call_method("BindShortcuts", &(&session, shortcuts, "", opts2))
            .await?;

        let msg = responses2
            .next()
            .await
            .ok_or_else(|| zbus::Error::Failure(tunante_core::i18n::tr("el portal colgó al vincular")))?;
        let (code, _results): (u32, HashMap<String, OwnedValue>) = msg.body().deserialize()?;
        if code != 0 {
            // 1 is the user cancelling the binding dialog — a decision, not
            // a malfunction, and the label should say which it was.
            return Err(zbus::Error::Failure(if code == 1 {
                tunante_core::i18n::tr("cancelado en el diálogo del sistema")
            } else {
                tunante_core::i18n::tr("el portal rechazó los atajos (código {})")
                    .replace("{}", &code.to_string())
            }));
        }

        let _ = tx.send(Msg::Status("sí".into()));

        // --- The point of all of the above ---------------------------------
        let mut activated = portal.receive_signal("Activated").await?;
        while let Some(msg) = activated.next().await {
            let Ok((_s, id, _t, _o)) = msg.body().deserialize::<(
                zbus::zvariant::ObjectPath,
                String,
                u64,
                HashMap<String, OwnedValue>,
            )>() else {
                continue;
            };
            if !forward.load(Ordering::Relaxed) {
                continue;
            }
            let out = match id.as_str() {
                "play-pause" => Msg::PlayPause,
                "next" => Msg::Next,
                "prev" => Msg::Prev,
                _ => continue,
            };
            if tx.send(out).is_err() {
                return Ok(());
            }
        }
        Ok(())
    })
}
