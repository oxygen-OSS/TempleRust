//! A 80x30 cell terminal on top of the VGA/VBE driver, with TempleOS menu bar,
//! window frame header, per-cell foreground and background colors, and theme support.

use core::cell::UnsafeCell;
use core::fmt;

use crate::fb;

pub const COLS: usize = 80;
pub const ROWS: usize = 30;

// Standard TempleOS 16-color palette indices
pub const BLACK: u8 = 0;
pub const BLUE: u8 = 1;
pub const GREEN: u8 = 2;
pub const CYAN: u8 = 3;
pub const RED: u8 = 4;
pub const PURPLE: u8 = 5;
pub const BROWN: u8 = 6;
pub const LIGHT_GRAY: u8 = 7;
pub const DARK_GRAY: u8 = 8;
pub const LIGHT_BLUE: u8 = 9;
pub const LIGHT_GREEN: u8 = 10;
pub const LIGHT_CYAN: u8 = 11;
pub const LIGHT_RED: u8 = 12;
pub const LIGHT_PURPLE: u8 = 13;
pub const YELLOW: u8 = 14;
pub const WHITE: u8 = 15;

pub const MENU_ROW: usize = 0;

// --- frame pacing & measured FPS ---------------------------------------------

/// Frame-rate limit of the OS main loop. Default: 60 FPS (was an unplanned
/// spin-wait before). Runtime-changeable with the `Fps(n);` command.
static mut FPS_CAP: u64 = 60;
/// FPS measurement: sliding window of recent frame stamps. Reporting raw
/// per-frame TSC deltas spikes to absurd values whenever input wakes the
/// wait early, so the displayed number is "frames completed in the last
/// second", exactly what a frame meter means.
static mut FRAME_TS: [u64; 128] = [0; 128];
static mut FRAME_N: usize = 0;
static mut FRAME_P: usize = 0;

fn record_frame(ts: u64) {
    unsafe {
        let p = core::ptr::addr_of_mut!(FRAME_P);
        let arr = core::ptr::addr_of_mut!(FRAME_TS);
        let n = core::ptr::addr_of_mut!(FRAME_N);
        (*arr)[*p] = ts;
        *p = (*p + 1) % (*arr).len();
        if *n < (*arr).len() {
            *n += 1;
        }
    }
}

pub fn fps() -> u64 {
    let pm = crate::shell::tsc_per_ms();
    if pm == 0 {
        return 0;
    }
    let cutoff = crate::shell::now().wrapping_sub(pm * 1000);
    unsafe {
        let n = *core::ptr::addr_of!(FRAME_N);
        if n == 0 {
            // never measured yet: don't display 0 during the first second
            return fps_cap();
        }
        let arr = core::ptr::addr_of!(FRAME_TS) as *const [u64; 128];
        let p = *core::ptr::addr_of!(FRAME_P);
        let mut count = 0u64;
        for i in 0..n {
            if (*arr)[(p + (*arr).len() - 1 - i) % (*arr).len()].wrapping_sub(cutoff)
                < (1u64 << 63)
            {
                count += 1;
            }
        }
        count
    }
}

/// TSC stamp of the end of the previous frame.
static mut LAST_FRAME: u64 = 0;

pub fn fps_cap() -> u64 {
    unsafe { *core::ptr::addr_of!(FPS_CAP) }
}

/// Set the frame-rate cap (clamped to 1..120). `Fps(n);` shell command.
pub fn set_fps_cap(n: u64) {
    let cap = n.clamp(1, 120);
    unsafe {
        core::ptr::addr_of_mut!(FPS_CAP).write(cap);
        // re-measure from a clean slate so the meter doesn't take ~seconds
        // to converge onto the new rate
        core::ptr::addr_of_mut!(FRAME_TS).write([0; 128]);
        core::ptr::addr_of_mut!(FRAME_N).write(0);
        core::ptr::addr_of_mut!(FRAME_P).write(0);
        core::ptr::addr_of_mut!(LAST_FRAME).write(0);
    }
}

/// Logical (visible) CPU count, straight from CPUID. Used by the menu bar.
/// Some models report an empty count in CPUID.1H.EBX[23:16]; fall back to 1.
pub fn logical_cpus() -> u32 {
    let r = core::arch::x86_64::__cpuid(1);
    ((r.ebx >> 16) & 0xffff).max(1)
}

