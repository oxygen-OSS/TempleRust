//! Minimal VGA mode 12h (640x480x16) driver.
//!
//! The screen is planar: every byte at 0xA0000 holds eight pixels, one per
//! bit, and the same address exists in four separate planes. We use two
//! hardware tricks instead of read-modify-write:
//!
//!   * set/reset + enable set/reset  -> fast solid fills
//!   * write mode 2 + bit mask       -> draw a glyph row in one store,
//!     after a read that loads the latches with the existing background

use crate::font;

pub const WIDTH: usize = 640;
pub const HEIGHT: usize = 480;

const BPR: usize = WIDTH / 8; // 80 bytes per row
const MEM: usize = 0xa0000;

#[inline]
unsafe fn outb(port: u16, val: u8) {
    core::arch::asm!("out dx, al", in("dx") port, in("al") val,
                     options(nostack, preserves_flags));
}

unsafe fn bit_mask(m: u8) {
    outb(0x3ce, 0x08);
    outb(0x3cf, m);
}

/// Select "set/reset" fill mode: every store writes `color` to all pixels
/// enabled by the bit mask.
unsafe fn fill_mode(color: u8) {
    outb(0x3c4, 0x02);
    outb(0x3c5, 0x0f); // map mask: all four planes
    outb(0x3ce, 0x00);
    outb(0x3cf, color & 0x0f); // set/reset
    outb(0x3ce, 0x01);
    outb(0x3cf, 0x0f); // enable set/reset
    outb(0x3ce, 0x03);
    outb(0x3cf, 0x00); // data rotate: none
    outb(0x3ce, 0x05);
    outb(0x3cf, 0x00); // write mode 0
    outb(0x3ce, 0x08);
    outb(0x3cf, 0xff); // bit mask: all pixels
}

unsafe fn glyph_mode() {
    outb(0x3c4, 0x02);
    outb(0x3c5, 0x0f);
    outb(0x3ce, 0x01);
    outb(0x3cf, 0x00); // set/reset off
    outb(0x3ce, 0x03);
    outb(0x3cf, 0x00);
    outb(0x3ce, 0x05);
    outb(0x3cf, 0x02); // write mode 2
    outb(0x3ce, 0x08);
    outb(0x3cf, 0xff);
}

pub fn init() {
    fill_rect(0, 0, WIDTH, HEIGHT, 1);
}

/// Byte mask covering pixels `a..=b` inside one 8-pixel byte (bit 7 = left).
fn span_mask(a: usize, b: usize) -> u8 {
    let mut m = 0u8;
    for i in a..=b {
        m |= 0x80 >> i;
    }
    m
}

pub fn fill_rect(x: usize, y: usize, w: usize, h: usize, color: u8) {
    if w == 0 || h == 0 {
        return;
    }
    let x0 = x.min(WIDTH);
    let x1 = (x + w).min(WIDTH);
    let y0 = y.min(HEIGHT);
    let y1 = (y + h).min(HEIGHT);
    if x0 >= x1 || y0 >= y1 {
        return;
    }

    let b0 = x0 / 8;
    let b1 = (x1 + 7) / 8; // exclusive
    let first = span_mask(x0 % 8, 7);
    let last = span_mask(0, (x1 - 1) % 8);

    unsafe {
        fill_mode(color);
        for yy in y0..y1 {
            let row = MEM + yy * BPR;
            if b0 + 1 == b1 {
                bit_mask(first & last);
                core::ptr::write_volatile((row + b0) as *mut u8, 0xff);
            } else {
                bit_mask(first);
                core::ptr::write_volatile((row + b0) as *mut u8, 0xff);
                bit_mask(0xff);
                let n = b1 - b0 - 2;
                if n > 0 {
                    core::ptr::write_bytes((row + b0 + 1) as *mut u8, 0xff, n);
                }
                bit_mask(last);
                core::ptr::write_volatile((row + b1 - 1) as *mut u8, 0xff);
            }
        }
        bit_mask(0xff);
    }
}

/// Draw one 8x16 glyph. `px` must be a multiple of 8.
pub fn draw_glyph(px: usize, py: usize, ch: u8, color: u8) {
    if px + 8 > WIDTH || py + 16 > HEIGHT {
        return;
    }
    let g = font::glyph(ch);
    let base = MEM + py * BPR + px / 8;

    unsafe {
        glyph_mode();
        for r in 0..16 {
            let bits = g[r];
            if bits == 0 {
                continue;
            }
            let addr = (base + r * BPR) as *mut u8;
            // Write mode 2 fills unmasked pixels from the four VGA latches.
            // The latches are only loaded by a CPU read, so read the byte we
            // are about to modify first; otherwise the background behind the
            // glyph becomes colour 0 (black) instead of staying put.
            core::ptr::read_volatile(addr as *const u8);
            bit_mask(bits);
            core::ptr::write_volatile(addr, color & 0x0f);
        }
        bit_mask(0xff);
    }
}


