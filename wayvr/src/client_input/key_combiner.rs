use std::sync::mpsc::Sender;

use super::ClientSideInput;

// Zoinked from the Linux kernel's own key code list
// include/uapi/linux/input-event-codes.h

const _KEY_RESERVED: u32 = 0;
const _KEY_ESC: u32 = 1;
const _KEY_1: u32 = 2;
const _KEY_2: u32 = 3;
const _KEY_3: u32 = 4;
const _KEY_4: u32 = 5;
const _KEY_5: u32 = 6;
const _KEY_6: u32 = 7;
const _KEY_7: u32 = 8;
const _KEY_8: u32 = 9;
const _KEY_9: u32 = 10;
const _KEY_0: u32 = 11;
const _KEY_MINUS: u32 = 12;
const _KEY_EQUAL: u32 = 13;
const _KEY_BACKSPACE: u32 = 14;
const _KEY_TAB: u32 = 15;
const _KEY_Q: u32 = 16;
const _KEY_W: u32 = 17;
const _KEY_E: u32 = 18;
const _KEY_R: u32 = 19;
const _KEY_T: u32 = 20;
const _KEY_Y: u32 = 21;
const _KEY_U: u32 = 22;
const _KEY_I: u32 = 23;
const _KEY_O: u32 = 24;
const _KEY_P: u32 = 25;
const _KEY_LEFTBRACE: u32 = 26;
const _KEY_RIGHTBRACE: u32 = 27;
const _KEY_ENTER: u32 = 28;
const KEY_LEFTCTRL: u32 = 29;
const _KEY_A: u32 = 30;
const _KEY_S: u32 = 31;
const _KEY_D: u32 = 32;
const _KEY_F: u32 = 33;
const _KEY_G: u32 = 34;
const _KEY_H: u32 = 35;
const _KEY_J: u32 = 36;
const _KEY_K: u32 = 37;
const _KEY_L: u32 = 38;
const _KEY_SEMICOLON: u32 = 39;
const _KEY_APOSTROPHE: u32 = 40;
const _KEY_GRAVE: u32 = 41;
const _KEY_LEFTSHIFT: u32 = 42;
const _KEY_BACKSLASH: u32 = 43;
const KEY_Z: u32 = 44;
const KEY_X: u32 = 45;
const _KEY_C: u32 = 46;
const _KEY_V: u32 = 47;
const _KEY_B: u32 = 48;
const _KEY_N: u32 = 49;
const _KEY_M: u32 = 50;
const _KEY_COMMA: u32 = 51;
const _KEY_DOT: u32 = 52;
const _KEY_SLASH: u32 = 53;
const _KEY_RIGHTSHIFT: u32 = 54;
const _KEY_KPASTERISK: u32 = 55;
const KEY_LEFTALT: u32 = 56;
const _KEY_SPACE: u32 = 57;
const _KEY_CAPSLOCK: u32 = 58;
const _KEY_F1: u32 = 59;
const _KEY_F2: u32 = 60;
const _KEY_F3: u32 = 61;
const _KEY_F4: u32 = 62;
const _KEY_F5: u32 = 63;
const _KEY_F6: u32 = 64;
const _KEY_F7: u32 = 65;
const _KEY_F8: u32 = 66;
const _KEY_F9: u32 = 67;
const _KEY_F10: u32 = 68;
const _KEY_NUMLOCK: u32 = 69;
const _KEY_SCROLLLOCK: u32 = 70;
const _KEY_KP7: u32 = 71;
const _KEY_KP8: u32 = 72;
const _KEY_KP9: u32 = 73;
const _KEY_KPMINUS: u32 = 74;
const _KEY_KP4: u32 = 75;
const _KEY_KP5: u32 = 76;
const _KEY_KP6: u32 = 77;
const _KEY_KPPLUS: u32 = 78;
const _KEY_KP1: u32 = 79;
const _KEY_KP2: u32 = 80;
const _KEY_KP3: u32 = 81;
const _KEY_KP0: u32 = 82;
const _KEY_KPDOT: u32 = 83;

