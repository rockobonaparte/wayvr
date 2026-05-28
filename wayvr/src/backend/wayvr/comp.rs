use anyhow::Context;
use smithay::backend::allocator::dmabuf::Dmabuf;
use smithay::backend::renderer::{BufferType, buffer_type};
use smithay::desktop::{PopupKind, PopupManager};
use smithay::input::{Seat, SeatHandler, SeatState};
use smithay::reexports::rustix::fs::{OFlags, fcntl_setfl};
use smithay::reexports::wayland_protocols::xdg::decoration::zv1::server::zxdg_toplevel_decoration_v1;
use smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel;
use smithay::reexports::wayland_protocols_misc::server_decoration::server::org_kde_kwin_server_decoration;
use smithay::reexports::wayland_server::Resource;
use smithay::reexports::wayland_server::protocol::{wl_buffer, wl_output, wl_seat};
use smithay::reexports::wayland_server::{self, DisplayHandle};
use smithay::wayland::buffer::BufferHandler;
use smithay::wayland::dmabuf::{
    DmabufFeedback, DmabufGlobal, DmabufHandler, DmabufState, ImportNotifier, get_dmabuf,
};
use smithay::wayland::fractional_scale::with_fractional_scale;
use smithay::wayland::output::OutputHandler;
use smithay::wayland::selection::{
    ext_data_control as selection_ext,
    primary_selection::{PrimarySelectionHandler, PrimarySelectionState, set_primary_focus},
    wlr_data_control as selection_wlr,
};
use smithay::wayland::shell::kde::decoration::{KdeDecorationHandler, KdeDecorationState};
use smithay::wayland::shell::xdg::decoration::XdgDecorationHandler;
use smithay::wayland::shm::{ShmHandler, ShmState, with_buffer_contents};
use smithay::wayland::single_pixel_buffer::get_single_pixel_buffer;
use smithay::{
    delegate_compositor, delegate_data_control, delegate_data_device, delegate_dmabuf,
    delegate_ext_data_control, delegate_kde_decoration, delegate_output,
    delegate_primary_selection, delegate_seat, delegate_shm, delegate_single_pixel_buffer,
    delegate_xdg_decoration, delegate_xdg_shell,
};
use std::collections::HashSet;
use std::fs::File;
use std::io::Write;
use std::os::fd::OwnedFd;
use std::sync::{Arc, Mutex, mpsc};

use smithay::utils::Serial;
use smithay::wayland::compositor::{self, BufferAssignment, SurfaceAttributes, send_surface_state};

use smithay::wayland::selection::data_device::{
    ClientDndGrabHandler, DataDeviceHandler, DataDeviceState, ServerDndGrabHandler,
    set_data_device_focus,
};
use smithay::wayland::selection::{self, SelectionHandler};
use smithay::wayland::shell::xdg::{
    PopupSurface, PositionerState, ToplevelSurface, XdgShellHandler, XdgShellState,
};
use wayland_server::Client;
use wayland_server::backend::{ClientData, ClientId, DisconnectReason};
use wayland_server::protocol::wl_surface::WlSurface;

use smithay_client_toolkit::{
    compositor::{CompositorHandler},
    delegate_compositor as sct_delegate_compositor,
    delegate_keyboard as sct_delegate_keyboard,
    delegate_layer as sct_delegate_layer,
    delegate_output as sct_delegate_output,
    delegate_pointer as sct_delegate_pointer,
    delegate_registry as sct_delegate_registry,
    delegate_seat as sct_delegate_seat,
    delegate_shm as sct_delegate_shm,
    output::{OutputHandler as SCT_OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        Capability, SeatHandler as SCT_SeatHandler, SeatState as SCT_SeatState,
        keyboard::{KeyEvent, KeyboardHandler, Keysym, Modifiers, RawModifiers, RepeatInfo},
        pointer::{PointerEvent, PointerEventKind, PointerHandler},
    },
    shell::{
        WaylandSurface,
        wlr_layer::{
            LayerShellHandler, LayerSurface, LayerSurfaceConfigure,
        },
    },
    shm::{Shm, ShmHandler as SCT_ShmHandler, slot::SlotPool},
};

