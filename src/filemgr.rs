//! TempleOS-style Graphical File Manager (FileMgr).
//!
//! A windowed dual-pane file browser with mouse support, directory navigation,
//! and inline file viewing and editing.

use crate::editor;
use crate::keyboard::{self, Key};
use crate::mouse;
use crate::term;
use crate::vfs;

const FM_TOP: usize = 2;
const FM_BOTTOM: usize = 27;
const FM_LEFT: usize = 2;
const FM_RIGHT: usize = 77;
const SPLIT_X: usize = 26;

const DIRS: &[&str] = &["C:/Home", "C:/Doc", "C:/Apps", "C:/Demo", "C:/"];

pub fn run() {
    let mut cur_dir_idx = 0usize;
    let mut cur_file_idx = 0usize;
    let mut in_files = false;

    term::clear();
    draw_chrome();
    draw_panes(cur_dir_idx, cur_file_idx, in_files);

    loop {
        if let Some(key) = keyboard::poll() {
            match key {
                Key::Esc | Key::Char(b'q') | Key::Char(b'Q') => break,
                Key::Tab => {
                    in_files = !in_files;
                    draw_panes(cur_dir_idx, cur_file_idx, in_files);
                }
                Key::Up => {
                    if !in_files {
                        if cur_dir_idx > 0 {
                            cur_dir_idx -= 1;
                            cur_file_idx = 0;
                            draw_panes(cur_dir_idx, cur_file_idx, in_files);
                        }
                    } else if cur_file_idx > 0 {
                        cur_file_idx -= 1;
                        draw_panes(cur_dir_idx, cur_file_idx, in_files);
                    }
                }
                Key::Down => {
                    if !in_files {
                        if cur_dir_idx + 1 < DIRS.len() {
                            cur_dir_idx += 1;
                            cur_file_idx = 0;
                            draw_panes(cur_dir_idx, cur_file_idx, in_files);
                        }
                    } else {
                        let count = files_in_dir_count(DIRS[cur_dir_idx]);
                        if cur_file_idx + 1 < count {
                            cur_file_idx += 1;
                            draw_panes(cur_dir_idx, cur_file_idx, in_files);
                        }
                    }
                }
                Key::Enter => {
                    if !in_files {
                        in_files = true;
                        cur_file_idx = 0;
                        draw_panes(cur_dir_idx, cur_file_idx, in_files);
                    } else {
                        // Open selected file in editor
                        if let Some(name) = get_file_name_at(DIRS[cur_dir_idx], cur_file_idx) {
                            editor::run_editor(name);
                            term::clear();
                            draw_chrome();
                            draw_panes(cur_dir_idx, cur_file_idx, in_files);
                        }
                    }
                }
                Key::Char(b'v') | Key::Char(b'V') => {
                    // View file
                    if let Some(name) = get_file_name_at(DIRS[cur_dir_idx], cur_file_idx) {
                        view_file_popup(name);
                        term::clear();
                        draw_chrome();
                        draw_panes(cur_dir_idx, cur_file_idx, in_files);
                    }
                }
                Key::Char(b'e') | Key::Char(b'E') => {
                    // Edit file
                    if let Some(name) = get_file_name_at(DIRS[cur_dir_idx], cur_file_idx) {
                        editor::run_editor(name);
                        term::clear();
                        draw_chrome();
                        draw_panes(cur_dir_idx, cur_file_idx, in_files);
                    }
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
                if row == FM_TOP && col >= FM_RIGHT - 4 {
                    break;
                }

                // Left pane click (directories)
                if col >= FM_LEFT + 1 && col < SPLIT_X && row >= FM_TOP + 2 && row < FM_TOP + 2 + DIRS.len() {
                    let d_idx = row - (FM_TOP + 2);
                    if d_idx < DIRS.len() {
                        cur_dir_idx = d_idx;
                        cur_file_idx = 0;
                        in_files = false;
                        draw_panes(cur_dir_idx, cur_file_idx, in_files);
                    }
                }

                // Right pane click (files)
                if col > SPLIT_X && col < FM_RIGHT && row >= FM_TOP + 2 {
                    let f_idx = row - (FM_TOP + 2);
                    let count = files_in_dir_count(DIRS[cur_dir_idx]);
                    if f_idx < count {
                        cur_file_idx = f_idx;
                        in_files = true;
                        draw_panes(cur_dir_idx, cur_file_idx, in_files);
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
    let border = term::CYAN;

    // Outer window border
    term::put_at_colored(FM_LEFT, FM_TOP, 0xda, border, bg);
    for x in FM_LEFT + 1..FM_RIGHT {
        term::put_at_colored(x, FM_TOP, 0xc4, border, bg);
    }
    term::put_at_colored(FM_RIGHT, FM_TOP, 0xbf, border, bg);

    let title = " [ TempleOS File Manager (FileMgr) ] ";
    term::draw_text_at_colored(FM_LEFT + 3, FM_TOP, title, term::YELLOW, bg);
    term::draw_text_at_colored(FM_RIGHT - 4, FM_TOP, "[X]", term::LIGHT_RED, bg);

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

    // Bottom hints
    for x in 0..term::COLS {
        term::put_at_colored(x, term::STATUS_ROW, b' ', term::LIGHT_GRAY, term::DARK_GRAY);
    }
    term::draw_text_at_colored(
        1,
        term::STATUS_ROW,
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
            core::hint::spin_loop();
        }
    }
}
