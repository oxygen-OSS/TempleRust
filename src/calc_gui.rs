//! Graphical interactive Calculator (CalcGui) — a movable WM window.

use crate::expr;
use crate::keyboard::Key;
use crate::term::{self, Cell};

const WIN_W: usize = 32;
const WIN_H: usize = 16;

const BTNS: [[&str; 4]; 4] = [
    ["7", "8", "9", "/"],
    ["4", "5", "6", "*"],
    ["1", "2", "3", "-"],
    ["0", "C", "=", "+"],
];

// --- WM window interface ------------------------------------------------------

static mut BUF: [Cell; WIN_W * WIN_H] = [Cell {
    ch: b' ',
    fg: 0,
    bg: 0,
}; WIN_W * WIN_H];

struct State {
    expr: [u8; 32],
    expr_len: usize,
    result: Option<i64>,
}

static mut STATE: State = State {
    expr: [0; 32],
    expr_len: 0,
    result: None,
};

pub fn win_size() -> (usize, usize) {
    (WIN_W, WIN_H)
}

pub fn win_buffer() -> *mut Cell {
    core::ptr::addr_of_mut!(BUF) as *mut Cell
}

/// Legacy entry point (menu / REPL command): open as a WM window.
pub fn run() {
    crate::wm::open(crate::wm::Kind::Calc);
}

pub fn win_open() {
    unsafe {
        let s = core::ptr::addr_of_mut!(STATE);
        (*s).expr = [0; 32];
        (*s).expr_len = 0;
        (*s).result = None;
    }
    draw_chrome();
    draw_display(&[], None);
}

/// One frame: process input, draw the client into the window buffer.
/// Returns true when the content changed (needs a reblit).
pub fn win_frame(
    focused: bool,
    key: Option<Key>,
    click: Option<(usize, usize)>,
) -> bool {
    unsafe {
        let s = core::ptr::addr_of_mut!(STATE);
        let mut changed = false;

        if focused {
            if let Some(key) = key {
                match key {
                    Key::Esc | Key::Char(b'q') | Key::Char(b'Q') => return false, // close
                    Key::Char(c) => match c {
                        b'0'..=b'9' | b'+' | b'-' | b'*' | b'/' | b'(' | b')' => {
                            if (*s).expr_len < (*s).expr.len() - 1 {
                                (*s).expr[(*s).expr_len] = c;
                                (*s).expr_len += 1;
                                (*s).result = None;
                                changed = true;
                            }
                        }
                        b'=' => {
                            eval(&mut *s);
                            changed = true;
                        }
                        b'c' | b'C' => {
                            (*s).expr_len = 0;
                            (*s).result = None;
                            changed = true;
                        }
                        _ => {}
                    },
                    Key::Enter => {
                        eval(&mut *s);
                        changed = true;
                    }
                    Key::Backspace => {
                        if (*s).expr_len > 0 {
                            (*s).expr_len -= 1;
                            (*s).result = None;
                            changed = true;
                        }
                    }
                    _ => {}
                }
            }

            if let Some((col, row)) = click {
                'outer: for r in 0..4 {
                    let by = 6 + r * 2;
                    if row != by {
                        continue;
                    }
                    for c in 0..4 {
                        let bx = 3 + c * 7;
                        if col >= bx && col < bx + 5 {
                            let label = BTNS[r][c];
                            match label {
                                "=" => eval(&mut *s),
                                "C" => {
                                    (*s).expr_len = 0;
                                    (*s).result = None;
                                }
                                _ => {
                                    if (*s).expr_len < (*s).expr.len() - 1 {
                                        (*s).expr[(*s).expr_len] = label.as_bytes()[0];
                                        (*s).expr_len += 1;
                                        (*s).result = None;
                                    }
                                }
                            }
                            changed = true;
                            break 'outer;
                        }
                    }
                }
            }
        }

        if changed {
            let s = &*core::ptr::addr_of!(STATE);
            draw_display(&s.expr[..s.expr_len], s.result);
        }
        changed
    }
}

