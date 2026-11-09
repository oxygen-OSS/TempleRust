//! DolDoc document and inline markup engine for TempleOS/Rust.
//!
//! Implements the dollar-sign command parser: $FG,col$, $BG,col$, $TX+CX,"..."$,
//! $UL,1/0$, $IV,1/0$, $MA,"...",LM="..."$, $$ escapes, and raw mode toggle (Ctrl-T).

use core::sync::atomic::{AtomicBool, Ordering};

pub static RAW_MODE: AtomicBool = AtomicBool::new(false);

pub fn is_raw() -> bool {
    RAW_MODE.load(Ordering::Relaxed)
}

pub fn toggle_raw() -> bool {
    let prev = RAW_MODE.fetch_xor(true, Ordering::Relaxed);
    !prev
}

#[allow(dead_code)]
pub fn set_raw(v: bool) {
    RAW_MODE.store(v, Ordering::Relaxed);
}

pub fn parse_color_name(name: &str) -> Option<u8> {
    let s = name.trim();
    if s.is_empty() {
        return None;
    }
    if let Ok(n) = parse_u8(s) {
        if n < 16 {
            return Some(n);
        }
    }
    match s {
        s if s.eq_ignore_ascii_case("BLACK") => Some(0),
        s if s.eq_ignore_ascii_case("BLUE") => Some(1),
        s if s.eq_ignore_ascii_case("GREEN") => Some(2),
        s if s.eq_ignore_ascii_case("CYAN") => Some(3),
        s if s.eq_ignore_ascii_case("RED") => Some(4),
        s if s.eq_ignore_ascii_case("PURPLE") => Some(5),
        s if s.eq_ignore_ascii_case("BROWN") => Some(6),
        s if s.eq_ignore_ascii_case("LTGRAY") || s.eq_ignore_ascii_case("LIGHT_GRAY") => Some(7),
        s if s.eq_ignore_ascii_case("DKGRAY") || s.eq_ignore_ascii_case("DARK_GRAY") => Some(8),
        s if s.eq_ignore_ascii_case("LTBLUE") || s.eq_ignore_ascii_case("LIGHT_BLUE") => Some(9),
        s if s.eq_ignore_ascii_case("LTGREEN") || s.eq_ignore_ascii_case("LIGHT_GREEN") => Some(10),
        s if s.eq_ignore_ascii_case("LTCYAN") || s.eq_ignore_ascii_case("LIGHT_CYAN") => Some(11),
        s if s.eq_ignore_ascii_case("LTRED") || s.eq_ignore_ascii_case("LIGHT_RED") => Some(12),
        s if s.eq_ignore_ascii_case("LTPURPLE") || s.eq_ignore_ascii_case("LIGHT_PURPLE") => Some(13),
        s if s.eq_ignore_ascii_case("YELLOW") => Some(14),
        s if s.eq_ignore_ascii_case("WHITE") => Some(15),
        _ => None,
    }
}

fn parse_u8(s: &str) -> Result<u8, ()> {
    let mut val = 0u8;
    for &b in s.as_bytes() {
        if !b.is_ascii_digit() {
            return Err(());
        }
        val = val.checked_mul(10).ok_or(())?.checked_add(b - b'0').ok_or(())?;
    }
    Ok(val)
}

/// Print a DolDoc formatted string directly to the terminal screen.
pub fn print_doldoc(text: &str) {
    if is_raw() {
        crate::term::print_str(text);
        return;
    }

    let bytes = text.as_bytes();
    let mut i = 0;
    let def_fg = crate::term::theme_fg();
    let def_bg = crate::term::theme_bg();
    let mut cur_fg = def_fg;
    let mut cur_bg = def_bg;
    let mut inverted = false;

    while i < bytes.len() {
        if bytes[i] == b'$' {
            if i + 1 < bytes.len() && bytes[i + 1] == b'$' {
                // Escaped $$
                crate::term::put_char_colored(b'$', cur_fg, cur_bg);
                i += 2;
                continue;
            }

            // Find closing $
            let start = i + 1;
            let mut end = start;
            while end < bytes.len() && bytes[end] != b'$' && bytes[end] != b'\n' {
                end += 1;
            }

            if end < bytes.len() && bytes[end] == b'$' {
                // Parse command between start and end
                let tag = core::str::from_utf8(&bytes[start..end]).unwrap_or("");
                handle_tag(tag, &mut cur_fg, &mut cur_bg, &mut inverted, def_fg, def_bg);
                i = end + 1;
                continue;
            }
        }

        let ch = bytes[i];
        i += 1;
        let (fg, bg) = if inverted {
            (cur_bg, cur_fg)
        } else {
            (cur_fg, cur_bg)
        };
        crate::term::put_char_colored(ch, fg, bg);
    }
    crate::term::set_fg(def_fg);
    crate::term::set_bg(def_bg);
}