const _KEY_ZENKAKUHANKAKU: u32 = 85;
const _KEY_102ND: u32 = 86;
const _KEY_F11: u32 = 87;
const _KEY_F12: u32 = 88;
const _KEY_RO: u32 = 89;
const _KEY_KATAKANA: u32 = 90;
const _KEY_HIRAGANA: u32 = 91;
const _KEY_HENKAN: u32 = 92;
const _KEY_KATAKANAHIRAGANA: u32 = 93;
const _KEY_MUHENKAN: u32 = 94;
const _KEY_KPJPCOMMA: u32 = 95;
const _KEY_KPENTER: u32 = 96;
const _KEY_RIGHTCTRL: u32 = 97;
const _KEY_KPSLASH: u32 = 98;
const _KEY_SYSRQ: u32 = 99;
const _KEY_RIGHTALT: u32 = 100;
const _KEY_LINEFEED: u32 = 101;
const _KEY_HOME: u32 = 102;
const _KEY_UP: u32 = 103;
const _KEY_PAGEUP: u32 = 104;
const _KEY_LEFT: u32 = 105;
const _KEY_RIGHT: u32 = 106;
const _KEY_END: u32 = 107;
const _KEY_DOWN: u32 = 108;
const _KEY_PAGEDOWN: u32 = 109;
const _KEY_INSERT: u32 = 110;
const _KEY_DELETE: u32 = 111;
const _KEY_MACRO: u32 = 112;
const _KEY_MUTE: u32 = 113;
const _KEY_VOLUMEDOWN: u32 = 114;
const _KEY_VOLUMEUP: u32 = 115;
const _KEY_POWER: u32 = 116;
const _KEY_KPEQUAL: u32 = 117;
const _KEY_KPPLUSMINUS: u32 = 118;
const _KEY_PAUSE: u32 = 119;
const _KEY_SCALE: u32 = 120;

const _KEY_KPCOMMA: u32 = 121;
const _KEY_HANGEUL: u32 = 122;
const _KEY_HANGUEL: u32 = _KEY_HANGEUL;
const _KEY_HANJA: u32 = 123;
const _KEY_YEN: u32 = 124;
const KEY_LEFTMETA: u32 = 125;
const _KEY_RIGHTMETA: u32 = 126;
const _KEY_COMPOSE: u32 = 127;

pub struct KeyCombiner {
    lctrl_pressed: bool,
    lalt_pressed: bool,
    lmeta_pressed: bool,
}

impl KeyCombiner {
    pub fn new() -> Self {
        Self {
            lctrl_pressed: false,
            lalt_pressed: false,
            lmeta_pressed: false,
        }
    }

    pub fn process(&mut self, tx: &Sender<ClientSideInput>, e: ClientSideInput) {
        match e {
            ClientSideInput::KeyDown(code) => {
                match code {
                    KEY_LEFTCTRL => {
                        self.lctrl_pressed = true;
                        let _ = tx.send(e);
                    },
                    KEY_LEFTALT => {
                        self.lalt_pressed = true;
                        let _ = tx.send(e);
                    },
                    KEY_LEFTMETA => {
                        self.lmeta_pressed = true;
                        let _ = tx.send(e);
                    },
                    KEY_Z => {
                        if self.lmeta_pressed {
                            let _ = tx.send(ClientSideInput::SpecialDebug {code: 1});
                        } else {
                            let _ = tx.send(e);
                        }
                    },
                    KEY_X => {
                        if self.lmeta_pressed {
                            let _ = tx.send(ClientSideInput::SpecialDebug {code: 2});
                        } else {
                            let _ = tx.send(e);
                        }                        
                    }
                    _ => {
                        let _ = tx.send(e);
                    }
                }
            },
            ClientSideInput::KeyUp(code) => {
                match code {
                    KEY_LEFTCTRL => {
                        self.lctrl_pressed = false;
                    },
                    KEY_LEFTALT => {
                        self.lalt_pressed = false;
                    },
                    KEY_LEFTMETA => {
                        self.lmeta_pressed = false;
                    },
                    _ => {}
                }
                let _ = tx.send(e);
            },
            _ => {
                let _ = tx.send(e);
            }
        }
    }
}