fn push_num(buf: &mut [u8], i: &mut usize, mut n: u64) {
    if *i >= buf.len() {
        return;
    }
    if n == 0 {
        buf[*i] = b'0';
        *i += 1;
        return;
    }
    let mut digits = [0u8; 4];
    let mut d = 0;
    while n > 0 && d < 4 {
        digits[d] = b'0' + (n % 10) as u8;
        n /= 10;
        d += 1;
    }
    for j in (0..d).rev() {
        if *i >= buf.len() {
            break;
        }
        buf[*i] = digits[j];
        *i += 1;
    }
}

/// Build "CPUs <n>  FPS <rate>" from live data into `buf`, return the slice.
fn info_str(buf: &mut [u8]) -> &str {
    let mut i = 0usize;
    for &c in b"CPUs " {
        if i < buf.len() {
            buf[i] = c;
            i += 1;
        }
    }
    push_num(buf, &mut i, logical_cpus() as u64);
    for &c in b"  FPS " {
        if i < buf.len() {
            buf[i] = c;
            i += 1;
        }
    }
    push_num(buf, &mut i, fps());
    core::str::from_utf8(&buf[..i]).unwrap_or("CPUs 1  FPS 60")
}

fn menu_colors() -> (u8, u8) {
    match theme() {
        Theme::Classic => (BROWN, WHITE),
        Theme::Blue => (LIGHT_GRAY, BLACK),
        Theme::Dark => (DARK_GRAY, WHITE),
    }
}

pub fn draw_panel_info() {
    let (menu_bg, menu_fg) = menu_colors();
    let mut buf = [0u8; 24];
    let info = info_str(&mut buf);
    // Clear a region wide enough for every possible value of the info line.
    let slack = 6usize;
    let x0 = COLS.saturating_sub(info.len() + slack + 1);
    for x in x0..COLS {
        put_at_colored(x, MENU_ROW, b' ', menu_fg, menu_bg);
    }
    if COLS > info.len() + 2 {
        draw_text_at_colored(COLS - info.len() - 1, MENU_ROW, info, YELLOW, menu_bg);
    }
}/// One frame of the OS main loop: idles until the next frame slot so the loop
/// runs at `FPS_CAP` overall, then stamps the frame for the FPS meter. The
/// CPU sleeps (`hlt`) in between, woken by the PIT deadline shot, an input
/// packet, or a freshly queued key; the OS runs with interrupts enabled, so
/// this costs no CPU while idle.
///
/// Mouse traffic does NOT cut the wait short: it is coalesced per frame by
/// the driver, and letting continuous packet streams break pacing would turn
/// the main loop into a spin lock at packet rate. Keys do — keystrokes are
/// far rarer and users expect them immediately.
static mut FRAME_BUSY: u32 = 100;

/// Percentage of the last paced frame spent doing work instead of sleeping.
/// ~0 = the loop idles in `hlt`; 100 = it runs flat out (input flood, busy
/// app). Shown by SysMon.
pub fn frame_busy() -> u64 {
    unsafe { *core::ptr::addr_of!(FRAME_BUSY) as u64 }
}

pub fn frame_wait() {
    let cap = fps_cap();
    let tsc_pm = crate::shell::tsc_per_ms();
    unsafe {
        let last_p = core::ptr::addr_of_mut!(LAST_FRAME);
        let t0 = *last_p;
        let interval = tsc_pm * 1000 / cap;
        let t_wake0 = crate::shell::now();
        if t0 != 0 && interval > 0 {
            let elapsed = crate::shell::now().wrapping_sub(t0);
            if elapsed < interval {
                wait_early_released(interval - elapsed);
            }
        }
        let waited = crate::shell::now().wrapping_sub(t_wake0);
        let t = crate::shell::now();
        let dt_total = t.wrapping_sub(t0);
        // only stamp meaningful frames; skip giant gaps (time spent inside
        // apps that have their own loops) entirely
        if t0 != 0 && dt_total > 0 && dt_total < tsc_pm * 2000 {
            record_frame(t);
        }
        // loop-load metric from frames where we actually got to sleep
        if waited > 0 && dt_total > 0 && dt_total < tsc_pm * 2000 {
            let busy = ((dt_total - waited) * 100 / dt_total) as u32;
            core::ptr::addr_of_mut!(FRAME_BUSY).write(busy.min(100));
        }
        *last_p = t;
    }
}

