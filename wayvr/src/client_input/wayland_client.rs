use crate::client_input::{ClientInputThread, ClientSideInput};

use std::sync::mpsc;
use std::sync::mpsc::Sender;
use std::thread;
use std::thread::JoinHandle;

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
            Anchor, KeyboardInteractivity, Layer, LayerShell, 
            LayerShellHandler, LayerSurface, LayerSurfaceConfigure,
        },
    },
    shm::{Shm, ShmHandler as SCT_ShmHandler, slot::SlotPool},
    compositor::{CompositorState},
};

use smithay::reexports::wayland_protocols::wp::{
    pointer_constraints::zv1::client::{
        zwp_locked_pointer_v1::ZwpLockedPointerV1,
        zwp_confined_pointer_v1::ZwpConfinedPointerV1,
        zwp_pointer_constraints_v1::{self, ZwpPointerConstraintsV1},
    },
    relative_pointer::zv1::client::{
        zwp_relative_pointer_manager_v1::ZwpRelativePointerManagerV1,
        zwp_relative_pointer_v1::{self, ZwpRelativePointerV1},
    },
};

use wayland_client::{
    Connection, Proxy, QueueHandle,
    protocol::{wl_keyboard, wl_output as wlc_output, wl_pointer, wl_seat as wlc_seat, wl_shm, wl_surface},
    globals::registry_queue_init,
};

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
    pub relative_pointer_manager: Option<ZwpRelativePointerManagerV1>,
    pub relative_pointer: Option<ZwpRelativePointerV1>,
    pub pointer_constraints: Option<ZwpPointerConstraintsV1>,
    pub locked_pointer: Option<ZwpLockedPointerV1>,
    pub screen_width: u32,
    pub screen_height: u32,    
    pub layer_wl_surface: Option<wayland_client::protocol::wl_surface::WlSurface>,
}

impl ClientSideInputApplication {
    // Pointer locking: keep the mouse cursor we're capturing from running
    // around the screen and triggering stuff like KDE hot corners.
    // We put this in a helper to be used in new_capability *and* configure
    // based on the order of initialization from different compositors
    fn try_lock_pointer(&mut self, qh: &QueueHandle<Self>) {
        if self.locked_pointer.is_some() { return; }
        let (Some(pc), Some(ptr), Some(surface)) = (
            self.pointer_constraints.as_ref(),
            self.pointer.as_ref(),
            self.layer_wl_surface.as_ref(),
        ) else { return; };

        let locked = pc.lock_pointer(
            surface,
            ptr,
            None,
            zwp_pointer_constraints_v1::Lifetime::Persistent,
            qh,
            (),
        );

        // After locking, set the hint to center of screen
        // These are in surface-local coordinates as wl_fixed
        locked.set_cursor_position_hint(
            (self.screen_width / 2) as f64,
            (self.screen_height / 2) as f64,
        );
        self.locked_pointer = Some(locked);
    }
}

sct_delegate_compositor!(ClientSideInputApplication);
sct_delegate_output!(ClientSideInputApplication);
sct_delegate_seat!(ClientSideInputApplication);
sct_delegate_keyboard!(ClientSideInputApplication);
sct_delegate_pointer!(ClientSideInputApplication);
sct_delegate_layer!(ClientSideInputApplication);
sct_delegate_shm!(ClientSideInputApplication);
sct_delegate_registry!(ClientSideInputApplication);

impl ClientInputThread for ClientSideInputApplication {
    fn launch_input_thread(tx: Sender<ClientSideInput>) -> JoinHandle<()> {
        thread::spawn(|| {
            let conn = Connection::connect_to_env().expect("client side input thread failed to connect to Wayland display");
            let (globals, mut event_queue) = registry_queue_init(&conn).unwrap();
            let qh: QueueHandle<ClientSideInputApplication> = event_queue.handle();

            let compositor_state = CompositorState::bind(&globals, &qh).unwrap();
            let layer_shell      = LayerShell::bind(&globals, &qh).unwrap();
            let client_shm  = Shm::bind(&globals, &qh).unwrap();

            let pointer_constraints: Option<ZwpPointerConstraintsV1> =
                globals.bind(&qh, 1..=1, ()).ok();

            // Relative-pointer manager — gives us compositor-wide delta motion
            // without requiring a pointer lock or owning the cursor.
            let relative_pointer_manager: Option<ZwpRelativePointerManagerV1> =
                globals.bind(&qh, 1..=1, ()).ok();

            let mut client_app = ClientSideInputApplication {
                client_shm,
                registry_state:          RegistryState::new(&globals),
                sct_seat_state:          SCT_SeatState::new(&globals, &qh),
                output_state:            OutputState::new(&globals, &qh),
                pool:                    None,
                keyboard:                None,
                pointer:                 None,
                is_key_logging:          true,
                tx,
                relative_pointer_manager,
                relative_pointer:        None,
                pointer_constraints,
                locked_pointer:          None,
                layer_wl_surface:        None,
                screen_width: 0,
                screen_height: 0,
            };

            // Layer surface: still needed for KeyboardInteractivity::Exclusive,
            // but size is now 1×1 (see LayerShellHandler::configure in comp.rs).
            let surface = compositor_state.create_surface(&qh);
            let layer_surface = layer_shell.create_layer_surface(
                &qh,
                surface,
                Layer::Overlay,
                Some("kbd-capture"),
                None,
            );
            // 0,0 means "use the full output size" in layer-shell
            layer_surface.set_size(0, 0);
            layer_surface.set_anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT | Anchor::BOTTOM);
            layer_surface.set_exclusive_zone(-1); // don't push other surfaces aside
            layer_surface.set_keyboard_interactivity(KeyboardInteractivity::Exclusive);

            layer_surface.commit();

            client_app.layer_wl_surface = Some(layer_surface.wl_surface().clone());

            while client_app.is_key_logging {
                event_queue.blocking_dispatch(&mut client_app).expect("client side input thread failed to dispatch input events");
            }
        })
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
                    // Surface entered — no action needed; relative motion is
                    // compositor-wide and does not require the surface to be entered.
                    let _ = serial;
                    println!("Mouse Enter Event");
                }
                PointerEventKind::Leave { .. } => {
                    println!("Mouse Leave Event");
                }
                PointerEventKind::Motion { .. } => {
                    // Absolute position — ignored here.
                    // Relative deltas arrive via ZwpRelativePointerV1::RelativeMotion.
                }
                PointerEventKind::Press { button, .. } => {
                    let _ = self.tx.send(ClientSideInput::MouseDown { button });
                    println!("Mouse Press {button}");
                }
                PointerEventKind::Release { button, .. } => {
                    let _ = self.tx.send(ClientSideInput::MouseUp { button });
                    println!("Mouse Release {button}");
                }
                PointerEventKind::Axis { horizontal, vertical, .. } => {
                    let _ = self.tx.send(ClientSideInput::MouseScroll {
                        dx: horizontal.absolute,
                        dy: vertical.absolute,
                    });
                    println!("Mouse Axis {0} {1}", horizontal.absolute, vertical.absolute);
                }
            }
        }
    }
}

