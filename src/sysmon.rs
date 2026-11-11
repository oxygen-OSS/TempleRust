//! Live System Monitor (SysMon).
//!
//! All numbers on this dashboard are measured at runtime — IRQ counters,
//! TSC-derived uptime/clock, the real frame-rate meter — nothing here is a
//! hardcoded prop. The window repaints its dynamic fields at the OS frame
//! rate and the loop idles in `hlt` between frames.

use crate::keyboard::{self, Key};
use crate::mouse;
use crate::sound;
use crate::term;

const SM_TOP: usize = 3;
const SM_BOTTOM: usize = 25;
const SM_LEFT: usize = 10;
const SM_RIGHT: usize = 70;
/// value column
const VX: usize = SM_LEFT + 26;
/// width of the value fields (kept clear of stale digits)
const FW: usize = 20;

pub fn run() {
    term::clear();
    draw_chrome();

    loop {
        if let Some(key) = keyboard::poll() {
            match key {
                Key::Esc | Key::Char(b'q') | Key::Char(b'Q') => break,
                Key::Char(b'b') | Key::Char(b'B') => {
                    sound::beep(600, 100, crate::shell::tsc_per_ms());
                }
                _ => {}
            }
        }

        if let Some(mev) = mouse::poll() {
            if mev.left {
                let cw = crate::fb::cell_w().max(1);
                let ch = crate::fb::cell_h().max(1);
                let col = mev.x / cw;
                let row = mev.y / ch;

                // Close button [X]
                if row == SM_TOP && col >= SM_RIGHT - 4 {
                    break;
                }

                // Check button row
                if row == SM_BOTTOM - 2 {
                    if col >= SM_LEFT + 4 && col <= SM_LEFT + 16 {
                        sound::beep(600, 100, crate::shell::tsc_per_ms());
                    } else if col >= SM_LEFT + 20 && col <= SM_LEFT + 36 {
                        sound::play("O4 L4 E E F G G F E D C C D E E. D8 D2", crate::shell::tsc_per_ms());
                    } else if col >= SM_LEFT + 40 && col <= SM_LEFT + 52 {
                        break;
                    }
                }
            }
        }

        update();
        term::frame_wait();
    }

    term::clear();
    term::status_bar();
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
    draw_field(VX, SM_TOP + 3, core::str::from_utf8(&ub[..un]).unwrap_or("?"), live, bg, FW);

    // logical CPUs
    let mut cb = [0u8; 10];
    let cn = push_num(&mut cb, term::logical_cpus() as u64);
    draw_field(VX, SM_TOP + 4, core::str::from_utf8(&cb[..cn]).unwrap_or("?"), live, bg, FW);

    // frame cap / measured fps / loop busy / TSC MHz
    let mut fb4 = [0u8; 10];
    let fn4 = push_num(&mut fb4, term::fps_cap());
    draw_field(VX, SM_TOP + 5, core::str::from_utf8(&fb4[..fn4]).unwrap_or("?"), live, bg, FW);

    let mut fb5 = [0u8; 10];
    let fn5 = push_num(&mut fb5, term::fps());
    draw_field(VX, SM_TOP + 6, core::str::from_utf8(&fb5[..fn5]).unwrap_or("?"), live, bg, FW);

    let mut bb = [0u8; 10];
    let bn = push_num(&mut bb, term::frame_busy());
    let mut busy = [0u8; 16];
    let mut b = bn;
    busy[..bn].copy_from_slice(&bb[..bn]);
    busy[b] = b'%';
    b += 1;
    draw_field(VX, SM_TOP + 7, core::str::from_utf8(&busy[..b]).unwrap_or("?"), live, bg, FW);

    let mut tb = [0u8; 10];
    let tn = push_num(&mut tb, crate::shell::tsc_per_ms() / 1000);
    let mut mhz = [0u8; 16];
    mhz[..tn].copy_from_slice(&tb[..tn]);
    let mut m = tn;
    mhz[m] = b' ';
    m += 1;
    mhz[m] = b'M';
    m += 1;
    mhz[m] = b'H';
    m += 1;
    mhz[m] = b'z';
    m += 1;
    draw_field(VX, SM_TOP + 8, core::str::from_utf8(&mhz[..m]).unwrap_or("?"), live, bg, FW);

    // interrupt activity
    let (pit, kbd, mse) = crate::int::irq_stats();
    let mut pb = [0u8; 10];
    let pn = push_num(&mut pb, pit as u64);
    draw_field(VX, SM_TOP + 10, core::str::from_utf8(&pb[..pn]).unwrap_or("?"), live, bg, FW);

    let mut kb = [0u8; 10];
    let kn = push_num(&mut kb, kbd as u64);
    draw_field(VX, SM_TOP + 11, core::str::from_utf8(&kb[..kn]).unwrap_or("?"), live, bg, FW);

    let mut mb = [0u8; 10];
    let mn = push_num(&mut mb, mse as u64);
    draw_field(VX, SM_TOP + 12, core::str::from_utf8(&mb[..mn]).unwrap_or("?"), live, bg, FW);

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
    draw_field(VX, SM_TOP + 14, core::str::from_utf8(&vb[..vn]).unwrap_or("?"), live, bg, FW);

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
    draw_field(VX, SM_TOP + 15, core::str::from_utf8(&mem[..mn2]).unwrap_or("?"), live, bg, FW);

    // live frame-rate bar: 20 cells, filled = measured/cap
    let cap = term::fps_cap();
    let f = term::fps().min(cap);
    let filled = (f * 20 / cap) as usize;
    for x in 0..20 {
        term::put_at_colored(
            VX + x,
            SM_TOP + 17,
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

    // Window border
    term::put_at_colored(SM_LEFT, SM_TOP, 0xda, border, bg);
    for x in SM_LEFT + 1..SM_RIGHT {
        term::put_at_colored(x, SM_TOP, 0xc4, border, bg);
    }
    term::put_at_colored(SM_RIGHT, SM_TOP, 0xbf, border, bg);

    let title = " [ System Monitor (SysMon) ] ";
    term::draw_text_at_colored(SM_LEFT + 3, SM_TOP, title, term::YELLOW, bg);
    term::draw_text_at_colored(SM_RIGHT - 4, SM_TOP, "[X]", term::LIGHT_RED, bg);

    for y in SM_TOP + 1..SM_BOTTOM {
        term::put_at_colored(SM_LEFT, y, 0xb3, border, bg);
        for x in SM_LEFT + 1..SM_RIGHT {
            term::put_at_colored(x, y, b' ', term::WHITE, bg);
        }
        term::put_at_colored(SM_RIGHT, y, 0xb3, border, bg);
    }

    term::put_at_colored(SM_LEFT, SM_BOTTOM, 0xc0, border, bg);
    for x in SM_LEFT + 1..SM_RIGHT {
        term::put_at_colored(x, SM_BOTTOM, 0xc4, border, bg);
    }
    term::put_at_colored(SM_RIGHT, SM_BOTTOM, 0xd9, border, bg);

    // Section 1: CPU & main loop
    term::draw_text_at_colored(SM_LEFT + 3, SM_TOP + 2, "CPU & Main Loop", term::YELLOW, bg);
    term::draw_text_at_colored(SM_LEFT + 5, SM_TOP + 3, "Uptime:", dim, bg);
    term::draw_text_at_colored(SM_LEFT + 5, SM_TOP + 4, "Logical CPUs:", dim, bg);
    term::draw_text_at_colored(SM_LEFT + 5, SM_TOP + 5, "Frame cap (Fps;):", dim, bg);
    term::draw_text_at_colored(SM_LEFT + 5, SM_TOP + 6, "Measured FPS:", dim, bg);
    term::draw_text_at_colored(SM_LEFT + 5, SM_TOP + 7, "Loop busy:", dim, bg);
    term::draw_text_at_colored(SM_LEFT + 5, SM_TOP + 8, "TSC clock:", dim, bg);

    // Section 2: interrupt activity
    term::draw_text_at_colored(SM_LEFT + 3, SM_TOP + 9, "Interrupts (since boot)", term::YELLOW, bg);
    term::draw_text_at_colored(SM_LEFT + 5, SM_TOP + 10, "PIT ticks (IRQ0):", dim, bg);
    term::draw_text_at_colored(SM_LEFT + 5, SM_TOP + 11, "Key bytes (IRQ1):", dim, bg);
    term::draw_text_at_colored(SM_LEFT + 5, SM_TOP + 12, "Mouse bytes (IRQ12):", dim, bg);

    // Section 3: video & memory
    term::draw_text_at_colored(SM_LEFT + 3, SM_TOP + 13, "Video & Memory", term::YELLOW, bg);
    term::draw_text_at_colored(SM_LEFT + 5, SM_TOP + 14, "Mode:", dim, bg);
    term::draw_text_at_colored(SM_LEFT + 5, SM_TOP + 15, "Kernel/BSS/VRAM KiB:", dim, bg);

    // Section 4: live frame-rate bar
    term::draw_text_at_colored(SM_LEFT + 5, SM_TOP + 17, "Frames:", dim, bg);
    term::draw_text_at_colored(VX + 21, SM_TOP + 17, "/sec", term::DARK_GRAY, bg);

    // Section 5: interactive buttons
    let btn_y = SM_BOTTOM - 2;
    term::draw_text_at_colored(SM_LEFT + 4, btn_y, "[ Beep Sound ]", term::BLACK, term::YELLOW);
    term::draw_text_at_colored(SM_LEFT + 20, btn_y, "[ Play Melody ]", term::BLACK, term::GREEN);
    term::draw_text_at_colored(SM_LEFT + 40, btn_y, "[ Close (X) ]", term::WHITE, term::RED);
}
