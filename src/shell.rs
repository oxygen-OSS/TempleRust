//! The TempleOS HolyC REPL: DolDoc banner, line editor, HolyC command parser.

use crate::doldoc;
use crate::editor;
use crate::expr;
use crate::keyboard::{self, Key};
use crate::sound;
use crate::term;
use crate::vfs;

const MAX_LINE: usize = 120;

pub fn run() -> ! {
    banner();
    let mut buf = [0u8; MAX_LINE];
    loop {
        term::set_fg(term::YELLOW);
        term::print_str("> ");
        term::set_fg(term::WHITE);
        term::cursor(true);
        let len = read_line(&mut buf);
        execute(&buf[..len]);
    }
}

fn banner() {
    doldoc::print_doldoc(
        "$FG,YELLOW$$TX+CX,\"=== TEMPLEOS / RUST ===\"$\n\
$FG,CYAN$$TX+CX,\"========================\"$\n\n\
$FG,WHITE$A ring-0, single-address-space toy OS with TempleOS DolDoc & GUIs.\n\
$FG,LTCYAN$Top menu is interactive: click $FG,YELLOW$[Menu]$FG,LTCYAN$ or press $FG,YELLOW$<F2>$FG,LTCYAN$ for drop-down tools.$FG,WHITE$\n\n\
  $MA,\"File Manager\",LM=\"FileMgr;\"$  \
  $MA,\"Calculator\",LM=\"CalcGui;\"$  \
  $MA,\"System Monitor\",LM=\"SysMon;\"$\n\
  $MA,\"Help Index\",LM=\"Help;\"$  \
  $MA,\"Edit Notes\",LM=\"Ed(\\\"Notes.DD\\\");\"$  \
  $MA,\"Game of Life\",LM=\"Life;\"$\n\n\
$FG,WHITE$Type $FG,YELLOW$Help;$FG,WHITE$ for commands, or bare expressions like $FG,YELLOW$2+3*4;$FG,WHITE$\n\n",
    );
}

pub fn now() -> u64 {
    unsafe { core::arch::x86_64::_rdtsc() }
}

#[inline]
fn inb(port: u16) -> u8 {
    let v: u8;
    unsafe {
        core::arch::asm!("in al, dx", in("dx") port, out("al") v,
                         options(nostack, preserves_flags));
    }
    v
}

#[inline]
fn outb(port: u16, val: u8) {
    unsafe {
        core::arch::asm!("out dx, al", in("dx") port, in("al") val,
                         options(nostack, preserves_flags));
    }
}

const PIT_HZ: u64 = 1_193_182;
const BENCH_MS: u64 = 50;

fn pit_start(ms: u64) {
    let count = (PIT_HZ * ms / 1000) as u16;
    let v = inb(0x61);
    outb(0x61, v & !0x02);
    outb(0x43, 0xb0);
    outb(0x42, (count & 0xff) as u8);
    outb(0x42, (count >> 8) as u8);
    outb(0x61, (inb(0x61) | 0x01) & !0x02);
}

fn pit_expired() -> bool {
    inb(0x61) & 0x20 != 0
}

static mut TSC_PER_MS: u64 = 0;

pub fn tsc_per_ms() -> u64 {
    unsafe {
        if TSC_PER_MS == 0 {
            let t0 = now();
            pit_start(BENCH_MS);
            while !pit_expired() {}
            let t1 = now();
            TSC_PER_MS = (t1 - t0) / BENCH_MS;
        }
        TSC_PER_MS
    }
}