use wayland_client::{
    Connection, Proxy, QueueHandle,
    protocol::{wl_keyboard, wl_output as wlc_output, wl_pointer, wl_seat as wlc_seat, wl_shm, wl_surface},
};

pub const CLIENT_CAP_WINDOW_WIDTH: u32 = 400;
pub const CLIENT_CAP_WINDOW_HEIGHT: u32 = 400;

use crate::backend::wayvr::image_importer::ImageImporter;
use crate::backend::wayvr::{SurfaceBufWithImage, WvrServerState, time};
use crate::ipc::event_queue::SyncEventQueue;

use super::WayVRTask;

pub struct Application {
    pub image_importer: ImageImporter,
    pub dmabuf_state: (DmabufState, DmabufGlobal, Option<DmabufFeedback>),
    pub compositor: compositor::CompositorState,
    pub xdg_shell: XdgShellState,
    pub seat_state: SeatState<Application>,
    pub shm: ShmState,
    pub data_device: DataDeviceState,
    pub primary_selection_state: PrimarySelectionState,
    pub ext_data_control_state: selection_ext::DataControlState,
    pub wlr_data_control_state: selection_wlr::DataControlState,
    pub kde_decoration_state: KdeDecorationState,
    pub wayvr_tasks: SyncEventQueue<WayVRTask>,
    pub redraw_requests: HashSet<wayland_server::backend::ObjectId>,
    pub popup_manager: PopupManager,
    pub display_handle: DisplayHandle,
}

pub enum ClientSideInput {
    KeyDown(u32),
    KeyUp(u32),
}

// Client-side external keyboard and mouse logging
pub struct ClientSideInputApplication {
    pub registry_state: RegistryState,
    pub sct_seat_state: SCT_SeatState,
    pub keyboard: Option<wl_keyboard::WlKeyboard>,
    pub pointer: Option<wl_pointer::WlPointer>,
    pub pool: Option<SlotPool>,
    pub is_key_logging: bool,  
    pub client_shm: Shm,
    pub output_state: OutputState,
    pub tx: mpsc::Sender<ClientSideInput>,
}

sct_delegate_compositor!(ClientSideInputApplication);
sct_delegate_output!(ClientSideInputApplication);
sct_delegate_seat!(ClientSideInputApplication);
sct_delegate_keyboard!(ClientSideInputApplication);
sct_delegate_pointer!(ClientSideInputApplication);
sct_delegate_layer!(ClientSideInputApplication);
sct_delegate_shm!(ClientSideInputApplication);
sct_delegate_registry!(ClientSideInputApplication);

impl Application {
    pub fn cleanup(&mut self) {
        self.image_importer.cleanup();
    }

    fn popups_commit(&mut self, surface: &WlSurface) {
        self.popup_manager.commit(surface);

        if let Some(popup) = self.popup_manager.find_popup(surface) {
            match popup {
                PopupKind::Xdg(ref popup) => {
                    if !popup.is_initial_configure_sent() {
                        smithay::wayland::compositor::with_states(surface, |states| {
                            send_surface_state(
                                surface,
                                states,
                                1,
                                smithay::utils::Transform::Normal,
                            );
                            with_fractional_scale(states, |fractional| {
                                fractional.set_preferred_scale(1.0);
                            });
                        });
                        popup.send_configure().expect("initial configure failed");
                    }
                }
                PopupKind::InputMethod(_) => {
                    // TODO?
                }
            }
        }
    }
}

impl compositor::CompositorHandler for Application {
    fn compositor_state(&mut self) -> &mut compositor::CompositorState {
        &mut self.compositor
    }

