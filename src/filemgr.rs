//! TempleOS-style Graphical File Manager (FileMgr).
//!
//! A dual-pane file browser with mouse support, directory navigation, and
//! inline file viewing and editing — a movable WM window.

use crate::editor;
use crate::keyboard::{self, Key};
use crate::term::{self, Cell};
use crate::vfs;

const FM_TOP: usize = 0;
const FM_BOTTOM: usize = 25;
const FM_LEFT: usize = 0;
const FM_RIGHT: usize = 75;
const SPLIT_X: usize = 24;
const WIN_W: usize = 76;
const WIN_H: usize = 27; // box rows 0..=25, hint row below at 26

const DIRS: &[&str] = &["C:/Home", "C:/Doc", "C:/Apps", "C:/Demo", "C:/"];

// --- WM window interface ------------------------------------------------------

static mut BUF: [Cell; WIN_W * WIN_H] = [Cell {
    ch: b' ',
    fg: 0,
    bg: 0,
}; WIN_W * WIN_H];

struct State {
    dir_idx: usize,
    file_idx: usize,
    in_files: bool,
}

static mut STATE: State = State {
    dir_idx: 0,
    file_idx: 0,
    in_files: false,
};

pub fn win_size() -> (usize, usize) {
    (WIN_W, WIN_H)
}

pub fn win_buffer() -> *mut Cell {
    core::ptr::addr_of_mut!(BUF) as *mut Cell
}

/// Legacy entry point (menu / REPL command): open as a WM window.
pub fn run() {
    crate::wm::open(crate::wm::Kind::FileMgr);
}

pub fn win_open() {
    unsafe {
        let s = core::ptr::addr_of_mut!(STATE);
        (*s).dir_idx = 0;
        (*s).file_idx = 0;
        (*s).in_files = false;
    }
    draw_chrome();
    unsafe {
        let s = &*core::ptr::addr_of!(STATE);
        draw_panes(s.dir_idx, s.file_idx, s.in_files);
    }
}

/// One frame: process input, draw the client into the window buffer.
/// Returns true when the content changed (needs a reblit).
pub fn win_frame(
    focused: bool,
    key: Option<Key>,
    click: Option<(usize, usize)>,
) -> bool {
    unsafe {
        let sp = core::ptr::addr_of_mut!(STATE);
        let mut changed = false;

        // --- keyboard ---
        if focused {
            if let Some(key) = key {
                match key {
                    Key::Esc | Key::Char(b'q') | Key::Char(b'Q') => return false, // close
                    Key::Tab => {
                        (*sp).in_files = !(*sp).in_files;
                        changed = true;
                    }
                    Key::Up => {
                        if !(*sp).in_files {
                            if (*sp).dir_idx > 0 {
                                (*sp).dir_idx -= 1;
                                (*sp).file_idx = 0;
                                changed = true;
                            }
                        } else if (*sp).file_idx > 0 {
                            (*sp).file_idx -= 1;
                            changed = true;
                        }
                    }
                    Key::Down => {
                        if !(*sp).in_files {
                            if (*sp).dir_idx + 1 < DIRS.len() {
                                (*sp).dir_idx += 1;
                                (*sp).file_idx = 0;
                                changed = true;
                            }
                        } else {
                            let count = files_in_dir_count(DIRS[(*sp).dir_idx]);
                            if (*sp).file_idx + 1 < count {
                                (*sp).file_idx += 1;
                                changed = true;
                            }
                        }
                    }
                    Key::Enter => {
                        if !(*sp).in_files {
                            (*sp).in_files = true;
                            (*sp).file_idx = 0;
                            changed = true;
                        } else if let Some(name) =
                            get_file_name_at(DIRS[(*sp).dir_idx], (*sp).file_idx)
                        {
                            open_fullscreen_modal();
                            editor::run_editor(name);
                            restore_after_modal(sp);
                            changed = true;
                        }
                    }
                    Key::Char(b'v') | Key::Char(b'V') => {
                        if let Some(name) = get_file_name_at(DIRS[(*sp).dir_idx], (*sp).file_idx)
                        {
                            open_fullscreen_modal();
                            view_file_popup(name);
                            restore_after_modal(sp);
                            changed = true;
                        }
                    }
                    Key::Char(b'e') | Key::Char(b'E') => {
                        if let Some(name) = get_file_name_at(DIRS[(*sp).dir_idx], (*sp).file_idx)
                        {
                            open_fullscreen_modal();
                            editor::run_editor(name);
                            restore_after_modal(sp);
                            changed = true;
                        }
                    }
                    _ => {}
                }
            }

            // --- mouse: pane clicks ---
            if let Some((col, row)) = click {
                // left pane click (directories)
                if col >= FM_LEFT + 1 && col < SPLIT_X && row >= FM_TOP + 2 && row < FM_TOP + 2 + DIRS.len()
                {
                    let d_idx = row - (FM_TOP + 2);
                    if d_idx < DIRS.len() {
                        (*sp).dir_idx = d_idx;
                        (*sp).file_idx = 0;
                        (*sp).in_files = false;
                        changed = true;
                    }
                }
                // right pane click (files)
                if col > SPLIT_X && col < FM_RIGHT && row >= FM_TOP + 2 {
                    let f_idx = row - (FM_TOP + 2);
                    let count = files_in_dir_count(DIRS[(*sp).dir_idx]);
                    if f_idx < count {
                        (*sp).file_idx = f_idx;
                        (*sp).in_files = true;
                        changed = true;
                    }
                }
            }
        }

        if changed {
            draw_panes((*sp).dir_idx, (*sp).file_idx, (*sp).in_files);
        }
        changed
    }
}

