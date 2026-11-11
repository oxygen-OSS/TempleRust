//! Minimal interrupt support: a 64-bit IDT, a remapped 8259 PIC, and the PS/2
//! keyboard / mouse lines feeding driver ring buffers. The PIT runs in
//! one-shot mode to wake the idle loop at the frame deadline.
//!
//! After `init()` the CPU runs with interrupts enabled (IF=1). The handlers
//! are deliberately tiny: they only drain one byte from the PS/2 controller
//! into a single-producer/single-consumer ring, ack the PIC, and return.
//! Everything else (translation, packet assembly, cursor logic) stays in the
//! polled driver code, which pops bytes from the rings in `cli` windows.

// --- port helpers ------------------------------------------------------------

use core::sync::atomic::{AtomicU32, Ordering};

#[inline]
fn outb(port: u16, val: u8) {
    unsafe {
        core::arch::asm!("out dx, al", in("dx") port, in("al") val,
                         options(nomem, nostack, preserves_flags));
    }
}

#[inline]
unsafe fn inb(port: u16) -> u8 {
    let v: u8;
    core::arch::asm!("in al, dx", in("dx") port, out("al") v,
                     options(nostack, preserves_flags));
    v
}

#[inline]
fn io_wait() {
    // ~1us on port 0x80, the classic ISA delay
    outb(0x80, 0);
}

// --- IDT ---------------------------------------------------------------------

#[repr(C, align(8))]
#[derive(Clone, Copy)]
struct Gate {
    lo: u16,
    sel: u16,
    ist: u8,
    attr: u8,
    mid: u16,
    hi: u32,
    zero: u32,
}

const VEC_KBD: usize = 33; // IRQ 1
const VEC_MOUSE: usize = 44; // IRQ 12
const VEC_PIT: usize = 32; // IRQ 0

static mut IDT: [Gate; 256] = [Gate {
    lo: 0,
    sel: 0,
    ist: 0,
    attr: 0,
    mid: 0,
    hi: 0,
    zero: 0,
}; 256];

#[repr(packed(1))]
struct IdtPtr {
    limit: u16,
    base: u64,
}

static mut IDT_PTR: IdtPtr = IdtPtr { limit: 0, base: 0 };

fn set_gate(n: usize, fnaddr: usize) {
    unsafe {
        let g = core::ptr::addr_of_mut!(IDT).cast::<Gate>().add(n);
        core::ptr::write(
            g,
            Gate {
                lo: (fnaddr & 0xffff) as u16,
                sel: 0x18, // stage2 GDT: 0x18 = 64-bit kernel code
                ist: 0,
                attr: 0x8e,
                mid: ((fnaddr >> 16) & 0xffff) as u16,
                hi: (fnaddr >> 32) as u32,
                zero: 0,
            },
        );
    }
}

// --- interrupt stubs ---------------------------------------------------------

// Stub layout: save all caller-saved registers, pass the vector in rdi,
// call the Rust handler, restore, iretq. The handler keeps its own
// callee-saved registers intact.
core::arch::global_asm!(
    "\n.section .text.irq,\"ax\"
.globl irq0_stub, irq1_stub, irq12_stub, irq_default_stub
irq0_stub:
    push rax; push rcx; push rdx; push rsi; push rdi
    push r8; push r9; push r10; push r11
    mov edi, {pit}
    call irq_common
    pop r11; pop r10; pop r9; pop r8
    pop rdi; pop rsi; pop rdx; pop rcx; pop rax
    iretq
irq1_stub:
    push rax; push rcx; push rdx; push rsi; push rdi
    push r8; push r9; push r10; push r11
    mov edi, {kbd}
    call irq_common
    pop r11; pop r10; pop r9; pop r8
    pop rdi; pop rsi; pop rdx; pop rcx; pop rax
    iretq
irq12_stub:
    push rax; push rcx; push rdx; push rsi; push rdi
    push r8; push r9; push r10; push r11
    mov edi, {mouse}
    call irq_common
    pop r11; pop r10; pop r9; pop r8
    pop rdi; pop rsi; pop rdx; pop rcx; pop rax
    iretq
irq_default_stub:
    iretq
",
    pit = const VEC_PIT,
    kbd = const VEC_KBD,
    mouse = const VEC_MOUSE,
);

