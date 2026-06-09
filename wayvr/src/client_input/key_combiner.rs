use std::sync::mpsc::Sender;

use super::ClientSideInput;

// Zoinked from the Linux kernel's own key code list
// include/uapi/linux/input-event-codes.h
const KEY_RESERVED: u32 = 0;
const KEY_ESC: u32 = 1;
const KEY_1: u32 = 2;
const KEY_2: u32 = 3;
const KEY_3: u32 = 4;
const KEY_4: u32 = 5;
const KEY_5: u32 = 6;
const KEY_6: u32 = 7;
const KEY_7: u32 = 8;
const KEY_8: u32 = 9;
const KEY_9: u32 = 10;
const KEY_0: u32 = 11;
const KEY_MINUS: u32 = 12;
const KEY_EQUAL: u32 = 13;
const KEY_BACKSPACE: u32 = 14;
const KEY_TAB: u32 = 15;
const KEY_Q: u32 = 16;
const KEY_W: u32 = 17;
const KEY_E: u32 = 18;
const KEY_R: u32 = 19;
const KEY_T: u32 = 20;
const KEY_Y: u32 = 21;
const KEY_U: u32 = 22;
const KEY_I: u32 = 23;
const KEY_O: u32 = 24;
const KEY_P: u32 = 25;
const KEY_LEFTBRACE: u32 = 26;
const KEY_RIGHTBRACE: u32 = 27;
const KEY_ENTER: u32 = 28;
const KEY_LEFTCTRL: u32 = 29;
const KEY_A: u32 = 30;
const KEY_S: u32 = 31;
const KEY_D: u32 = 32;
const KEY_F: u32 = 33;
const KEY_G: u32 = 34;
const KEY_H: u32 = 35;
const KEY_J: u32 = 36;
const KEY_K: u32 = 37;
const KEY_L: u32 = 38;
const KEY_SEMICOLON: u32 = 39;
const KEY_APOSTROPHE: u32 = 40;
const KEY_GRAVE: u32 = 41;
const KEY_LEFTSHIFT: u32 = 42;
const KEY_BACKSLASH: u32 = 43;
const KEY_Z: u32 = 44;
const KEY_X: u32 = 45;
const KEY_C: u32 = 46;
const KEY_V: u32 = 47;
const KEY_B: u32 = 48;
const KEY_N: u32 = 49;
const KEY_M: u32 = 50;
const KEY_COMMA: u32 = 51;
const KEY_DOT: u32 = 52;
const KEY_SLASH: u32 = 53;
const KEY_RIGHTSHIFT: u32 = 54;
const KEY_KPASTERISK: u32 = 55;
const KEY_LEFTALT: u32 = 56;
const KEY_SPACE: u32 = 57;
const KEY_CAPSLOCK: u32 = 58;
const KEY_F1: u32 = 59;
const KEY_F2: u32 = 60;
const KEY_F3: u32 = 61;
const KEY_F4: u32 = 62;
const KEY_F5: u32 = 63;
const KEY_F6: u32 = 64;
const KEY_F7: u32 = 65;
const KEY_F8: u32 = 66;
const KEY_F9: u32 = 67;
const KEY_F10: u32 = 68;
const KEY_NUMLOCK: u32 = 69;
const KEY_SCROLLLOCK: u32 = 70;
const KEY_KP7: u32 = 71;
const KEY_KP8: u32 = 72;
const KEY_KP9: u32 = 73;
const KEY_KPMINUS: u32 = 74;
const KEY_KP4: u32 = 75;
const KEY_KP5: u32 = 76;
const KEY_KP6: u32 = 77;
const KEY_KPPLUS: u32 = 78;
const KEY_KP1: u32 = 79;
const KEY_KP2: u32 = 80;
const KEY_KP3: u32 = 81;
const KEY_KP0: u32 = 82;
const KEY_KPDOT: u32 = 83;

const KEY_ZENKAKUHANKAKU: u32 = 85;
const KEY_102ND: u32 = 86;
const KEY_F11: u32 = 87;
const KEY_F12: u32 = 88;
const KEY_RO: u32 = 89;
const KEY_KATAKANA: u32 = 90;
const KEY_HIRAGANA: u32 = 91;
const KEY_HENKAN: u32 = 92;
const KEY_KATAKANAHIRAGANA: u32 = 93;
const KEY_MUHENKAN: u32 = 94;
const KEY_KPJPCOMMA: u32 = 95;
const KEY_KPENTER: u32 = 96;
const KEY_RIGHTCTRL: u32 = 97;
const KEY_KPSLASH: u32 = 98;
const KEY_SYSRQ: u32 = 99;
const KEY_RIGHTALT: u32 = 100;
const KEY_LINEFEED: u32 = 101;
const KEY_HOME: u32 = 102;
const KEY_UP: u32 = 103;
const KEY_PAGEUP: u32 = 104;
const KEY_LEFT: u32 = 105;
const KEY_RIGHT: u32 = 106;
const KEY_END: u32 = 107;
const KEY_DOWN: u32 = 108;
const KEY_PAGEDOWN: u32 = 109;
const KEY_INSERT: u32 = 110;
const KEY_DELETE: u32 = 111;
const KEY_MACRO: u32 = 112;
const KEY_MUTE: u32 = 113;
const KEY_VOLUMEDOWN: u32 = 114;
const KEY_VOLUMEUP: u32 = 115;
const KEY_POWER: u32 = 116;
const KEY_KPEQUAL: u32 = 117;
const KEY_KPPLUSMINUS: u32 = 118;
const KEY_PAUSE: u32 = 119;
const KEY_SCALE: u32 = 120;

const KEY_KPCOMMA: u32 = 121;
const KEY_HANGEUL: u32 = 122;
const KEY_HANGUEL: u32 = KEY_HANGEUL;
const KEY_HANJA: u32 = 123;
const KEY_YEN: u32 = 124;
const KEY_LEFTMETA: u32 = 125;
const KEY_RIGHTMETA: u32 = 126;
const KEY_COMPOSE: u32 = 127;

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