//! The volume panel as a Wayland layer surface.
//!
//! A Wayland client cannot place its own window — the compositor does, and it
//! centres ours. So the panel stops being a window. `zwlr_layer_shell_v1` is
//! what every desktop's own OSD, panel and notification area is built on: the
//! surface says which edge it wants and how far from it, and the compositor
//! obeys. Anchored bottom-right it lands exactly where the old desktop's popup
//! put itself, above the panel the tray icon sits in, always on top, never
//! taking focus, and with no rule from anybody.
//!
//! It draws itself. The Slint component cannot be borrowed for this: Slint
//! allows one platform per process and the app's is winit, so a software
//! render into our own buffer is not on offer. The panel is a rounded
//! rectangle, a speaker, a bar and a number — the same parts, the same
//! palette, and the same 1.5 s as everywhere else.
//!
//! Its own thread with its own connection, like MPRIS and the portal
//! shortcuts; the UI thread only ever sends a percentage down a channel.

use smithay_client_toolkit::reexports::calloop::channel::Sender;
use std::sync::OnceLock;

/// Logical size of the panel, and how far it sits from the screen's corner.
const W: u32 = 200;
const H: u32 = 48;
const MARGIN: i32 = 16;
/// How long it stays up after the last notch, as everywhere else.
const HOLD: std::time::Duration = std::time::Duration::from_millis(1500);

enum Msg {
    /// Build the surface now, empty, so the compositor's "a thing appeared"
    /// animation happens at startup where nobody is looking. Without it the
    /// first notch of every session arrives with a zoom.
    Prime {
        icon: Option<(i32, i32)>,
    },
    Show {
        percent: u32,
        dark: bool,
        /// Where the tray icon is, when the tray has been told (see
        /// `tray::icon_pos`). The panel goes beside it; without it, the
        /// corner.
        icon: Option<(i32, i32)>,
    },
}

static TX: OnceLock<Option<Sender<Msg>>> = OnceLock::new();

/// Show the panel, and say whether this path took the job.
///
/// `false` means there is no Wayland session or no layer shell in it (X11,
/// GNOME, Windows, macOS): the caller then falls back to its own window, which
/// on those can place itself.
pub fn try_show(percent: u32, dark: bool, icon: Option<(i32, i32)>) -> bool {
    let Some(tx) = TX.get_or_init(spawn) else {
        return false;
    };
    tx.send(Msg::Show { percent, dark, icon }).is_ok()
}

/// Put the surface up, empty and silent, at startup.
///
/// Costs one transparent surface with no input region for the life of the
/// session, and buys a panel that never animates in.
pub fn prime(icon: Option<(i32, i32)>) {
    if let Some(tx) = TX.get_or_init(spawn) {
        let _ = tx.send(Msg::Prime { icon });
    }
}

/// Start the thread, and wait just long enough to learn whether it has a layer
/// shell to talk to. Once, on the first notch of the session.
fn spawn() -> Option<Sender<Msg>> {
    let (ready_tx, ready_rx) = std::sync::mpsc::channel::<Option<Sender<Msg>>>();
    std::thread::Builder::new()
        .name("volume-osd".into())
        // The same 1 MB the other D-Bus/Wayland threads take: musl's default
        // stack is 128 KB and this machinery wants more.
        .stack_size(1024 * 1024)
        .spawn(move || run(ready_tx))
        .ok()?;
    ready_rx
        .recv_timeout(std::time::Duration::from_millis(500))
        .ok()
        .flatten()
}

fn run(ready: std::sync::mpsc::Sender<Option<Sender<Msg>>>) {
    match build() {
        Ok((mut state, mut event_loop, tx)) => {
            if ready.send(Some(tx)).is_err() {
                return;
            }
            let _ = event_loop.run(None, &mut state, |_| {});
        }
        Err(e) => {
            log::info!("osd: sin capa de Wayland ({e}); el panel irá en ventana");
            let _ = ready.send(None);
        }
    }
}

use inner::build;

