use crate::client_input::{ClientInputThread, ClientSideInput};
use crate::client_input::key_combiner::KeyCombiner;

use std::sync::mpsc::Sender;
use std::thread;
use std::thread::JoinHandle;

use input::{
    event::{
        keyboard::{KeyState, KeyboardEventTrait},
        pointer::{Axis, ButtonState, PointerScrollEvent},
        Event,
        keyboard::KeyboardEvent,
        PointerEvent,
    },
    Libinput, LibinputInterface,
};
use libc::{O_ACCMODE, O_RDONLY, O_RDWR, O_WRONLY};
use std::{
    collections::HashMap,
    fs::OpenOptions,
    io,
    os::unix::{fs::OpenOptionsExt, io::{AsRawFd, OwnedFd, RawFd}},
    path::Path,
    sync::{Arc, Mutex},
};

// This is how you grab the keyboard and mouse exclusively and tell
// your window manager to kindly die in a ditch.
const EVIOCGRAB: libc::c_ulong = 0x40044590;

fn eviocgrab(fd: RawFd, grab: bool) -> io::Result<()> {
    let val: libc::c_int = if grab { 1 } else { 0 };
    let ret = unsafe { libc::ioctl(fd, EVIOCGRAB, val) };
    if ret < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[derive(Default)]
struct GrabState {
    /// All fds currently opened by libinput, keyed by raw fd number.
    fds:    HashMap<RawFd, ()>,
    /// Whether we are currently grabbing.
    active: bool,
}

impl GrabState {
    fn grab_all(&mut self) {
        for &fd in self.fds.keys() {
            if let Err(e) = eviocgrab(fd, true) {
                eprintln!("EVIOCGRAB({fd}) failed: {e}");
            }
        }
        self.active = true;
        println!("[GRAB]  exclusive grab active — desktop input suppressed");
        println!("        press Escape to release");
    }

    fn ungrab_all(&mut self) {
        for &fd in self.fds.keys() {
            let _ = eviocgrab(fd, false); // best-effort
        }
        self.active = false;
        println!("[GRAB]  grab released — desktop input restored");
        println!("        press Escape to re-grab");
    }
}

struct DirectInterface {
    state: Arc<Mutex<GrabState>>,
}

impl LibinputInterface for DirectInterface {
    fn open_restricted(&mut self, path: &Path, flags: i32) -> Result<OwnedFd, i32> {
        let file = OpenOptions::new()
            .custom_flags(flags)
            .read((flags & O_ACCMODE == O_RDONLY) | (flags & O_ACCMODE == O_RDWR))
            .write((flags & O_ACCMODE == O_WRONLY) | (flags & O_ACCMODE == O_RDWR))
            .open(path)
            .map_err(|e| e.raw_os_error().unwrap_or(libc::EIO))?;

        let raw = file.as_raw_fd();
        let mut st = self.state.lock().unwrap();
        st.fds.insert(raw, ());

        // If a grab is already active when a new device appears, grab it too.
        if st.active {
            if let Err(e) = eviocgrab(raw, true) {
                eprintln!("EVIOCGRAB({raw}) on new device failed: {e}");
            }
        }

        Ok(file.into())
    }

    fn close_restricted(&mut self, fd: OwnedFd) {
        let raw = fd.as_raw_fd();
        self.state.lock().unwrap().fds.remove(&raw);
        drop(fd); // fd closed here; kernel releases grab automatically
    }
}

fn check_input_group() {
    let in_group = std::process::Command::new("id")
        .arg("-Gn")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.split_whitespace().any(|g| g == "input"))
        .unwrap_or(false);

    if !in_group {
        eprintln!("WARNING: not in the `input` group.");
        eprintln!("  Run: sudo usermod -aG input $USER");
        eprintln!("  Then log out and back in (or: newgrp input).");
        eprintln!();
    }
}

fn button_name(code: u32) -> &'static str {
    match code {
        0x110 => "BTN_LEFT",
        0x111 => "BTN_RIGHT",
        0x112 => "BTN_MIDDLE",
        0x113 => "BTN_SIDE",
        0x114 => "BTN_EXTRA",
        _     => "BTN_?",
    }
}

// KEY_ESC raw keycode (not a keysym — this is the evdev scancode)
const KEY_ESC: u32 = 1;

pub struct LibInputApplication {

}