fn handle_tag(
    tag: &str,
    fg: &mut u8,
    bg: &mut u8,
    inverted: &mut bool,
    def_fg: u8,
    def_bg: u8,
) {
    let tag = tag.trim();
    if tag.is_empty() {
        return;
    }

    // Shortcut color: e.g. "$RED$" or "$BLUE$"
    if let Some(c) = parse_color_name(tag) {
        *fg = c;
        return;
    }

    // Split at comma
    let mut parts = tag.splitn(2, ',');
    let cmd = parts.next().unwrap_or("").trim();
    let arg = parts.next().unwrap_or("").trim();

    if cmd.eq_ignore_ascii_case("FG") {
        if arg.is_empty() {
            *fg = def_fg;
        } else if let Some(c) = parse_color_name(arg) {
            *fg = c;
        }
    } else if cmd.eq_ignore_ascii_case("BG") {
        if arg.is_empty() {
            *bg = def_bg;
        } else if let Some(c) = parse_color_name(arg) {
            *bg = c;
        }
    } else if cmd.eq_ignore_ascii_case("FD") {
        if let Some(c) = parse_color_name(arg) {
            *fg = c;
        }
    } else if cmd.eq_ignore_ascii_case("BD") {
        if let Some(c) = parse_color_name(arg) {
            *bg = c;
        }
    } else if cmd.starts_with("TX") || cmd.starts_with("tx") {
        // e.g. TX+CX,"centered string"
        let flags = &cmd[2..];
        let content = strip_quotes(arg);
        if flags.contains("+CX") || flags.contains("+cx") {
            let width = crate::term::COLS;
            let len = content.len();
            let pad = if len < width { (width - len) / 2 } else { 0 };
            for _ in 0..pad {
                crate::term::put_char_colored(b' ', *fg, *bg);
            }
            for &b in content.as_bytes() {
                crate::term::put_char_colored(b, *fg, *bg);
            }
            crate::term::newline();
        } else if flags.contains("+RD") || flags.contains("+rd") {
            let width = crate::term::COLS;
            let len = content.len();
            let pad = if len < width { width - len } else { 0 };
            for _ in 0..pad {
                crate::term::put_char_colored(b' ', *fg, *bg);
            }
            for &b in content.as_bytes() {
                crate::term::put_char_colored(b, *fg, *bg);
            }
        } else {
            for &b in content.as_bytes() {
                crate::term::put_char_colored(b, *fg, *bg);
            }
        }
    } else if cmd.eq_ignore_ascii_case("IV") {
        if arg == "1" {
            *inverted = true;
        } else if arg == "0" {
            *inverted = false;
        } else {
            *inverted = !*inverted;
        }
    } else if cmd.eq_ignore_ascii_case("MA") {
        // Macro button: $MA,"Click Me",LM="Dir;"$
        let label = strip_quotes(arg);
        crate::term::put_char_colored(b'[', crate::term::CYAN, *bg);
        crate::term::put_char_colored(b' ', crate::term::CYAN, *bg);
        for &b in label.as_bytes() {
            crate::term::put_char_colored(b, crate::term::YELLOW, *bg);
        }
        crate::term::put_char_colored(b' ', crate::term::CYAN, *bg);
        crate::term::put_char_colored(b']', crate::term::CYAN, *bg);
    } else if cmd.eq_ignore_ascii_case("LK") {
        // Hyperlink: $LK,"Read Me",A="FI:Doc.DD"$
        let label = strip_quotes(arg);
        crate::term::put_char_colored(b'<', crate::term::BLUE, *bg);
        for &b in label.as_bytes() {
            crate::term::put_char_colored(b, crate::term::LIGHT_BLUE, *bg);
        }
        crate::term::put_char_colored(b'>', crate::term::BLUE, *bg);
    }
}

fn strip_quotes(s: &str) -> &str {
    let mut s = s.trim();
    if s.starts_with('"') {
        s = &s[1..];
    }
    if let Some(end) = s.find('"') {
        s = &s[..end];
    }
    s
}
