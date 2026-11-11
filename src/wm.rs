//! Minimal overlapping-window server (icewm-simple) + desktop shell.
//!
//! Painter's-algorithm compositor over a static desktop (black bg + panel).
//! Apps live in movable windows with a one-cell title bar (drag to move,
//! `[X]` to close, click to focus/raise). Each app draws into its own
//! private cell buffer every frame via `term::begin_target()`, and the WM
//! recomposes desktop+windows each frame for smooth, trail-free movement.

use crate::keyboard::{self, Key};
use crate::mouse;
use crate::term::{self, Cell};

#[derive(Clone, Copy, PartialEq)]
pub enum Kind {
    Terminal,
    Calc,
    SysMon,
    FileMgr,
}

impl Kind {
    fn title(self) -> &'static str {
        match self {
            Kind::Terminal => "Terminal",
            Kind::Calc => "Calculator",
            Kind::SysMon => "System Monitor",
            Kind::FileMgr => "File Manager",
        }
    }
}

#[derive(Clone, Copy)]
pub struct Win {
    kind: Kind,
    x: isize,
    y: isize,
    w: usize,
    h: usize, // client area in cells (title bar is one extra row above)
    /// marks the buffer as needing a blit (app content changed)
    changed: bool,
}

const MAX_WINDOWS: usize = 6;
static mut WINS: [Win; MAX_WINDOWS] = [Win {
    kind: Kind::Calc,
    x: 0,
    y: 0,
    w: 0,
    h: 0,
    changed: false,
}; MAX_WINDOWS];
static mut N: usize = 0;

/// drag state
static mut DRAG: bool = false;
static mut GRAB_DX: isize = 0;
static mut GRAB_DY: isize = 0;
static mut PREV_LEFT: bool = false;
static mut PREV_MENU: bool = false;
static mut DESKTOP_MODE: bool = false;

struct AppApi {
    size: fn() -> (usize, usize),
    open: fn(),
    frame: fn(focused: bool, key: Option<Key>, click: Option<(usize, usize)>) -> bool,
    buffer: fn() -> *mut Cell,
}

fn api(kind: Kind) -> AppApi {
    match kind {
        Kind::Terminal => AppApi {
            size: crate::terminal::win_size,
            open: crate::terminal::win_open,
            frame: crate::terminal::win_frame,
            buffer: crate::terminal::win_buffer,
        },
        Kind::Calc => AppApi {
            size: crate::calc_gui::win_size,
            open: crate::calc_gui::win_open,
            frame: crate::calc_gui::win_frame,
            buffer: crate::calc_gui::win_buffer,
        },
        Kind::SysMon => AppApi {
            size: crate::sysmon::win_size,
            open: crate::sysmon::win_open,
            frame: crate::sysmon::win_frame,
            buffer: crate::sysmon::win_buffer,
        },
        Kind::FileMgr => AppApi {
            size: crate::filemgr::win_size,
            open: crate::filemgr::win_open,
            frame: crate::filemgr::win_frame,
            buffer: crate::filemgr::win_buffer,
        },
    }
}

/// Open a window for `kind` (focus/raise if it already exists).
/// In desktop mode this just adds the window; otherwise runs a blocking
/// compositor loop until all windows close.
pub fn open(kind: Kind) {
    unsafe {
        let n = *core::ptr::addr_of!(N);
        let wins = core::ptr::addr_of_mut!(WINS);
        // already open? raise + focus
        for i in 0..n {
            if (*wins)[i].kind == kind {
                raise(i);
                return;
            }
        }
        if n >= MAX_WINDOWS {
            return;
        }
        let a = api(kind);
        let (w, h) = (a.size)();
        let (x, y) = cascade(kind, n, w, h);
        (*wins)[n] = Win {
            kind,
            x,
            y,
            w,
            h,
            changed: true,
        };
        core::ptr::addr_of_mut!(N).write(n + 1);
        term::begin_target((a.buffer)(), w, h);
        (a.open)();
        term::end_target();
        if !*core::ptr::addr_of!(DESKTOP_MODE) {
            term::snapshot_desktop();
            run();
        }
    }
}

