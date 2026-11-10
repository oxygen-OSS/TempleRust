//! Unified graphics layer.
//!
//! Prefers the VBE linear framebuffer chosen by the bootloader (16/24/32 bpp)
//! and falls back to the planar VGA mode 12h driver if VBE was unavailable.
//!
//! The terminal always works on an 80x30 grid; this module scales cells to
//! whatever resolution the firmware gave us.

use core::cell::UnsafeCell;

use crate::boot_info;
use crate::font;
use crate::vga;

pub const COLS: usize = 80;
pub const ROWS: usize = 30;

/// Standard VGA 16-colour palette as 0x00RRGGBB.
const PALETTE: [u32; 16] = [
    0x000000, 0x0000aa, 0x00aa00, 0x00aaaa, 0xaa0000, 0xaa00aa, 0xaa5500, 0xaaaaaa,
    0x555555, 0x5555ff, 0x55ff55, 0x55ffff, 0xff5555, 0xff55ff, 0xffff55, 0xffffff,
];

struct Mode {
    addr: usize,
    width: usize,
    height: usize,
    pitch: usize,
    bpp: usize,
    cell_w: usize,
    cell_h: usize,
    linear: bool,
}

struct SyncCell<T>(UnsafeCell<T>);
unsafe impl<T> Sync for SyncCell<T> {}

static MODE: SyncCell<Mode> = SyncCell(UnsafeCell::new(Mode {
    addr: 0,
    width: 640,
    height: 480,
    pitch: 80,
    bpp: 4,
    cell_w: 8,
    cell_h: 16,
    linear: false,
}));

fn m() -> *mut Mode {
    MODE.0.get()
}

pub fn init() {
    let bi = boot_info();
    unsafe {
        let p = m();
        if bi.flags & 1 != 0 && bi.fb_addr != 0 && bi.fb_bpp != 0 {
            (*p).addr = bi.fb_addr as usize;
            (*p).width = bi.fb_width as usize;
            (*p).height = bi.fb_height as usize;
            (*p).pitch = bi.fb_pitch as usize;
            (*p).bpp = bi.fb_bpp as usize;
            (*p).linear = true;
        } else {
            (*p).addr = 0xa0000;
            (*p).width = vga::WIDTH;
            (*p).height = vga::HEIGHT;
            (*p).pitch = vga::WIDTH / 8;
            (*p).bpp = 4;
            (*p).linear = false;
        }
        (*p).cell_w = ((*p).width / COLS).max(1);
        (*p).cell_h = ((*p).height / ROWS).max(1);
    }

    if unsafe { (*m()).linear } {
        fill_rect(0, 0, width(), height(), 1); // blue
    } else {
        vga::init();
    }
}

pub fn width() -> usize {
    unsafe { (*m()).width }
}

pub fn height() -> usize {
    unsafe { (*m()).height }
}

pub fn cell_w() -> usize {
    unsafe { (*m()).cell_w }
}

pub fn cell_h() -> usize {
    unsafe { (*m()).cell_h }
}

pub fn is_linear() -> bool {
    unsafe { (*m()).linear }
}

pub fn bpp() -> usize {
    unsafe { (*m()).bpp }
}

pub fn pitch() -> usize {
    unsafe { (*m()).pitch }
}

unsafe fn fill_span(x: usize, y: usize, w: usize, rgb: u32) {
    let p = m();
    if w == 0 || y >= (*p).height {
        return;
    }
    let x0 = x.min((*p).width);
    let x1 = (x + w).min((*p).width);
    if x0 >= x1 {
        return;
    }
    let base = (*p).addr + y * (*p).pitch;
    match (*p).bpp {
        32 => {
            let dst = (base + x0 * 4) as *mut u32;
            for i in 0..(x1 - x0) {
                core::ptr::write_unaligned(dst.add(i), rgb);
            }
        }
        24 => {
            let dst = (base + x0 * 3) as *mut u8;
            let b = rgb as u8;
            let g = (rgb >> 8) as u8;
            let r = (rgb >> 16) as u8;
            for i in 0..(x1 - x0) {
                let o = i * 3;
                core::ptr::write_unaligned(dst.add(o), b);
                core::ptr::write_unaligned(dst.add(o + 1), g);
                core::ptr::write_unaligned(dst.add(o + 2), r);
            }
        }
        16 => {
            let r = (rgb >> 16) & 0xff;
            let g = (rgb >> 8) & 0xff;
            let b = rgb & 0xff;
            let v = (((r >> 3) << 11) | ((g >> 2) << 5) | (b >> 3)) as u16;
            let dst = (base + x0 * 2) as *mut u16;
            for i in 0..(x1 - x0) {
                core::ptr::write_unaligned(dst.add(i), v);
            }
        }
        _ => {}
    }
}

pub fn fill_rect(x: usize, y: usize, w: usize, h: usize, color: u8) {
    let p = unsafe { &*m() };
    if !p.linear {
        vga::fill_rect(x, y, w, h, color);
        return;
    }
    let rgb = PALETTE[(color & 0x0f) as usize];
    let y0 = y.min(p.height);
    let y1 = (y + h).min(p.height);
    for yy in y0..y1 {
        unsafe {
            fill_span(x, yy, w, rgb);
        }
    }
}