fn read_line(buf: &mut [u8]) -> usize {
    let mut len = 0usize;
    let mut on = true;
    let mut last = now();

    loop {
        if let Some(key) = keyboard::poll() {
            match key {
                Key::F(1) => {
                    term::newline();
                    cmd_help();
                    term::set_fg(term::BLUE);
                    term::print_str(vfs::cwd_str());
                    term::set_fg(term::RED);
                    term::print_str("> ");
                    term::set_fg(term::theme_fg());
                    for &b in &buf[..len] {
                        term::put_char(b);
                    }
                }
                Key::F(2) => {
                    crate::menu::toggle_menu(crate::menu::MenuId::File);
                }
                Key::Ctrl(b't') => {
                    let raw = doldoc::toggle_raw();
                    term::newline();
                    term::set_fg(term::CYAN);
                    term::print_str(if raw {
                        "[DolDoc: Raw Markup Mode]\n"
                    } else {
                        "[DolDoc: Formatted Mode]\n"
                    });
                    term::set_fg(term::BLUE);
                    term::print_str(vfs::cwd_str());
                    term::set_fg(term::RED);
                    term::print_str("> ");
                    term::set_fg(term::theme_fg());
                    for &b in &buf[..len] {
                        term::put_char(b);
                    }
                }
                Key::Ctrl(b'l') => {
                    term::clear();
                    term::set_fg(term::BLUE);
                    term::print_str(vfs::cwd_str());
                    term::set_fg(term::RED);
                    term::print_str("> ");
                    term::set_fg(term::theme_fg());
                    for &b in &buf[..len] {
                        term::put_char(b);
                    }
                }
                Key::Ctrl(b'c') => {
                    term::print_str("^C\n");
                    return 0;
                }
                Key::Char(c) => {
                    if len < buf.len() {
                        buf[len] = c;
                        len += 1;
                        term::put_char(c);
                    }
                }
                Key::Backspace => {
                    if len > 0 {
                        len -= 1;
                        term::backspace();
                    }
                }
                Key::Enter => {
                    term::newline();
                    return len;
                }
                _ => {}
            }
            on = true;
            term::cursor(true);
            last = now();
        }

        if let Some(mev) = crate::mouse::poll() {
            if mev.left {
                let cw = crate::fb::cell_w().max(1);
                let ch = crate::fb::cell_h().max(1);
                let col = mev.x / cw;
                let row = mev.y / ch;

                if row == 1 && col >= term::COLS.saturating_sub(6) {
                    // Clicked [X] close/clear box
                    term::clear();
                    term::set_fg(term::BLUE);
                    term::print_str(vfs::cwd_str());
                    term::set_fg(term::RED);
                    term::print_str("> ");
                    term::set_fg(term::theme_fg());
                    for &b in &buf[..len] {
                        term::put_char(b);
                    }
                } else if let Some(cmd) = crate::menu::handle_click(col, row) {
                    term::newline();
                    term::set_fg(term::YELLOW);
                    term::print_str(cmd);
                    term::newline();
                    term::set_fg(term::theme_fg());
                    execute(cmd.as_bytes());
                    term::set_fg(term::BLUE);
                    term::print_str(vfs::cwd_str());
                    term::set_fg(term::RED);
                    term::print_str("> ");
                    term::set_fg(term::theme_fg());
                    len = 0;
                }
            }
        }

        let t = now();
        if t.wrapping_sub(last) > tsc_per_ms() * 450 {
            last = t;
            on = !on;
            term::cursor(on);
        }
        core::hint::spin_loop();
    }
}

/// Normalize input by trimming whitespace and optional trailing semicolon.
fn parse_command(input: &str) -> (&str, &str) {
    let mut s = input.trim();
    if s.ends_with(';') {
        s = &s[..s.len() - 1].trim_end();
    }
    if let Some(open) = s.find('(') {
        let cmd = s[..open].trim();
        let mut rest = &s[open + 1..];
        if rest.ends_with(')') {
            rest = &rest[..rest.len() - 1].trim();
        }
        (cmd, rest)
    } else {
        match s.find(' ') {
            Some(i) => (s[..i].trim(), s[i + 1..].trim()),
            None => (s, ""),
        }
    }
}

fn unquote(s: &str) -> &str {
    let mut s = s.trim();
    if s.starts_with('"') && s.ends_with('"') && s.len() >= 2 {
        s = &s[1..s.len() - 1];
    }
    s
}

