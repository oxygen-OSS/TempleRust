//! Interactive DolDoc full-screen text editor ("Ed").
//!
//! Controls:
//!   Arrow keys: navigate
//!   Typing: insert characters
//!   Enter: insert newline
//!   Backspace: delete character
//!   <ESC>: save changes to VFS and exit
//!   <CTRL-Q>: exit without saving
//!   <CTRL-T>: toggle raw DolDoc display

use crate::keyboard::{self, Key};
use crate::term;
use crate::vfs;

const EDIT_MAX: usize = 2048;
const EDIT_TOP: usize = 1;
const EDIT_BOTTOM: usize = 28;
const EDIT_ROWS: usize = EDIT_BOTTOM - EDIT_TOP + 1; // 28 rows

pub fn run_editor(path: &str) {
    let mut buf = [0u8; EDIT_MAX];
    let mut len = 0;

    // Load existing content if file exists
    if let Some(content) = vfs::read_file(path) {
        let bytes = content.as_bytes();
        len = bytes.len().min(EDIT_MAX);
        buf[..len].copy_from_slice(&bytes[..len]);
    }

    let mut cursor_idx: usize = len;
    let mut scroll_line: usize = 0;

    term::clear();
    draw_chrome(path);
    render_text(&buf[..len], cursor_idx, &mut scroll_line);

    loop {
        if let Some(key) = keyboard::poll() {
            match key {
                Key::Esc => {
                    // Save and exit
                    let (parent, name) = vfs::resolve_path(path);
                    let content = core::str::from_utf8(&buf[..len]).unwrap_or("");
                    vfs::add_file(parent.as_str(), name.as_str(), content);
                    term::clear();
                    term::status_bar();
                    term::set_fg(term::GREEN);
                    term::print_str("Saved \"");
                    term::print_str(path);
                    term::print_str("\"\n");
                    term::set_fg(term::theme_fg());
                    return;
                }
                Key::Ctrl(b'q') => {
                    // Abort without saving
                    term::clear();
                    term::status_bar();
                    term::set_fg(term::YELLOW);
                    term::print_str("Edit cancelled.\n");
                    term::set_fg(term::theme_fg());
                    return;
                }
                Key::Ctrl(b't') => {
                    crate::doldoc::toggle_raw();
                    render_text(&buf[..len], cursor_idx, &mut scroll_line);
                }
                Key::Char(c) => {
                    if len < EDIT_MAX - 1 {
                        // Insert at cursor
                        let mut i = len;
                        while i > cursor_idx {
                            buf[i] = buf[i - 1];
                            i -= 1;
                        }
                        buf[cursor_idx] = c;
                        len += 1;
                        cursor_idx += 1;
                        render_text(&buf[..len], cursor_idx, &mut scroll_line);
                    }
                }
                Key::Enter => {
                    if len < EDIT_MAX - 1 {
                        let mut i = len;
                        while i > cursor_idx {
                            buf[i] = buf[i - 1];
                            i -= 1;
                        }
                        buf[cursor_idx] = b'\n';
                        len += 1;
                        cursor_idx += 1;
                        render_text(&buf[..len], cursor_idx, &mut scroll_line);
                    }
                }
                Key::Backspace => {
                    if cursor_idx > 0 {
                        cursor_idx -= 1;
                        for i in cursor_idx..len - 1 {
                            buf[i] = buf[i + 1];
                        }
                        len -= 1;
                        render_text(&buf[..len], cursor_idx, &mut scroll_line);
                    }
                }
                Key::Left => {
                    if cursor_idx > 0 {
                        cursor_idx -= 1;
                        render_text(&buf[..len], cursor_idx, &mut scroll_line);
                    }
                }
                Key::Right => {
                    if cursor_idx < len {
                        cursor_idx += 1;
                        render_text(&buf[..len], cursor_idx, &mut scroll_line);
                    }
                }
                Key::Up => {
                    cursor_idx = move_up(&buf[..len], cursor_idx);
                    render_text(&buf[..len], cursor_idx, &mut scroll_line);
                }
                Key::Down => {
                    cursor_idx = move_down(&buf[..len], cursor_idx);
                    render_text(&buf[..len], cursor_idx, &mut scroll_line);
                }
                Key::Home => {
                    cursor_idx = line_start(&buf[..len], cursor_idx);
                    render_text(&buf[..len], cursor_idx, &mut scroll_line);
                }
                Key::End => {
                    cursor_idx = line_end(&buf[..len], cursor_idx);
                    render_text(&buf[..len], cursor_idx, &mut scroll_line);
                }
                _ => {}
            }
        }
        core::hint::spin_loop();
    }
}

fn line_start(buf: &[u8], idx: usize) -> usize {
    let mut i = idx;
    while i > 0 && buf[i - 1] != b'\n' {
        i -= 1;
    }
    i
}

fn line_end(buf: &[u8], idx: usize) -> usize {
    let mut i = idx;
    while i < buf.len() && buf[i] != b'\n' {
        i += 1;
    }
    i
}

fn move_up(buf: &[u8], idx: usize) -> usize {
    let start = line_start(buf, idx);
    if start == 0 {
        return 0;
    }
    let col = idx - start;
    let prev_start = line_start(buf, start - 1);
    let prev_len = (start - 1) - prev_start;
    prev_start + col.min(prev_len)
}

