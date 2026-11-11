//! Live System Monitor (SysMon) — a movable WM window.
//!
//! All numbers on this dashboard are measured at runtime — IRQ counters,
//! TSC-derived uptime/clock, the real frame-rate meter — nothing here is a
//! hardcoded prop. The window redraws its dynamic fields at the OS frame
//! rate; the desktop loop idles in `hlt` between frames.

use crate::keyboard::Key;
use crate::sound;
use crate::term::{self, Cell};

const WIN_W: usize = 61;
const WIN_H: usize = 23;
/// value column
const VX: usize = 26;
/// width of the value fields (kept clear of stale digits)
const FW: usize = 20;

// --- WM window interface ------------------------------------------------------

static mut BUF: [Cell; WIN_W * WIN_H] = [Cell {
    ch: b' ',
    fg: 0,
    bg: 0,
}; WIN_W * WIN_H];

pub fn win_size() -> (usize, usize) {
    (WIN_W, WIN_H)
}

pub fn win_buffer() -> *mut Cell {
    core::ptr::addr_of_mut!(BUF) as *mut Cell
}

/// Legacy entry point (menu / REPL command): open as a WM window.
pub fn run() {
    crate::wm::open(crate::wm::Kind::SysMon);
}

pub fn win_open() {
    draw_chrome();
    update(); // paint all fields once so the first blit is complete
}

/// One frame: refresh the live fields into the window buffer.
/// Returns true when the content changed (needs a reblit).
pub fn win_frame(
    focused: bool,
    key: Option<Key>,
    click: Option<(usize, usize)>,
) -> bool {
    let mut beeped = false;
    if focused {
        match key {
            Some(Key::Esc) | Some(Key::Char(b'q')) | Some(Key::Char(b'Q')) => {
                return false; // close window
            }
            Some(Key::Char(b'b')) | Some(Key::Char(b'B')) => {
                sound::beep(600, 100, crate::shell::tsc_per_ms());
                beeped = true;
            }
            _ => {}
        }
        if let Some((col, row)) = click {
            let btn_y = WIN_H - 3;
            if row == btn_y {
                if col >= 4 && col <= 16 && !beeped {
                    sound::beep(600, 100, crate::shell::tsc_per_ms());
                } else if col >= 20 && col <= 34 {
                    sound::play(
                        "O4 L4 E E F G G F E D C C D E E. D8 D2",
                        crate::shell::tsc_per_ms(),
                    );
                }
            }
        }
    }

    update();
    true // uptime/counters tick every frame
}

// --- small text helpers -------------------------------------------------------

/// Draw `text` at (x,y) and blank the rest of a `width`-wide field so
/// shrinking values (1234 -> 56) don't leave stale digits behind.
fn draw_field(x: usize, y: usize, text: &str, fg: u8, bg: u8, width: usize) {
    term::draw_text_at_colored(x, y, text, fg, bg);
    for i in text.len()..width {
        term::put_at_colored(x + i, y, b' ', fg, bg);
    }
}

/// decimal digits of `n` into `buf`, return length
fn push_num(buf: &mut [u8], mut n: u64) -> usize {
    let mut tmp = [0u8; 10];
    let mut d = 0;
    if n == 0 {
        tmp[0] = b'0';
        d = 1;
    }
    while n > 0 {
        tmp[d] = b'0' + (n % 10) as u8;
        n /= 10;
        d += 1;
    }
    for j in 0..d {
        buf[j] = tmp[d - 1 - j];
    }
    d
}

fn push_2digit(buf: &mut [u8], n: u64) -> usize {
    buf[0] = b'0' + (n / 10) as u8;
    buf[1] = b'0' + (n % 10) as u8;
    2
}

// --- live field painting ------------------------------------------------------

