//! Interrupt-driven PS/2 keyboard driver. Scancode set 1, US layout.
//!
//! IRQ 1 pushes raw bytes into `int`'s ring buffer; `poll()` consumes and
//! translates them here. Keystrokes are timestamped by their delivery, so
//! clicking a key never waits for the frame loop.

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Char(u8),
    Enter,
    Backspace,
    Tab,
    Esc,
    Delete,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    F(u8),
    Ctrl(u8),
}

struct State {
    shift: bool,
    caps: bool,
    ctrl: bool,
    alt: bool,
    extended: bool,
}

static mut STATE: State = State {
    shift: false,
    caps: false,
    ctrl: false,
    alt: false,
    extended: false,
};

fn st() -> *mut State {
    core::ptr::addr_of_mut!(STATE)
}

#[allow(dead_code)]
pub fn is_ctrl() -> bool {
    unsafe { (*st()).ctrl }
}

#[allow(dead_code)]
pub fn is_alt() -> bool {
    unsafe { (*st()).alt }
}

#[allow(dead_code)]
pub fn is_shift() -> bool {
    unsafe { (*st()).shift }
}

/// (scancode, unshifted, shifted)
const MAP: [(u8, u8, u8); 48] = [
    (0x02, b'1', b'!'),
    (0x03, b'2', b'@'),
    (0x04, b'3', b'#'),
    (0x05, b'4', b'$'),
    (0x06, b'5', b'%'),
    (0x07, b'6', b'^'),
    (0x08, b'7', b'&'),
    (0x09, b'8', b'*'),
    (0x0a, b'9', b'('),
    (0x0b, b'0', b')'),
    (0x0c, b'-', b'_'),
    (0x0d, b'=', b'+'),
    (0x10, b'q', b'Q'),
    (0x11, b'w', b'W'),
    (0x12, b'e', b'E'),
    (0x13, b'r', b'R'),
    (0x14, b't', b'T'),
    (0x15, b'y', b'Y'),
    (0x16, b'u', b'U'),
    (0x17, b'i', b'I'),
    (0x18, b'o', b'O'),
    (0x19, b'p', b'P'),
    (0x1a, b'[', b'{'),
    (0x1b, b']', b'}'),
    (0x1e, b'a', b'A'),
    (0x1f, b's', b'S'),
    (0x20, b'd', b'D'),
    (0x21, b'f', b'F'),
    (0x22, b'g', b'G'),
    (0x23, b'h', b'H'),
    (0x24, b'j', b'J'),
    (0x25, b'k', b'K'),
    (0x26, b'l', b'L'),
    (0x27, b';', b':'),
    (0x28, b'\'', b'"'),
    (0x29, b'`', b'~'),
    (0x2b, b'\\', b'|'),
    (0x2c, b'z', b'Z'),
    (0x2d, b'x', b'X'),
    (0x2e, b'c', b'C'),
    (0x2f, b'v', b'V'),
    (0x30, b'b', b'B'),
    (0x31, b'n', b'N'),
    (0x32, b'm', b'M'),
    (0x33, b',', b'<'),
    (0x34, b'.', b'>'),
    (0x35, b'/', b'?'),
    (0x39, b' ', b' '),
];

fn map(sc: u8, shift: bool) -> Option<u8> {
    for &(code, plain, shifted) in MAP.iter() {
        if code == sc {
            return Some(if shift { shifted } else { plain });
        }
    }
    None
}

pub fn poll() -> Option<Key> {
    let sc = crate::int::kb_pop()?;
    unsafe {
        let p = st();

        if sc == 0xe0 {
            (*p).extended = true;
            return None;
        }
        if sc == 0xe1 {
            return None; // pause key, eat it
        }
        let ext = (*p).extended;
        (*p).extended = false;

        if sc & 0x80 != 0 {
            let make = sc & 0x7f;
            if make == 0x2a || make == 0x36 {
                (*p).shift = false;
            } else if make == 0x1d {
                (*p).ctrl = false;
            } else if make == 0x38 {
                (*p).alt = false;
            }
            return None;
        }

        match sc {
            0x2a | 0x36 => {
                (*p).shift = true;
                return None;
            }
            0x1d => {
                (*p).ctrl = true;
                return None;
            }
            0x38 => {
                (*p).alt = true;
                return None;
            }
            0x3a => {
                (*p).caps = !(*p).caps;
                return None;
            }
            0x1c => return Some(Key::Enter),
            0x0e => return Some(Key::Backspace),
            0x0f => return Some(Key::Tab),
            0x01 => return Some(Key::Esc),
            0x3b..=0x44 => return Some(Key::F(sc - 0x3b + 1)),
            0x57 => return Some(Key::F(11)),
            0x58 => return Some(Key::F(12)),
            _ => {}
        }

        if ext {
            return match sc {
                0x48 => Some(Key::Up),
                0x50 => Some(Key::Down),
                0x4b => Some(Key::Left),
                0x4d => Some(Key::Right),
                0x47 => Some(Key::Home),
                0x4f => Some(Key::End),
                0x49 => Some(Key::PageUp),
                0x51 => Some(Key::PageDown),
                0x53 => Some(Key::Delete),
                _ => None,
            };
        }

        let mut c = map(sc, (*p).shift)?;
        if (*p).ctrl {
            let lower = c.to_ascii_lowercase();
            return Some(Key::Ctrl(lower));
        }
        if (*p).caps {
            if c.is_ascii_lowercase() {
                c = c.to_ascii_uppercase();
            } else if c.is_ascii_uppercase() {
                c = c.to_ascii_lowercase();
            }
        }
        Some(Key::Char(c))
    }
}