    fn client_compositor_state<'a>(
        &self,
        client: &'a Client,
    ) -> &'a compositor::CompositorClientState {
        &client.get_data::<ClientState>().unwrap().compositor_state
    }

    #[allow(clippy::significant_drop_tightening)]
    fn commit(&mut self, surface: &WlSurface) {
        self.popups_commit(surface);

        smithay::wayland::compositor::with_states(surface, |states| {
            let mut guard = states.cached_state.get::<SurfaceAttributes>();
            let attrs = guard.current();

            match attrs.buffer.take() {
                Some(BufferAssignment::NewBuffer(buffer)) => {
                    match buffer_type(&buffer) {
                        Some(BufferType::Dma) => {
                            let dmabuf = get_dmabuf(&buffer).unwrap(); // always Ok due to buffer_type
                            if let Ok(image) = self
                                .image_importer
                                .get_or_import_dmabuf(dmabuf.clone())
                                .inspect_err(|e| {
                                    log::warn!("wayland_server failed to import DMA-buf: {e:?}");
                                })
                            {
                                let sbwi = SurfaceBufWithImage {
                                    image,
                                    transform: wl_transform_to_frame_transform(
                                        attrs.buffer_transform,
                                    ),
                                    scale: attrs.buffer_scale,
                                    dmabuf: true,
                                };
                                sbwi.apply_to_surface(states);
                            }
                        }
                        Some(BufferType::Shm) => {
                            let _ = with_buffer_contents(&buffer, |data, size, buf| {
                                if let Ok(image) = self
                                    .image_importer
                                    .import_shm(data, size, buf)
                                    .inspect_err(|e| {
                                        log::warn!("wayland_server failed to import SHM: {e:?}");
                                    })
                                {
                                    let sbwi = SurfaceBufWithImage {
                                        image,
                                        transform: wl_transform_to_frame_transform(
                                            attrs.buffer_transform,
                                        ),
                                        scale: attrs.buffer_scale,
                                        dmabuf: false,
                                    };
                                    sbwi.apply_to_surface(states);
                                }
                            });
                        }
                        Some(BufferType::SinglePixel) => {
                            let spb = get_single_pixel_buffer(&buffer).unwrap(); // always Ok
                            if let Ok(image) =
                                self.image_importer.import_spb(spb).inspect_err(|e| {
                                    log::warn!("wayland_server failed to import SPB: {e:?}");
                                })
                            {
                                let sbwi = SurfaceBufWithImage {
                                    image,
                                    transform: wl_transform_to_frame_transform(
                                        // does this even matter
                                        attrs.buffer_transform,
                                    ),
                                    scale: attrs.buffer_scale,
                                    dmabuf: false,
                                };
                                sbwi.apply_to_surface(states);
                            }
                        }
                        Some(other) => log::warn!("Unsupported wl_buffer format: {other:?}"),
                        None => { /* don't draw anything */ }
                    }
                    buffer.release();
                }
                Some(BufferAssignment::Removed) | None => {}
            }

            let t = time::get_millis() as u32;
            let callbacks = std::mem::take(&mut attrs.frame_callbacks);
            for cb in callbacks {
                cb.done(t);
            }
        });

        self.redraw_requests.insert(surface.id());
    }
}

////////////////////////////////////////////////////////////////////////////////////
// Client-side external keyboard and mouse logging app
impl ProvidesRegistryState for ClientSideInputApplication {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState, SCT_SeatState];
}

// Client-toolkit's CompositorHandler only needs these four surface callbacks.
// No compositor_state() accessor - that's the server-side Smithay crate.
impl CompositorHandler for ClientSideInputApplication {
    fn scale_factor_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: i32,
    ) {
    }

    fn transform_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: wlc_output::Transform,
    ) {
    }

    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {}

    fn surface_enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wlc_output::WlOutput,
    ) {
    }

    fn surface_leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wlc_output::WlOutput,
    ) {
    }
}