fn execute(line: &[u8]) {
    let s = match core::str::from_utf8(line) {
        Ok(s) => s.trim(),
        Err(_) => {
            term::set_fg(term::LIGHT_RED);
            term::print_str("bad input\n");
            term::set_fg(term::theme_fg());
            return;
        }
    };
    if s.is_empty() {
        return;
    }

    let (cmd, raw_arg) = parse_command(s);
    let arg = unquote(raw_arg);

    if cmd.eq_ignore_ascii_case("DIR") || cmd.eq_ignore_ascii_case("LS") {
        cmd_dir(arg);
    } else if cmd.eq_ignore_ascii_case("CD") {
        cmd_cd(arg);
    } else if cmd.eq_ignore_ascii_case("PWD") {
        term::set_fg(term::CYAN);
        term::print_str(vfs::cwd_str());
        term::newline();
        term::set_fg(term::theme_fg());
    } else if cmd.eq_ignore_ascii_case("TYPE") || cmd.eq_ignore_ascii_case("CAT") {
        cmd_type(arg);
    } else if cmd.eq_ignore_ascii_case("ED") || cmd.eq_ignore_ascii_case("EDIT") {
        cmd_ed(arg);
    } else if cmd.eq_ignore_ascii_case("DEL") || cmd.eq_ignore_ascii_case("RM") {
        cmd_del(arg);
    } else if cmd.eq_ignore_ascii_case("MKDIR") || cmd.eq_ignore_ascii_case("DIRMK") {
        cmd_mkdir(arg);
    } else if cmd.eq_ignore_ascii_case("BEEP") {
        sound::beep(500, 150, tsc_per_ms());
    } else if cmd.eq_ignore_ascii_case("SND") {
        cmd_snd(arg);
    } else if cmd.eq_ignore_ascii_case("PLAY") {
        cmd_play(arg);
    } else if cmd.eq_ignore_ascii_case("MEMREP") || cmd.eq_ignore_ascii_case("MEM") {
        cmd_memrep();
    } else if cmd.eq_ignore_ascii_case("CPUREP") || cmd.eq_ignore_ascii_case("CPU") {
        cmd_cpurep();
    } else if cmd.eq_ignore_ascii_case("DATE") || cmd.eq_ignore_ascii_case("TIME") {
        cmd_time();
    } else if cmd.eq_ignore_ascii_case("WHO") {
        cmd_who();
    } else if cmd.eq_ignore_ascii_case("THEME") {
        cmd_theme(arg);
    } else if cmd.eq_ignore_ascii_case("DOCTOGGLE") {
        let raw = doldoc::toggle_raw();
        term::print_str(if raw { "DolDoc: Raw\n" } else { "DolDoc: Formatted\n" });
    } else if cmd.eq_ignore_ascii_case("HELP") || cmd.eq_ignore_ascii_case("MAN") || cmd.eq_ignore_ascii_case("DOC") {
        cmd_help();
    } else if cmd.eq_ignore_ascii_case("ABOUT") {
        about();
    } else if cmd.eq_ignore_ascii_case("VER") {
        ver();
    } else if cmd.eq_ignore_ascii_case("CLS") || cmd.eq_ignore_ascii_case("CLEAR") {
        term::clear();
    } else if cmd.eq_ignore_ascii_case("FILEMGR") || cmd.eq_ignore_ascii_case("FM") {
        crate::filemgr::run();
    } else if cmd.eq_ignore_ascii_case("CALCGUI") || (cmd.eq_ignore_ascii_case("CALC") && arg.is_empty()) {
        crate::calc_gui::run();
    } else if cmd.eq_ignore_ascii_case("SYSMON") || cmd.eq_ignore_ascii_case("MON") {
        crate::sysmon::run();
    } else if cmd.eq_ignore_ascii_case("FONT") {
        font();
    } else if cmd.eq_ignore_ascii_case("ECHO") {
        term::print_str(arg);
        term::newline();
    } else if cmd.eq_ignore_ascii_case("CALC") {
        calc(arg);
    } else if cmd.eq_ignore_ascii_case("BENCH") || cmd.eq_ignore_ascii_case("FPS") {
        bench();
    } else if cmd.eq_ignore_ascii_case("SNAKE") {
        snake();
    } else if cmd.eq_ignore_ascii_case("LIFE") {
        demo_life();
    } else if cmd.eq_ignore_ascii_case("LINES") || cmd.eq_ignore_ascii_case("GRAPHICS") {
        demo_lines();
    } else if cmd.eq_ignore_ascii_case("REBOOT") {
        reboot();
    } else if cmd.eq_ignore_ascii_case("HALT") {
        halt();
    } else if cmd.eq_ignore_ascii_case("RUN") {
        crate::hcapps::cmd_run(arg);
    } else if crate::hcapps::lookup(cmd) {
        // Bare app name, TempleOS-style: `Plasma;`.
        crate::hcapps::cmd_run(cmd);
    } else if let Some(v) = expr::eval(s.trim_end_matches(';')) {
        term::set_fg(term::GREEN);
        term::print_num(v);
        term::set_fg(term::theme_fg());
        term::newline();
    } else {
        term::set_fg(term::LIGHT_RED);
        term::print_str("unknown command: ");
        term::print_str(cmd);
        term::print_str(" (type Help; for list)\n");
        term::set_fg(term::theme_fg());
    }
}

pub fn cmd_dir(arg: &str) {
    let target = if arg.is_empty() { vfs::cwd_str() } else { arg };
    let (parent, name) = vfs::resolve_path(target);
    let dir_path = if name.as_str().is_empty() {
        parent.as_str()
    } else {
        target
    };

    doldoc::print_doldoc("$FG,CYAN$Directory of $FG,YELLOW$");
    term::print_str(dir_path);
    doldoc::print_doldoc("$FG,BLACK$\n");

    let count = vfs::entries_count();
    let mut entries_found = 0;

    for i in 0..count {
        if let Some(e) = vfs::get_entry(i) {
            if e.path_str().eq_ignore_ascii_case(dir_path) {
                entries_found += 1;
                match e.ftype {
                    vfs::FileType::Dir => {
                        doldoc::print_doldoc("  $FG,PURPLE$DIR  $FG,LTBLUE$");
                        term::print_str(e.name_str());
                        doldoc::print_doldoc("/$FG,BLACK$\n");
                    }
                    vfs::FileType::File => {
                        doldoc::print_doldoc("  $FG,WHITE$");
                        term::print_num(e.size as i64);
                        let len = num_len(e.size as i64);
                        for _ in 0..(7usize.saturating_sub(len)) {
                            term::print_str(" ");
                        }
                        if e.name_str().ends_with(".DD") {
                            doldoc::print_doldoc("$FG,YELLOW$");
                        } else if e.name_str().ends_with(".HC") {
                            doldoc::print_doldoc("$FG,LTGREEN$");
                        } else {
                            doldoc::print_doldoc("$FG,WHITE$");
                        }
                        term::print_str(e.name_str());
                        doldoc::print_doldoc("$FG,BLACK$\n");
                    }
                }
            }
        }
    }

    if entries_found == 0 {
        term::set_fg(term::DARK_GRAY);
        term::print_str("  (empty or invalid directory)\n");
        term::set_fg(term::theme_fg());
    }
}