unsafe fn cascade(kind: Kind, n: usize, w: usize, h: usize) -> (isize, isize) {
    if kind == Kind::Terminal && n == 0 {
        return (2, 2);
    }
    let step = 3isize;
    let x = 2 + (n % 5) as isize * step;
    let y = 2 + (n % 5) as isize;
    (
        x.clamp(0, (term::COLS as isize - w as isize - 1).max(0)),
        y.clamp(1, (term::ROWS as isize - h as isize - 2).max(1)),
    )
}

/// move window `i` to the top of the z order (last = topmost + focused)
unsafe fn raise(i: usize) {
    let n = *core::ptr::addr_of!(N);
    if i >= n || i + 1 == n {
        return;
    }
    let wins = core::ptr::addr_of_mut!(WINS);
    let w = (*wins)[i];
    for j in i..n - 1 {
        (*wins)[j] = (*wins)[j + 1];
    }
    (*wins)[n - 1] = w;
    for k in 0..n {
        (*wins)[k].changed = true;
    }
}

unsafe fn close(i: usize) {
    let n = *core::ptr::addr_of!(N);
    if i >= n {
        return;
    }
    let wins = core::ptr::addr_of_mut!(WINS);
    for j in i..n - 1 {
        (*wins)[j] = (*wins)[j + 1];
    }
    core::ptr::addr_of_mut!(N).write(n - 1);
    term::mark_bg_dirty();
}

/// True while any window is open.
pub fn has_windows() -> bool {
    unsafe { *core::ptr::addr_of!(N) > 0 }
}

/// Blocking compositor loop (non-desktop mode). Returns when all windows close.
pub fn run() {
    while unsafe { *core::ptr::addr_of!(N) } > 0 {
        frame();
        term::frame_wait();
    }
}

/// Desktop shell: black background, top panel, always-on compositor.
pub fn desktop_loop() -> ! {
    unsafe {
        core::ptr::addr_of_mut!(DESKTOP_MODE).write(true);
    }
    term::set_modern_ui(true);
    draw_desktop_base();
    term::snapshot_desktop();
    // start with a terminal window
    open(Kind::Terminal);
    loop {
        frame();
        term::frame_wait();
    }
}

pub fn reset_desktop() {
    draw_desktop_base();
    term::snapshot_desktop();
}

fn draw_desktop_base() {
    // black desktop everywhere
    let bg = term::BLACK;
    for y in 0..term::ROWS {
        for x in 0..term::COLS {
            term::put_at_colored(x, y, b' ', term::LIGHT_GRAY, bg);
        }
    }
    draw_panel();
}

fn draw_panel() {
    let bg = term::DARK_GRAY;
    let fg = term::WHITE;
    // panel bar on row 0
    for x in 0..term::COLS {
        term::put_at_colored(x, 0, b' ', fg, bg);
    }
    term::draw_text_at_colored(1, 0, "TempleRust", term::YELLOW, bg);
    // app launchers
    term::draw_text_at_colored(13, 0, "[ Term ]", term::LIGHT_GREEN, term::BLUE);
    term::draw_text_at_colored(23, 0, "[ Files ]", term::LIGHT_CYAN, term::BLUE);
    term::draw_text_at_colored(34, 0, "[ Calc ]", term::YELLOW, term::BLUE);
    term::draw_text_at_colored(44, 0, "[ Monitor ]", term::LIGHT_GREEN, term::BLUE);
    // right side: clock + fps
    let secs = crate::shell::uptime_ms() / 1000;
    let (h, m, s) = ((secs / 3600) % 100, (secs / 60) % 60, secs % 60);
    let mut tb = [0u8; 16];
    // HH:MM:SS
    tb[0] = b'0' + (h / 10) as u8;
    tb[1] = b'0' + (h % 10) as u8;
    tb[2] = b':';
    tb[3] = b'0' + (m / 10) as u8;
    tb[4] = b'0' + (m % 10) as u8;
    tb[5] = b':';
    tb[6] = b'0' + (s / 10) as u8;
    tb[7] = b'0' + (s % 10) as u8;
    if let Ok(ts) = core::str::from_utf8(&tb[..8]) {
        let mut info = [0u8; 32];
        let mut i = 0usize;
        for &c in b"UP " {
            info[i] = c;
            i += 1;
        }
        for &c in ts.as_bytes() {
            info[i] = c;
            i += 1;
        }
        for &c in b"  FPS " {
            info[i] = c;
            i += 1;
        }
        // fps
        let f = term::fps();
        let mut fb = [0u8; 10];
        let mut j = fb.len();
        let mut v = f;
        if v == 0 {
            j -= 1;
            fb[j] = b'0';
        }
        while v > 0 {
            j -= 1;
            fb[j] = b'0' + (v % 10) as u8;
            v /= 10;
        }
        for &c in &fb[j..] {
            info[i] = c;
            i += 1;
        }
        if let Ok(s) = core::str::from_utf8(&info[..i]) {
            let col = term::COLS.saturating_sub(s.len() + 1);
            term::draw_text_at_colored(col, 0, s, term::LIGHT_GRAY, bg);
        }
    }
}

