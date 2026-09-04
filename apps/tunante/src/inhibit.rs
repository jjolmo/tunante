//! Keep the phone awake while sound is coming out.
//!
//! Left alone with the screen off, PowerDevil suspends the phone on idle — in
//! the middle of a track, fifteen minutes in. That alone would be reason
//! enough. On this device it is worse: the sound card does not reliably
//! survive the resume, and when it does not, `/proc/asound/cards` reads `--- no
//! soundcards ---` until the phone is rebooted. The music does not come back
//! at all.
//!
//! So the app takes a logind sleep inhibitor while it is playing, and drops it
//! the moment it is not. Paused, it costs nothing and the phone sleeps as it
//! normally would.
//!
//! # Where this runs
//!
//! On the D-Bus thread in [`crate::mpris`], not on its own. logind is async and
//! that thread already owns the only executor in the process; the Slint event
//! loop is not one, and blocking on it to make a D-Bus call would stutter the
//! audio it is trying to protect.
//!
//! # Why the file descriptor is the lock
//!
//! `Inhibit` hands back an fd and the lock lasts exactly as long as it stays
//! open. There is no "release" call: closing it is the release, and so is
//! dying, which is the property that matters — a crash cannot leave the phone
//! unable to sleep.

use zbus::zvariant::OwnedFd;
use zbus::Connection;

#[zbus::proxy(
    interface = "org.freedesktop.login1.Manager",
    default_service = "org.freedesktop.login1",
    default_path = "/org/freedesktop/login1"
)]
trait Manager {
    fn inhibit(&self, what: &str, who: &str, why: &str, mode: &str) -> zbus::Result<OwnedFd>;
}

pub struct Inhibitor {
    manager: Option<ManagerProxy<'static>>,
    lock: Option<OwnedFd>,
    /// So a system bus that is not there is complained about once, not twice a
    /// second for as long as the app runs.
    complained: bool,
}

impl Inhibitor {
    /// Connect to the system bus, or decide to live without it.
    ///
    /// No session bus means no lock screen; no system bus means the phone may
    /// sleep under us. Both are a worse app, neither is a broken one.
    pub async fn new() -> Self {
        let manager = match Connection::system().await {
            Ok(conn) => ManagerProxy::new(&conn).await.ok(),
            Err(e) => {
                eprintln!("inhibit: sin bus de sistema ({e}); el móvil podrá suspenderse sonando");
                None
            }
        };
        Self { manager, lock: None, complained: false }
    }

    /// Hold the lock while `playing`, release it when not.
    ///
    /// Cheap to call on every tick: the transitions are what do the work.
    pub async fn set_playing(&mut self, playing: bool) {
        match (playing, self.lock.is_some()) {
            (true, false) => self.take().await,
            (false, true) => self.lock = None,
            _ => {}
        }
    }

    async fn take(&mut self) {
        let Some(manager) = &self.manager else { return };
        match manager
            .inhibit(
                "sleep",
                "Tunante mini",
                // logind shows this verbatim in `systemd-inhibit --list`, so it
                // is addressed to whoever is looking there wondering why the
                // phone will not sleep.
                "Reproduciendo música",
                "block",
            )
            .await
        {
            Ok(fd) => self.lock = Some(fd),
            Err(e) => {
                if !self.complained {
                    eprintln!("inhibit: logind ha rechazado el bloqueo ({e}); el móvil podrá suspenderse sonando");
                    self.complained = true;
                }
            }
        }
    }
}