mod inner {
    use super::{Msg, HOLD, H, MARGIN, W};
    use smithay_client_toolkit::compositor::{CompositorHandler, CompositorState, Region};
    use smithay_client_toolkit::output::{OutputHandler, OutputState};
    use smithay_client_toolkit::reexports::calloop::channel::{channel, Event, Sender};
    use smithay_client_toolkit::reexports::calloop::timer::{TimeoutAction, Timer};
    use smithay_client_toolkit::reexports::calloop::{EventLoop, RegistrationToken};
    use smithay_client_toolkit::reexports::calloop_wayland_source::WaylandSource;
    use smithay_client_toolkit::reexports::client::globals::registry_queue_init;
    use smithay_client_toolkit::reexports::client::protocol::{wl_output, wl_shm, wl_surface};
    use smithay_client_toolkit::reexports::client::{Connection, QueueHandle};
    use smithay_client_toolkit::registry::{ProvidesRegistryState, RegistryState};
    use smithay_client_toolkit::shell::wlr_layer::{
        Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
        LayerSurfaceConfigure,
    };
    use smithay_client_toolkit::shell::WaylandSurface;
    use smithay_client_toolkit::shm::slot::SlotPool;
    use smithay_client_toolkit::shm::{Shm, ShmHandler};
    use smithay_client_toolkit::{delegate_registry, registry_handlers};

    pub struct State {
        registry: RegistryState,
        output: OutputState,
        shm: Shm,
        compositor: CompositorState,
        layer_shell: LayerShell,
        pool: SlotPool,
        /// Built on the way in and destroyed on the way out. A layer surface
        /// that has been unmapped needs a fresh configure before it can draw
        /// again, and building a new one is the short way to get it.
        layer: Option<LayerSurface>,
        /// Set by the configure event: nothing may be drawn before it.
        configured: bool,
        /// Whether the panel is showing. Hiding does *not* take the surface
        /// down: an unmapped surface is animated back in by the compositor
        /// (KWin scales it up, which reads as a zoom on every notch) and
        /// mapping costs a round trip. It stays mapped and draws nothing
        /// instead, which is instant and silent.
        visible: bool,
        scale: i32,
        percent: u32,
        dark: bool,
        /// Where the panel is asking to be, so a second notch does not repeat
        /// the request.
        place: Option<Placement>,
        qh: QueueHandle<State>,
        loop_handle: smithay_client_toolkit::reexports::calloop::LoopHandle<'static, State>,
        hide: Option<RegistrationToken>,
    }