impl SCT_OutputHandler for ClientSideInputApplication {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wlc_output::WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wlc_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wlc_output::WlOutput) {}
}

impl SCT_ShmHandler for ClientSideInputApplication {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.client_shm
    }
}

impl SeatHandler for Application {
    type KeyboardFocus = WlSurface;
    type PointerFocus = WlSurface;
    type TouchFocus = WlSurface;

    fn seat_state(&mut self) -> &mut SeatState<Self> {
        &mut self.seat_state
    }

    fn focus_changed(&mut self, seat: &Seat<Self>, focused: Option<&WlSurface>) {
        let dh = &self.display_handle;
        let client = focused.and_then(|s| dh.get_client(s.id()).ok());
        set_data_device_focus(dh, seat, client.clone());
        set_primary_focus(dh, seat, client);
    }

    fn cursor_image(
        &mut self,
        _seat: &Seat<Self>,
        _image: smithay::input::pointer::CursorImageStatus,
    ) {
    }
}

impl KeyboardHandler for ClientSideInputApplication {
    fn enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        surface: &wl_surface::WlSurface,
        _serial: u32,
        _raw: &[u32],
        _keysyms: &[Keysym],
    ) {
        // Proxy::id() requires `use wayland_client::Proxy` in scope
        println!("Keyboard focus entered surface {:?}", surface.id());
    }

    fn leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: &wl_surface::WlSurface,
        _serial: u32,
    ) {
        println!("Keyboard focus left");
    }

    fn press_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _serial: u32,
        event: KeyEvent,
    ) {
        println!(
            "Key pressed: sym={:?}  raw={}",
            event.keysym, event.raw_code
        );
        if event.keysym == Keysym::Escape {
            println!("Escape pressed - exiting.");
            self.is_key_logging = false;
        }
        let _ = self.tx.send(ClientSideInput::KeyDown(event.raw_code));
    }

    fn release_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _serial: u32,
        event: KeyEvent,
    ) {
        println!(
            "Key release: sym={:?}  raw={}",
            event.keysym, event.raw_code
        );
        let _ = self.tx.send(ClientSideInput::KeyUp(event.raw_code));
    }

    fn update_modifiers(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _serial: u32,
        modifiers: Modifiers,
        _raw: RawModifiers,
        _layout: u32,
    ) {
        println!(
            "Modifiers: ctrl={} alt={} shift={} super={}",
            modifiers.ctrl, modifiers.alt, modifiers.shift, modifiers.logo
        );
    }

    // repeat_key is required in 0.20 - called when key-repeat fires.
    fn repeat_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _serial: u32,
        event: KeyEvent,
    ) {
        println!(
            "Key repeat:   sym={:?}  raw={}",
            event.keysym, event.raw_code
        );
    }

    // update_repeat_info has a default impl, but shown here for clarity
    fn update_repeat_info(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        info: RepeatInfo,
    ) {
        println!("Repeat info: {:?}", info);
    }
}

impl PointerHandler for ClientSideInputApplication {
    fn pointer_frame(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_pointer::WlPointer,
        events: &[PointerEvent],
    ) {
        for event in events {
            match event.kind {
                PointerEventKind::Enter { serial } => {
                    println!("Pointer entered surface, serial={serial}");
                }
                PointerEventKind::Leave { serial } => {
                    println!("Pointer left surface, serial={serial}");
                }
                PointerEventKind::Motion { time } => {
                    // event.position is (f64, f64) surface-local coordinates
                    println!("Motion t={time} pos={:.1?}", event.position);
                }
                PointerEventKind::Press {
                    button,
                    serial,
                    time,
                } => {
                    // button uses Linux evdev codes: 0x110=left, 0x111=right, 0x112=middle
                    println!("Button press   button={button:#x} serial={serial} t={time}");
                }
                PointerEventKind::Release {
                    button,
                    serial,
                    time,
                } => {
                    println!("Button release button={button:#x} serial={serial}, t={time}");
                }
                PointerEventKind::Axis {
                    horizontal,
                    vertical,
                    ..
                } => {
                    // AxisScroll has absolute (f64 pixels) and .discrete (scroll steps, i32)
                    println!(
                        "Scroll t=(time) h={:.1}/{:?} v={:.1}/{:?}",
                        horizontal.absolute,
                        horizontal.discrete,
                        vertical.absolute,
                        vertical.discrete,
                    );
                }
            }
        }
    }
}

