//! One instance, and the second launch talks to the first.
//!
//! A Unix socket in `$XDG_RUNTIME_DIR` rather than D-Bus: the whole protocol
//! is one line — `raise`, or `play <path>` when a file manager hands a track
//! to what turns out to be an already-running player — and a socket needs no
//! bus, no service file and no async runtime. The UI timer polls the
//! listener twice a second, the same way it drains MPRIS and the tray.
//!
//! Claiming is connect-first: if somebody answers, they are the instance and
//! we are the message. A stale socket (crash, power loss) refuses the
//! connection, gets unlinked, and the new instance binds in its place.

#[cfg(unix)]
mod imp {
    use std::io::{Read, Write};
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::PathBuf;

    pub struct Instance(Option<UnixListener>);

    pub enum Start {
        /// We are the app. Poll the instance from the UI timer.
        Primary(Instance),
        /// Somebody else is; the message was delivered. Exit quietly.
        Secondary,
    }

    fn sock_path() -> PathBuf {
        std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
            .join("tunante-mini.sock")
    }

    pub fn claim(message: &str) -> Start {
        let path = sock_path();
        if let Ok(mut s) = UnixStream::connect(&path) {
            let _ = s.write_all(message.as_bytes());
            return Start::Secondary;
        }
        let _ = std::fs::remove_file(&path);
        match UnixListener::bind(&path) {
            Ok(l) => {
                let _ = l.set_nonblocking(true);
                Start::Primary(Instance(Some(l)))
            }
            // No runtime dir, odd permissions: the app still runs, it just
            // cannot promise uniqueness.
            Err(e) => {
                eprintln!("sin instancia única ({e}); la app funciona igual");
                Start::Primary(Instance(None))
            }
        }
    }

    impl Instance {
        /// One queued message from a second launch, if any arrived.
        pub fn poll(&self) -> Option<String> {
            let (mut s, _) = self.0.as_ref()?.accept().ok()?;
            // The writer sends its line and closes; a bounded blocking read
            // keeps a malicious slow writer from stalling the UI timer.
            let _ = s.set_read_timeout(Some(std::time::Duration::from_millis(100)));
            let mut buf = String::new();
            let _ = s.take(4096).read_to_string(&mut buf);
            Some(buf)
        }
    }
}

#[cfg(not(unix))]
mod imp {
    pub struct Instance;

    pub enum Start {
        Primary(Instance),
        Secondary,
    }

    pub fn claim(_message: &str) -> Start {
        Start::Primary(Instance)
    }

    impl Instance {
        pub fn poll(&self) -> Option<String> {
            None
        }
    }
}

pub use imp::{claim, Start};