    type Built = (State, EventLoop<'static, State>, Sender<Msg>);

    pub fn build() -> Result<Built, Box<dyn std::error::Error>> {
        let conn = Connection::connect_to_env()?;
        let (globals, queue) = registry_queue_init::<State>(&conn)?;
        let qh: QueueHandle<State> = queue.handle();

        let event_loop: EventLoop<State> = EventLoop::try_new()?;
        let loop_handle = event_loop.handle();
        WaylandSource::new(conn, queue).insert(loop_handle.clone())?;

        let shm = Shm::bind(&globals, &qh)?;
        let pool = SlotPool::new((W * H * 4) as usize * 4, &shm)?;
        let state = State {
            registry: RegistryState::new(&globals),
            output: OutputState::new(&globals, &qh),
            compositor: CompositorState::bind(&globals, &qh)?,
            // The one that decides whether this path exists at all.
            layer_shell: LayerShell::bind(&globals, &qh)?,
            shm,
            pool,
            layer: None,
            configured: false,
            visible: false,
            scale: 1,
            percent: 100,
            dark: true,
            place: None,
            qh: qh.clone(),
            loop_handle: loop_handle.clone(),
            hide: None,
        };

        let (tx, rx) = channel::<Msg>();
        loop_handle.insert_source(rx, |event, _, state: &mut State| {
            match event {
                Event::Msg(Msg::Show { percent, dark, icon }) => state.show(percent, dark, icon),
                Event::Msg(Msg::Prime { icon }) => state.prime(icon),
                _ => {}
            }
        })?;

        Ok((state, event_loop, tx))
    }

    /// One edge pair and the two margins that go with it — everything the
    /// compositor needs to put the panel where we want it.
    #[derive(Clone, Copy, PartialEq, Eq)]
    pub struct Placement {
        anchor: Anchor,
        top: i32,
        bottom: i32,
        left: i32,
        right: i32,
    }

    impl Placement {
        fn apply(&self, layer: &LayerSurface) {
            layer.set_anchor(self.anchor);
            layer.set_margin(self.top, self.right, self.bottom, self.left);
        }
    }

    impl State {
        /// Beside the icon when the tray knows where it is; the corner it used
        /// to fall back to when it does not.
        ///
        /// The icon's own side of the screen decides which edge the panel hangs
        /// from, so a panel at the top of the screen gets the OSD under it
        /// rather than at the far end of the desktop.
        fn placement(&self, icon: Option<(i32, i32)>) -> Placement {
            let Some((ix, iy)) = icon else {
                return Placement {
                    anchor: Anchor::BOTTOM | Anchor::RIGHT,
                    top: 0,
                    right: MARGIN,
                    bottom: MARGIN,
                    left: 0,
                };
            };
            let (ow, oh) = self.output_size();
            // Centred under the icon, and never hanging off the screen.
            let left = (ix - W as i32 / 2).clamp(MARGIN, (ow - W as i32 - MARGIN).max(MARGIN));
            let bottom_half = iy * 2 >= oh;
            Placement {
                anchor: if bottom_half {
                    Anchor::BOTTOM | Anchor::LEFT
                } else {
                    Anchor::TOP | Anchor::LEFT
                },
                top: if bottom_half { 0 } else { MARGIN },
                right: 0,
                bottom: if bottom_half { MARGIN } else { 0 },
                left,
            }
        }

        /// The screen the panel will live on, in logical pixels. One monitor's
        /// worth: the icon's coordinates are global, and clamping to the first
        /// output is closer than not clamping at all.
        fn output_size(&self) -> (i32, i32) {
            self.output
                .outputs()
                .find_map(|o| self.output.info(&o).and_then(|i| i.logical_size))
                .unwrap_or((1920, 1080))
        }

        /// Build the surface without showing anything on it.
        fn prime(&mut self, icon: Option<(i32, i32)>) {
            if self.layer.is_none() {
                self.visible = false;
                // Where the panel will actually appear, when the database
                // already remembers the icon: nothing to move on the way in.
                self.build_surface(self.placement(icon));
            }
        }

        fn show(&mut self, percent: u32, dark: bool, icon: Option<(i32, i32)>) {
            self.percent = percent.min(100);
            self.dark = dark;
            // A panel already up moves to the icon too: the tray may have
            // learnt where it is since the last time.
            let place = self.placement(icon);
            if self.place != Some(place) {
                self.place = Some(place);
                if let Some(layer) = self.layer.as_ref() {
                    place.apply(layer);
                    layer.commit();
                }
            }
            self.visible = true;
            if self.layer.is_none() {
                self.build_surface(place);
            } else {
                self.draw();
            }
            self.arm_hide();
        }

        fn build_surface(&mut self, place: Placement) {
            let qh = self.qh.clone();
            let surface = self.compositor.create_surface(&qh);
            // Nothing on this panel can be clicked, and it sits on the overlay
            // layer: without an empty input region it would eat every click in
            // its corner, including the tray icon's.
            if let Ok(region) = Region::new(&self.compositor) {
                surface.set_input_region(Some(region.wl_region()));
            }
            let layer = self.layer_shell.create_layer_surface(
                &qh,
                surface,
                // Overlay: above full-screen windows too, which is what an OSD
                // is for.
                Layer::Overlay,
                Some("tunante-volume"),
                None,
            );
            layer.set_size(W, H);
            // An exclusive zone of 0 respects what the panels reserved, so the
            // surface lands beside the taskbar rather than under it.
            layer.set_exclusive_zone(0);
            layer.set_keyboard_interactivity(KeyboardInteractivity::None);
            place.apply(&layer);
            layer.commit();
            self.configured = false;
            self.place = Some(place);
            self.layer = Some(layer);
        }

        /// Restart the countdown. Each notch pushes it back, so a spun wheel
        /// keeps the panel up and it goes 1.5 s after the last one.
        fn arm_hide(&mut self) {
            if let Some(token) = self.hide.take() {
                self.loop_handle.remove(token);
            }
            self.hide = self
                .loop_handle
                .insert_source(Timer::from_duration(HOLD), |_, _, state: &mut State| {
                    state.visible = false;
                    state.hide = None;
                    state.draw();
                    TimeoutAction::Drop
                })
                .ok();
        }

        fn draw(&mut self) {
            let Some(layer) = self.layer.as_ref() else { return };
            if !self.configured {
                return;
            }
            let scale = self.scale.max(1) as u32;
            let (w, h) = (W * scale, H * scale);
            let stride = w as i32 * 4;
            let Ok((buffer, canvas)) =
                self.pool
                    .create_buffer(w as i32, h as i32, stride, wl_shm::Format::Argb8888)
            else {
                return;
            };
            if self.visible {
                super::paint::render(canvas, w, h, scale, self.percent, self.dark);
            } else {
                // Gone, without going anywhere: a fully transparent frame.
                canvas.fill(0);
            }
            let surface = layer.wl_surface();
            surface.set_buffer_scale(self.scale.max(1));
            surface.damage_buffer(0, 0, w as i32, h as i32);
            if buffer.attach_to(surface).is_ok() {
                layer.commit();
            }
        }
    }

    impl LayerShellHandler for State {
        fn closed(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _layer: &LayerSurface) {
            self.layer = None;
            self.configured = false;
        }

        fn configure(
            &mut self,
            _conn: &Connection,
            _qh: &QueueHandle<Self>,
            _layer: &LayerSurface,
            _configure: LayerSurfaceConfigure,
            _serial: u32,
        ) {
            self.configured = true;
            self.draw();
        }
    }

    impl CompositorHandler for State {
        fn scale_factor_changed(
            &mut self,
            _conn: &Connection,
            _qh: &QueueHandle<Self>,
            _surface: &wl_surface::WlSurface,
            new_factor: i32,
        ) {
            self.scale = new_factor.max(1);
            self.draw();
        }

        fn transform_changed(
            &mut self,
            _conn: &Connection,
            _qh: &QueueHandle<Self>,
            _surface: &wl_surface::WlSurface,
            _new_transform: wl_output::Transform,
        ) {
        }

        fn frame(
            &mut self,
            _conn: &Connection,
            _qh: &QueueHandle<Self>,
            _surface: &wl_surface::WlSurface,
            _time: u32,
        ) {
        }

        fn surface_enter(
            &mut self,
            _conn: &Connection,
            _qh: &QueueHandle<Self>,
            _surface: &wl_surface::WlSurface,
            _output: &wl_output::WlOutput,
        ) {
        }

        fn surface_leave(
            &mut self,
            _conn: &Connection,
            _qh: &QueueHandle<Self>,
            _surface: &wl_surface::WlSurface,
            _output: &wl_output::WlOutput,
        ) {
        }
    }

    impl OutputHandler for State {
        fn output_state(&mut self) -> &mut OutputState {
            &mut self.output
        }
        fn new_output(
            &mut self,
            _conn: &Connection,
            _qh: &QueueHandle<Self>,
            _output: wl_output::WlOutput,
        ) {
        }
        fn update_output(
            &mut self,
            _conn: &Connection,
            _qh: &QueueHandle<Self>,
            _output: wl_output::WlOutput,
        ) {
        }
        fn output_destroyed(
            &mut self,
            _conn: &Connection,
            _qh: &QueueHandle<Self>,
            _output: wl_output::WlOutput,
        ) {
        }
    }

    impl ShmHandler for State {
        fn shm_state(&mut self) -> &mut Shm {
            &mut self.shm
        }
    }

    impl ProvidesRegistryState for State {
        fn registry(&mut self) -> &mut RegistryState {
            &mut self.registry
        }
        registry_handlers![OutputState];
    }

    delegate_registry!(State);
    // 0.21 routes every protocol through one blanket impl instead of a macro
    // per module.
    smithay_client_toolkit::delegate_dispatch2!(State);
}

/// The panel's own drawing: the parts the Slint component has, in pixels.
///
/// Nothing here is a general-purpose renderer — it is one rounded rectangle, a
/// speaker, a bar and a number, which is exactly what the panel is. Corners
/// and curves are sampled 4×4 so they are not staircases.
mod paint {
    /// (r, g, b), straight from `ui/theme.slint` so the panel matches the app.
    struct Palette {
        bg: (u8, u8, u8),
        border: (u8, u8, u8),
        track: (u8, u8, u8),
        accent: (u8, u8, u8),
        text: (u8, u8, u8),
    }