fn handle_panel_click(col: usize) -> bool {
    // [ Term ] 13..21, [ Files ] 23..32, [ Calc ] 34..42, [ Monitor ] 44..55
    if col >= 13 && col <= 21 {
        open(Kind::Terminal);
        true
    } else if col >= 23 && col <= 32 {
        open(Kind::FileMgr);
        true
    } else if col >= 34 && col <= 42 {
        open(Kind::Calc);
        true
    } else if col >= 44 && col <= 55 {
        open(Kind::SysMon);
        true
    } else {
        false
    }
}

/// One compositor frame: input routing, app updates, compositing.
pub fn frame() {
    unsafe {
        let mut n = *core::ptr::addr_of!(N);
        let wins = core::ptr::addr_of_mut!(WINS) as *mut Win;
        let cw = crate::fb::cell_w().max(1) as isize;
        let ch = crate::fb::cell_h().max(1) as isize;

        // --- input -------------------------------------------------------
        let key = keyboard::poll();
        let mev = mouse::poll();

        let mut drag = *core::ptr::addr_of!(DRAG);
        let mut gdx = *core::ptr::addr_of!(GRAB_DX);
        let mut gdy = *core::ptr::addr_of!(GRAB_DY);

        // press edge detection
        let prev_left = *core::ptr::addr_of!(PREV_LEFT);
        let left_now = mev.map(|m| m.left).unwrap_or(false);
        let pressed = left_now && !prev_left;
        core::ptr::addr_of_mut!(PREV_LEFT).write(left_now);

        if let Some(mev) = mev {
            let mcol = mev.x as isize / cw;
            let mrow = mev.y as isize / ch;

            if left_now {
                if !drag && pressed {
                    // hit-test windows topmost first
                    let mut hit: usize = usize::MAX;
                    let mut window_hit = false;
                    for i in (0..n).rev() {
                        let w = &*wins.add(i);
                        let (wx, wy, ww) = (w.x, w.y, w.w as isize);
                        // title bar
                        if mrow == wy && mcol >= wx && mcol < wx + ww {
                            window_hit = true;
                            if mcol >= wx + ww - 3 {
                                close(i);
                            } else {
                                hit = i;
                                drag = true;
                                gdx = mcol - wx;
                                gdy = 0;
                                core::ptr::addr_of_mut!(GRAB_DX).write(gdx);
                                core::ptr::addr_of_mut!(GRAB_DY).write(gdy);
                                core::ptr::addr_of_mut!(DRAG).write(true);
                            }
                            break;
                        }
                        // client area
                        if mrow > wy && mrow <= wy + w.h as isize && mcol >= wx && mcol < wx + ww {
                            window_hit = true;
                            hit = i;
                            break;
                        }
                    }
                    if hit != usize::MAX && hit < *core::ptr::addr_of!(N) {
                        raise(hit);
                    }
                    if !window_hit && mrow == 0 {
                        handle_panel_click(mcol as usize);
                    }
                } else if drag {
                    let nn = *core::ptr::addr_of!(N);
                    if nn > 0 {
                        let i = nn - 1;
                        let w = &mut *wins.add(i);
                        let nx = (mcol - gdx).clamp(0, (term::COLS as isize - w.w as isize).max(0));
                        let ny = (mrow - gdy).clamp(1, (term::ROWS as isize - w.h as isize - 1).max(1));
                        if nx != w.x || ny != w.y {
                            w.x = nx;
                            w.y = ny;
                        }
                    }
                }
            } else if drag {
                drag = false;
                core::ptr::addr_of_mut!(DRAG).write(false);
            }
        }

        n = *core::ptr::addr_of!(N);

        // --- app frames --------------------------------------------------
        let focused = if n > 0 { n - 1 } else { 0 };
        for i in 0..n {
            let (kind, wx, wy, w, h) = {
                let wp = &*wins.add(i);
                (wp.kind, wp.x, wp.y, wp.w, wp.h)
            };
            let is_focus = n > 0 && i == focused && !drag;
            let a = api(kind);
            let click = if is_focus && pressed {
                mev.map(|m| {
                    (
                        (m.x as isize / cw - wx) as isize,
                        (m.y as isize / ch - wy - 1) as isize,
                    )
                })
                .filter(|(c, r)| *c >= 0 && *r >= 0)
                .map(|(c, r)| (c as usize, r as usize))
                .filter(|(c, r)| *c < w && *r < h)
            } else {
                None
            };
            term::begin_target((a.buffer)(), w, h);
            // keep terminal cursor state coherent across frames via its own state;
            // other apps repaint fully
            let keep = (a.frame)(is_focus, if is_focus { key } else { None }, click);
            term::end_target();
            if !keep {
                close(i);
                n = *core::ptr::addr_of!(N);
                break; // restart frame logic next tick (simple)
            } else {
                let wp = &mut *wins.add(i);
                wp.changed = true;
            }
        }

        // --- composite ----------------------------------------------------
        n = *core::ptr::addr_of!(N);
        term::restore_desktop();
        // refresh panel info on the restored desktop
        draw_panel();
        for i in 0..n {
            let (kind, x, y, w, h) = {
                let wp = &*wins.add(i);
                (wp.kind, wp.x, wp.y, wp.w, wp.h)
            };
            term::blit_cells_to_screen((api(kind).buffer)(), w, h, x, y + 1);
        }
        // paint composed base once
        term::repaint();
        // title bars on top
        for i in 0..n {
            let (kind, x, y, w) = {
                let wp = &*wins.add(i);
                (wp.kind, wp.x, wp.y, wp.w)
            };
            draw_title(kind, x, y, w, i == n.saturating_sub(1));
        }
        mouse::draw_cursor();
    }
}

fn draw_title(kind: Kind, x: isize, y: isize, w: usize, focused: bool) {
    let (fg, bg) = if focused {
        (term::WHITE, term::DARK_GRAY)
    } else {
        (term::LIGHT_GRAY, term::BLACK)
    };
    for i in 0..w as isize {
        let px = x + i;
        if px >= 0 && (px as usize) < term::COLS {
            term::put_at_colored(px as usize, y as usize, b' ', fg, bg);
        }
    }
    // left accent + title
    let t = kind.title();
    for (i, b) in t.bytes().enumerate() {
        let px = x + 2 + i as isize;
        if px >= 0 && (px as usize) < term::COLS && px < x + w as isize - 4 {
            term::put_at_colored(px as usize, y as usize, b, if focused { term::YELLOW } else { fg }, bg);
        }
    }
    // close button
    let bx = x + w as isize - 3;
    if bx >= 0 {
        term::draw_text_at_colored(bx as usize, y as usize, "[X]", term::LIGHT_RED, bg);
    }
}
