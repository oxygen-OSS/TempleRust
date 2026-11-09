//! Graphical System Monitor Dashboard (SysMon).

use crate::keyboard::{self, Key};
use crate::mouse;
use crate::sound;
use crate::term;

const SM_TOP: usize = 3;
const SM_BOTTOM: usize = 25;
const SM_LEFT: usize = 10;
const SM_RIGHT: usize = 70;

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
        core::hint::spin_loop();
    }

    term::clear();
    term::status_bar();
}

fn draw_chrome() {
    let bg = term::theme_bg();
    let border = term::LIGHT_CYAN;

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

    // Section 1: CPU & Kernel
    term::draw_text_at_colored(SM_LEFT + 3, SM_TOP + 2, "CPU Architecture & Clocks", term::YELLOW, bg);
    term::draw_text_at_colored(SM_LEFT + 5, SM_TOP + 3, "Mode:        x86_64 Long Mode (LME / Ring 0)", term::WHITE, bg);
    term::draw_text_at_colored(SM_LEFT + 5, SM_TOP + 4, "TSC Speed:   ~", term::WHITE, bg);
    let mhz = (crate::shell::tsc_per_ms() / 1000) as i64;
    draw_num(SM_LEFT + 19, SM_TOP + 4, mhz, term::LIGHT_GREEN, bg);
    term::draw_text_at_colored(SM_LEFT + 24, SM_TOP + 4, "MHz (calibrated with PIT ch2)", term::WHITE, bg);

    // CPU Bar meter
    term::draw_text_at_colored(SM_LEFT + 5, SM_TOP + 5, "CPU Activity [", term::WHITE, bg);
    for x in 0..28 {
        term::put_at_colored(SM_LEFT + 19 + x, SM_TOP + 5, 0xdb, term::GREEN, bg);
    }
    term::draw_text_at_colored(SM_LEFT + 47, SM_TOP + 5, "] 100%", term::LIGHT_GREEN, bg);

    // Section 2: Memory
    term::draw_text_at_colored(SM_LEFT + 3, SM_TOP + 7, "Physical Memory Layout", term::YELLOW, bg);
    term::draw_text_at_colored(SM_LEFT + 5, SM_TOP + 8, "0x00000 - 0x05000: Real Mode BIOS & IVT", term::WHITE, bg);
    term::draw_text_at_colored(SM_LEFT + 5, SM_TOP + 9, "0x05000 - 0x06000: BootInfo (TOS1)", term::WHITE, bg);
    term::draw_text_at_colored(SM_LEFT + 5, SM_TOP + 10, "0x10000 - 0x80000: Kernel Binary (Rust)", term::LIGHT_GREEN, bg);
    term::draw_text_at_colored(SM_LEFT + 5, SM_TOP + 11, "0x90000 - 0x96000: PML4/PDPT Page Tables (4 GiB Map)", term::WHITE, bg);

    // Section 3: Framebuffer & Audio
    term::draw_text_at_colored(SM_LEFT + 3, SM_TOP + 13, "Video & Audio Hardware", term::YELLOW, bg);
    term::draw_text_at_colored(SM_LEFT + 5, SM_TOP + 14, "Video Mode:  ", term::WHITE, bg);
    term::draw_text_at_colored(SM_LEFT + 18, SM_TOP + 14, term::mode_text(), term::LIGHT_CYAN, bg);
    term::draw_text_at_colored(
        SM_LEFT + 32,
        SM_TOP + 14,
        if crate::fb::is_linear() { "(VBE Linear FB)" } else { "(VGA 12h Planar)" },
        term::WHITE,
        bg,
    );
    term::draw_text_at_colored(SM_LEFT + 5, SM_TOP + 15, "Speaker:     PIT Channel 2 (Port 0x61 bit 0 & 1)", term::WHITE, bg);

    // Section 4: Interactive Buttons
    let btn_y = SM_BOTTOM - 2;
    term::draw_text_at_colored(SM_LEFT + 4, btn_y, "[ Beep Sound ]", term::BLACK, term::YELLOW);
    term::draw_text_at_colored(SM_LEFT + 20, btn_y, "[ Play Melody ]", term::BLACK, term::GREEN);
    term::draw_text_at_colored(SM_LEFT + 40, btn_y, "[ Close (X) ]", term::WHITE, term::RED);
}

fn draw_num(x: usize, y: usize, mut v: i64, fg: u8, bg: u8) {
    let mut buf = [0u8; 10];
    let mut i = buf.len();
    if v == 0 {
        i -= 1;
        buf[i] = b'0';
    }
    while v > 0 {
        i -= 1;
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    term::draw_text_at_colored(x, y, core::str::from_utf8(&buf[i..]).unwrap_or(""), fg, bg);
}