    fn palette(dark: bool) -> Palette {
        if dark {
            Palette {
                bg: (0x25, 0x25, 0x26),
                border: (0x3e, 0x3e, 0x42),
                track: (0x3e, 0x3e, 0x42),
                accent: (0x00, 0x7a, 0xcc),
                text: (0xcc, 0xcc, 0xcc),
            }
        } else {
            Palette {
                bg: (0xf3, 0xf3, 0xf3),
                border: (0xd4, 0xd4, 0xd4),
                track: (0xd4, 0xd4, 0xd4),
                accent: (0x00, 0x7a, 0xcc),
                text: (0x1e, 0x1e, 0x1e),
            }
        }
    }

    /// 5×7 glyphs for the only characters the panel can show. One bit a pixel,
    /// high bit leftmost.
    const GLYPH_W: usize = 5;
    const GLYPH_H: usize = 7;
    fn glyph(c: u8) -> [u8; GLYPH_H] {
        match c {
            b'0' => [0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110],
            b'1' => [0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110],
            b'2' => [0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111],
            b'3' => [0b11111, 0b00010, 0b00100, 0b00010, 0b00001, 0b10001, 0b01110],
            b'4' => [0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010],
            b'5' => [0b11111, 0b10000, 0b11110, 0b00001, 0b00001, 0b10001, 0b01110],
            b'6' => [0b00110, 0b01000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110],
            b'7' => [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000],
            b'8' => [0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110],
            b'9' => [0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00010, 0b01100],
            b'%' => [0b11001, 0b11010, 0b00010, 0b00100, 0b01000, 0b01011, 0b10011],
            _ => [0; GLYPH_H],
        }
    }