fn num_len(mut n: i64) -> usize {
    if n <= 0 {
        return 1;
    }
    let mut len = 0;
    while n > 0 {
        len += 1;
        n /= 10;
    }
    len
}

fn cmd_cd(arg: &str) {
    if arg.is_empty() {
        vfs::set_cwd("C:/Home");
    } else {
        let (parent, name) = vfs::resolve_path(arg);
        let cand = if name.as_str().is_empty() {
            parent.as_str()
        } else {
            arg
        };
        // Verify directory exists
        let count = vfs::entries_count();
        let mut exists = cand == "C:/" || cand == "/" || cand == "::";
        if !exists {
            for i in 0..count {
                if let Some(e) = vfs::get_entry(i) {
                    if e.ftype == vfs::FileType::Dir {
                        let mut full = [0u8; 64];
                        let p = e.path_str();
                        let n = e.name_str();
                        let total = p.len() + 1 + n.len();
                        if total <= 64 {
                            full[..p.len()].copy_from_slice(p.as_bytes());
                            full[p.len()] = b'/';
                            full[p.len() + 1..total].copy_from_slice(n.as_bytes());
                            if let Ok(fs) = core::str::from_utf8(&full[..total]) {
                                if fs.eq_ignore_ascii_case(cand) {
                                    exists = true;
                                    break;
                                }
                            }
                        }
                    }
                }
            }
        }
        if exists {
            vfs::set_cwd(cand);
        } else {
            term::set_fg(term::LIGHT_RED);
            term::print_str("directory not found: ");
            term::print_str(cand);
            term::newline();
            term::set_fg(term::theme_fg());
        }
    }
    term::status_bar();
}

pub fn cmd_type(arg: &str) {
    if arg.is_empty() {
        term::set_fg(term::LIGHT_RED);
        term::print_str("usage: Type(\"filename\");\n");
        term::set_fg(term::theme_fg());
        return;
    }
    if let Some(content) = vfs::read_file(arg) {
        doldoc::print_doldoc(content);
        term::newline();
    } else {
        term::set_fg(term::LIGHT_RED);
        term::print_str("file not found: ");
        term::print_str(arg);
        term::newline();
        term::set_fg(term::theme_fg());
    }
}

fn cmd_ed(arg: &str) {
    if arg.is_empty() {
        term::set_fg(term::LIGHT_RED);
        term::print_str("usage: Ed(\"filename\");\n");
        term::set_fg(term::theme_fg());
        return;
    }
    editor::run_editor(arg);
}

fn cmd_del(arg: &str) {
    if arg.is_empty() {
        term::set_fg(term::LIGHT_RED);
        term::print_str("usage: Del(\"filename\");\n");
        term::set_fg(term::theme_fg());
        return;
    }
    if vfs::delete_file(arg) {
        term::set_fg(term::GREEN);
        term::print_str("Deleted ");
        term::print_str(arg);
        term::newline();
    } else {
        term::set_fg(term::LIGHT_RED);
        term::print_str("file not found: ");
        term::print_str(arg);
        term::newline();
    }
    term::set_fg(term::theme_fg());
}

fn cmd_mkdir(arg: &str) {
    if arg.is_empty() {
        term::set_fg(term::LIGHT_RED);
        term::print_str("usage: MkDir(\"dirname\");\n");
        term::set_fg(term::theme_fg());
        return;
    }
    if vfs::mkdir(arg) {
        term::set_fg(term::GREEN);
        term::print_str("Created directory ");
        term::print_str(arg);
        term::newline();
    } else {
        term::set_fg(term::LIGHT_RED);
        term::print_str("could not create directory\n");
    }
    term::set_fg(term::theme_fg());
}

fn cmd_snd(arg: &str) {
    if arg.is_empty() {
        sound::snd(0);
        return;
    }
    if let Some(freq) = expr::eval(arg) {
        sound::snd(freq as u32);
    } else {
        term::set_fg(term::LIGHT_RED);
        term::print_str("usage: Snd(freq); or Snd;\n");
        term::set_fg(term::theme_fg());
    }
}

fn cmd_play(arg: &str) {
    let score = if arg.is_empty() {
        "O4 L4 C D E C C D E C E F G2 E F G2"
    } else {
        arg
    };
    term::set_fg(term::YELLOW);
    term::print_str("Playing music macro...\n");
    term::set_fg(term::theme_fg());
    sound::play(score, tsc_per_ms());
}

fn cmd_theme(arg: &str) {
    if arg.eq_ignore_ascii_case("classic") || arg.eq_ignore_ascii_case("white") {
        term::set_theme(term::Theme::Classic);
    } else if arg.eq_ignore_ascii_case("blue") {
        term::set_theme(term::Theme::Blue);
    } else if arg.eq_ignore_ascii_case("dark") || arg.eq_ignore_ascii_case("black") {
        term::set_theme(term::Theme::Dark);
    } else {
        term::set_fg(term::LIGHT_RED);
        term::print_str("usage: Theme(\"classic\"); Theme(\"blue\"); Theme(\"dark\");\n");
        term::set_fg(term::theme_fg());
    }
}