/// Idle from now until `remaining` TSC ticks have passed (`hlt`-based) or a
/// *keystroke* is queued. IRQ0 is armed in PIT one-shot slices of at most
/// 65535 ticks.
fn wait_early_released(remaining_start: u64) {
    if remaining_start == 0 {
        return;
    }
    let tsc_pm = crate::shell::tsc_per_ms().max(1);
    let t0 = unsafe { *core::ptr::addr_of!(LAST_FRAME) };
    let mut remaining = remaining_start;
    while remaining > 0 {
        if crate::int::kb_pending() {
            return;
        }
        // remaining TSC ticks -> PIT ticks; slices capped at 65535
        let pit_ticks = (remaining * crate::int::PIT_HZ / (tsc_pm * 1000)).min(65535) as u16;
        if pit_ticks > 2 {
            crate::int::pit_oneshot(pit_ticks);
            core::hint::spin_loop(); // ordering fence into the idle wait
            crate::int::idle();
        }
        let elapsed = crate::shell::now().wrapping_sub(t0);
        remaining = remaining_start.saturating_sub(elapsed);
    }
}
pub const TITLE_ROW: usize = 1;
pub const BODY_TOP: usize = 2;
pub const BODY_BOTTOM: usize = 28;
pub const STATUS_ROW: usize = 29;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Theme {
    Classic, // TempleOS default: White background, Black text, Red/Blue chrome
    Blue,    // Retro Blue background, White text
    Dark,    // Dark background, White text
}

#[derive(Clone, Copy)]
pub struct Cell {
    pub ch: u8,
    pub fg: u8,
    pub bg: u8,
}

const EMPTY_BLUE: Cell = Cell {
    ch: b' ',
    fg: WHITE,
    bg: BLUE,
};

struct Screen {
    cells: [[Cell; COLS]; ROWS],
    x: usize,
    y: usize,
    fg: u8,
    bg: u8,
    theme: Theme,
    cur_x: usize,
    cur_y: usize,
}

struct SyncCell<T>(UnsafeCell<T>);
unsafe impl<T> Sync for SyncCell<T> {}

// --- window-target mode (used by the WM's per-window cell buffers) -----------
// When active, put_at_colored writes into a private cell buffer instead of
// the screen grid, and does not touch the framebuffer. The WM composites
// buffers onto the screen itself (term::blit_cells).
static mut TGT: *mut Cell = core::ptr::null_mut();
static mut TGT_W: usize = 0;
static mut TGT_H: usize = 0;
static mut TGT_CX: usize = 0;
static mut TGT_CY: usize = 0;
static mut TGT_FG: u8 = WHITE;
static mut TGT_BG: u8 = BLACK;
static mut MODERN_UI: bool = false;

pub fn set_modern_ui(v: bool) {
    unsafe {
        core::ptr::addr_of_mut!(MODERN_UI).write(v);
    }
}

pub fn is_modern_ui() -> bool {
    unsafe { *core::ptr::addr_of!(MODERN_UI) }
}
/// so the compositor knows the desktop background changed and needs a repaint.
static mut BG_DIRTY: bool = false;

pub fn begin_target(buf: *mut Cell, w: usize, h: usize) {
    unsafe {
        core::ptr::addr_of_mut!(TGT).write(buf);
        core::ptr::addr_of_mut!(TGT_W).write(w);
        core::ptr::addr_of_mut!(TGT_H).write(h);
        // default: continue at top-left with theme colors; caller can override
        core::ptr::addr_of_mut!(TGT_CX).write(0);
        core::ptr::addr_of_mut!(TGT_CY).write(0);
        core::ptr::addr_of_mut!(TGT_FG).write(theme_fg());
        core::ptr::addr_of_mut!(TGT_BG).write(theme_bg());
    }
}

pub fn end_target() {
    unsafe {
        core::ptr::addr_of_mut!(TGT).write(core::ptr::null_mut());
    }
}

pub fn in_target() -> bool {
    unsafe { !core::ptr::addr_of!(TGT).read().is_null() }
}

pub fn tgt_cursor() -> (usize, usize) {
    unsafe {
        (
            *core::ptr::addr_of!(TGT_CX),
            *core::ptr::addr_of!(TGT_CY),
        )
    }
}

