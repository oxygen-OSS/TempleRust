//! TempleRust -- a tiny TempleOS-inspired operating system.
//!
//! Booted by a hand-written two-stage BIOS bootloader, running in 64-bit
//! long mode, ring 0, single address space. No libc, no allocator, no
//! processes, no permissions. Interrupts are kept to a bare minimum: a
//! remapped 8259 PIC with handlers for IRQ0 (PIT frame timer) and IRQ1
//! (PS/2 keyboard/mouse feeding ring buffers) lets the idle loop `hlt` for
//! free CPU instead of spinning.

#![no_std]
#![no_main]

mod calc_gui;
mod doldoc;
mod editor;
mod expr;
mod fb;
mod filemgr;
mod font;
mod hcapi;
mod hcapps;
mod int;
mod keyboard;
mod mouse;
mod shell;
mod sound;
mod sysmon;
mod term;
mod terminal;
mod vfs;
mod vga;
mod wm;

use core::ffi::{c_char, c_int, c_void};
use core::panic::PanicInfo;

/// Written by stage 2 at physical address 0x5000 before the jump to
/// long mode. See boot/stage2.S.
#[repr(C)]
pub struct BootInfo {
    pub font_phys: u32,
    pub vga_mode: u16,
    pub boot_drive: u16,
    pub kernel_bytes: u32,
    pub magic: u32,
    pub fb_addr: u64,
    pub fb_width: u32,
    pub fb_height: u32,
    pub fb_pitch: u32,
    pub fb_bpp: u32,
    pub fb_mode: u16,
    /// bit 0: linear framebuffer (VBE), otherwise VGA mode 12h planar.
    pub flags: u16,
}

pub const BOOT_INFO_ADDR: usize = 0x5000;

pub fn boot_info() -> &'static BootInfo {
    unsafe { &*(BOOT_INFO_ADDR as *const BootInfo) }
}

/// Bytes of .bss actually used by the kernel (for SysMon).
pub fn bss_size() -> usize {
    unsafe { &__bss_end as *const u8 as usize - &__bss_start as *const u8 as usize }
}

extern "C" {
    static __bss_start: u8;
    static __bss_end: u8;
}

#[unsafe(no_mangle)]
#[unsafe(link_section = ".text.entry")]
pub extern "C" fn _start() -> ! {
    // objcopy does not emit NOBITS sections, so clear .bss ourselves.
    unsafe {
        let start = &__bss_start as *const u8 as usize;
        let end = &__bss_end as *const u8 as usize;
        core::ptr::write_bytes(start as *mut u8, 0, end - start);
    }

    shell::mark_boot();
    int::init(); // IDT + PIC + PS/2 rings; interrupt vectors only exist after this

    fb::init();
    hcapi::init_env();
    vfs::init();
    mouse::init();
    // device bring-up above does synchronous controller exchanges; with
    // IF on from here on, IRQ1 delivers into the keyboard ring; the mouse
    // line is frame-polled instead (see int::mouse_drain)
    int::events_on();

    term::init();
    wm::desktop_loop();
}

// --- freestanding runtime bits ---------------------------------------------
//
// `core` and the compiler expect these to exist. The volatile accesses keep
// LLVM from recognising the loops and turning them into calls to themselves.

#[unsafe(no_mangle)]
pub unsafe extern "C" fn memset(dst: *mut c_void, c: c_int, n: usize) -> *mut c_void {
    let d = dst as *mut u8;
    let mut i = 0;
    while i < n {
        core::ptr::write_volatile(d.add(i), c as u8);
        i += 1;
    }
    dst
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn memcpy(dst: *mut c_void, src: *const c_void, n: usize) -> *mut c_void {
    let d = dst as *mut u8;
    let s = src as *const u8;
    let mut i = 0;
    while i < n {
        core::ptr::write_volatile(d.add(i), core::ptr::read_volatile(s.add(i)));
        i += 1;
    }
    dst
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn memmove(dst: *mut c_void, src: *const c_void, n: usize) -> *mut c_void {
    let d = dst as *mut u8;
    let s = src as *const u8;
    if (d as usize) < (s as usize) {
        let mut i = 0;
        while i < n {
            core::ptr::write_volatile(d.add(i), core::ptr::read_volatile(s.add(i)));
            i += 1;
        }
    } else {
        let mut i = n;
        while i > 0 {
            i -= 1;
            core::ptr::write_volatile(d.add(i), core::ptr::read_volatile(s.add(i)));
        }
    }
    dst
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn memcmp(a: *const c_void, b: *const c_void, n: usize) -> c_int {
    let a = a as *const u8;
    let b = b as *const u8;
    let mut i = 0;
    while i < n {
        let x = core::ptr::read_volatile(a.add(i));
        let y = core::ptr::read_volatile(b.add(i));
        if x != y {
            return x as c_int - y as c_int;
        }
        i += 1;
    }
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn bcmp(a: *const c_void, b: *const c_void, n: usize) -> c_int {
    memcmp(a, b, n)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn strlen(s: *const c_char) -> usize {
    let mut n = 0;
    while *(s.add(n)) != 0 {
        n += 1;
    }
    n
}

#[unsafe(no_mangle)]
pub extern "C" fn rust_eh_personality() {}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    term::set_fg(term::LIGHT_RED);
    crate::print!("\nPANIC: {}\n", info);
    loop {
        unsafe {
            core::arch::asm!("cli", "hlt", options(nomem, nostack));
        }
    }
}