/// The view/editor modals are fullscreen and draw straight to the screen:
/// leave window-buffer mode for their duration.
fn open_fullscreen_modal() {
    term::end_target();
}

/// Restore state after a fullscreen modal: clear the desktop, redraw this
/// window's client, and tell the compositor everything changed.
unsafe fn restore_after_modal(sp: *mut State) {
    (*sp).dir_idx = (*sp).dir_idx.min(DIRS.len() - 1);
    term::mark_bg_dirty();
    draw_chrome();
    draw_panes((*sp).dir_idx, (*sp).file_idx, (*sp).in_files);
}

fn draw_chrome() {
    let bg = term::theme_bg();
    let border = term::CYAN;

    // Outer window border (title bar above is drawn by the WM)
    term::put_at_colored(FM_LEFT, FM_TOP, 0xda, border, bg);
    for x in FM_LEFT + 1..FM_RIGHT {
        term::put_at_colored(x, FM_TOP, 0xc4, border, bg);
    }
    term::put_at_colored(FM_RIGHT, FM_TOP, 0xbf, border, bg);

    for y in FM_TOP + 1..FM_BOTTOM {
        term::put_at_colored(FM_LEFT, y, 0xb3, border, bg);
        term::put_at_colored(SPLIT_X, y, 0xb3, border, bg);
        term::put_at_colored(FM_RIGHT, y, 0xb3, border, bg);
    }

    term::put_at_colored(FM_LEFT, FM_BOTTOM, 0xc0, border, bg);
    for x in FM_LEFT + 1..FM_RIGHT {
        term::put_at_colored(x, FM_BOTTOM, 0xc4, border, bg);
    }
    term::put_at_colored(SPLIT_X, FM_BOTTOM, 0xc1, border, bg); // bottom tee
    term::put_at_colored(SPLIT_X, FM_TOP, 0xc2, border, bg); // top tee
    term::put_at_colored(FM_RIGHT, FM_BOTTOM, 0xd9, border, bg);

    // Bottom hints (inside the window, below the box)
    let hy = FM_BOTTOM + 1;
    for x in 0..WIN_W {
        term::put_at_colored(x, hy, b' ', term::LIGHT_GRAY, term::DARK_GRAY);
    }
    term::draw_text_at_colored(
        1,
        hy,
        "Tab: Switch Pane   Arrows: Select   Enter/E: Edit   V: View   Q: Exit",
        term::WHITE,
        term::DARK_GRAY,
    );
}