pub fn set_tgt_cursor(x: usize, y: usize) {
    unsafe {
        core::ptr::addr_of_mut!(TGT_CX).write(x);
        core::ptr::addr_of_mut!(TGT_CY).write(y);
    }
}

pub fn set_tgt_colors(fg: u8, bg: u8) {
    unsafe {
        core::ptr::addr_of_mut!(TGT_FG).write(fg);
        core::ptr::addr_of_mut!(TGT_BG).write(bg);
    }
}

pub fn mark_bg_dirty() {
    unsafe {
        core::ptr::addr_of_mut!(BG_DIRTY).write(true);
    }
}


static mut DESK: [[Cell; COLS]; ROWS] = [[EMPTY_BLUE; COLS]; ROWS];

/// Snapshot current screen as the desktop background (call when desktop is clean).
pub fn snapshot_desktop() {
    unsafe {
        core::ptr::addr_of_mut!(DESK).write((*s()).cells);
    }
}

/// Restore screen cells from the desktop snapshot (no draw).
pub fn restore_desktop() {
    unsafe {
        (*s()).cells = *core::ptr::addr_of!(DESK);
    }
}

static SCREEN: SyncCell<Screen> = SyncCell(UnsafeCell::new(Screen {
    cells: [[EMPTY_BLUE; COLS]; ROWS],
    x: 0,
    y: BODY_TOP,
    fg: WHITE,
    bg: BLUE,
    theme: Theme::Blue,
    cur_x: 0,
    cur_y: BODY_TOP,
}));

fn s() -> *mut Screen {
    SCREEN.0.get()
}

static MODE_TEXT: SyncCell<([u8; 24], usize)> = SyncCell(UnsafeCell::new(([0; 24], 0)));

fn put_uint(buf: &mut [u8], mut v: u64) -> usize {
    let mut tmp = [0u8; 20];
    let mut i = tmp.len();
    if v == 0 {
        i -= 1;
        tmp[i] = b'0';
    }
    while v > 0 {
        i -= 1;
        tmp[i] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    let s = &tmp[i..];
    buf[..s.len()].copy_from_slice(s);
    s.len()
}

fn set_mode_text() {
    unsafe {
        let (buf, n) = &mut *MODE_TEXT.0.get();
        let mut i = 0;
        i += put_uint(&mut buf[i..], fb::width() as u64);
        buf[i] = b'x';
        i += 1;
        i += put_uint(&mut buf[i..], fb::height() as u64);
        buf[i] = b'x';
        i += 1;
        i += put_uint(&mut buf[i..], if fb::is_linear() { fb::bpp() as u64 } else { 16 });
        *n = i;
    }
}

pub fn mode_text() -> &'static str {
    unsafe {
        let (buf, n) = &*MODE_TEXT.0.get();
        core::str::from_utf8(&buf[..*n]).unwrap_or("?")
    }
}

pub fn theme() -> Theme {
    unsafe { (*s()).theme }
}

pub fn theme_fg() -> u8 {
    if is_modern_ui() {
        return LIGHT_GRAY;
    }
    unsafe {
        match (*s()).theme {
            Theme::Classic => BLACK,
            Theme::Blue | Theme::Dark => WHITE,
        }
    }
}

pub fn theme_bg() -> u8 {
    if is_modern_ui() {
        return BLACK;
    }
    unsafe {
        match (*s()).theme {
            Theme::Classic => WHITE,
            Theme::Blue => BLUE,
            Theme::Dark => BLACK,
        }
    }
}

pub fn set_theme(t: Theme) {
    unsafe {
        let p = s();
        (*p).theme = t;
        let bg = theme_bg();
        let fg = theme_fg();
        (*p).fg = fg;
        (*p).bg = bg;
        for y in BODY_TOP..=BODY_BOTTOM {
            for x in 0..COLS {
                if (*p).cells[y][x].bg == WHITE || (*p).cells[y][x].bg == BLUE || (*p).cells[y][x].bg == BLACK {
                    (*p).cells[y][x].bg = bg;
                }
                if (*p).cells[y][x].fg == WHITE || (*p).cells[y][x].fg == BLACK {
                    (*p).cells[y][x].fg = fg;
                }
            }
        }
    }
    draw_chrome();
    repaint();
}

