//! Polling PS/2 mouse driver with TempleOS-style arrow cursor rendering.

use crate::fb;
use crate::term;

#[derive(Clone, Copy, Debug)]
pub struct MouseEvent {
    pub x: usize,
    pub y: usize,
    pub left: bool,
    #[allow(dead_code)]
    pub right: bool,
}

struct MouseState {
    x: isize,
    y: isize,
    left: bool,
    right: bool,
    packet: [u8; 3],
    packet_idx: usize,
    cursor_visible: bool,
}

static mut MOUSE: MouseState = MouseState {
    x: 320,
    y: 240,
    left: false,
    right: false,
    packet: [0; 3],
    packet_idx: 0,
    cursor_visible: true,
};

#[inline]
unsafe fn inb(port: u16) -> u8 {
    let v: u8;
    core::arch::asm!("in al, dx", in("dx") port, out("al") v,
                     options(nostack, preserves_flags));
    v
}

#[inline]
unsafe fn outb(port: u16, val: u8) {
    core::arch::asm!("out dx, al", in("dx") port, in("al") val,
                     options(nostack, preserves_flags));
}

unsafe fn wait_write() {
    for _ in 0..100_000 {
        if inb(0x64) & 2 == 0 {
            break;
        }
    }
}

unsafe fn wait_read() {
    for _ in 0..100_000 {
        if inb(0x64) & 1 != 0 {
            break;
        }
    }
}

pub fn init() {
    unsafe {
        // Enable auxiliary mouse device
        wait_write();
        outb(0x64, 0xa8);

        // Read command byte
        wait_write();
        outb(0x64, 0x20);
        wait_read();
        let mut cfg = inb(0x60);
        cfg |= 0x02; // enable mouse IRQ
        cfg &= !0x20; // disable mouse clock inhibit

        // Write command byte
        wait_write();
        outb(0x64, 0x60);
        wait_write();
        outb(0x60, cfg);

        // Tell mouse to use defaults (0xf6)
        wait_write();
        outb(0x64, 0xd4);
        wait_write();
        outb(0x60, 0xf6);
        wait_read();
        let _ = inb(0x60); // ACK

        // Enable data reporting (0xf4)
        wait_write();
        outb(0x64, 0xd4);
        wait_write();
        outb(0x60, 0xf4);
        wait_read();
        let _ = inb(0x60); // ACK

        let m = &mut *core::ptr::addr_of_mut!(MOUSE);
        m.x = (fb::width() / 2) as isize;
        m.y = (fb::height() / 2) as isize;
        m.packet_idx = 0;
        m.cursor_visible = true;

        draw_cursor();
    }
}

// TempleOS arrow cursor: 12x18
// 0 = transparent, 1 = black outline, 2 = bright yellow fill
const CURSOR_W: usize = 12;
const CURSOR_H: usize = 18;

