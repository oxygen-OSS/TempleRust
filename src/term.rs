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
    unsafe {
        match (*s()).theme {
            Theme::Classic => BLACK,
            Theme::Blue | Theme::Dark => WHITE,
        }
    }
}

pub fn theme_bg() -> u8 {
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
    if x >= COLS || y >= ROWS {
        return;
    }
    unsafe {
        (*s()).cells[y][x] = Cell { ch, fg, bg };
        draw_cell(x, y);
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

pub fn set_fg(fg: u8) {
    unsafe {
        (*s()).fg = fg;
    }
}

pub fn set_bg(bg: u8) {
    unsafe {
        (*s()).bg = bg;
    }
}

pub fn newline() {
    unsafe {
        let p = s();
        (*p).x = 0;
        if (*p).y >= BODY_BOTTOM {
            scroll();
        } else {
            (*p).y += 1;
        }
    }
}

pub fn scroll() {
    unsafe {
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
                (*p).x += 1;
                if (*p).x >= COLS {
                    newline();
                }
            }
        }
    }
}

pub fn put_char(c: u8) {
    let (fg, bg) = unsafe { ((*s()).fg, (*s()).bg) };
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
        let c = if nib < 10 {
            b'0' + nib
        } else {
            b'a' + nib - 10
        };
        put_char(c);
    }
}

pub fn print_hex64(v: u64) {
    print_str("0x");
    for shift in (0..16).rev() {
        let nib = ((v >> (shift * 4)) & 0xf) as u8;
        let c = if nib < 10 {
            b'0' + nib
        } else {
            b'a' + nib - 10
        };
        put_char(c);
    }
}

pub fn cursor(on: bool) {
    unsafe {
        let p = s();
        if (*p).cur_x != (*p).x || (*p).cur_y != (*p).y {
            draw_cell((*p).cur_x, (*p).cur_y);
            (*p).cur_x = (*p).x;
            (*p).cur_y = (*p).y;
        }
        draw_cell((*p).x, (*p).y);
        if on {
            let cw = fb::cell_w();
            let ch = fb::cell_h();
            let cur_col = if (*p).bg == BLACK { WHITE } else { BLACK };
            fb::fill_rect((*p).x * cw, (*p).y * ch + ch - 2, cw, 2, cur_col);
        }
    }
}

pub fn clear() {
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