pub unsafe fn draw_cell(x: usize, y: usize) {
    let p = s();
    let cell = (*p).cells[y][x];
    let cw = fb::cell_w();
    let ch = fb::cell_h();
    fb::fill_rect(x * cw, y * ch, cw, ch, cell.bg);
    if cell.ch != b' ' {
        fb::draw_glyph(x, y, cell.ch, cell.fg);
    }
}

pub fn put_at_colored(x: usize, y: usize, ch: u8, fg: u8, bg: u8) {
    unsafe {
        let t = core::ptr::addr_of!(TGT).read();
        if !t.is_null() {
            // window-buffer mode: no immediate framebuffer rendering
            let (w, h) = (
                core::ptr::addr_of!(TGT_W).read(),
                core::ptr::addr_of!(TGT_H).read(),
            );
            if x < w && y < h {
                *t.add(y * w + x) = Cell { ch, fg, bg };
            }
            return;
        }
    }
    if x >= COLS || y >= ROWS {
        return;
    }
    unsafe {
        (*s()).cells[y][x] = Cell { ch, fg, bg };
        draw_cell(x, y);
        core::ptr::addr_of_mut!(BG_DIRTY).write(true);
    }
}

#[allow(dead_code)]
pub fn put_at(x: usize, y: usize, ch: u8, fg: u8) {
    let bg = unsafe { (*s()).cells[y][x].bg };
    put_at_colored(x, y, ch, fg, bg);
}

pub fn draw_text_at_colored(x: usize, y: usize, text: &str, fg: u8, bg: u8) {
    for (i, b) in text.bytes().enumerate() {
        put_at_colored(x + i, y, b, fg, bg);
    }
}

#[allow(dead_code)]
pub fn draw_text_at(x: usize, y: usize, text: &str, fg: u8) {
    for (i, b) in text.bytes().enumerate() {
        put_at(x + i, y, b, fg);
    }
}

pub fn draw_chrome() {
    if is_modern_ui() {
        return;
    }
    let th = theme();
    let (menu_bg, menu_fg) = match th {
        Theme::Classic => (BROWN, WHITE),
        Theme::Blue => (LIGHT_GRAY, BLACK),
        Theme::Dark => (DARK_GRAY, WHITE),
    };

    // Row 0: Top Menu Bar (TempleOS Gold/Brown Menu)
    for x in 0..COLS {
        put_at_colored(x, MENU_ROW, b' ', menu_fg, menu_bg);
    }
    draw_text_at_colored(1, MENU_ROW, "[Menu]", YELLOW, menu_bg);
    draw_text_at_colored(8, MENU_ROW, "[File]", menu_fg, menu_bg);
    draw_text_at_colored(15, MENU_ROW, "[Edit]", menu_fg, menu_bg);
    draw_text_at_colored(22, MENU_ROW, "[Doc]", menu_fg, menu_bg);
    draw_text_at_colored(28, MENU_ROW, "[Sound]", menu_fg, menu_bg);
    draw_text_at_colored(36, MENU_ROW, "[Demos]", menu_fg, menu_bg);
    draw_text_at_colored(44, MENU_ROW, "[Theme]", menu_fg, menu_bg);
    draw_text_at_colored(52, MENU_ROW, "[Help]", menu_fg, menu_bg);

    let mut info_buf = [0u8; 24];
    let right_info = info_str(&mut info_buf);
    if COLS > right_info.len() + 2 {
        draw_text_at_colored(COLS - right_info.len() - 1, MENU_ROW, right_info, YELLOW, menu_bg);
    }

    // Row 1: Window Frame Header
    // CP437 box drawing: ┌ (0xda), ─ (0xc4), ┐ (0xbf), │ (0xb3), └ (0xc0), ┘ (0xd9)
    let frame_fg = match th {
        Theme::Classic => BLUE,
        Theme::Blue => LIGHT_CYAN,
        Theme::Dark => DARK_GRAY,
    };
    let bg = theme_bg();
    put_at_colored(0, TITLE_ROW, 0xda, frame_fg, bg); // '┌'
    for x in 1..COLS - 1 {
        put_at_colored(x, TITLE_ROW, 0xc4, frame_fg, bg); // '─'
    }
    put_at_colored(COLS - 1, TITLE_ROW, 0xbf, frame_fg, bg); // '┐'

    let (title_fg, close_fg) = match th {
        Theme::Classic => (RED, RED),
        Theme::Blue => (YELLOW, LIGHT_RED),
        Theme::Dark => (WHITE, LIGHT_RED),
    };

    let title_prefix = " [ ";
    let title = "C:/Home/CommandLine";
    let title_suffix = " ] ";
    let col = 3;
    draw_text_at_colored(col, TITLE_ROW, title_prefix, frame_fg, bg);
    draw_text_at_colored(col + title_prefix.len(), TITLE_ROW, title, title_fg, bg);
    draw_text_at_colored(col + title_prefix.len() + title.len(), TITLE_ROW, title_suffix, frame_fg, bg);

    let close_box = "[X]";
    if COLS > close_box.len() + 3 {
        draw_text_at_colored(COLS - close_box.len() - 2, TITLE_ROW, close_box, close_fg, bg);
    }

    status_bar();
}

