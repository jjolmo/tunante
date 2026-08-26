//! MPRIS: the lock screen, and the buttons on a Bluetooth headset.
//!
//! On this phone the 3.5 mm jack has no driver, so headphones mean Bluetooth,
//! and Bluetooth means the only controls within reach are the ones on the
//! headset itself. Without MPRIS you have to unlock the phone to skip a track.
//! That moves this from a nice-to-have to the way the app is actually used.
//!
//! # Threading
//!
//! zbus wants an async executor; Slint's event loop is not one. So the D-Bus
//! server runs on its own thread with its own executor, and the two sides talk
//! through channels:
//!
//! - headset button → [`Command`] → the UI thread, via `invoke_from_event_loop`
//! - track change → [`Update`] → the D-Bus thread, which republishes the metadata
//!
//! Nothing is shared but the channels, so neither side can block the other —
//! which matters, because a stalled D-Bus call must never stutter the audio.

use std::sync::mpsc::{Receiver, Sender};

use mpris_server::{
    LoopStatus, Metadata, PlaybackStatus, Player, Time, TrackId,
};
use tunante_core::RepeatMode;

/// What the outside world asks of the player.
#[derive(Debug, Clone)]
pub enum Command {
    PlayPause,
    Play,
    Pause,
    Stop,
    Next,
    Previous,
    SetVolume(f64),
    SetRepeat(RepeatMode),
    SetShuffle(bool),
    Seek(i64),
}

/// What the player tells the outside world.
#[derive(Debug, Clone)]
pub struct Update {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration_ms: u64,
    pub position_ms: u64,
    pub playing: bool,
    pub has_track: bool,
    pub volume: f64,
    pub shuffle: bool,
    pub repeat: RepeatMode,
}

fn to_loop_status(mode: RepeatMode) -> LoopStatus {
    match mode {
        RepeatMode::Off => LoopStatus::None,
        RepeatMode::All => LoopStatus::Playlist,
        RepeatMode::One => LoopStatus::Track,
    }
}

fn from_loop_status(status: LoopStatus) -> RepeatMode {
    match status {
        LoopStatus::None => RepeatMode::Off,
        LoopStatus::Playlist => RepeatMode::All,
        LoopStatus::Track => RepeatMode::One,
    }
}

/// Start the D-Bus server on its own thread.
///
/// Returns the channel to push [`Update`]s into, and the one commands arrive on.
/// The caller drains the command side from the Slint event loop — it already
/// runs a timer, and going through a channel keeps the player's `Rc` where it
/// belongs, on one thread, instead of forcing it to be `Send`.
///
/// Failure is not fatal and is not propagated: no session bus means no lock
/// screen controls, which is a worse app, not a broken one.
pub fn spawn() -> (Sender<Update>, Receiver<Command>) {
    let (update_tx, update_rx) = std::sync::mpsc::channel::<Update>();
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel::<Command>();

    std::thread::Builder::new()
        .name("mpris".into())
        // musl gives a thread 128 KB by default where glibc gives megabytes,
        // and zbus's async machinery is deeper than that comfortably allows.
        .stack_size(1024 * 1024)
        .spawn(move || {
            if let Err(e) = run(update_rx, cmd_tx) {
                eprintln!("mpris: no se pudo publicar en D-Bus ({e}); sin controles en la pantalla de bloqueo");
            }
        })
        .ok();

    (update_tx, cmd_rx)
}

