//! Terminal window: a REPL running inside a movable WM window.

use crate::keyboard::Key;
use crate::shell;
use crate::term::{self, Cell};
use crate::vfs;

const W: usize = 76;
const H: usize = 22;

static mut BUF: [Cell; W * H] = [Cell {
    ch: b' ',
    fg: term::LIGHT_GRAY,
    bg: term::BLACK,
}; W * H];

struct State {
    input: [u8; shell::MAX_LINE],
    len: usize,
    cx: usize,
    cy: usize,
    on: bool,
    last_blink: u64,
    ready: bool,
}

static mut ST: State = State {
    input: [0; shell::MAX_LINE],
    len: 0,
    cx: 0,
    cy: 0,
    on: true,
    last_blink: 0,
    ready: false,
};

pub fn win_size() -> (usize, usize) {
    (W, H)
}

pub fn win_buffer() -> *mut Cell {
    core::ptr::addr_of_mut!(BUF) as *mut Cell
}

pub fn run() {
    crate::wm::open(crate::wm::Kind::Terminal);
}

fn fill_bg() {
    unsafe {
        let buf = win_buffer();
        for i in 0..W * H {
            *buf.add(i) = Cell {
                ch: b' ',
                fg: term::LIGHT_GRAY,
                bg: term::BLACK,
            };
        }
    }
}

fn prompt() {
    term::set_tgt_colors(term::LIGHT_GREEN, term::BLACK);
    term::print_str(vfs::cwd_str());
    term::set_tgt_colors(term::LIGHT_CYAN, term::BLACK);
    term::print_str(" $ ");
    term::set_tgt_colors(term::LIGHT_GRAY, term::BLACK);
}

pub fn win_open() {
    fill_bg();
    unsafe {
        let st = core::ptr::addr_of_mut!(ST);
        (*st).len = 0;
        (*st).input = [0; shell::MAX_LINE];
        (*st).cx = 0;
        (*st).cy = 0;
        (*st).on = true;
        (*st).last_blink = shell::now();
        (*st).ready = true;
        // banner
        term::set_tgt_colors(term::YELLOW, term::BLACK);
        term::print_str("TempleRust Terminal");
        term::set_tgt_colors(term::DARK_GRAY, term::BLACK);
        term::print_str("  -  type Help; for commands\n\n");
        prompt();
        let (cx, cy) = term::tgt_cursor();
        (*st).cx = cx;
        (*st).cy = cy;
    }
}

fn is_fullscreen_cmd(line: &[u8]) -> bool {
    // trim and strip trailing semicolon
    let mut s = line;
    while let Some((&b' ', rest)) = s.split_first() { s = rest; }
    while let Some((&b, rest)) = s.split_last() { if b == b';' || b == b' ' { s = rest; } else { break; } }
    // command token
    let end = s.iter().position(|&b| b == b' ' || b == b'(').unwrap_or(s.len());
    let cmd = &s[..end];
    fn eq(a: &[u8], b: &[u8]) -> bool {
        a.len() == b.len() && a.iter().zip(b).all(|(x,y)| x.to_ascii_lowercase()==y.to_ascii_lowercase())
    }
    eq(cmd, b"snake") || eq(cmd, b"life") || eq(cmd, b"lines") || eq(cmd, b"graphics")
        || eq(cmd, b"bench") || eq(cmd, b"ed") || eq(cmd, b"edit")
}

fn run_fullscreen(st: &mut State, cmd: &[u8]) {
    // leave window buffer / compositor context, run on real screen
    term::end_target();
    crate::wm::reset_desktop();
    crate::shell::execute(cmd);
    // restore desktop for compositor and terminal buffer
    crate::wm::reset_desktop();
    term::begin_target(win_buffer(), W, H);
    fill_bg();
    term::set_tgt_cursor(0, 0);
    prompt();
    let (cx, cy) = term::tgt_cursor();
    st.cx = cx;
    st.cy = cy;
    st.len = 0;
    st.on = true;
    st.last_blink = crate::shell::now();
}
fn redraw_input(st: &mut State) {
    // cursor is positioned after prompt; caller manages cx/cy context
    // clear from current cx to end of line in buffer, print input
    let (cx, cy) = (st.cx, st.cy);
    term::set_tgt_cursor(cx, cy);
    term::set_tgt_colors(term::LIGHT_GRAY, term::BLACK);
    for &b in &st.input[..st.len] {
        term::put_char(b);
    }
    // clear rest of line
    let (ex, _) = term::tgt_cursor();
    for x in ex..W {
        term::put_at_colored(x, cy, b' ', term::LIGHT_GRAY, term::BLACK);
    }
    term::set_tgt_cursor(ex, cy);
}