pub fn status_bar() {
    if is_modern_ui() {
        return;
    }
    let th = theme();
    let (stat_bg, stat_fg) = match th {
        Theme::Classic => (LIGHT_GRAY, BLACK),
        Theme::Blue => (DARK_GRAY, LIGHT_GRAY),
        Theme::Dark => (DARK_GRAY, LIGHT_GRAY),
    };

    for x in 0..COLS {
        put_at_colored(x, STATUS_ROW, b' ', stat_fg, stat_bg);
    }
    draw_text_at_colored(1, STATUS_ROW, "READY", RED, stat_bg);
    draw_text_at_colored(8, STATUS_ROW, "F1: Help", BLACK, stat_bg);
    draw_text_at_colored(18, STATUS_ROW, "^T: DolDoc", BLACK, stat_bg);

    let cwd = crate::vfs::cwd_str();
    draw_text_at_colored(30, STATUS_ROW, cwd, BLUE, stat_bg);

    let mt = mode_text();
    let suffix = "  ring 0";
    let col = COLS - (mt.len() + suffix.len()) - 1;
    draw_text_at_colored(col, STATUS_ROW, mt, DARK_GRAY, stat_bg);
    draw_text_at_colored(col + mt.len(), STATUS_ROW, suffix, DARK_GRAY, stat_bg);
}

pub fn init() {
    set_mode_text();
    unsafe {
        let p = s();
        let bg = theme_bg();
        let fg = theme_fg();
        for y in 0..ROWS {
            for x in 0..COLS {
                (*p).cells[y][x] = Cell { ch: b' ', fg, bg };
            }
        }
        (*p).x = 0;
        (*p).y = BODY_TOP;
        (*p).fg = fg;
        (*p).bg = bg;
        (*p).cur_x = 0;
        (*p).cur_y = BODY_TOP;
    }
    draw_chrome();
    repaint();
}

pub fn repaint() {
    unsafe {
        for y in 0..ROWS {
            for x in 0..COLS {
                draw_cell(x, y);
            }
        }
    }
}

/// Copy desktop buffer to screen cells (no framebuffer draw).

/// Blit window buffer into screen cells without drawing to framebuffer.

/// Blit window buffer into screen cells without drawing to framebuffer.
pub fn blit_cells_to_screen(buf: *const Cell, w: usize, h: usize, dx: isize, dy: isize) {
    unsafe {
        let p = s();
        for by in 0..h {
            let sy = dy + by as isize;
            if sy < 0 || sy >= ROWS as isize {
                continue;
            }
            for bx in 0..w {
                let sx = dx + bx as isize;
                if sx < 0 || sx >= COLS as isize {
                    continue;
                }
                (*p).cells[sy as usize][sx as usize] = *buf.add(by * w + bx);
            }
        }
    }
}

/// Composite a window cell buffer onto the screen at (dx,dy) and render the
/// covered cells. Off-screen cells are clipped.

pub fn set_fg(fg: u8) {
    unsafe {
        if in_target() {
            core::ptr::addr_of_mut!(TGT_FG).write(fg);
        } else {
            (*s()).fg = fg;
        }
    }
}

pub fn set_bg(bg: u8) {
    unsafe {
        if in_target() {
            core::ptr::addr_of_mut!(TGT_BG).write(bg);
        } else {
            (*s()).bg = bg;
        }
    }
}