fn run(
    rx: Receiver<Update>,
    cmd_tx: Sender<Command>,
) -> Result<(), Box<dyn std::error::Error>> {
    async_io::block_on(async move {
        let player = Player::builder("com.tunante.mini")
            .identity("Tunante mini")
            .desktop_entry("tunante-mini")
            .can_play(true)
            .can_pause(true)
            .can_go_next(true)
            .can_go_previous(true)
            .can_seek(false)
            .can_control(true)
            .build()
            .await?;

        macro_rules! wire {
            ($connect:ident, $cmd:expr) => {{
                let tx = cmd_tx.clone();
                player.$connect(move |_| {
                    let _ = tx.send($cmd);
                });
            }};
        }

        wire!(connect_play_pause, Command::PlayPause);
        wire!(connect_play, Command::Play);
        wire!(connect_pause, Command::Pause);
        wire!(connect_stop, Command::Stop);
        wire!(connect_next, Command::Next);
        wire!(connect_previous, Command::Previous);

        {
            let tx = cmd_tx.clone();
            player.connect_set_volume(move |_, v| {
                let _ = tx.send(Command::SetVolume(v));
            });
        }
        {
            let tx = cmd_tx.clone();
            player.connect_set_loop_status(move |_, s| {
                let _ = tx.send(Command::SetRepeat(from_loop_status(s)));
            });
        }
        {
            let tx = cmd_tx.clone();
            player.connect_set_shuffle(move |_, on| {
                let _ = tx.send(Command::SetShuffle(on));
            });
        }
        {
            let tx = cmd_tx.clone();
            player.connect_seek(move |_, t| {
                let _ = tx.send(Command::Seek(t.as_millis()));
            });
        }

        // The server task has to keep running for the bus name to stay claimed.
        let task = player.run();

        // Riding on this thread because it is the only executor in the process.
        // See the module docs in inhibit.rs.
        let mut inhibitor = crate::inhibit::Inhibitor::new().await;

        let pump = async {
            let mut last: Option<Update> = None;
            loop {
                // The channel is sync; poll it rather than blocking the
                // executor, which also has the D-Bus task to drive.
                match rx.try_recv() {
                    Ok(u) => {
                        inhibitor.set_playing(u.playing && u.has_track).await;
                        publish(&player, &u, last.as_ref()).await;
                        last = Some(u);
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => {
                        async_io::Timer::after(std::time::Duration::from_millis(200)).await;
                    }
                    // The UI is gone; so are we.
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => return,
                }
            }
        };

        futures_lite::future::or(task, pump).await;
        Ok(())
    })
}

/// Push what changed, and only what changed.
///
/// Every setter here is a D-Bus property emission. Sending all of them twice a
/// second — which is what the UI's progress timer would cause — wakes every
/// listener on the bus for nothing, and on a phone that is battery.
async fn publish(player: &Player, u: &Update, last: Option<&Update>) {
    let track_changed = last.is_none_or(|l| {
        l.title != u.title || l.artist != u.artist || l.duration_ms != u.duration_ms
    });

    if track_changed {
        let mut md = Metadata::new();
        if u.has_track {
            md.set_trackid(Some(
                TrackId::try_from("/com/tunante/mini/track").unwrap_or_default(),
            ));
            md.set_title(Some(u.title.clone()));
            if !u.artist.is_empty() {
                md.set_artist(Some(vec![u.artist.clone()]));
            }
            if !u.album.is_empty() {
                md.set_album(Some(u.album.clone()));
            }
            if u.duration_ms > 0 {
                md.set_length(Some(Time::from_millis(u.duration_ms as i64)));
            }
        }
        let _ = player.set_metadata(md).await;
    }

    if last.is_none_or(|l| l.playing != u.playing || l.has_track != u.has_track) {
        let status = if !u.has_track {
            PlaybackStatus::Stopped
        } else if u.playing {
            PlaybackStatus::Playing
        } else {
            PlaybackStatus::Paused
        };
        let _ = player.set_playback_status(status).await;
    }

    // These three are writable under MPRIS, so a client that sets one expects
    // to read it back. Publishing them is also what makes the property tell
    // the truth about what the UI is doing, rather than a constant.
    if last.is_none_or(|l| l.repeat != u.repeat) {
        let _ = player.set_loop_status(to_loop_status(u.repeat)).await;
    }
    if last.is_none_or(|l| l.shuffle != u.shuffle) {
        let _ = player.set_shuffle(u.shuffle).await;
    }
    if last.is_none_or(|l| l.volume != u.volume) {
        let _ = player.set_volume(u.volume).await;
    }

    // Position is not a property change under MPRIS — clients poll it — so this
    // is a local store, not a bus message.
    player.set_position(Time::from_millis(u.position_ms as i64));
}