impl SCT_SeatHandler for ClientSideInputApplication {
    fn seat_state(&mut self) -> &mut SCT_SeatState {
        &mut self.sct_seat_state
    }

    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wlc_seat::WlSeat) {}

    fn new_capability(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        seat: wlc_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Keyboard && self.keyboard.is_none() {
            println!("Keyboard capability found, binding...");
            let kbd = self.sct_seat_state.get_keyboard(qh, &seat, None).unwrap();
            self.keyboard = Some(kbd);
        }
        if capability == Capability::Pointer && self.pointer.is_none() {
            let pointer = self.sct_seat_state.get_pointer(qh, &seat).unwrap();
            self.pointer = Some(pointer);
        }
    }

    fn remove_capability(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: wlc_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Keyboard {
            if let Some(kbd) = self.keyboard.take() {
                kbd.release();
            }
        }
        if capability == Capability::Pointer {
            if let Some(ptr) = self.pointer.take() {
                ptr.release();
            }
        }
    }

    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wlc_seat::WlSeat) {}

}

// This sets up the buffer and canvas for capturing external keyboard and mouse inputs.
impl LayerShellHandler for ClientSideInputApplication {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &LayerSurface) {
        self.is_key_logging = false
    }

    fn configure(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        layer: &LayerSurface,
        _configure: LayerSurfaceConfigure,
        _serial: u32,
    ) {
        // Commit a minimal 1x1 transparent buffer to satisfy the compositor's
        // requirement that a surface must have a buffer before it is considered mapped.
        if self.pool.is_none() {
            self.pool = Some(SlotPool::new(4, &self.client_shm).unwrap());
        }
        let pool = self.pool.as_mut().unwrap();
        // We're creating a window so we can get some mouse events
        let (buffer, canvas) = pool
            .create_buffer(
                CLIENT_CAP_WINDOW_WIDTH as i32,
                CLIENT_CAP_WINDOW_HEIGHT as i32,
                400,
                wl_shm::Format::Argb8888)
            .unwrap();
        canvas.fill(0); // fully transparent

        layer.wl_surface().attach(Some(buffer.wl_buffer()), 0, 0);
        layer.wl_surface().damage_buffer(
            0,
            0,
            CLIENT_CAP_WINDOW_WIDTH as i32,
            CLIENT_CAP_WINDOW_HEIGHT as i32);
        layer.wl_surface().commit();
        let _ = qh;
    }
}

impl BufferHandler for Application {
    fn buffer_destroyed(&mut self, _buffer: &wl_buffer::WlBuffer) {}
}

impl ClientDndGrabHandler for Application {}

impl ServerDndGrabHandler for Application {
    fn send(&mut self, _mime_type: String, _fd: OwnedFd, _seat: Seat<Self>) {}
}

impl DataDeviceHandler for Application {
    fn data_device_state(&self) -> &DataDeviceState {
        &self.data_device
    }
}

impl SelectionHandler for Application {
    type SelectionUserData = Arc<[u8]>;

    fn send_selection(
        &mut self,
        _ty: selection::SelectionTarget,
        _mime_type: String,
        fd: OwnedFd,
        _seat: Seat<Self>,
        user_data: &Self::SelectionUserData,
    ) {
        let buf = user_data.clone();
        std::thread::spawn(move || {
            // Clear O_NONBLOCK, otherwise File::write_all() will stop halfway.
            if let Err(err) = fcntl_setfl(&fd, OFlags::empty()) {
                log::warn!("error clearing flags on selection target fd: {err:?}");
            }
            if let Err(err) = File::from(fd).write_all(&buf) {
                log::warn!("error writing selection: {err:?}");
            }
        });
    }