pub fn newline() {
    unsafe {
        if in_target() {
            let h = *core::ptr::addr_of!(TGT_H);
            core::ptr::addr_of_mut!(TGT_CX).write(0);
            let cy = *core::ptr::addr_of!(TGT_CY);
            if cy + 1 >= h {
                tgt_scroll();
            } else {
                core::ptr::addr_of_mut!(TGT_CY).write(cy + 1);
            }
            return;
        }
        let p = s();
        (*p).x = 0;
        if (*p).y >= BODY_BOTTOM {
            scroll();
        } else {
            (*p).y += 1;
        }
    }
}

unsafe fn tgt_scroll() {
    let t = core::ptr::addr_of!(TGT).read();
    let (w, h) = (
        *core::ptr::addr_of!(TGT_W),
        *core::ptr::addr_of!(TGT_H),
    );
    let fg = *core::ptr::addr_of!(TGT_FG);
    let bg = *core::ptr::addr_of!(TGT_BG);
    if w == 0 || h == 0 {
        return;
    }
    for y in 0..h.saturating_sub(1) {
        for x in 0..w {
            *t.add(y * w + x) = *t.add((y + 1) * w + x);
        }
    }
    let last = h - 1;
    for x in 0..w {
        *t.add(last * w + x) = Cell { ch: b' ', fg, bg };
    }
    core::ptr::addr_of_mut!(TGT_CY).write(last);
    core::ptr::addr_of_mut!(TGT_CX).write(0);
}

pub fn scroll() {
    unsafe {
        if in_target() {
            tgt_scroll();
            return;
        }
        let p = s();
        let def_bg = (*p).bg;
        let def_fg = (*p).fg;
        for y in BODY_TOP..BODY_BOTTOM {
            for x in 0..COLS {
                (*p).cells[y][x] = (*p).cells[y + 1][x];
            }
        }
        for x in 0..COLS {
            (*p).cells[BODY_BOTTOM][x] = Cell {
                ch: b' ',
                fg: def_fg,
                bg: def_bg,
            };
        }
        (*p).y = BODY_BOTTOM;
        (*p).x = 0;
    }
    repaint();
}

pub fn backspace() {
    unsafe {
        if in_target() {
            let cx = *core::ptr::addr_of!(TGT_CX);
            if cx > 0 {
                let nx = cx - 1;
                core::ptr::addr_of_mut!(TGT_CX).write(nx);
                let t = core::ptr::addr_of!(TGT).read();
                let w = *core::ptr::addr_of!(TGT_W);
                let cy = *core::ptr::addr_of!(TGT_CY);
                let fg = *core::ptr::addr_of!(TGT_FG);
                let bg = *core::ptr::addr_of!(TGT_BG);
                if w > 0 {
                    *t.add(cy * w + nx) = Cell { ch: b' ', fg, bg };
                }
            }
            return;
        }
        let p = s();
        if (*p).x > 0 {
            (*p).x -= 1;
            (*p).cells[(*p).y][(*p).x] = Cell {
                ch: b' ',
                fg: (*p).fg,
                bg: (*p).bg,
            };
            draw_cell((*p).x, (*p).y);
        }
    }
}

pub fn put_char_colored(c: u8, fg: u8, bg: u8) {
    unsafe {
        if in_target() {
            core::ptr::addr_of_mut!(TGT_FG).write(fg);
            core::ptr::addr_of_mut!(TGT_BG).write(bg);
            let w = *core::ptr::addr_of!(TGT_W);
            let h = *core::ptr::addr_of!(TGT_H);
            if w == 0 || h == 0 {
                return;
            }
            match c {
                b'\n' => newline(),
                b'\r' => core::ptr::addr_of_mut!(TGT_CX).write(0),
                b'\t' => {
                    for _ in 0..4 {
                        put_char_colored(b' ', fg, bg);
                    }
                }
                8 => backspace(),
                _ => {
                    let cx = *core::ptr::addr_of!(TGT_CX);
                    let cy = *core::ptr::addr_of!(TGT_CY);
                    let t = core::ptr::addr_of!(TGT).read();
                    *t.add(cy * w + cx) = Cell { ch: c, fg, bg };
                    let nx = cx + 1;
                    if nx >= w {
                        newline();
                    } else {
                        core::ptr::addr_of_mut!(TGT_CX).write(nx);
                    }
                }
            }
            return;
        }
        match c {
            b'\n' => newline(),
            b'\r' => (*s()).x = 0,
            b'\t' => {
                for _ in 0..4 {
                    put_char_colored(b' ', fg, bg);
                }
            }
            8 => backspace(),
            _ => {
                let p = s();
                (*p).cells[(*p).y][(*p).x] = Cell { ch: c, fg, bg };
                draw_cell((*p).x, (*p).y);
                core::ptr::addr_of_mut!(BG_DIRTY).write(true);
                (*p).x += 1;
                if (*p).x >= COLS {
                    newline();
                }
            }
        }
    }
}

