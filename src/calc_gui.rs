//! Graphical interactive Calculator Window (CalcGui).

use crate::expr;
use crate::keyboard::{self, Key};
use crate::mouse;
use crate::term;

const WIN_X: usize = 24;
const WIN_Y: usize = 6;
const WIN_W: usize = 32;
const WIN_H: usize = 16;

const BTNS: [[&str; 4]; 4] = [
    ["7", "8", "9", "/"],
    ["4", "5", "6", "*"],
    ["1", "2", "3", "-"],
    ["0", "C", "=", "+"],
];

pub fn run() {
    term::clear();
    draw_window();

    let mut expr_buf = [0u8; 32];
    let mut expr_len = 0usize;
    let mut result_val: Option<i64> = None;

    draw_display(&expr_buf[..expr_len], result_val);

    loop {
        if let Some(key) = keyboard::poll() {
            match key {
                Key::Esc | Key::Char(b'q') | Key::Char(b'Q') => break,
                Key::Char(c) => match c {
                    b'0'..=b'9' | b'+' | b'-' | b'*' | b'/' | b'(' | b')' => {
                        if expr_len < expr_buf.len() - 1 {
                            expr_buf[expr_len] = c;
                            expr_len += 1;
                            result_val = None;
                            draw_display(&expr_buf[..expr_len], result_val);
                        }
                    }
                    b'=' => {
                        if let Ok(s) = core::str::from_utf8(&expr_buf[..expr_len]) {
                            result_val = expr::eval(s);
                            draw_display(&expr_buf[..expr_len], result_val);
                        }
                    }
                    b'c' | b'C' => {
                        expr_len = 0;
                        result_val = None;
                        draw_display(&expr_buf[..expr_len], result_val);
                    }
                    _ => {}
                },
                Key::Enter => {
                    if let Ok(s) = core::str::from_utf8(&expr_buf[..expr_len]) {
                        result_val = expr::eval(s);
                        draw_display(&expr_buf[..expr_len], result_val);
                    }
                }
                Key::Backspace => {
                    if expr_len > 0 {
                        expr_len -= 1;
                        result_val = None;
                        draw_display(&expr_buf[..expr_len], result_val);
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

                // Close [X]
                if row == WIN_Y && col >= WIN_X + WIN_W - 4 {
                    break;
                }

                // Check keypad buttons
                for r in 0..4 {
                    let by = WIN_Y + 6 + r * 2;
                    if row == by {
                        for c in 0..4 {
                            let bx = WIN_X + 3 + c * 7;
                            if col >= bx && col < bx + 5 {
                                let label = BTNS[r][c];
                                match label {
                                    "=" => {
                                        if let Ok(s) = core::str::from_utf8(&expr_buf[..expr_len]) {
                                            result_val = expr::eval(s);
                                            draw_display(&expr_buf[..expr_len], result_val);
                                        }
                                    }
                                    "C" => {
                                        expr_len = 0;
                                        result_val = None;
                                        draw_display(&expr_buf[..expr_len], result_val);
                                    }
                                    _ => {
                                        if expr_len < expr_buf.len() - 1 {
                                            expr_buf[expr_len] = label.as_bytes()[0];
                                            expr_len += 1;
                                            result_val = None;
                                            draw_display(&expr_buf[..expr_len], result_val);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        core::hint::spin_loop();
    }

    term::clear();
    term::status_bar();
}

fn draw_window() {
    let bg = term::LIGHT_GRAY;
    let fg = term::BLACK;
    let border = term::DARK_GRAY;

    // Window chrome
    term::put_at_colored(WIN_X, WIN_Y, 0xda, border, bg);
    for x in WIN_X + 1..WIN_X + WIN_W - 1 {
        term::put_at_colored(x, WIN_Y, 0xc4, border, bg);
    }
    term::put_at_colored(WIN_X + WIN_W - 1, WIN_Y, 0xbf, border, bg);

    term::draw_text_at_colored(WIN_X + 2, WIN_Y, "[ TempleOS Calculator ]", term::BLUE, bg);
    term::draw_text_at_colored(WIN_X + WIN_W - 4, WIN_Y, "[X]", term::RED, bg);

    for y in WIN_Y + 1..WIN_Y + WIN_H - 1 {
        term::put_at_colored(WIN_X, y, 0xb3, border, bg);
        for x in WIN_X + 1..WIN_X + WIN_W - 1 {
            term::put_at_colored(x, y, b' ', fg, bg);
        }
        term::put_at_colored(WIN_X + WIN_W - 1, y, 0xb3, border, bg);
    }

    term::put_at_colored(WIN_X, WIN_Y + WIN_H - 1, 0xc0, border, bg);
    for x in WIN_X + 1..WIN_X + WIN_W - 1 {
        term::put_at_colored(x, WIN_Y + WIN_H - 1, 0xc4, border, bg);
    }
    term::put_at_colored(WIN_X + WIN_W - 1, WIN_Y + WIN_H - 1, 0xd9, border, bg);

    // Display box border
    let disp_y = WIN_Y + 2;
    term::put_at_colored(WIN_X + 2, disp_y, 0xda, border, term::WHITE);
    for x in WIN_X + 3..WIN_X + WIN_W - 3 {
        term::put_at_colored(x, disp_y, 0xc4, border, term::WHITE);
    }
    term::put_at_colored(WIN_X + WIN_W - 3, disp_y, 0xbf, border, term::WHITE);

    term::put_at_colored(WIN_X + 2, disp_y + 1, 0xb3, border, term::WHITE);
    for x in WIN_X + 3..WIN_X + WIN_W - 3 {
        term::put_at_colored(x, disp_y + 1, b' ', term::BLACK, term::WHITE);
    }
    term::put_at_colored(WIN_X + WIN_W - 3, disp_y + 1, 0xb3, border, term::WHITE);

    term::put_at_colored(WIN_X + 2, disp_y + 2, 0xc0, border, term::WHITE);
    for x in WIN_X + 3..WIN_X + WIN_W - 3 {
        term::put_at_colored(x, disp_y + 2, 0xc4, border, term::WHITE);
    }
    term::put_at_colored(WIN_X + WIN_W - 3, disp_y + 2, 0xd9, border, term::WHITE);

    // Keypad buttons
    for r in 0..4 {
        let by = WIN_Y + 6 + r * 2;
        for c in 0..4 {
            let bx = WIN_X + 3 + c * 7;
            let label = BTNS[r][c];
            term::draw_text_at_colored(bx, by, "[ ", term::DARK_GRAY, term::CYAN);
            term::draw_text_at_colored(bx + 2, by, label, term::BLACK, term::CYAN);
            term::draw_text_at_colored(bx + 3, by, " ]", term::DARK_GRAY, term::CYAN);
        }
    }
}

fn draw_display(expr: &[u8], res: Option<i64>) {
    let disp_y = WIN_Y + 3;
    let disp_x = WIN_X + 3;
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