fn move_down(buf: &[u8], idx: usize) -> usize {
    let start = line_start(buf, idx);
    let end = line_end(buf, idx);
    if end >= buf.len() {
        return buf.len();
    }
    let col = idx - start;
    let next_start = end + 1;
    let next_end = line_end(buf, next_start);
    let next_len = next_end - next_start;
    next_start + col.min(next_len)
}

fn draw_chrome(path: &str) {
    // Title bar
    for x in 0..term::COLS {
        term::put_at_colored(x, 0, b' ', term::BLACK, term::LIGHT_GRAY);
    }
    term::draw_text_at_colored(1, 0, "[ Ed: ", term::BLACK, term::LIGHT_GRAY);
    term::draw_text_at_colored(7, 0, path, term::RED, term::LIGHT_GRAY);
    term::draw_text_at_colored(7 + path.len(), 0, " ]", term::BLACK, term::LIGHT_GRAY);

    let right = "TempleOS DolDoc Editor";
    if term::COLS > right.len() + 2 {
        term::draw_text_at_colored(term::COLS - right.len() - 1, 0, right, term::DARK_GRAY, term::LIGHT_GRAY);
    }

    // Status bar
    update_status(1, 1);
}

fn update_status(line: usize, col: usize) {
    for x in 0..term::COLS {
        term::put_at_colored(x, term::STATUS_ROW, b' ', term::WHITE, term::DARK_GRAY);
    }
    let hints = "<ESC>: Save & Exit   <CTRL-Q>: Abort   <CTRL-T>: Raw";
    term::draw_text_at_colored(1, term::STATUS_ROW, hints, term::WHITE, term::DARK_GRAY);

    let mut pos_buf = [0u8; 32];
    let mut i = 0;
    i += copy_str(&mut pos_buf[i..], "Ln ");
    i += put_num(&mut pos_buf[i..], line as u64);
    i += copy_str(&mut pos_buf[i..], ", Col ");
    i += put_num(&mut pos_buf[i..], col as u64);
    let s = core::str::from_utf8(&pos_buf[..i]).unwrap_or("");
    if term::COLS > s.len() + 2 {
        term::draw_text_at_colored(term::COLS - s.len() - 1, term::STATUS_ROW, s, term::YELLOW, term::DARK_GRAY);
    }
}

fn copy_str(dst: &mut [u8], src: &str) -> usize {
    let b = src.as_bytes();
    dst[..b.len()].copy_from_slice(b);
    b.len()
}

fn put_num(buf: &mut [u8], mut v: u64) -> usize {
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

fn render_text(buf: &[u8], cursor_idx: usize, scroll_line: &mut usize) {
    // Compute current line and col of cursor
    let mut cur_line = 0usize;
    let mut cur_col = 0usize;
    let mut line_starts = [0usize; 256];
    let mut num_lines = 1;
    line_starts[0] = 0;

    for (i, &b) in buf.iter().enumerate() {
        if i == cursor_idx {
            cur_line = num_lines - 1;
            cur_col = i - line_starts[cur_line];
        }
        if b == b'\n' {
            if num_lines < line_starts.len() {
                line_starts[num_lines] = i + 1;
                num_lines += 1;
            }
        }
    }
    if cursor_idx == buf.len() {
        cur_line = num_lines - 1;
        cur_col = buf.len() - line_starts[cur_line];
    }

    // Adjust scroll
    if cur_line < *scroll_line {
        *scroll_line = cur_line;
    } else if cur_line >= *scroll_line + EDIT_ROWS {
        *scroll_line = cur_line - EDIT_ROWS + 1;
    }

    // Render screen
    let bg = term::theme_bg();
    let fg = term::theme_fg();

    for row in 0..EDIT_ROWS {
        let scr_y = EDIT_TOP + row;
        let line_num = *scroll_line + row;
        for x in 0..term::COLS {
            term::put_at_colored(x, scr_y, b' ', fg, bg);
        }

        if line_num < num_lines {
            let start = line_starts[line_num];
            let end = if line_num + 1 < num_lines {
                line_starts[line_num + 1].saturating_sub(1)
            } else {
                buf.len()
            };
            let line_bytes = &buf[start..end];
            for (col, &b) in line_bytes.iter().enumerate().take(term::COLS) {
                if (32..=126).contains(&b) {
                    term::put_at_colored(col, scr_y, b, fg, bg);
                } else if b == b'\t' {
                    term::put_at_colored(col, scr_y, b' ', fg, bg);
                }
            }
        }
    }

    // Highlight cursor
    if cur_line >= *scroll_line && cur_line < *scroll_line + EDIT_ROWS {
        let scr_y = EDIT_TOP + (cur_line - *scroll_line);
        let scr_x = cur_col.min(term::COLS - 1);
        let ch = if cursor_idx < buf.len() && buf[cursor_idx] != b'\n' {
            buf[cursor_idx]
        } else {
            b' '
        };
        term::put_at_colored(scr_x, scr_y, ch, bg, fg);
    }

    update_status(cur_line + 1, cur_col + 1);
}