fn cmd_who() {
    doldoc::print_doldoc(
        "$FG,CYAN$=== TASK REPORT ===\n\
$FG,WHITE$Task 0:  $FG,YELLOW$Adam/Kernel$FG,WHITE$  Ring 0  64-bit Long Mode\n\
Memory:  Identity-mapped 4 GiB, single address space\n\
Context: Non-preemptive cooperative multitasking\n",
    );
}

fn cmd_time() {
    let tsc = now();
    let ms = tsc / tsc_per_ms().max(1);
    let s = ms / 1000;
    let m = s / 60;
    let h = m / 60;

    term::set_fg(term::CYAN);
    term::print_str("System Uptime: ");
    term::set_fg(term::WHITE);
    term::print_num(h as i64);
    term::print_str("h ");
    term::print_num((m % 60) as i64);
    term::print_str("m ");
    term::print_num((s % 60) as i64);
    term::print_str("s (TSC: ");
    term::print_hex64(tsc);
    term::print_str(")\n");
}

fn cmd_help() {
    if let Some(content) = vfs::read_file("C:/Doc/Help.DD") {
        doldoc::print_doldoc(content);
        term::newline();
    }
}

fn about() {
    doldoc::print_doldoc(
        "$FG,RED$$TX+CX,\"TempleOS/Rust 0.1\"$\n\
$FG,DKGRAY$$TX+CX,\"================================\"$\n\n\
$FG,WHITE$A TempleOS-inspired operating system written in Rust.\n\
Boots from BIOS into 64-bit long mode with ring-0 privileges,\n\
a single address space, no libc, no processes, and no networking.\n\n\
$FG,YELLOW$Features:$FG,WHITE$\n\
* DolDoc hypertext document format with dollar-sign markup ($$$$FG,col$$$$).\n\
* HolyC command syntax with optional no-arg parentheses and trailing semicolons.\n\
* Full 256-glyph CP437 character set with classic box-drawing frames.\n\
* Real PC speaker tone generator and music macro player.\n\
* Contiguous in-memory RedSea virtual filesystem.\n\
* Interactive DolDoc text editor (Ed).\n\
* Conway's Game of Life and Snake mini-games.\n",
    );
}

fn ver() {
    term::print_str("TempleOS/Rust ");
    term::print_str(env!("CARGO_PKG_VERSION"));
    term::print_str("\nx86_64-unknown-linux-gnu, no_std, no_main, ring 0\n");
}

fn cmd_memrep() {
    let bi = crate::boot_info();
    doldoc::print_doldoc("$FG,CYAN$=== MEMORY & BOOT REPORT ===$FG,WHITE$\n");
    term::print_str("  BootInfo @ 0x5000: magic=");
    term::print_hex32(bi.magic);
    term::print_str(if bi.magic == 0x544f_5331 { " (TOS1 OK)\n" } else { " (bad)\n" });
    term::print_str("  Kernel size: ");
    term::print_num(bi.kernel_bytes as i64);
    term::print_str(" bytes identity-mapped at 0x10000\n");
    term::print_str("  Page Tables: 0x90000 (PML4 -> PDPT -> 4 PDs, 4 GiB)\n");
    term::print_str("  Framebuffer: ");
    term::print_hex64(bi.fb_addr);
    term::print_str(" (");
    term::print_num(bi.fb_width as i64);
    term::print_str("x");
    term::print_num(bi.fb_height as i64);
    term::print_str("x");
    term::print_num(if bi.flags & 1 != 0 { bi.fb_bpp as i64 } else { 16 });
    term::print_str(" ");
    term::print_str(if bi.flags & 1 != 0 { "VBE linear" } else { "planar VGA" });
    term::print_str(")\n");
    term::print_str("  TSC clock estimate: ~");
    term::print_num((tsc_per_ms() / 1000) as i64);
    term::print_str(" MHz\n");
}

fn cmd_cpurep() {
    doldoc::print_doldoc("$FG,CYAN$=== CPU REGISTER REPORT ===$FG,WHITE$\n");
    term::print_str("  CR0: ");
    term::print_hex64(cr0());
    term::print_str(" (PE | PG active)\n");
    term::print_str("  CR3: ");
    term::print_hex64(cr3());
    term::print_str(" (PML4 base address)\n");
    term::print_str("  CR4: ");
    term::print_hex64(cr4());
    term::print_str(" (PAE | OSFXSR | OSXMMEXCPT)\n");
    term::print_str("  EFER: LME enabled (64-bit Long Mode)\n");
    term::print_str("  RSP:  Ring 0 kernel stack\n");
}

fn cr0() -> u64 {
    let v: u64;
    unsafe {
        core::arch::asm!("mov {}, cr0", out(reg) v,
                         options(nomem, nostack, preserves_flags));
    }
    v
}

fn cr3() -> u64 {
    let v: u64;
    unsafe {
        core::arch::asm!("mov {}, cr3", out(reg) v,
                         options(nomem, nostack, preserves_flags));
    }
    v
}