extern "C" {
    fn irq0_stub();
    fn irq1_stub();
    fn irq12_stub();
    fn irq_default_stub();
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn irq_common(vec: usize) {
    if vec == VEC_KBD {
        // keyboard byte waiting at the controller
        let st = inb(0x64);
        if st & 1 != 0 && st & 0x20 == 0 {
            kb_push(inb(0x60));
            KBD_BYTES.fetch_add(1, Ordering::Relaxed);
        }
        outb(0x20, 0x20); // EOI
    } else if vec == VEC_MOUSE {
        let st = inb(0x64);
        if st & 1 != 0 && st & 0x20 != 0 {
            mouse_push(inb(0x60));
        }
        outb(0xa0, 0x20); // EOI slave + master
        outb(0x20, 0x20);
    } else {
        PIT_TICKS.fetch_add(1, Ordering::Relaxed);
        outb(0x20, 0x20);
    }
}

// --- PS/2 ring buffers -------------------------------------------------------

const RING: usize = 256;
// IRQ activity counters, read by SysMon. Atomics: written from IRQ context,
// read from the main loop — and `static mut` read-modify-write gets
// miscompiled under optimization (counter never incremented).
static PIT_TICKS: AtomicU32 = AtomicU32::new(0);
static KBD_BYTES: AtomicU32 = AtomicU32::new(0);
static MOUSE_BYTES: AtomicU32 = AtomicU32::new(0);
static mut KBUF: [u8; RING] = [0; RING];
static mut KHEAD: u8 = 0; // written by the IRQ only
static mut KTAIL: u8 = 0; // written by the main loop only
static mut MBUF: [u8; RING] = [0; RING];
static mut MHEAD: u8 = 0;
static mut MTAIL: u8 = 0;

unsafe fn kb_push(b: u8) {
    let (h, t) = (*core::ptr::addr_of!(KHEAD), *core::ptr::addr_of!(KTAIL));
    if h.wrapping_sub(t) != (RING - 1) as u8 {
        KBUF[h as usize % RING] = b;
        KHEAD = h.wrapping_add(1);
    }
}

unsafe fn mouse_push(b: u8) {
    MOUSE_BYTES.fetch_add(1, Ordering::Relaxed);
    let (h, t) = (*core::ptr::addr_of!(MHEAD), *core::ptr::addr_of!(MTAIL));
    if h.wrapping_sub(t) != (RING - 1) as u8 {
        MBUF[h as usize % RING] = b;
        MHEAD = h.wrapping_add(1);
    }
}

/// Read one keyboard byte (or None); critical section is IF-transparent.
pub fn kb_pop() -> Option<u8> {
    unsafe {
        let mut flags: u64;
        core::arch::asm!("pushfq; cli; pop {}", out(reg) flags, options(nomem, nostack));
        let t = *core::ptr::addr_of!(KTAIL);
        let out = if *core::ptr::addr_of!(KHEAD) != t {
            let b = KBUF[t as usize % RING];
            KTAIL = t.wrapping_add(1);
            Some(b)
        } else {
            None
        };
        core::arch::asm!("push {}; popfq", in(reg) flags, options(nomem, nostack));
        out
    }
}

/// Frame-polled mouse drain. IRQ12 is masked; each `mouse::poll()` (once per
/// frame) pulls every byte waiting at the 8042 straight into the ring under
/// `cli` so no IRQ handler can race us on the shared output buffer.
pub fn mouse_drain() {
    unsafe {
        let mut flags: u64;
        core::arch::asm!("pushfq; cli; pop {}", out(reg) flags, options(nomem, nostack));
        // cap the work per frame; coalescing means dropped packets are
        // harmless (the cursor just moves straight to the newer position)
        for _ in 0..128 {
            let st = inb(0x64);
            if st & 1 == 0 {
                break;
            }
            let b = inb(0x60);
            if st & 0x20 != 0 {
                mouse_push(b);
            } else {
                // rare: a keyboard byte arrived between IRQs — route it right
                kb_push(b);
            }
        }
        core::arch::asm!("push {}; popfq", in(reg) flags, options(nomem, nostack));
    }
}

pub fn mouse_pop() -> Option<u8> {
    unsafe {
        let mut flags: u64;
        core::arch::asm!("pushfq; cli; pop {}", out(reg) flags, options(nomem, nostack));
        let t = *core::ptr::addr_of!(MTAIL);
        let out = if *core::ptr::addr_of!(MHEAD) != t {
            let b = MBUF[t as usize % RING];
            MTAIL = t.wrapping_add(1);
            Some(b)
        } else {
            None
        };
        core::arch::asm!("push {}; popfq", in(reg) flags, options(nomem, nostack));
        out
    }
}

pub fn kb_pending() -> bool {
    unsafe {
        *core::ptr::addr_of!(KHEAD) != *core::ptr::addr_of!(KTAIL)
    }
}

/// (PIT ticks, keyboard bytes, mouse bytes) since boot — live IRQ activity
/// for SysMon.
pub fn irq_stats() -> (u32, u32, u32) {
    (
        PIT_TICKS.load(Ordering::Relaxed),
        KBD_BYTES.load(Ordering::Relaxed),
        MOUSE_BYTES.load(Ordering::Relaxed),
    )
}

/// Mouse byte availability, for callers that want to distinguish the two
/// rings (the frame wait only early-releases on keyboard).
#[allow(dead_code)]
pub fn mouse_pending() -> bool {
    unsafe {
        *core::ptr::addr_of!(MHEAD) != *core::ptr::addr_of!(MTAIL)
    }
}

// --- PIT (channel 0, one-shot) -----------------------------------------------

pub const PIT_HZ: u64 = 1_193_182;

/// Arm IRQ0 to fire once after `count` PIT ticks (max 65535).
pub fn pit_oneshot(count: u16) {
    outb(0x43, 0x30); // channel 0, mode 0 (countdown), binary
    outb(0x40, (count & 0xff) as u8);
    outb(0x40, (count >> 8) as u8);
}

/// Enable interrupts and sleep until *something* wakes us (PIT deadline,
/// keypress, mouse packet). IF must already be on for `hlt` to be safe-yet-
/// wakeable; the compiler barrier pair keeps the ordering tight.
#[inline]
pub fn idle() {
    unsafe {
        core::arch::asm!(
            "hlt",
            options(nomem, nostack)
        );
    }
}

/// Bring IF up. Everything before this runs as it always did (IF off).
pub fn events_on() {
    unsafe {
        core::arch::asm!("sti", options(nomem, nostack));
    }
}

// --- bring-up ----------------------------------------------------------------

type StubFn = unsafe extern "C" fn();

fn irq_default_stub_ptr() -> usize {
    let f: StubFn = irq_default_stub;
    f as usize
}

fn irq0_stub_ptr() -> usize {
    let f: StubFn = irq0_stub;
    f as usize
}

fn irq1_stub_ptr() -> usize {
    let f: StubFn = irq1_stub;
    f as usize
}

fn irq12_stub_ptr() -> usize {
    let f: StubFn = irq12_stub;
    f as usize
}

pub fn init() {
    unsafe {
        core::arch::asm!("cli", options(nomem, nostack));

        // default gate first, then the real handlers
        let dflt = irq_default_stub_ptr();
        let st0 = irq0_stub_ptr();
        let st1 = irq1_stub_ptr();
        let st12 = irq12_stub_ptr();
        for i in 0..256 {
            set_gate(i, dflt);
        }
        set_gate(VEC_PIT, st0);
        set_gate(VEC_KBD, st1);
        set_gate(VEC_MOUSE, st12);

        let idtp = core::ptr::addr_of_mut!(IDT_PTR);
        (*idtp).limit = core::mem::size_of::<[Gate; 256]>() as u16 - 1;
        (*idtp).base = core::ptr::addr_of!(IDT) as *const _ as u64;
        core::arch::asm!("lidt [{}]",
            in(reg) core::ptr::addr_of!(IDT_PTR) as *const IdtPtr,
            options(readonly, nostack));

        // remap the 8259 pair: master -> 0x20..0x27, slave -> 0x28..0x2f
        outb(0x20, 0x11);
        io_wait();
        outb(0xa0, 0x11);
        io_wait();
        outb(0x21, 0x20);
        io_wait();
        outb(0xa1, 0x28);
        io_wait();
        outb(0x21, 0x04); // slave on IRQ2
        io_wait();
        outb(0xa1, 0x02);
        io_wait();
        outb(0x21, 0x01);
        io_wait();
        outb(0xa1, 0x01);
        io_wait();
        // master: unmask PIT (0), keyboard (1) and the cascade (2)
        outb(0x21, 0xf8);
        // slave: all masked — the mouse is drained by frame-polled code
        // (mouse::poll -> int::mouse_drain). A streaming host mouse can
        // emit hundreds of packets/second; per-byte IRQs would flood the
        // CPU for zero gain since packets coalesce per frame anyway.
        outb(0xa1, 0xff);
    }
}