    /// One pixel of the buffer, in the premultiplied ARGB the compositor wants.
    struct Canvas<'a> {
        px: &'a mut [u8],
        w: u32,
        h: u32,
    }

    impl Canvas<'_> {
        fn blend(&mut self, x: i32, y: i32, (r, g, b): (u8, u8, u8), a: f32) {
            if a <= 0.0 || x < 0 || y < 0 || x >= self.w as i32 || y >= self.h as i32 {
                return;
            }
            let a = a.min(1.0);
            let i = ((y as u32 * self.w + x as u32) * 4) as usize;
            let old = &self.px[i..i + 4];
            // Premultiplied, so the source is already scaled by its own alpha
            // and the destination keeps (1 - a) of itself.
            let mix = |src: u8, dst: u8| -> u8 {
                (src as f32 * a + dst as f32 * (1.0 - a)).round().clamp(0.0, 255.0) as u8
            };
            let (ob, og, or, oa) = (old[0], old[1], old[2], old[3]);
            let out = [mix(b, ob), mix(g, og), mix(r, or), mix(255, oa)];
            self.px[i..i + 4].copy_from_slice(&out);
        }
    }

    /// How much of the pixel at (x, y) is inside the shape, by sampling it
    /// 4×4. `inside` works in logical coordinates.
    fn coverage(x: u32, y: u32, scale: u32, inside: impl Fn(f32, f32) -> bool) -> f32 {
        const N: u32 = 4;
        let mut hits = 0;
        for sy in 0..N {
            for sx in 0..N {
                let px = (x as f32 + (sx as f32 + 0.5) / N as f32) / scale as f32;
                let py = (y as f32 + (sy as f32 + 0.5) / N as f32) / scale as f32;
                if inside(px, py) {
                    hits += 1;
                }
            }
        }
        hits as f32 / (N * N) as f32
    }

    /// Distance from a point to a rounded rectangle, negative inside. The one
    /// piece of maths here, and both the fill and the border are drawn from it.
    fn rounded_rect_sdf(px: f32, py: f32, x: f32, y: f32, w: f32, h: f32, r: f32) -> f32 {
        let (cx, cy) = (x + w / 2.0, y + h / 2.0);
        let (qx, qy) = ((px - cx).abs() - (w / 2.0 - r), (py - cy).abs() - (h / 2.0 - r));
        let outside = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt();
        outside + qx.max(qy).min(0.0) - r
    }

    pub fn render(px: &mut [u8], w: u32, h: u32, scale: u32, percent: u32, dark: bool) {
        let pal = palette(dark);
        px.fill(0);
        let mut c = Canvas { px, w, h };

        let (lw, lh) = (w as f32 / scale as f32, h as f32 / scale as f32);
        let radius = 8.0;

        // The panel itself, and its hairline edge.
        for y in 0..h {
            for x in 0..w {
                let fill = coverage(x, y, scale, |px, py| {
                    rounded_rect_sdf(px, py, 0.0, 0.0, lw, lh, radius) < 0.0
                });
                if fill > 0.0 {
                    c.blend(x as i32, y as i32, pal.bg, fill);
                }
                let edge = coverage(x, y, scale, |px, py| {
                    let d = rounded_rect_sdf(px, py, 0.0, 0.0, lw, lh, radius);
                    (-1.0..0.0).contains(&d)
                });
                if edge > 0.0 {
                    c.blend(x as i32, y as i32, pal.border, edge);
                }
            }
        }

        speaker(&mut c, scale, percent, pal.text);
        bar(&mut c, scale, percent, &pal);
        number(&mut c, scale, percent, pal.text);
    }

    /// A speaker: a box, a cone, and two waves that only sound when there is
    /// something to hear. At zero it wears a cross instead.
    fn speaker(c: &mut Canvas, scale: u32, percent: u32, colour: (u8, u8, u8)) {
        let (cx, cy) = (22.0_f32, 24.0_f32);
        for y in 0..c.h {
            for x in 0..c.w {
                let a = coverage(x, y, scale, |px, py| {
                    let (dx, dy) = (px - cx, py - cy);
                    // Box.
                    if (-8.0..=-4.0).contains(&dx) && (-3.0..=3.0).contains(&dy) {
                        return true;
                    }
                    // Cone: widens from the box out to the rim.
                    if (-4.0..=2.0).contains(&dx) {
                        let half = 3.0 + (dx + 4.0) * 1.0;
                        return dy.abs() <= half;
                    }
                    false
                });
                if a > 0.0 {
                    c.blend(x as i32, y as i32, colour, a);
                }
                let a = if percent == 0 {
                    // Muted: a small cross where the waves would be.
                    coverage(x, y, scale, |px, py| {
                        let (dx, dy) = (px - cx - 6.0, py - cy);
                        if dx.abs() > 3.5 || dy.abs() > 3.5 {
                            return false;
                        }
                        (dx - dy).abs() < 1.0 || (dx + dy).abs() < 1.0
                    })
                } else {
                    coverage(x, y, scale, |px, py| {
                        let (dx, dy) = (px - cx + 1.0, py - cy);
                        if dx <= 0.0 {
                            return false;
                        }
                        let d = (dx * dx + dy * dy).sqrt();
                        // Two arcs, the outer one only once it is worth
                        // hearing, both cut to the front of the speaker.
                        let ring = |r: f32| (d - r).abs() < 0.8;
                        let front = dx > d * 0.55;
                        front && (ring(6.0) || (percent >= 50 && ring(9.5)))
                    })
                };
                if a > 0.0 {
                    c.blend(x as i32, y as i32, colour, a);
                }
            }
        }
    }

    /// The bar: a track the width of the panel's middle, filled to the volume.
    fn bar(c: &mut Canvas, scale: u32, percent: u32, pal: &Palette) {
        let (x0, x1, y0, hgt) = (40.0_f32, 132.0_f32, 22.0_f32, 4.0_f32);
        let filled = x0 + (x1 - x0) * (percent as f32 / 100.0);
        for y in 0..c.h {
            for x in 0..c.w {
                let track = coverage(x, y, scale, |px, py| {
                    rounded_rect_sdf(px, py, x0, y0, x1 - x0, hgt, hgt / 2.0) < 0.0
                });
                if track > 0.0 {
                    c.blend(x as i32, y as i32, pal.track, track);
                }
                if percent > 0 {
                    let fill = coverage(x, y, scale, |px, py| {
                        rounded_rect_sdf(px, py, x0, y0, x1 - x0, hgt, hgt / 2.0) < 0.0
                            && px <= filled
                    });
                    if fill > 0.0 {
                        c.blend(x as i32, y as i32, pal.accent, fill);
                    }
                }
            }
        }
    }

    /// The number, right-aligned so "9%" and "100%" end in the same place.
    fn number(c: &mut Canvas, scale: u32, percent: u32, colour: (u8, u8, u8)) {
        let text = format!("{percent}%");
        let size = 2 * scale as i32; // one glyph pixel, in buffer pixels
        let advance = (GLYPH_W as i32 + 1) * size;
        let width = advance * text.len() as i32 - size;
        let right = 186 * scale as i32;
        let mut x = right - width;
        // Centred on the panel's middle line, like everything else on it.
        let top = 24 * scale as i32 - (GLYPH_H as i32 * size) / 2;
        for ch in text.bytes() {
            let rows = glyph(ch);
            for (row, bits) in rows.iter().enumerate() {
                for col in 0..GLYPH_W {
                    if bits & (1 << (GLYPH_W - 1 - col)) == 0 {
                        continue;
                    }
                    for dy in 0..size {
                        for dx in 0..size {
                            c.blend(
                                x + col as i32 * size + dx,
                                top + row as i32 * size + dy,
                                colour,
                                1.0,
                            );
                        }
                    }
                }
            }
            x += advance;
        }
    }
}