fn cr4() -> u64 {
    let v: u64;
    unsafe {
        core::arch::asm!("mov {}, cr4", out(reg) v,
                         options(nomem, nostack, preserves_flags));
    }
    v
}

fn font() {
    doldoc::print_doldoc("$FG,CYAN$Character set 0..255 (CP437)$FG,WHITE$\n");
    let mut n = 0;
    for c in 0u8..=255 {
        term::put_char(c);
        n += 1;
        if n % 32 == 0 {
            term::newline();
        }
    }
    if n % 32 != 0 {
        term::newline();
    }
}

fn calc(rest: &str) {
    if rest.is_empty() {
        term::set_fg(term::LIGHT_RED);
        term::print_str("usage: Calc(expr);\n");
        term::set_fg(term::theme_fg());
        return;
    }
    match expr::eval(rest) {
        Some(v) => {
            term::set_fg(term::GREEN);
            term::print_num(v);
            term::set_fg(term::theme_fg());
            term::newline();
        }
        None => {
            term::set_fg(term::LIGHT_RED);
            term::print_str("bad expression\n");
            term::set_fg(term::theme_fg());
        }
    }
}

// --- CONWAY'S GAME OF LIFE ---

const LIFE_W: usize = 78;
const LIFE_H: usize = 25;

pub fn demo_life() {
    term::clear();
    term::draw_text_at_colored(1, 0, "[ Conway's Game of Life - TempleOS ]", term::YELLOW, term::BROWN);
    term::draw_text_at_colored(1, term::STATUS_ROW, "<SPACE>: Step   <R>: Randomize   <Q>: Exit", term::WHITE, term::DARK_GRAY);

    let mut grid = [[false; LIFE_W]; LIFE_H];
    let mut next = [[false; LIFE_W]; LIFE_H];

    // Seed with glider and pulsar
    let mut rng = now();
    for y in 0..LIFE_H {
        for x in 0..LIFE_W {
            rng ^= rng << 13;
            rng ^= rng >> 7;
            rng ^= rng << 17;
            grid[y][x] = (rng & 0x7) == 0;
        }
    }

    let bg = term::theme_bg();
    let fg = term::LIGHT_GREEN;

    let mut auto = false;

    loop {
        // Draw grid
        for y in 0..LIFE_H {
            for x in 0..LIFE_W {
                let ch = if grid[y][x] { 0xfe } else { b' ' }; // CP437 solid square
                term::put_at_colored(x + 1, y + 2, ch, fg, bg);
            }
        }

        if let Some(key) = keyboard::poll() {
            match key {
                Key::Esc | Key::Char(b'q') | Key::Char(b'Q') => break,
                Key::Char(b' ') => auto = !auto,
                Key::Char(b'r') | Key::Char(b'R') => {
                    for y in 0..LIFE_H {
                        for x in 0..LIFE_W {
                            rng ^= rng << 13;
                            rng ^= rng >> 7;
                            rng ^= rng << 17;
                            grid[y][x] = (rng & 0x7) == 0;
                        }
                    }
                }
                _ => {}
            }
        }

        // Compute next generation
        for y in 0..LIFE_H {
            for x in 0..LIFE_W {
                let mut neighbors = 0;
                for dy in [-1isize, 0, 1] {
                    for dx in [-1isize, 0, 1] {
                        if dy == 0 && dx == 0 {
                            continue;
                        }
                        let ny = (y as isize + dy).rem_euclid(LIFE_H as isize) as usize;
                        let nx = (x as isize + dx).rem_euclid(LIFE_W as isize) as usize;
                        if grid[ny][nx] {
                            neighbors += 1;
                        }
                    }
                }
                next[y][x] = if grid[y][x] {
                    neighbors == 2 || neighbors == 3
                } else {
                    neighbors == 3
                };
            }
        }
        grid = next;

        // Delay
        let delay_ms = if auto { 80 } else { 200 };
        let start = now();
        let target = tsc_per_ms() * delay_ms;
        while now().wrapping_sub(start) < target {
            if keyboard::poll().is_some() {
                break;
            }
            core::hint::spin_loop();
        }
    }

    term::clear();
    term::status_bar();
}

// --- 16-COLOR GEOMETRIC GRAPHICS DEMO ---

pub fn demo_lines() {
    term::clear();
    let bg = term::theme_bg();
    term::draw_text_at_colored(1, 0, "[ TempleOS 16-Color Graphics Demo ]", term::YELLOW, term::BROWN);
    term::draw_text_at_colored(1, term::STATUS_ROW, "Press any key to return...", term::WHITE, term::DARK_GRAY);

    let colors = [
        term::BLUE, term::GREEN, term::CYAN, term::RED, term::PURPLE,
        term::BROWN, term::LIGHT_GRAY, term::DARK_GRAY, term::LIGHT_BLUE,
        term::LIGHT_GREEN, term::LIGHT_CYAN, term::LIGHT_RED, term::LIGHT_PURPLE,
        term::YELLOW, term::WHITE,
    ];

    for step in 0..15 {
        let col = colors[step % colors.len()];
        let y = 3 + step;
        term::put_at_colored(5 + step * 2, y, 0xda, col, bg);
        for x in (6 + step * 2)..=(70 - step * 2) {
            term::put_at_colored(x, y, 0xc4, col, bg);
        }
        term::put_at_colored(71 - step * 2, y, 0xbf, col, bg);
    }

    term::draw_text_at_colored(24, 15, "16-COLOR PALETTE SYSTEM", term::YELLOW, bg);
    for (i, &c) in colors.iter().enumerate() {
        term::put_at_colored(25 + i * 2, 17, 0xdb, c, bg);
        term::put_at_colored(25 + i * 2 + 1, 17, 0xdb, c, bg);
    }

    while keyboard::poll().is_none() {
        core::hint::spin_loop();
    }
    term::clear();
    term::status_bar();
}