fn update() {
    let bg = term::theme_bg();
    let live = term::LIGHT_GREEN;

    // uptime: H:MM:SS
    let secs = crate::shell::uptime_ms() / 1000;
    let mut ub = [0u8; 12];
    let mut un = 0usize;
    un += push_num(&mut ub[un..], (secs / 3600) % 1000);
    ub[un] = b':';
    un += 1;
    un += push_2digit(&mut ub[un..], (secs / 60) % 60);
    ub[un] = b':';
    un += 1;
    un += push_2digit(&mut ub[un..], secs % 60);
    draw_field(VX, 3, core::str::from_utf8(&ub[..un]).unwrap_or("?"), live, bg, FW);

    // logical CPUs
    let mut cb = [0u8; 10];
    let cn = push_num(&mut cb, term::logical_cpus() as u64);
    draw_field(VX, 4, core::str::from_utf8(&cb[..cn]).unwrap_or("?"), live, bg, FW);

    // frame cap / measured fps / loop busy / TSC MHz
    let mut fc = [0u8; 10];
    let fn5 = push_num(&mut fc, term::fps_cap());
    draw_field(VX, 5, core::str::from_utf8(&fc[..fn5]).unwrap_or("?"), live, bg, FW);

    let mut fb = [0u8; 10];
    let fn6 = push_num(&mut fb, term::fps());
    draw_field(VX, 6, core::str::from_utf8(&fb[..fn6]).unwrap_or("?"), live, bg, FW);

    let mut bb = [0u8; 10];
    let bn = push_num(&mut bb, term::frame_busy());
    let mut busy = [0u8; 16];
    busy[..bn].copy_from_slice(&bb[..bn]);
    busy[bn] = b'%';
    draw_field(VX, 7, core::str::from_utf8(&busy[..bn + 1]).unwrap_or("?"), live, bg, FW);

    let mut tb = [0u8; 10];
    let tn = push_num(&mut tb, crate::shell::tsc_per_ms() / 1000);
    let mut mhz = [0u8; 16];
    mhz[..tn].copy_from_slice(&tb[..tn]);
    mhz[tn] = b' ';
    mhz[tn + 1] = b'M';
    mhz[tn + 2] = b'H';
    mhz[tn + 3] = b'z';
    draw_field(VX, 8, core::str::from_utf8(&mhz[..tn + 4]).unwrap_or("?"), live, bg, FW);

    // interrupt activity
    let (pit, kbd, mse) = crate::int::irq_stats();
    let mut pb = [0u8; 10];
    let pn = push_num(&mut pb, pit as u64);
    draw_field(VX, 10, core::str::from_utf8(&pb[..pn]).unwrap_or("?"), live, bg, FW);

    let mut kb = [0u8; 10];
    let kn = push_num(&mut kb, kbd as u64);
    draw_field(VX, 11, core::str::from_utf8(&kb[..kn]).unwrap_or("?"), live, bg, FW);

    let mut mb = [0u8; 10];
    let mn = push_num(&mut mb, mse as u64);
    draw_field(VX, 12, core::str::from_utf8(&mb[..mn]).unwrap_or("?"), live, bg, FW);

    // video mode: WxHxBPP
    let (w, h) = (crate::fb::width(), crate::fb::height());
    let bpp = crate::fb::bpp();
    let pitch = crate::fb::pitch();
    let mut vb = [0u8; 20];
    let mut vn = 0usize;
    vn += push_num(&mut vb[vn..], w as u64);
    vb[vn] = b'x';
    vn += 1;
    vn += push_num(&mut vb[vn..], h as u64);
    vb[vn] = b'x';
    vn += 1;
    vn += push_num(&mut vb[vn..], bpp as u64);
    draw_field(VX, 14, core::str::from_utf8(&vb[..vn]).unwrap_or("?"), live, bg, FW);

    // kernel / bss / vram in KiB
    let kern = crate::boot_info().kernel_bytes as u64 / 1024;
    let bss = crate::bss_size() as u64 / 1024;
    let vram = (w * pitch / 1024) as u64;
    let mut mem = [0u8; 40];
    let mut mn2 = 0usize;
    mn2 += push_num(&mut mem[mn2..], kern);
    mem[mn2] = b'K';
    mn2 += 1;
    mn2 += push_num(&mut mem[mn2..], bss);
    mem[mn2] = b'K';
    mn2 += 1;
    mn2 += push_num(&mut mem[mn2..], vram);
    mem[mn2] = b'K';
    mn2 += 1;
    draw_field(VX, 15, core::str::from_utf8(&mem[..mn2]).unwrap_or("?"), live, bg, FW);

    // live frame-rate bar: 20 cells, filled = measured/cap
    let cap = term::fps_cap();
    let f = term::fps().min(cap);
    let filled = (f * 20 / cap) as usize;
    for x in 0..20 {
        term::put_at_colored(
            VX + x,
            17,
            if x < filled { 0xdb } else { b' ' },
            if x < filled { term::GREEN } else { term::DARK_GRAY },
            bg,
        );
    }
}