const CURSOR_PIXELS: [[u8; CURSOR_W]; CURSOR_H] = [
    [1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    [1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    [1, 2, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    [1, 2, 2, 1, 0, 0, 0, 0, 0, 0, 0, 0],
    [1, 2, 2, 2, 1, 0, 0, 0, 0, 0, 0, 0],
    [1, 2, 2, 2, 2, 1, 0, 0, 0, 0, 0, 0],
    [1, 2, 2, 2, 2, 2, 1, 0, 0, 0, 0, 0],
    [1, 2, 2, 2, 2, 2, 2, 1, 0, 0, 0, 0],
    [1, 2, 2, 2, 2, 2, 2, 2, 1, 0, 0, 0],
    [1, 2, 2, 2, 2, 2, 2, 2, 2, 1, 0, 0],
    [1, 2, 2, 2, 2, 1, 1, 1, 1, 1, 0, 0],
    [1, 2, 1, 2, 2, 1, 0, 0, 0, 0, 0, 0],
    [1, 1, 0, 1, 2, 2, 1, 0, 0, 0, 0, 0],
    [1, 0, 0, 1, 2, 2, 1, 0, 0, 0, 0, 0],
    [0, 0, 0, 0, 1, 2, 2, 1, 0, 0, 0, 0],
    [0, 0, 0, 0, 1, 2, 2, 1, 0, 0, 0, 0],
    [0, 0, 0, 0, 0, 1, 1, 0, 0, 0, 0, 0],
    [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
];

/// Keep the stored cursor position inside the current mode (call after a
/// resolution change shrinks the screen).
pub fn clamp_to_screen() {
    unsafe {
        let m = &mut *core::ptr::addr_of_mut!(MOUSE);
        let w = fb::width() as isize;
        let h = fb::height() as isize;
        m.x = m.x.clamp(0, w.saturating_sub(CURSOR_W as isize));
        m.y = m.y.clamp(0, h.saturating_sub(CURSOR_H as isize));
        m.cursor_visible = true;
    }
}

#[allow(dead_code)]
pub fn get_pos() -> (usize, usize) {
    unsafe {
        let m = &*core::ptr::addr_of!(MOUSE);
        (m.x.max(0) as usize, m.y.max(0) as usize)
    }
}

pub fn erase_cursor(px: usize, py: usize) {
    let cw = fb::cell_w().max(1);
    let ch = fb::cell_h().max(1);
    let c0 = px / cw;
    let c1 = ((px + CURSOR_W).min(fb::width().saturating_sub(1))) / cw;
    let r0 = py / ch;
    let r1 = ((py + CURSOR_H).min(fb::height().saturating_sub(1))) / ch;

    for r in r0..=r1 {
        for c in c0..=c1 {
            if c < term::COLS && r < term::ROWS {
                unsafe {
                    term::draw_cell(c, r);
                }
            }
        }
    }
}

pub fn poll() -> Option<MouseEvent> {
    let data = match crate::int::mouse_pop() {
        Some(b) => b,
        None => return None,
    };
    unsafe {
        let m = &mut *core::ptr::addr_of_mut!(MOUSE);

        if m.packet_idx == 0 && (data & 0x08) == 0 {
            // Out of sync
            return None;
        }

        m.packet[m.packet_idx] = data;
        m.packet_idx += 1;

        if m.packet_idx == 3 {
            m.packet_idx = 0;
            let flags = m.packet[0];
            let mut dx = m.packet[1] as isize;
            let mut dy = m.packet[2] as isize;

            if (flags & 0x10) != 0 {
                dx -= 256;
            }
            if (flags & 0x20) != 0 {
                dy -= 256;
            }

            let old_x = m.x;
            let old_y = m.y;

            m.x += dx;
            m.y -= dy; // PS/2 Y goes up, screen goes down

            let w = fb::width() as isize;
            let h = fb::height() as isize;
            m.x = m.x.clamp(0, w.saturating_sub(CURSOR_W as isize));
            m.y = m.y.clamp(0, h.saturating_sub(CURSOR_H as isize));

            m.left = (flags & 0x01) != 0;
            m.right = (flags & 0x02) != 0;

            if m.cursor_visible && (old_x != m.x || old_y != m.y) {
                erase_cursor(old_x as usize, old_y as usize);
                draw_cursor();
            }

            return Some(MouseEvent {
                x: m.x as usize,
                y: m.y as usize,
                left: m.left,
                right: m.right,
            });
        }
        None
    }
}

pub fn draw_cursor() {
    unsafe {
        let m = &mut *core::ptr::addr_of_mut!(MOUSE);
        let px = m.x as usize;
        let py = m.y as usize;

        // Draw cursor using fill_rect pixels
        for cy in 0..CURSOR_H {
            for cx in 0..CURSOR_W {
                let p = CURSOR_PIXELS[cy][cx];
                if p == 1 {
                    fb::fill_rect(px + cx, py + cy, 1, 1, crate::term::BLACK);
                } else if p == 2 {
                    fb::fill_rect(px + cx, py + cy, 1, 1, crate::term::YELLOW);
                }
            }
        }
    }
}
