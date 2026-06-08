pub mod wayland_client;
pub mod libinput_client;
pub mod key_combiner;

use std::thread::JoinHandle;
use std::sync::mpsc::Sender;

// Trait with static method to create the producer thread for transmitting
// client inputs. It's assumed whatever is made with this will send ClientSideInput
// events using the given tx pipe.
pub trait ClientInputThread {
    fn launch_input_thread(tx: Sender<ClientSideInput>) -> JoinHandle<()>;
}

pub enum ClientSideInput {
    KeyDown(u32),
    KeyUp(u32),
    MouseMove { dx: f64, dy: f64 },
    MouseDown { button: u32 },
    MouseUp   { button: u32 },
    MouseScroll { dx: f64, dy: f64 },
    SpecialDebug { code: u32 },
}