fn draw_chrome() {
    let bg = term::theme_bg();
    let border = term::LIGHT_CYAN;
    let dim = term::LIGHT_GRAY;

    // window border (title bar above is drawn by the WM)
    term::put_at_colored(0, 0, 0xda, border, bg);
    for x in 1..WIN_W - 1 {
        term::put_at_colored(x, 0, 0xc4, border, bg);
    }
    term::put_at_colored(WIN_W - 1, 0, 0xbf, border, bg);

    for y in 1..WIN_H - 1 {
        term::put_at_colored(0, y, 0xb3, border, bg);
        for x in 1..WIN_W - 1 {
            term::put_at_colored(x, y, b' ', term::WHITE, bg);
        }
        term::put_at_colored(WIN_W - 1, y, 0xb3, border, bg);
    }

    term::put_at_colored(0, WIN_H - 1, 0xc0, border, bg);
    for x in 1..WIN_W - 1 {
        term::put_at_colored(x, WIN_H - 1, 0xc4, border, bg);
    }
    term::put_at_colored(WIN_W - 1, WIN_H - 1, 0xd9, border, bg);

    // Section 1: CPU & main loop
    term::draw_text_at_colored(3, 2, "CPU & Main Loop", term::YELLOW, bg);
    term::draw_text_at_colored(5, 3, "Uptime:", dim, bg);
    term::draw_text_at_colored(5, 4, "Logical CPUs:", dim, bg);
    term::draw_text_at_colored(5, 5, "Frame cap (Fps;):", dim, bg);
    term::draw_text_at_colored(5, 6, "Measured FPS:", dim, bg);
    term::draw_text_at_colored(5, 7, "Loop busy:", dim, bg);
    term::draw_text_at_colored(5, 8, "TSC clock:", dim, bg);

    // Section 2: interrupt activity
    term::draw_text_at_colored(3, 9, "Interrupts (since boot)", term::YELLOW, bg);
    term::draw_text_at_colored(5, 10, "PIT ticks (IRQ0):", dim, bg);
    term::draw_text_at_colored(5, 11, "Key bytes (IRQ1):", dim, bg);
    term::draw_text_at_colored(5, 12, "Mouse bytes (polled):", dim, bg);

    // Section 3: video & memory
    term::draw_text_at_colored(3, 13, "Video & Memory", term::YELLOW, bg);
    term::draw_text_at_colored(5, 14, "Mode:", dim, bg);
    term::draw_text_at_colored(5, 15, "Kernel/BSS/VRAM KiB:", dim, bg);

    // Section 4: live frame-rate bar
    term::draw_text_at_colored(5, 17, "Frames:", dim, bg);
    term::draw_text_at_colored(VX + 21, 17, "/sec", term::DARK_GRAY, bg);

    // Section 5: interactive buttons
    let btn_y = WIN_H - 3;
    term::draw_text_at_colored(4, btn_y, "[ Beep Sound ]", term::BLACK, term::YELLOW);
    term::draw_text_at_colored(20, btn_y, "[ Play Melody ]", term::BLACK, term::GREEN);
    term::draw_text_at_colored(40, btn_y, "Esc: close", dim, bg);
}