// --- runtime mode change ------------------------------------------------------
//
// The Bochs/QEMU VBE "dispi" registers (port 0x1CE index / 0x1CF data) can be
// reprogrammed directly from ring 0, no BIOS round-trip needed. The linear
// framebuffer base stays fixed, so only the scanout geometry and pitch change.

const DISPI_INDEX: u16 = 0x1ce;
const DISPI_DATA: u16 = 0x1cf;
const DISPI_XRES: u16 = 1;
const DISPI_YRES: u16 = 2;
const DISPI_BPP: u16 = 3;
const DISPI_ENABLE: u16 = 4;
const DISPI_VIRT_W: u16 = 6;
const DISPI_VIRT_H: u16 = 7;
const DISPI_ENABLED: u16 = 0x01;
// keep fb contents & tell the LFB is the visible region
const DISPI_NOCLEARMEM: u16 = 0x20;
const DISPI_LFB: u16 = 0x40;

fn dispi_write(index: u16, val: u16) {
    unsafe {
        core::arch::asm!("out dx, al",
            in("dx") DISPI_INDEX, in("al") (index as u8),
            options(nomem, nostack, preserves_flags));
        core::arch::asm!("out dx, ax",
            in("dx") DISPI_DATA, in("ax") val,
            options(nomem, nostack, preserves_flags));
    }
}

/// The terminal is always 80x30 cells; clamp the mode to something sane and
/// keep the width a multiple of 8 as the hardware expects.
pub const MIN_W: usize = 320;
pub const MAX_W: usize = 1920;
pub const MIN_H: usize = 200;
pub const MAX_H: usize = 1080;

/// Reset the stored cursor position to a valid spot on the current screen.
pub fn clamp_mouse() {
    crate::mouse::clamp_to_screen();
}

/// Switch the linear framebuffer backend to `w`x`h` at runtime. Returns false
/// when unsupported (planar VGA fallback) or out of the supported range.
pub fn set_resolution(w: usize, h: usize) -> bool {
    if !(unsafe { (*m()).linear }) {
        return false;
    }
    let w = w.clamp(MIN_W, MAX_W) & !7;
    let h = h.clamp(MIN_H, MAX_H);
    if w < MIN_W || h < MIN_H {
        return false;
    }
    unsafe {
        let p = m();
        if (*p).width == w && (*p).height == h {
            return true;
        }
        // disable, program, re-enable: the sequence the hardware expects
        dispi_write(DISPI_ENABLE, 0);
        dispi_write(DISPI_XRES, w as u16);
        dispi_write(DISPI_YRES, h as u16);
        dispi_write(DISPI_BPP, 32);
        dispi_write(DISPI_VIRT_W, w as u16);
        dispi_write(DISPI_VIRT_H, h as u16);
        dispi_write(DISPI_ENABLE, DISPI_ENABLED | DISPI_NOCLEARMEM | DISPI_LFB);

        (*p).width = w;
        (*p).height = h;
        (*p).pitch = w * 4;
        (*p).bpp = 32;
        (*p).cell_w = (w / COLS).max(1);
        (*p).cell_h = (h / ROWS).max(1);
    }
    // Keep the HolyC app ABI in sync so blobs see the new geometry.
    unsafe {
        crate::hcapi::HC_FBW = w as u32;
        crate::hcapi::HC_FBH = h as u32;
        crate::hcapi::HC_FBPITCH = (w * 4) as u32;
    }
    true
}

/// Draw one glyph at cell (`col`, `row`), scaled to the cell size.
pub fn draw_glyph(col: usize, row: usize, ch: u8, color: u8) {
    let p = unsafe { &*m() };
    if !p.linear {
        vga::draw_glyph(col * 8, row * 16, ch, color);
        return;
    }
    let rgb = PALETTE[(color & 0x0f) as usize];
    let g = font::glyph(ch);
    let cw = p.cell_w;
    let chh = p.cell_h;

    // Keep the font's 1:2 aspect ratio and centre the glyph in the cell.
    // At 1920x1080 cells are 24x36, so the glyph is 18x36 with 3px of
    // padding on each side instead of a horizontally stretched 24x36.
    let (gw, gh) = if chh >= cw * 2 {
        (cw, cw * 2)
    } else {
        (chh / 2, chh)
    };
    let x0 = col * cw + (cw - gw) / 2;
    let y0 = row * chh + (chh - gh) / 2;

    let mut xs = [(0usize, 0usize); 8];
    for gx in 0..8 {
        xs[gx] = (gx * gw / 8, (gx + 1) * gw / 8);
    }

    for gy in 0..16 {
        let bits = g[gy];
        if bits == 0 {
            continue;
        }
        let sy = gy * gh / 16;
        let sy2 = (gy + 1) * gh / 16;
        for yy in sy..sy2 {
            for gx in 0..8 {
                if bits & (0x80 >> gx) != 0 {
                    let (sx, sx2) = xs[gx];
                    if sx2 > sx {
                        unsafe {
                            fill_span(x0 + sx, y0 + yy, sx2 - sx, rgb);
                        }
                    }
                }
            }
        }
    }
}