impl ClientInputThread for LibInputApplication {
    fn launch_input_thread(tx: Sender<ClientSideInput>) -> JoinHandle<()> {
        thread::spawn(move || {
            let mut key_combiner = KeyCombiner::new();
            check_input_group();

            let state = Arc::new(Mutex::new(GrabState::default()));

            let iface = DirectInterface { state: Arc::clone(&state) };
            let mut li = Libinput::new_with_udev(iface);
            li.udev_assign_seat("seat0")
                .expect("udev_assign_seat failed — are you in the `input` group?");

            // Grab immediately so the very first events are captured.
            state.lock().unwrap().grab_all();

            println!("libinput-demo — Escape toggles grab, Ctrl-C quits");
            println!();

            let fd = li.as_raw_fd();

            loop {
                let mut grabbed = state.lock().unwrap();
                let virtual_sysnames = grabbed.virtual_sysnames.clone();
                drop(grabbed);

                let mut pfd = libc::pollfd { fd, events: libc::POLLIN, revents: 0 };
                let ret = unsafe { libc::poll(&mut pfd as *mut libc::pollfd, 1, -1) };
                if ret < 0 {
                    let err = io::Error::last_os_error();
                    if err.kind() == io::ErrorKind::Interrupted { break; }
                    // Not returning this for now since the JoinHandle is just empty.
                    // We are sorting out how we want to handle errors in the input thread.
                    //return Err(err);
                }

                li.dispatch().expect("libinput dispatch failed");

                for event in &mut li {
                    match event {
                        Event::Keyboard(KeyboardEvent::Key(key)) => {
                            let pressed = key.key_state() == KeyState::Pressed;

                            // Escape toggles the grab on key-down only.
                            if key.key() == KEY_ESC && pressed {
                                let mut st = state.lock().unwrap();
                                if st.active { st.ungrab_all(); } else { st.grab_all(); }
                                continue;
                            }

                            let state_str = if pressed { "PRESSED " } else { "released" };
                            println!("[KBD] key {:>5}  {}", key.key(), state_str);

                            if pressed {
                                //let _ = tx.send(ClientSideInput::KeyDown(key.key()));
                                key_combiner.process(&tx, ClientSideInput::KeyDown(key.key()));
                            } else {
                                //let _ = tx.send(ClientSideInput::KeyUp(key.key()));
                                key_combiner.process(&tx, ClientSideInput::KeyUp(key.key()));
                            }
                        }

                        Event::Pointer(ptr) => match ptr {
                            PointerEvent::Motion(m) => {
                                println!("[PTR] motion  dx={:+7.2}  dy={:+7.2}", m.dx(), m.dy());
                                let _ = tx.send(ClientSideInput::MouseMove { dx: m.dx(), dy: m.dy() });
                            }
                            PointerEvent::Button(b) => {
                                let state_str = match b.button_state() {
                                    ButtonState::Pressed  => "PRESSED ",
                                    ButtonState::Released => "RELEASED",
                                };
                                println!(
                                    "[PTR] button  {}  {}  (code=0x{:03x})",
                                    button_name(b.button()), state_str, b.button()
                                );
                                let _ = match b.button_state() {
                                    ButtonState::Pressed  => tx.send(ClientSideInput::MouseDown { button: b.button() }),
                                    ButtonState::Released => tx.send(ClientSideInput::MouseUp { button: b.button() }),
                                };
                                
                            }                    
                            PointerEvent::ScrollWheel(w) => {
                                // TODO: I smell a macro for these
                                let dx = if w.has_axis(Axis::Horizontal) {
                                    w.scroll_value(Axis::Horizontal)
                                } else {
                                    0.0
                                };

                                let dy = if w.has_axis(Axis::Vertical) {
                                    w.scroll_value(Axis::Vertical)
                                } else {
                                    0.0
                                };
                                println!("wheel dx: {} dy: {}", dx, dy);
                                let _ = tx.send(ClientSideInput::MouseScroll {dx, dy});
                            }
                            PointerEvent::ScrollFinger(f) => {
                                let dx
                                 = if f.has_axis(Axis::Horizontal) {
                                    f.scroll_value(Axis::Horizontal)
                                } else {
                                    0.0
                                };

                                let dy = if f.has_axis(Axis::Vertical) {
                                    f.scroll_value(Axis::Vertical)
                                } else {
                                    0.0
                                };
                                println!("finger dx: {} dy: {}", dx, dy);
                            }
                            PointerEvent::ScrollContinuous(c) => {
                                let dx = if c.has_axis(Axis::Horizontal) {
                                    c.scroll_value(Axis::Horizontal)
                                } else {
                                    0.0
                                };

                                let dy = if c.has_axis(Axis::Vertical) {
                                    c.scroll_value(Axis::Vertical)
                                } else {
                                    0.0
                                };
                                println!("continuous dx: {} dy: {}", dx, dy);
                            }
                            _ => {}
                        }

                        _ => {}
                    }
                }
            }

            // Clean release on Ctrl-C (SIGINT arrives as EINTR on poll, breaking the loop).
            state.lock().unwrap().ungrab_all();
        })
    }
}