pub fn put_char(c: u8) {
    let (fg, bg) = unsafe {
        if in_target() {
            (
                *core::ptr::addr_of!(TGT_FG),
                *core::ptr::addr_of!(TGT_BG),
            )
        } else {
            ((*s()).fg, (*s()).bg)
        }
    };
    put_char_colored(c, fg, bg);
}

pub fn print_str(text: &str) {
    for b in text.bytes() {
        put_char(b);
    }
}

pub fn print_num(n: i64) {
    if n < 0 {
        put_char(b'-');
    }
    let mut u = n.unsigned_abs();
    if u == 0 {
        put_char(b'0');
        return;
    }
    let mut buf = [0u8; 20];
    let mut i = buf.len();
    while u > 0 {
        i -= 1;
        buf[i] = b'0' + (u % 10) as u8;
        u /= 10;
    }
    for &b in &buf[i..] {
        put_char(b);
    }
}

pub fn print_hex32(v: u32) {
    print_str("0x");
    for shift in (0..8).rev() {
        let nib = ((v >> (shift * 4)) & 0xf) as u8;
        let c = if nib < 10 { b'0' + nib } else { b'a' + nib - 10 };
        put_char(c);
    }
}

pub fn print_hex64(v: u64) {
    print_str("0x");
    for shift in (0..16).rev() {
        let nib = ((v >> (shift * 4)) & 0xf) as u8;
        let c = if nib < 10 { b'0' + nib } else { b'a' + nib - 10 };
        put_char(c);
    }
}


pub fn clear() {
    if in_target() {
        // window-buffer mode: only clear the private buffer
        unsafe {
            let t = core::ptr::addr_of!(TGT).read();
            let (w, h) = (
                core::ptr::addr_of!(TGT_W).read(),
                core::ptr::addr_of!(TGT_H).read(),
            );
            let bg = if is_modern_ui() { BLACK } else { theme_bg() };
            let fg = if is_modern_ui() { LIGHT_GRAY } else { theme_fg() };
            for i in 0..w * h {
                *t.add(i) = Cell { ch: b' ', fg, bg };
            }
        }
        return;
    }
    if is_modern_ui() {
        unsafe {
            let p = s();
            for y in 0..ROWS {
                for x in 0..COLS {
                    (*p).cells[y][x] = Cell { ch: b' ', fg: LIGHT_GRAY, bg: BLACK };
                }
            }
            (*p).x = 0;
            (*p).y = 0;
            (*p).fg = LIGHT_GRAY;
            (*p).bg = BLACK;
            (*p).cur_x = 0;
            (*p).cur_y = 0;
        }
        repaint();
        return;
    }
    unsafe {
        let p = s();
        let bg = theme_bg();
        let fg = theme_fg();
        for y in BODY_TOP..=BODY_BOTTOM {
            for x in 0..COLS {
                (*p).cells[y][x] = Cell { ch: b' ', fg, bg };
            }
        }
        (*p).x = 0;
        (*p).y = BODY_TOP;
        (*p).fg = fg;
        (*p).bg = bg;
        (*p).cur_x = 0;
        (*p).cur_y = BODY_TOP;
        core::ptr::addr_of_mut!(BG_DIRTY).write(true);
    }
    draw_chrome();
    repaint();
}

// --- core::fmt support ---

pub struct Writer;

impl fmt::Write for Writer {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        print_str(text);
        Ok(())
    }
}

pub fn _print(args: fmt::Arguments) {
    use core::fmt::Write;
    let _ = Writer.write_fmt(args);
}

#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => {
        $crate::term::_print(core::format_args!($($arg)*))
    };
}

#[macro_export]
macro_rules! println {
    () => {
        $crate::print!("\n")
    };
    ($($arg:tt)*) => {
        $crate::print!("{}\n", core::format_args!($($arg)*))
    };
}
