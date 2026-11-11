//! TempleOS Interactive Pull-Down Menu System.
//!
//! Provides top menu bar dropdowns: [File], [Doc], [Sound], [Demos], [Theme], [Help].
//! Supports mouse clicking and hotkey activation.

use crate::term;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum MenuId {
    None,
    File,
    Doc,
    Sound,
    Demos,
    Theme,
    Help,
}

pub struct MenuItem {
    pub label: &'static str,
    pub cmd: &'static str,
}

pub const FILE_ITEMS: &[MenuItem] = &[
    MenuItem { label: "File Manager (GUI);", cmd: "FileMgr;" },
    MenuItem { label: "Dir (List);", cmd: "Dir;" },
    MenuItem { label: "Edit Notes.DD;", cmd: "Ed(\"Notes.DD\");" },
    MenuItem { label: "Clear Screen;", cmd: "Cls;" },
];

pub const DOC_ITEMS: &[MenuItem] = &[
    MenuItem { label: "Help.DD;", cmd: "Type(\"C:/Doc/Help.DD\");" },
    MenuItem { label: "DolDoc.DD;", cmd: "Type(\"C:/Doc/DolDoc.DD\");" },
    MenuItem { label: "HolyC.DD;", cmd: "Type(\"C:/Doc/HolyC.DD\");" },
    MenuItem { label: "Charter.DD;", cmd: "Type(\"C:/Doc/Charter.DD\");" },
    MenuItem { label: "Toggle Raw DolDoc (^T);", cmd: "DocToggle;" },
];

pub const SOUND_ITEMS: &[MenuItem] = &[
    MenuItem { label: "Beep;", cmd: "Beep;" },
    MenuItem { label: "Play Ode to Joy;", cmd: "Play(\"O4 L4 E E F G G F E D C C D E E. D8 D2\");" },
    MenuItem { label: "Silence (Snd);", cmd: "Snd;" },
];

pub const DEMO_ITEMS: &[MenuItem] = &[
    MenuItem { label: "File Manager (GUI);", cmd: "FileMgr;" },
    MenuItem { label: "Calculator (GUI);", cmd: "CalcGui;" },
    MenuItem { label: "System Monitor (GUI);", cmd: "SysMon;" },
    MenuItem { label: "Snake Game;", cmd: "Snake;" },
    MenuItem { label: "Game of Life;", cmd: "Life;" },
    MenuItem { label: "16-Color Lines;", cmd: "Lines;" },
    MenuItem { label: "Benchmark (FPS);", cmd: "Bench;" },
];

pub const THEME_ITEMS: &[MenuItem] = &[
    MenuItem { label: "Classic White (TempleOS);", cmd: "Theme(\"classic\");" },
    MenuItem { label: "Retro Blue;", cmd: "Theme(\"blue\");" },
    MenuItem { label: "Dark Mode;", cmd: "Theme(\"dark\");" },
];

pub const HELP_ITEMS: &[MenuItem] = &[
    MenuItem { label: "Master Help (<F1>);", cmd: "Help;" },
    MenuItem { label: "Memory Report (MemRep);", cmd: "MemRep;" },
    MenuItem { label: "CPU Report (CpuRep);", cmd: "CpuRep;" },
    MenuItem { label: "About TempleRust;", cmd: "About;" },
];

static mut ACTIVE_MENU: MenuId = MenuId::None;

pub fn active_menu() -> MenuId {
    unsafe { ACTIVE_MENU }
}

pub fn close_menu() {
    unsafe {
        ACTIVE_MENU = MenuId::None;
    }
    // don't force a full screen repaint here — the WM compositor will reblit
    // windows over the area the dropdown occupied. In non-WM contexts (plain
    // REPL with no windows) mark the background dirty so the caller/loop can
    // refresh; also do a targeted repaint of the affected body rows.
    term::mark_bg_dirty();
    if !crate::wm::has_windows() {
        term::repaint();
        term::draw_chrome();
    }
}