    fn new_selection(
        &mut self,
        _ty: selection::SelectionTarget,
        _source: Option<selection::SelectionSource>,
        _seat: Seat<Self>,
    ) {
    }
}

#[derive(Default)]
pub struct ClientState {
    compositor_state: compositor::CompositorClientState,
    pub disconnected: Arc<Mutex<bool>>,
}

impl ClientData for ClientState {
    fn initialized(&self, client_id: ClientId) {
        log::debug!("Client ID {client_id:?} connected");
    }

    fn disconnected(&self, client_id: ClientId, reason: DisconnectReason) {
        *self.disconnected.lock().unwrap() = true;
        log::debug!("Client ID {client_id:?} disconnected. Reason: {reason:?}");
    }
}

impl AsMut<compositor::CompositorState> for Application {
    fn as_mut(&mut self) -> &mut compositor::CompositorState {
        &mut self.compositor
    }
}

impl XdgShellHandler for Application {
    fn xdg_shell_state(&mut self) -> &mut XdgShellState {
        &mut self.xdg_shell
    }

    fn new_toplevel(&mut self, surface: ToplevelSurface) {
        if let Some(client) = surface.wl_surface().client() {
            self.wayvr_tasks
                .send(WayVRTask::NewToplevel(client.id(), surface.clone()));
        }
        surface.with_pending_state(|state| {
            state.states.set(xdg_toplevel::State::Activated);
        });
        surface.send_configure();
    }

    fn toplevel_destroyed(&mut self, surface: ToplevelSurface) {
        if let Some(client) = surface.wl_surface().client() {
            self.wayvr_tasks
                .send(WayVRTask::DropToplevel(client.id(), surface.clone()));
        }
    }

    fn new_popup(&mut self, surface: PopupSurface, _positioner: PositionerState) {
        let _ = self
            .popup_manager
            .track_popup(PopupKind::Xdg(surface))
            .context("Could not track xdg_popup")
            .inspect_err(|e| log::warn!("{e:?}"));
    }

    fn popup_destroyed(&mut self, _surface: PopupSurface) {
        self.popup_manager.cleanup();
    }

    fn grab(&mut self, _surface: PopupSurface, _seat: wl_seat::WlSeat, _serial: Serial) {
        // Handle popup grab here
    }

    fn reposition_request(
        &mut self,
        _surface: PopupSurface,
        _positioner: PositionerState,
        _token: u32,
    ) {
        // Handle popup reposition here
    }

    // If the app wants to be fullscreen, make it think that it's fullscreen.
    fn fullscreen_request(
        &mut self,
        surface: ToplevelSurface,
        _output: Option<wl_output::WlOutput>,
    ) {
        surface.with_pending_state(|state| {
            state.states.set(xdg_toplevel::State::Fullscreen);
        });
        surface.send_configure();
    }
    fn unfullscreen_request(&mut self, surface: ToplevelSurface) {
        surface.with_pending_state(|state| {
            state.states.unset(xdg_toplevel::State::Fullscreen);
        });
        surface.send_configure();
    }
    // If the app wants to be maximized, make it think that it's maximized.
    fn maximize_request(&mut self, surface: ToplevelSurface) {
        surface.with_pending_state(|state| {
            state.states.set(xdg_toplevel::State::Maximized);
        });
        surface.send_configure();
    }
    fn unmaximize_request(&mut self, surface: ToplevelSurface) {
        surface.with_pending_state(|state| {
            state.states.unset(xdg_toplevel::State::Maximized);
        });
        surface.send_configure();
    }
    // If the app requests minimize, hide its window
    fn minimize_request(&mut self, surface: ToplevelSurface) {
        if let Some(client) = surface.wl_surface().client() {
            self.wayvr_tasks
                .send(WayVRTask::MinimizeRequest(client.id(), surface.clone()));
        }
    }
}

