//! The kernel side of the HolyC AOT ABI.
//!
//! Anything marked `#[unsafe(link_section = ".hcapi")]` is placed at a fixed
//! address by linker.ld (0x68000). build.sh collects those symbols with `nm`,
//! and the host compiler (`hcc/`) resolves bare `Print`, `Snd`, `Snake`, ...
//! calls to them. Just like TempleOS: plain `CALL rel32`, no syscall, no
//! user/kernel split — an app can touch the framebuffer globals directly.

use crate::keyboard::{self, Key};
use crate::term;
use crate::{fb, sound};

unsafe fn cstr(mut p: *const u8) -> &'static str {
    let mut n = 0usize;
    while *p.add(n) != 0 {
        n += 1;
    }
    core::str::from_utf8(core::slice::from_raw_parts(p, n)).unwrap_or("")
}

// --- framebuffer globals, set by init_env() ---------------------------------

#[unsafe(no_mangle)]
#[unsafe(link_section = ".hcapi.d")]
pub static mut HC_FBADDR: u64 = 0;
#[unsafe(no_mangle)]
#[unsafe(link_section = ".hcapi.d")]
pub static mut HC_FBW: u32 = 0;
#[unsafe(no_mangle)]
#[unsafe(link_section = ".hcapi.d")]
pub static mut HC_FBH: u32 = 0;
#[unsafe(no_mangle)]
#[unsafe(link_section = ".hcapi.d")]
pub static mut HC_FBPITCH: u32 = 0;

pub fn init_env() {
    let bi = crate::boot_info();
    unsafe {
        HC_FBADDR = bi.fb_addr;
        HC_FBW = bi.fb_width;
        HC_FBH = bi.fb_height;
        HC_FBPITCH = bi.fb_pitch;
    }
}

// --- text / sound / input ---------------------------------------------------

#[unsafe(no_mangle)]
#[unsafe(link_section = ".hcapi")]
pub extern "C" fn HC_Print(fmt: u64, a0: u64, a1: u64, a2: u64, a3: u64, a4: u64) -> i64 {
    if fmt == 0 {
        return 0;
    }
    let f = unsafe { cstr(fmt as *const u8) };
    let args = [a0, a1, a2, a3, a4];
    let mut nx = 0usize; // next arg slot
    let mut n = 0i64; // substitution count
    let b = f.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        let c = b[i];
        if c == b'%' && i + 1 < b.len() {
            let spec = b[i + 1];
            i += 2;
            if spec == b'%' {
                term::put_char(b'%');
                continue;
            }
            let v = args[nx.min(4)];
            nx = (nx + 1).min(5);
            n += 1;
            match spec {
                b'd' | b'i' => term::print_num(v as i64),
                b'x' => term::print_hex64(v as u64),
                b'X' => term::print_hex32(v as u32),
                b'c' => term::put_char(v as u8),
                b's' => {
                    let s = unsafe { cstr(v as *const u8) };
                    term::print_str(s);
                }
                other => {
                    term::put_char(b'%');
                    term::put_char(other);
                }
            }
        } else {
            term::put_char(c);
            i += 1;
        }
    }
    n
}

#[unsafe(no_mangle)]
#[unsafe(link_section = ".hcapi")]
pub extern "C" fn HC_Putc(c: u64) -> i64 {
    term::put_char(c as u8);
    0
}

#[unsafe(no_mangle)]
#[unsafe(link_section = ".hcapi")]
pub extern "C" fn HC_Cls() -> i64 {
    term::clear();
    0
}

#[unsafe(no_mangle)]
#[unsafe(link_section = ".hcapi")]
pub extern "C" fn HC_Beep() -> i64 {
    let tsc = crate::shell::tsc_per_ms();
    sound::beep(500, 150, tsc);
    0
}

#[unsafe(no_mangle)]
#[unsafe(link_section = ".hcapi")]
pub extern "C" fn HC_Snd(freq: i64) -> i64 {
    if freq <= 0 {
        return -1;
    }
    let tsc = crate::shell::tsc_per_ms();
    sound::beep(freq as u32, 120, tsc);
    0
}