pub fn open_menu(id: MenuId) {
    unsafe {
        ACTIVE_MENU = id;
    }
    if !crate::wm::has_windows() {
        draw_active_dropdown();
    }
    // when windows are up, wm::frame draws the dropdown after compositing
}

pub fn toggle_menu(id: MenuId) {
    if active_menu() == id {
        close_menu();
    } else {
        open_menu(id);
    }
}

pub fn menu_col(id: MenuId) -> usize {
    match id {
        MenuId::None => 0,
        MenuId::File => 8,
        MenuId::Doc => 22,
        MenuId::Sound => 28,
        MenuId::Demos => 36,
        MenuId::Theme => 44,
        MenuId::Help => 52,
    }
}

fn menu_items(id: MenuId) -> &'static [MenuItem] {
    match id {
        MenuId::None => &[],
        MenuId::File => FILE_ITEMS,
        MenuId::Doc => DOC_ITEMS,
        MenuId::Sound => SOUND_ITEMS,
        MenuId::Demos => DEMO_ITEMS,
        MenuId::Theme => THEME_ITEMS,
        MenuId::Help => HELP_ITEMS,
    }
}

pub fn draw_active_dropdown() {
    let id = active_menu();
    if id == MenuId::None {
        return;
    }
    let items = menu_items(id);
    let col = menu_col(id);

    // Compute width
    let mut max_len = 0;
    for it in items {
        if it.label.len() > max_len {
            max_len = it.label.len();
        }
    }
    let box_w = max_len + 4;
    let _box_h = items.len() + 2;

    let bg = term::LIGHT_GRAY;
    let fg = term::BLACK;
    let border = term::DARK_GRAY;

    // Draw top border
    term::put_at_colored(col, 1, 0xda, border, bg);
    for x in 1..box_w - 1 {
        term::put_at_colored(col + x, 1, 0xc4, border, bg);
    }
    term::put_at_colored(col + box_w - 1, 1, 0xbf, border, bg);

    // Draw rows
    for (idx, it) in items.iter().enumerate() {
        let y = 2 + idx;
        term::put_at_colored(col, y, 0xb3, border, bg);
        term::put_at_colored(col + 1, y, b' ', fg, bg);
        term::draw_text_at_colored(col + 2, y, it.label, fg, bg);
        let pad = box_w.saturating_sub(2 + it.label.len() + 1);
        for p in 0..pad {
            term::put_at_colored(col + 2 + it.label.len() + p, y, b' ', fg, bg);
        }
        term::put_at_colored(col + box_w - 1, y, 0xb3, border, bg);
    }

    // Draw bottom border
    let bot_y = 2 + items.len();
    term::put_at_colored(col, bot_y, 0xc0, border, bg);
    for x in 1..box_w - 1 {
        term::put_at_colored(col + x, bot_y, 0xc4, border, bg);
    }
    term::put_at_colored(col + box_w - 1, bot_y, 0xd9, border, bg);
}

/// Handle a mouse click at cell coordinates (col, row). Returns command to execute if any.
pub fn handle_click(col: usize, row: usize) -> Option<&'static str> {
    // Check if clicked top menu bar
    if row == 0 {
        let clicked_menu = match col {
            8..=13 => MenuId::File,
            22..=26 => MenuId::Doc,
            28..=34 => MenuId::Sound,
            36..=42 => MenuId::Demos,
            44..=50 => MenuId::Theme,
            52..=58 => MenuId::Help,
            _ => MenuId::None,
        };
        if clicked_menu != MenuId::None {
            toggle_menu(clicked_menu);
            return None;
        }
    }

    // If a menu is open, check if clicked an item
    let active = active_menu();
    if active != MenuId::None {
        let m_col = menu_col(active);
        let items = menu_items(active);
        let _box_h = items.len() + 2;

        if row >= 2 && row < 2 + items.len() {
            let idx = row - 2;
            let it = &items[idx];
            let box_w = 30; // approximate bounds
            if col >= m_col && col <= m_col + box_w {
                close_menu();
                return Some(it.cmd);
            }
        }
        close_menu();
    }
    None
}