impl wayland_client::Dispatch<ZwpRelativePointerManagerV1, ()> for ClientSideInputApplication {
    fn event(
        _state: &mut Self,
        _proxy: &ZwpRelativePointerManagerV1,
        _event: smithay::reexports::wayland_protocols::wp::relative_pointer::zv1::client::zwp_relative_pointer_manager_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        // no events on the manager itself
    }
}

impl wayland_client::Dispatch<ZwpRelativePointerV1, ()> for ClientSideInputApplication {
    fn event(
        state: &mut Self,
        _proxy: &ZwpRelativePointerV1,
        event: zwp_relative_pointer_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        if let zwp_relative_pointer_v1::Event::RelativeMotion { dx, dy, .. } = event {
            println!("Mouse Relative: {dx} {dy}");
            let _ = state.tx.send(ClientSideInput::MouseMove { dx, dy });

            // Keep warping back to center after every motion event
            // This will lock the pointer so it can't bump into controls like
            // hot corners in KDE.
            if let Some(locked) = &state.locked_pointer {
                locked.set_cursor_position_hint(
                    (state.screen_width / 2) as f64,
                    (state.screen_height / 2) as f64,
                );
            }            
        }
    }
}

impl wayland_client::Dispatch<wayland_client::protocol::wl_region::WlRegion, ()> 
    for ClientSideInputApplication 
{
    fn event(_: &mut Self, _: &wayland_client::protocol::wl_region::WlRegion,
        _: wayland_client::protocol::wl_region::Event,
        _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

// These two are needed to satisfy wayland-client's Dispatch bounds even though
// we never create these object types.  They have no events in practice.
impl wayland_client::Dispatch<ZwpPointerConstraintsV1, ()> for ClientSideInputApplication {
    fn event(_: &mut Self, _: &ZwpPointerConstraintsV1,
        _: zwp_pointer_constraints_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>) {}
}
impl wayland_client::Dispatch<ZwpLockedPointerV1, ()> for ClientSideInputApplication {
    fn event(_: &mut Self, _: &ZwpLockedPointerV1,
        _: smithay::reexports::wayland_protocols::wp::pointer_constraints::zv1::client::zwp_locked_pointer_v1::Event,
        _: &(), _: &Connection, _: &QueueHandle<Self>) {

        }
}
impl wayland_client::Dispatch<ZwpConfinedPointerV1, ()> for ClientSideInputApplication {
    fn event(_: &mut Self, _: &ZwpConfinedPointerV1,
        _: smithay::reexports::wayland_protocols::wp::pointer_constraints::zv1::client::zwp_confined_pointer_v1::Event,
        _: &(), _: &Connection, _: &QueueHandle<Self>) {}
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

            // Subscribe to relative motion on this pointer.
            // No lock needed — ZwpRelativePointerV1 fires alongside normal pointer
            // events without stealing focus or suppressing delivery elsewhere.
            if let Some(rpm) = &self.relative_pointer_manager {
                self.relative_pointer = Some(rpm.get_relative_pointer(&pointer, qh, ()));
            }

            self.pointer = Some(pointer);
            self.try_lock_pointer(qh);
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
            // Destroy relative pointer first — it holds a reference to wl_pointer.
            if let Some(rp) = self.relative_pointer.take() {
                rp.destroy();
            }
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
        configure: LayerSurfaceConfigure,
        _serial: u32,
    ) {
        // Use the compositor-assigned size; fall back to 1x1 if zero
        let w = configure.new_size.0.max(1);
        let h = configure.new_size.1.max(1);

        if self.pool.is_none() {
            self.pool = Some(SlotPool::new(
                (w * h * 4) as usize,
                &self.client_shm,
            ).unwrap());
        }
        let pool = self.pool.as_mut().unwrap();

        let (buffer, canvas) = pool
            .create_buffer(w as i32, h as i32, w as i32 * 4, wl_shm::Format::Argb8888)
            .unwrap();
        canvas.fill(0); // fully transparent

        layer.wl_surface().attach(Some(buffer.wl_buffer()), 0, 0);
        layer.wl_surface().damage_buffer(0, 0, w as i32, h as i32);
        layer.wl_surface().commit();

        self.try_lock_pointer(qh);
    }
}