fn draw_panes(cur_dir_idx: usize, cur_file_idx: usize, in_files: bool) {
    let bg = term::theme_bg();

    // Headers
    term::draw_text_at_colored(FM_LEFT + 2, FM_TOP + 1, "Folders", term::LIGHT_CYAN, bg);
    term::draw_text_at_colored(SPLIT_X + 2, FM_TOP + 1, "Files in ", term::LIGHT_CYAN, bg);
    term::draw_text_at_colored(SPLIT_X + 11, FM_TOP + 1, DIRS[cur_dir_idx], term::YELLOW, bg);

    // Left pane: Directory tree
    for (i, &d) in DIRS.iter().enumerate() {
        let y = FM_TOP + 2 + i;
        let is_sel = !in_files && i == cur_dir_idx;
        let (fg, row_bg) = if is_sel {
            (term::BLACK, term::YELLOW)
        } else {
            (term::WHITE, bg)
        };
        for x in FM_LEFT + 1..SPLIT_X {
            term::put_at_colored(x, y, b' ', fg, row_bg);
        }
        term::draw_text_at_colored(FM_LEFT + 2, y, d, fg, row_bg);
    }

    // Right pane: File list
    let dir = DIRS[cur_dir_idx];
    let count = vfs::entries_count();
    let mut file_row = 0usize;

    for i in 0..count {
        if let Some(e) = vfs::get_entry(i) {
            if e.ftype == vfs::FileType::File && e.path_str().eq_ignore_ascii_case(dir) {
                let y = FM_TOP + 2 + file_row;
                if y < FM_BOTTOM {
                    let is_sel = in_files && file_row == cur_file_idx;
                    let (fg, row_bg) = if is_sel {
                        (term::BLACK, term::CYAN)
                    } else if e.name_str().ends_with(".DD") {
                        (term::YELLOW, bg)
                    } else if e.name_str().ends_with(".HC") {
                        (term::LIGHT_GREEN, bg)
                    } else {
                        (term::WHITE, bg)
                    };

                    for x in SPLIT_X + 1..FM_RIGHT {
                        term::put_at_colored(x, y, b' ', fg, row_bg);
                    }
                    term::draw_text_at_colored(SPLIT_X + 2, y, e.name_str(), fg, row_bg);

                    // File size
                    draw_num(FM_RIGHT - 10, y, e.size as i64, fg, row_bg);
                    term::draw_text_at_colored(FM_RIGHT - 4, y, "B", fg, row_bg);
                }
                file_row += 1;
            }
        }
    }

    // Clear unused rows in right pane
    for y in (FM_TOP + 2 + file_row)..FM_BOTTOM {
        for x in SPLIT_X + 1..FM_RIGHT {
            term::put_at_colored(x, y, b' ', term::WHITE, bg);
        }
    }
}

fn files_in_dir_count(dir: &str) -> usize {
    let count = vfs::entries_count();
    let mut n = 0;
    for i in 0..count {
        if let Some(e) = vfs::get_entry(i) {
            if e.ftype == vfs::FileType::File && e.path_str().eq_ignore_ascii_case(dir) {
                n += 1;
            }
        }
    }
    n
}

fn get_file_name_at(dir: &str, target_idx: usize) -> Option<&'static str> {
    let count = vfs::entries_count();
    let mut n = 0;
    for i in 0..count {
        if let Some(e) = vfs::get_entry(i) {
            if e.ftype == vfs::FileType::File && e.path_str().eq_ignore_ascii_case(dir) {
                if n == target_idx {
                    return Some(e.name_str());
                }
                n += 1;
            }
        }
    }
    None
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

fn view_file_popup(name: &str) {
    if let Some(content) = vfs::read_file(name) {
        term::clear();
        term::draw_text_at_colored(1, 0, "[ Viewing File: ", term::YELLOW, term::DARK_GRAY);
        term::draw_text_at_colored(17, 0, name, term::WHITE, term::DARK_GRAY);
        term::draw_text_at_colored(17 + name.len(), 0, " ] (Press any key to return)", term::YELLOW, term::DARK_GRAY);

        crate::doldoc::print_doldoc(content);
        term::newline();

        while keyboard::poll().is_none() {
            term::frame_wait();
        }
    }
}