impl ShmHandler for Application {
    fn shm_state(&self) -> &ShmState {
        &self.shm
    }
}

impl OutputHandler for Application {}

impl DmabufHandler for Application {
    fn dmabuf_state(&mut self) -> &mut DmabufState {
        &mut self.dmabuf_state.0
    }

    fn dmabuf_imported(
        &mut self,
        _global: &DmabufGlobal,
        dmabuf: Dmabuf,
        notifier: ImportNotifier,
    ) {
        if self.image_importer.get_or_import_dmabuf(dmabuf).is_ok() {
            let _ = notifier.successful::<Self>();
        } else {
            notifier.failed();
        }
    }
}

impl PrimarySelectionHandler for Application {
    fn primary_selection_state(&self) -> &PrimarySelectionState {
        &self.primary_selection_state
    }
}

impl selection_wlr::DataControlHandler for Application {
    fn data_control_state(&self) -> &selection_wlr::DataControlState {
        &self.wlr_data_control_state
    }
}

impl selection_ext::DataControlHandler for Application {
    fn data_control_state(&self) -> &selection_ext::DataControlState {
        &self.ext_data_control_state
    }
}

impl XdgDecorationHandler for Application {
    fn new_decoration(&mut self, toplevel: ToplevelSurface) {
        toplevel.with_pending_state(|state| {
            state.decoration_mode = Some(zxdg_toplevel_decoration_v1::Mode::ServerSide);
        });
    }

    fn request_mode(
        &mut self,
        _toplevel: ToplevelSurface,
        _mode: zxdg_toplevel_decoration_v1::Mode,
    ) {
        // no switching away from SSD
    }

    fn unset_mode(&mut self, _toplevel: ToplevelSurface) {
        // no switching away from SSD
    }
}

impl KdeDecorationHandler for Application {
    fn kde_decoration_state(&self) -> &KdeDecorationState {
        &self.kde_decoration_state
    }

    fn request_mode(
        &mut self,
        _surface: &WlSurface,
        decoration: &org_kde_kwin_server_decoration::OrgKdeKwinServerDecoration,
        _mode: wayland_server::WEnum<org_kde_kwin_server_decoration::Mode>,
    ) {
        decoration.mode(org_kde_kwin_server_decoration::Mode::Server);
    }
}

delegate_dmabuf!(Application);
delegate_xdg_shell!(Application);
delegate_compositor!(Application);
delegate_shm!(Application);
delegate_seat!(Application);
delegate_data_device!(Application);
delegate_output!(Application);
delegate_primary_selection!(Application);
delegate_data_control!(Application);
delegate_ext_data_control!(Application);
delegate_xdg_decoration!(Application);
delegate_kde_decoration!(Application);
delegate_single_pixel_buffer!(Application);

const fn wl_transform_to_frame_transform(
    transform: wl_output::Transform,
) -> wlx_capture::frame::Transform {
    match transform {
        wl_output::Transform::Normal => wlx_capture::frame::Transform::Normal,
        wl_output::Transform::_90 => wlx_capture::frame::Transform::Rotated90,
        wl_output::Transform::_180 => wlx_capture::frame::Transform::Rotated180,
        wl_output::Transform::_270 => wlx_capture::frame::Transform::Rotated270,
        wl_output::Transform::Flipped => wlx_capture::frame::Transform::Flipped,
        wl_output::Transform::Flipped90 => wlx_capture::frame::Transform::Flipped90,
        wl_output::Transform::Flipped180 => wlx_capture::frame::Transform::Flipped180,
        wl_output::Transform::Flipped270 => wlx_capture::frame::Transform::Flipped270,
        _ => wlx_capture::frame::Transform::Undefined,
    }
}