pub fn win_frame(focused: bool, key: Option<Key>, _click: Option<(usize, usize)>) -> bool {
    unsafe {
        let st = &mut *core::ptr::addr_of_mut!(ST);
        if !st.ready {
            return false;
        }
        let mut changed = false;

        // restore cursor context for this buffer
        term::set_tgt_cursor(st.cx, st.cy);
        term::set_tgt_colors(term::LIGHT_GRAY, term::BLACK);

        if focused {
            if let Some(key) = key {
                match key {
                    Key::Char(c) => {
                        if st.len < shell::MAX_LINE - 1 {
                            st.input[st.len] = c;
                            st.len += 1;
                            redraw_input(st);
                            term::set_tgt_cursor(st.cx + st.len, st.cy);
                            changed = true;
                        }
                    }
                    Key::Backspace => {
                        if st.len > 0 {
                            st.len -= 1;
                            redraw_input(st);
                            term::set_tgt_cursor(st.cx + st.len, st.cy);
                            changed = true;
                        }
                    }
                    Key::Enter => {
                        // erase visible cursor
                        term::put_at_colored(
                            st.cx + st.len,
                            st.cy,
                            b' ',
                            term::LIGHT_GRAY,
                            term::BLACK,
                        );
                        term::set_tgt_cursor(st.cx + st.len, st.cy);
                        term::print_str("\n");
                        shell::set_hist_view(-1);
                        if st.len > 0 {
                            shell::hist_push(&st.input[..st.len]);
                        }
                        let mut cmd = [0u8; shell::MAX_LINE];
                        cmd[..st.len].copy_from_slice(&st.input[..st.len]);
                        let n = st.len;
                        if is_fullscreen_cmd(&cmd[..n]) {
                            run_fullscreen(st, &cmd[..n]);
                            changed = true;
                        } else {
                            st.len = 0;
                            shell::execute(&cmd[..n]);
                            // after execute (may have run nested WM loops), repaint prompt
                            term::set_tgt_colors(term::LIGHT_GRAY, term::BLACK);
                            prompt();
                            let (cx, cy) = term::tgt_cursor();
                            st.cx = cx;
                            st.cy = cy;
                            st.on = true;
                            st.last_blink = shell::now();
                            changed = true;
                        }
                    }
                    Key::Up => {
                        let hn = shell::hist_count();
                        if hn > 0 {
                            let view = shell::hist_view();
                            let newv = if view < 0 {
                                shell::draft_set(&st.input[..st.len]);
                                (hn - 1) as isize
                            } else if view > 0 {
                                view - 1
                            } else {
                                view
                            };
                            if newv != view {
                                shell::set_hist_view(newv);
                                let row = shell::hist_get(newv as usize);
                                let l = row.len().min(st.input.len());
                                st.input[..l].copy_from_slice(&row[..l]);
                                st.len = l;
                                redraw_input(st);
                                term::set_tgt_cursor(st.cx + st.len, st.cy);
                                changed = true;
                            }
                        }
                    }
                    Key::Down => {
                        let view = shell::hist_view();
                        if view >= 0 {
                            let hn = shell::hist_count();
                            let next = view + 1;
                            if (next as usize) < hn {
                                shell::set_hist_view(next);
                                let row = shell::hist_get(next as usize);
                                let l = row.len().min(st.input.len());
                                st.input[..l].copy_from_slice(&row[..l]);
                                st.len = l;
                            } else {
                                shell::set_hist_view(-1);
                                let d = shell::draft_get();
                                let l = d.len().min(st.input.len());
                                st.input[..l].copy_from_slice(&d[..l]);
                                st.len = l;
                            }
                            redraw_input(st);
                            term::set_tgt_cursor(st.cx + st.len, st.cy);
                            changed = true;
                        }
                    }
                    Key::Ctrl(b'l') => {
                        fill_bg();
                        term::set_tgt_cursor(0, 0);
                        prompt();
                        let (cx, cy) = term::tgt_cursor();
                        st.cx = cx;
                        st.cy = cy;
                        redraw_input(st);
                        term::set_tgt_cursor(cx + st.len, cy);
                        changed = true;
                    }
                    Key::Ctrl(b'c') => {
                        term::set_tgt_cursor(st.cx + st.len, st.cy);
                        term::print_str("^C\n");
                        st.len = 0;
                        shell::set_hist_view(-1);
                        prompt();
                        let (cx, cy) = term::tgt_cursor();
                        st.cx = cx;
                        st.cy = cy;
                        changed = true;
                    }
                    _ => {}
                }
                st.on = true;
                st.last_blink = shell::now();
            }
        }

        // cursor blink (draw into buffer)
        let t = shell::now();
        if t.wrapping_sub(st.last_blink) > shell::tsc_per_ms() * 450 {
            st.last_blink = t;
            st.on = !st.on;
            changed = true;
        }
        let cpos = st.cx + st.len;
        if cpos < W {
            if st.on && focused {
                term::put_at_colored(cpos, st.cy, 0xdb, term::LIGHT_GRAY, term::BLACK);
            } else {
                term::put_at_colored(cpos, st.cy, b' ', term::LIGHT_GRAY, term::BLACK);
            }
            changed = true;
        }

        changed
    }
}