// --- SNAKE ---

const SX0: usize = 1;
const SX1: usize = 78;
const SY0: usize = 3;
const SY1: usize = 27;
const MAX_SNAKE: usize = 1024;

static mut SNAKE_X: [u8; MAX_SNAKE] = [0; MAX_SNAKE];
static mut SNAKE_Y: [u8; MAX_SNAKE] = [0; MAX_SNAKE];
static mut RNG: u64 = 0x9e37_79b9_7f4a_7c15;

#[derive(Clone, Copy, PartialEq)]
enum Dir {
    Up,
    Down,
    Left,
    Right,
}

fn rnd() -> u64 {
    unsafe {
        RNG ^= RNG << 13;
        RNG ^= RNG >> 7;
        RNG ^= RNG << 17;
        RNG
    }
}

fn draw_num_at(x: usize, y: usize, v: i64, fg: u8, bg: u8) {
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
    term::draw_text_at_colored(x, y, core::str::from_utf8(&buf[i..]).unwrap_or(""), fg, bg);
}

fn place_food(len: usize) -> (usize, usize) {
    loop {
        let x = SX0 + (rnd() as usize) % (SX1 - SX0 + 1);
        let y = SY0 + (rnd() as usize) % (SY1 - SY0 + 1);
        let mut clash = false;
        unsafe {
            for i in 0..len {
                if SNAKE_X[i] as usize == x && SNAKE_Y[i] as usize == y {
                    clash = true;
                    break;
                }
            }
        }
        if !clash {
            return (x, y);
        }
    }
}

pub fn snake() {
    unsafe {
        RNG ^= now();
    }
    term::clear();

    let bg = term::theme_bg();
    let border_fg = term::CYAN;

    for x in 0..term::COLS {
        term::put_at_colored(x, SY0 - 1, 0xcd, border_fg, bg);
        term::put_at_colored(x, SY1 + 1, 0xcd, border_fg, bg);
    }
    for y in (SY0 - 1)..=(SY1 + 1) {
        term::put_at_colored(0, y, 0xba, border_fg, bg);
        term::put_at_colored(term::COLS - 1, y, 0xba, border_fg, bg);
    }

    let head_x = 40usize;
    let head_y = 15usize;
    let mut len = 3usize;
    unsafe {
        for i in 0..len {
            SNAKE_X[i] = (head_x - i) as u8;
            SNAKE_Y[i] = head_y as u8;
        }
    }
    for i in 0..len {
        let ch = if i == 0 { b'@' } else { b'o' };
        let fg = if i == 0 { term::YELLOW } else { term::GREEN };
        term::put_at_colored(head_x - i, head_y, ch, fg, bg);
    }

    let mut food = place_food(len);
    term::put_at_colored(food.0, food.1, b'*', term::LIGHT_RED, bg);

    let mut dir = Dir::Right;
    let mut pending = Dir::Right;
    let mut score = 0i64;
    let mut last = now();
    let step = tsc_per_ms() * 140;

    term::draw_text_at_colored(1, term::STATUS_ROW, "SNAKE  score ", term::YELLOW, term::DARK_GRAY);
    draw_num_at(14, term::STATUS_ROW, score, term::YELLOW, term::DARK_GRAY);
    term::draw_text_at_colored(20, term::STATUS_ROW, "arrows move, Q quits", term::LIGHT_GRAY, term::DARK_GRAY);

    loop {
        while let Some(key) = keyboard::poll() {
            match key {
                Key::Up if dir != Dir::Down => pending = Dir::Up,
                Key::Down if dir != Dir::Up => pending = Dir::Down,
                Key::Left if dir != Dir::Right => pending = Dir::Left,
                Key::Right if dir != Dir::Left => pending = Dir::Right,
                Key::Esc | Key::Char(b'q') | Key::Char(b'Q') => return game_over(score, true),
                _ => {}
            }
        }

        let t = now();
        if t.wrapping_sub(last) < step {
            core::hint::spin_loop();
            continue;
        }
        last = t;
        dir = pending;

        let (mut nx, mut ny) = unsafe { (SNAKE_X[0] as usize, SNAKE_Y[0] as usize) };
        match dir {
            Dir::Up => ny = ny.wrapping_sub(1),
            Dir::Down => ny += 1,
            Dir::Left => nx = nx.wrapping_sub(1),
            Dir::Right => nx += 1,
        }

        if nx < SX0 || nx > SX1 || ny < SY0 || ny > SY1 {
            return game_over(score, false);
        }
        let mut hit = false;
        unsafe {
            for i in 0..len - 1 {
                if SNAKE_X[i] as usize == nx && SNAKE_Y[i] as usize == ny {
                    hit = true;
                    break;
                }
            }
        }
        if hit {
            return game_over(score, false);
        }

        let ate = nx == food.0 && ny == food.1;
        unsafe {
            if !ate {
                let tx = SNAKE_X[len - 1] as usize;
                let ty = SNAKE_Y[len - 1] as usize;
                term::put_at_colored(tx, ty, b' ', term::theme_fg(), bg);
            }
            let from = if ate { len } else { len - 1 };
            let mut i = from;
            while i > 0 {
                SNAKE_X[i] = SNAKE_X[i - 1];
                SNAKE_Y[i] = SNAKE_Y[i - 1];
                i -= 1;
            }
            SNAKE_X[0] = nx as u8;
            SNAKE_Y[0] = ny as u8;
            if ate {
                if len + 1 >= MAX_SNAKE {
                    return game_over(score, false);
                }
                len += 1;
            }
        }
        term::put_at_colored(nx, ny, b'@', term::YELLOW, bg);
        unsafe {
            let bx = SNAKE_X[1] as usize;
            let by = SNAKE_Y[1] as usize;
            term::put_at_colored(bx, by, b'o', term::GREEN, bg);
        }

        if ate {
            score += 1;
            sound::beep(800, 30, tsc_per_ms());
            term::draw_text_at_colored(14, term::STATUS_ROW, "      ", term::YELLOW, term::DARK_GRAY);
            draw_num_at(14, term::STATUS_ROW, score, term::YELLOW, term::DARK_GRAY);
            food = place_food(len);
            term::put_at_colored(food.0, food.1, b'*', term::LIGHT_RED, bg);
        }
    }
}