#[unsafe(no_mangle)]
#[unsafe(link_section = ".hcapi")]
pub extern "C" fn HC_Sleep(ms: i64) -> i64 {
    if ms <= 0 {
        return -1;
    }
    let per = crate::shell::tsc_per_ms();
    let end = crate::shell::now() + (ms as u64).saturating_mul(per);
    while crate::shell::now() < end {
        core::hint::spin_loop();
    }
    0
}

/// Scancode-free key probe: -1 = none, else the byte value
/// (0x100.. = special keys).
#[unsafe(no_mangle)]
#[unsafe(link_section = ".hcapi")]
pub extern "C" fn HC_KEY() -> i64 {
    if let Some(k) = keyboard::poll() {
        return match k {
            Key::Char(c) => c as i64,
            Key::Enter => b'\n' as i64,
            Key::Backspace => 8,
            Key::Tab => 9,
            Key::Esc => 27,
            Key::Delete => 0x1_00,
            Key::Up => 0x1_01,
            Key::Down => 0x1_02,
            Key::Left => 0x1_03,
            Key::Right => 0x1_04,
            Key::Home => 0x1_05,
            Key::End => 0x1_06,
            Key::PageUp => 0x1_07,
            Key::PageDown => 0x1_08,
            Key::F(n) => 0x2_00 + n as i64,
            Key::Ctrl(c) => 0x3_00 + c as i64,
        };
    }
    -1
}

/// Wait for a keypress, returns the same value as HC_KEY.
#[unsafe(no_mangle)]
#[unsafe(link_section = ".hcapi")]
pub extern "C" fn HC_WaitKey() -> i64 {
    let mut r = -1;
    while r < 0 {
        r = HC_KEY();
        core::hint::spin_loop();
    }
    r
}

// --- files ------------------------------------------------------------------

#[unsafe(no_mangle)]
#[unsafe(link_section = ".hcapi")]
pub extern "C" fn HC_Dir(path: u64) -> i64 {
    let s = if path == 0 {
        ""
    } else {
        unsafe { cstr(path as *const u8) }
    };
    crate::shell::cmd_dir(s);
    0
}

#[unsafe(no_mangle)]
#[unsafe(link_section = ".hcapi")]
pub extern "C" fn HC_Type(path: u64) -> i64 {
    let s = if path == 0 {
        ""
    } else {
        unsafe { cstr(path as *const u8) }
    };
    crate::shell::cmd_type(s);
    0
}

#[unsafe(no_mangle)]
#[unsafe(link_section = ".hcapi")]
pub extern "C" fn HC_Snake() -> i64 {
    crate::shell::snake();
    0
}

#[unsafe(no_mangle)]
#[unsafe(link_section = ".hcapi")]
pub extern "C" fn HC_Life() -> i64 {
    crate::shell::demo_life();
    0
}

#[unsafe(no_mangle)]
#[unsafe(link_section = ".hcapi")]
pub extern "C" fn HC_Lines() -> i64 {
    crate::shell::demo_lines();
    0
}

// --- framebuffer ------------------------------------------------------------

#[unsafe(no_mangle)]
#[unsafe(link_section = ".hcapi")]
pub extern "C" fn HC_Pixel(x: i64, y: i64, color: i64) -> i64 {
    if x < 0 || y < 0 {
        return -1;
    }
    fb::fill_rect(
        x as usize,
        y as usize,
        1,
        1,
        color as u8,
    );
    0
}

#[unsafe(no_mangle)]
#[unsafe(link_section = ".hcapi")]
pub extern "C" fn HC_Fill(x: i64, y: i64, w: i64, h: i64, color: i64) -> i64 {
    if w < 0 || h < 0 {
        return -1;
    }
    fb::fill_rect(x as usize, y as usize, w as usize, h as usize, color as u8);
    0
}

#[unsafe(no_mangle)]
#[unsafe(link_section = ".hcapi")]
pub extern "C" fn HC_Glyph(col: i64, row: i64, ch: i64, color: i64) -> i64 {
    fb::draw_glyph(
        col as usize,
        row as usize,
        ch as u8,
        color as u8,
    );
    0
}