unsafe fn eval(s: &mut State) {
    if let Ok(txt) = core::str::from_utf8(&s.expr[..s.expr_len]) {
        s.result = expr::eval(txt);
    }
}

// --- drawing (all coordinates client-relative) ---------------------------------

fn draw_chrome() {
    let bg = term::LIGHT_GRAY;
    let fg = term::BLACK;
    let border = term::DARK_GRAY;

    // window border
    term::put_at_colored(0, 0, 0xda, border, bg);
    for x in 1..WIN_W - 1 {
        term::put_at_colored(x, 0, 0xc4, border, bg);
    }
    term::put_at_colored(WIN_W - 1, 0, 0xbf, border, bg);

    for y in 1..WIN_H - 1 {
        term::put_at_colored(0, y, 0xb3, border, bg);
        for x in 1..WIN_W - 1 {
            term::put_at_colored(x, y, b' ', fg, bg);
        }
        term::put_at_colored(WIN_W - 1, y, 0xb3, border, bg);
    }

    term::put_at_colored(0, WIN_H - 1, 0xc0, border, bg);
    for x in 1..WIN_W - 1 {
        term::put_at_colored(x, WIN_H - 1, 0xc4, border, bg);
    }
    term::put_at_colored(WIN_W - 1, WIN_H - 1, 0xd9, border, bg);

    // display box
    let disp_y = 2;
    term::put_at_colored(2, disp_y, 0xda, border, term::WHITE);
    for x in 3..WIN_W - 3 {
        term::put_at_colored(x, disp_y, 0xc4, border, term::WHITE);
    }
    term::put_at_colored(WIN_W - 3, disp_y, 0xbf, border, term::WHITE);

    term::put_at_colored(2, disp_y + 1, 0xb3, border, term::WHITE);
    for x in 3..WIN_W - 3 {
        term::put_at_colored(x, disp_y + 1, b' ', term::BLACK, term::WHITE);
    }
    term::put_at_colored(WIN_W - 3, disp_y + 1, 0xb3, border, term::WHITE);

    term::put_at_colored(2, disp_y + 2, 0xc0, border, term::WHITE);
    for x in 3..WIN_W - 3 {
        term::put_at_colored(x, disp_y + 2, 0xc4, border, term::WHITE);
    }
    term::put_at_colored(WIN_W - 3, disp_y + 2, 0xd9, border, term::WHITE);

    // keypad
    for r in 0..4 {
        let by = 6 + r * 2;
        for c in 0..4 {
            let bx = 3 + c * 7;
            let label = BTNS[r][c];
            term::draw_text_at_colored(bx, by, "[ ", term::DARK_GRAY, term::CYAN);
            term::draw_text_at_colored(bx + 2, by, label, term::BLACK, term::CYAN);
            term::draw_text_at_colored(bx + 3, by, " ]", term::DARK_GRAY, term::CYAN);
        }
    }
}

fn draw_display(expr: &[u8], res: Option<i64>) {
    let disp_y = 3;
    let disp_x = 3;
    let disp_w = WIN_W - 6;

    for x in 0..disp_w {
        term::put_at_colored(disp_x + x, disp_y, b' ', term::BLACK, term::WHITE);
    }

    if let Some(v) = res {
        let mut buf = [0u8; 20];
        let mut i = buf.len();
        let mut u = v.unsigned_abs();
        if u == 0 {
            i -= 1;
            buf[i] = b'0';
        }
        while u > 0 {
            i -= 1;
            buf[i] = b'0' + (u % 10) as u8;
            u /= 10;
        }
        if v < 0 {
            i -= 1;
            buf[i] = b'-';
        }
        let s = core::str::from_utf8(&buf[i..]).unwrap_or("0");
        let start = disp_x + disp_w.saturating_sub(s.len());
        term::draw_text_at_colored(start, disp_y, s, term::BLUE, term::WHITE);
    } else {
        let s = core::str::from_utf8(expr).unwrap_or("");
        let start = disp_x + disp_w.saturating_sub(s.len());
        term::draw_text_at_colored(start, disp_y, s, term::BLACK, term::WHITE);
    }
}