fn game_over(score: i64, quit: bool) {
    sound::beep(200, 200, tsc_per_ms());
    let y = (term::BODY_TOP + term::BODY_BOTTOM) / 2;
    let msg = if quit { "BYE" } else { "GAME OVER" };
    let x = (term::COLS - msg.len()) / 2;
    let bg = term::theme_bg();
    term::draw_text_at_colored(x, y, msg, term::LIGHT_RED, bg);
    term::draw_text_at_colored(x - 3, y + 1, "score", term::theme_fg(), bg);
    draw_num_at(x + 3, y + 1, score, term::YELLOW, bg);
    term::draw_text_at_colored(x - 8, y + 3, "press any key to return", term::DARK_GRAY, bg);
    while keyboard::poll().is_none() {
        core::hint::spin_loop();
    }
    term::clear();
    term::status_bar();
}

fn bench() {
    let w = crate::fb::width();
    let h = crate::fb::height();
    let per_sec = 1000 / BENCH_MS;

    term::set_fg(term::CYAN);
    term::print_str("BENCH: ");
    term::print_str(term::mode_text());
    term::print_str(if crate::fb::is_linear() {
        " linear, "
    } else {
        " planar, "
    });
    term::print_num(BENCH_MS as i64);
    term::print_str(" ms windows\n");
    term::set_fg(term::theme_fg());

    let mut frames: u64 = 0;
    pit_start(BENCH_MS);
    while !pit_expired() {
        crate::fb::fill_rect(0, 0, w, h, (frames & 0x0f) as u8);
        frames += 1;
    }
    let bytes = (crate::fb::pitch() as u64) * (h as u64);
    let fill_fps = frames * per_sec;
    let mibs = frames * bytes * per_sec / (1024 * 1024);

    let mut paints: u64 = 0;
    pit_start(BENCH_MS);
    while !pit_expired() {
        term::repaint();
        paints += 1;
    }
    let paint_fps = paints * per_sec;

    term::repaint();

    term::print_str("  full-screen fill  ");
    term::print_num(fill_fps as i64);
    term::print_str(" fps   ");
    term::print_num(mibs as i64);
    term::print_str(" MiB/s\n");
    term::print_str("  text repaint      ");
    term::print_num(paint_fps as i64);
    term::print_str(" fps\n");
}

fn reboot() -> ! {
    term::set_fg(term::YELLOW);
    term::print_str("rebooting...\n");
    unsafe {
        for _ in 0..1_000_000 {
            let st: u8;
            core::arch::asm!("in al, dx", in("dx") 0x64u16, out("al") st,
                             options(nostack, preserves_flags));
            if st & 2 == 0 {
                break;
            }
        }
        core::arch::asm!("out dx, al", in("dx") 0x64u16, in("al") 0xfeu8,
                         options(nostack, preserves_flags));
    }
    halt()
}

fn halt() -> ! {
    term::set_fg(term::DARK_GRAY);
    term::print_str("CPU halted. Power off the machine.\n");
    loop {
        unsafe {
            core::arch::asm!("cli", "hlt", options(nomem, nostack));
        }
    }
}
