//! In-memory contiguous RedSea-style virtual filesystem.
//!
//! Organizes files in C:/Home, C:/Doc, C:/Apps, C:/Demo with genuine DolDoc
//! documents and HolyC source headers (strictly technical and non-religious).

pub const MAX_FILES: usize = 32;
pub const MAX_FILE_SIZE: usize = 2048;
pub const MAX_NAME: usize = 32;
pub const MAX_PATH: usize = 64;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FileType {
    File,
    Dir,
}

#[derive(Clone, Copy)]
pub struct VfsEntry {
    pub name: [u8; MAX_NAME],
    pub name_len: usize,
    pub path: [u8; MAX_PATH], // parent directory, e.g. "C:/Doc"
    pub path_len: usize,
    pub ftype: FileType,
    pub size: usize,
    pub data: [u8; MAX_FILE_SIZE],
}

impl VfsEntry {
    pub const fn empty() -> Self {
        Self {
            name: [0; MAX_NAME],
            name_len: 0,
            path: [0; MAX_PATH],
            path_len: 0,
            ftype: FileType::File,
            size: 0,
            data: [0; MAX_FILE_SIZE],
        }
    }

    pub fn name_str(&self) -> &str {
        core::str::from_utf8(&self.name[..self.name_len]).unwrap_or("")
    }

    pub fn path_str(&self) -> &str {
        core::str::from_utf8(&self.path[..self.path_len]).unwrap_or("")
    }

    pub fn content_str(&self) -> &str {
        core::str::from_utf8(&self.data[..self.size]).unwrap_or("")
    }
}

pub struct Vfs {
    pub entries: [VfsEntry; MAX_FILES],
    pub count: usize,
    pub cwd: [u8; MAX_PATH],
    pub cwd_len: usize,
}

static mut VFS: Vfs = Vfs {
    entries: [VfsEntry::empty(); MAX_FILES],
    count: 0,
    cwd: [0; MAX_PATH],
    cwd_len: 0,
};

fn vfs() -> *mut Vfs {
    core::ptr::addr_of_mut!(VFS)
}

pub fn init() {
    let v = unsafe { &mut *vfs() };
    v.count = 0;
    set_cwd("C:/Home");

    // Create default directories
    mkdir("C:/");
    mkdir("C:/Home");
    mkdir("C:/Doc");
    mkdir("C:/Apps");
    mkdir("C:/Demo");

    // Populate documentation and source files
    add_file(
        "C:/Doc",
        "Charter.DD",
        "$FG,YELLOW$$TX+CX,\"=== TEMPLEOS DESIGN CHARTER ===\"$\n\n\
$FG,WHITE$1. Ring 0, single-address-space operating system.\n\
2. Fixed 640x480 resolution with 16-color palette.\n\
3. Non-preemptive, cooperative multitasking.\n\
4. Built-in HolyC JIT compiler and DolDoc hypertext.\n\
5. 100,000 lines-of-code limit to maintain simplicity.\n\
6. Contiguous 64-bit RedSea filesystem without fragmentation.\n\
7. Direct hardware access: screen, keyboard, PC speaker.\n\
8. Complete transparency: every line of kernel and user code is editable.\n",
    );

    add_file(
        "C:/Doc",
        "DolDoc.DD",
        "$FG,YELLOW$$TX+CX,\"=== DOLDOC HYPERTEXT SPECIFICATION ===\"$\n\n\
$FG,WHITE$DolDoc is TempleOS's document format, UI layer, and terminal output stream.\n\
Commands are enclosed in dollar signs ($FG,CYAN$$$$$FG,WHITE$).\n\n\
$FG,LTGREEN$Supported Commands:$FG,WHITE$\n\
  $FG,YELLOW$$$$FG,col$$$$$FG,WHITE$   Set foreground color (0..15 or name)\n\
  $FG,YELLOW$$$$BG,col$$$$$FG,WHITE$   Set background color\n\
  $FG,YELLOW$$$$FD,col$$$$$FG,WHITE$   Set default foreground color\n\
  $FG,YELLOW$$$$BD,col$$$$$FG,WHITE$   Set default background color\n\
  $FG,YELLOW$$$$TX+CX,\"text\"$$$$$FG,WHITE$ Center-aligned text block\n\
  $FG,YELLOW$$$$IV,1$$$$$FG,WHITE$     Invert foreground and background\n\
  $FG,YELLOW$$$$MA,\"label\",LM=\"cmd\"$$$$$FG,WHITE$ Interactive macro button\n\
  $FG,YELLOW$$$$$$$$$FG,WHITE$         Escaped literal dollar sign\n\n\
$FG,CYAN$Press $FG,YELLOW$<CTRL-T>$FG,CYAN$ at any time to toggle between formatted and raw view!$FG,WHITE$\n",
    );

    add_file(
        "C:/Doc",
        "HolyC.DD",
        "$FG,YELLOW$$TX+CX,\"=== HOLYC LANGUAGE REFERENCE ===\"$\n\n\
$FG,WHITE$HolyC is an extension of C designed for instant interactive execution.\n\n\
$FG,LTGREEN$Key Differences from C:$FG,WHITE$\n\
* Functions with zero arguments can omit parentheses:\n\
    $FG,YELLOW$Dir;       $FG,DKGRAY$// equivalent to Dir();$FG,WHITE$\n\
* Statements at the shell prompt are JIT-compiled directly:\n\
    $FG,YELLOW$Type(\"Charter.DD\");$FG,WHITE$\n\
* App sources in apps/*.HC are AOT-compiled off-line (host tool hcc/)\n\
  into flat .BIN blobs embedded in the kernel; run with Run Hello;.\n\
* All commands use forward slashes for paths: $FG,YELLOW$C:/Doc$FG,WHITE$\n\
* Dynamic typing and default function arguments are standard.\n\
* Ring 0 access: in/out hardware instructions and direct memory access.\n",
    );

    add_file(
        "C:/Doc",
        "Help.DD",
        "$FG,YELLOW$$TX+CX,\"=== TEMPLEOS/RUST COMMAND INDEX ===\"$\n\n\
$FG,LTGREEN$Graphical Applications (GUIs):$FG,WHITE$\n\
  $FG,YELLOW$FileMgr;         $FG,WHITE$Dual-pane graphical file manager with mouse support\n\
  $FG,YELLOW$CalcGui;         $FG,WHITE$Interactive windowed desktop calculator\n\
  $FG,YELLOW$SysMon;          $FG,WHITE$System monitor dashboard and hardware meters\n\
  $FG,YELLOW$Ed(\"file\");      $FG,WHITE$Interactive DolDoc full-screen text editor\n\n\
$FG,LTGREEN$File Navigation:$FG,WHITE$\n\
  $FG,YELLOW$Dir;            $FG,WHITE$List directory contents with DolDoc links\n\
  $FG,YELLOW$Cd(\"path\");      $FG,WHITE$Change directory (e.g. Cd(\"C:/Doc\"); Cd(\"..\");)\n\
  $FG,YELLOW$Pwd;            $FG,WHITE$Print working directory\n\
  $FG,YELLOW$Type(\"file\");    $FG,WHITE$Display text/DolDoc file (or Cat(\"file\");)\n\
  $FG,YELLOW$Del(\"file\");     $FG,WHITE$Delete file\n\n\
$FG,LTGREEN$Audio & Hardware:$FG,WHITE$\n\
  $FG,YELLOW$Beep;           $FG,WHITE$Play standard system beep\n\
  $FG,YELLOW$Snd(freq);      $FG,WHITE$PC speaker tone (e.g. Snd(440); Snd; to stop)\n\
  $FG,YELLOW$Play(\"notes\");   $FG,WHITE$Play music macro (e.g. Play(\"O4 L8 C D E F G A B\");)\n\n\
$FG,LTGREEN$System & Diagnostics:$FG,WHITE$\n\
  $FG,YELLOW$MemRep;         $FG,WHITE$Display memory allocation and boot tables\n\
  $FG,YELLOW$CpuRep;         $FG,WHITE$Display CPU registers, TSC, and long mode info\n\
  $FG,YELLOW$Theme(\"name\");   $FG,WHITE$Switch theme (\"classic\" white canvas / \"blue\" / \"dark\")\n\
  $FG,YELLOW$DocToggle;      $FG,WHITE$Toggle raw DolDoc markup ($FG,YELLOW$<CTRL-T>$FG,WHITE$)\n\
  $FG,YELLOW$Cls;            $FG,WHITE$Clear screen\n\
  $FG,YELLOW$Fps(n);         $FG,WHITE$Set the main-loop frame-rate cap (e.g. Fps(30); Fps(60); Fps;)\n\
  $FG,YELLOW$Video(w x h);   $FG,WHITE$Change screen resolution at runtime (e.g. Video(1280x800);)\n\
  $FG,YELLOW$Bench;          $FG,WHITE$Measure fill & text repaint FPS\n\
  $FG,YELLOW$Reboot; / Halt; $FG,WHITE$Reboot or halt system\n\n\
$FG,LTGREEN$Compiled HolyC Apps (AOT):$FG,WHITE$\n\
  $FG,YELLOW$Run;$FG,WHITE$            List AOT-compiled HolyC apps with sizes\n\
  $FG,YELLOW$Run Name;$FG,WHITE$       Load and run a compiled app in ring 0 (Run Hello;)\n\
  $FG,YELLOW$Name;$FG,WHITE$          Bare app name is shorthand (Hello; Plasma;)\n\n\
$FG,LTGREEN$Demos & Games:$FG,WHITE$\n\
  $FG,YELLOW$Snake;          $FG,WHITE$Play classic snake game\n\
  $FG,YELLOW$Life;           $FG,WHITE$Conway's Game of Life simulation\n\
  $FG,YELLOW$Lines;          $FG,WHITE$16-color geometric graphics demo\n",
    );

    add_file(
        "C:/Home",
        "Notes.DD",
        "$FG,YELLOW$$TX+CX,\"=== USER SCRATCHPAD ===\"$\n\n\
$FG,LTBLUE$Welcome to TempleOS/Rust!$FG,WHITE$\n\
Use $FG,YELLOW$Ed(\"Notes.DD\");$FG,WHITE$ to edit this document in real time.\n\
Press $FG,YELLOW$<ESC>$FG,WHITE$ inside the editor to save and exit.\n\n\
$MA,\"Open Help\",LM=\"Help;\"$\n",
    );

    add_file(
        "C:/Apps",
        "Snake.HC",
        "// Snake Arcade Game for TempleOS/Rust\n\
// Controls: Arrow keys to steer, 'Q' to quit.\n\
U0 SnakeMain()\n\
{\n\
    \"Starting Snake...\\n\";\n\
    Snake;\n\
}\n",
    );

    add_file(
        "C:/Apps",
        "Life.HC",
        "// Conway's Game of Life Simulation\n\
// Simulates cellular automata on a 640x480 grid.\n\
U0 LifeMain()\n\
{\n\
    \"Running Life simulation...\\n\";\n\
    Life;\n\
}\n",
    );

    add_file(
        "C:/Demo",
        "Speaker.HC",
        "// PC Speaker & Music Macro Demonstration\n\
U0 PlayOdeToJoy()\n\
{\n\
    \"Playing Ode to Joy on PC speaker...\\n\";\n\
    Play(\"O4 L4 E E F G G F E D C C D E E. D8 D2\");\n\
}\n",
    );
}

pub fn cwd_str() -> &'static str {
    let v = unsafe { &*vfs() };
    core::str::from_utf8(&v.cwd[..v.cwd_len]).unwrap_or("C:/Home")
}

pub fn set_cwd(path: &str) {
    let v = unsafe { &mut *vfs() };
    let bytes = path.as_bytes();
    let len = bytes.len().min(MAX_PATH);
    v.cwd[..len].copy_from_slice(&bytes[..len]);
    v.cwd_len = len;
}

pub fn mkdir(path: &str) -> bool {
    let v = unsafe { &mut *vfs() };
    if v.count >= MAX_FILES {
        return false;
    }
    let (parent, name) = split_parent_name(path);
    if find_entry(parent, name).is_some() {
        return true;
    }
    let mut entry = VfsEntry::empty();
    entry.ftype = FileType::Dir;
    let nbytes = name.as_bytes();
    let nlen = nbytes.len().min(MAX_NAME);
    entry.name[..nlen].copy_from_slice(&nbytes[..nlen]);
    entry.name_len = nlen;

    let pbytes = parent.as_bytes();
    let plen = pbytes.len().min(MAX_PATH);
    entry.path[..plen].copy_from_slice(&pbytes[..plen]);
    entry.path_len = plen;

    v.entries[v.count] = entry;
    v.count += 1;
    true
}

pub fn add_file(parent: &str, name: &str, content: &str) -> bool {
    let v = unsafe { &mut *vfs() };
    if v.count >= MAX_FILES {
        return false;
    }
    if let Some(idx) = find_entry(parent, name) {
        // Overwrite
        let cbytes = content.as_bytes();
        let clen = cbytes.len().min(MAX_FILE_SIZE);
        v.entries[idx].data[..clen].copy_from_slice(&cbytes[..clen]);
        v.entries[idx].size = clen;
        return true;
    }
    let mut entry = VfsEntry::empty();
    entry.ftype = FileType::File;
    let nbytes = name.as_bytes();
    let nlen = nbytes.len().min(MAX_NAME);
    entry.name[..nlen].copy_from_slice(&nbytes[..nlen]);
    entry.name_len = nlen;

    let pbytes = parent.as_bytes();
    let plen = pbytes.len().min(MAX_PATH);
    entry.path[..plen].copy_from_slice(&pbytes[..plen]);
    entry.path_len = plen;

    let cbytes = content.as_bytes();
    let clen = cbytes.len().min(MAX_FILE_SIZE);
    entry.data[..clen].copy_from_slice(&cbytes[..clen]);
    entry.size = clen;

    v.entries[v.count] = entry;
    v.count += 1;
    true
}

pub fn entries_count() -> usize {
    unsafe { (*vfs()).count }
}

pub fn get_entry(idx: usize) -> Option<&'static VfsEntry> {
    let v = unsafe { &*vfs() };
    if idx < v.count {
        Some(&v.entries[idx])
    } else {
        None
    }
}

pub fn delete_file(path: &str) -> bool {
    let v = unsafe { &mut *vfs() };
    let (parent, name) = resolve_path(path);
    if let Some(idx) = find_entry(parent.as_str(), name.as_str()) {
        for i in idx..v.count - 1 {
            v.entries[i] = v.entries[i + 1];
        }
        v.count -= 1;
        true
    } else {
        false
    }
}

pub fn read_file(path: &str) -> Option<&'static str> {
    let (parent, name) = resolve_path(path);
    let v = unsafe { &*vfs() };
    for i in 0..v.count {
        let e = &v.entries[i];
        if e.ftype == FileType::File
            && e.path_str().eq_ignore_ascii_case(parent.as_str())
            && e.name_str().eq_ignore_ascii_case(name.as_str())
        {
            return Some(e.content_str());
        }
    }
    None
}

pub fn find_entry(parent: &str, name: &str) -> Option<usize> {
    let v = unsafe { &*vfs() };
    for i in 0..v.count {
        let e = &v.entries[i];
        if e.path_str().eq_ignore_ascii_case(parent)
            && e.name_str().eq_ignore_ascii_case(name)
        {
            return Some(i);
        }
    }
    None
}

fn split_parent_name(path: &str) -> (&str, &str) {
    let path = path.trim();
    if path == "C:/" || path == "/" || path == "::" {
        return ("C:/", "");
    }
    let p = path.trim_end_matches('/');
    if let Some(i) = p.rfind('/') {
        let parent = if i == 2 && &p[1..2] == ":" {
            &p[..=2] // "C:/"
        } else if i == 0 {
            "C:/"
        } else {
            &p[..i]
        };
        (parent, &p[i + 1..])
    } else {
        ("C:/", p)
    }
}

/// Resolve relative or absolute path against cwd. Returns (parent, name).
pub fn resolve_path(path: &str) -> (HeaplessString, HeaplessString) {
    let path = path.trim();
    let mut full = [0u8; MAX_PATH];
    let mut flen = 0;

    let cwd = cwd_str();
    if path.starts_with("C:/") || path.starts_with("c:/") {
        let b = path.as_bytes();
        flen = b.len().min(MAX_PATH);
        full[..flen].copy_from_slice(&b[..flen]);
    } else if path.starts_with('/') {
        let p = "C:";
        let b = path.as_bytes();
        let total = p.len() + b.len();
        flen = total.min(MAX_PATH);
        full[..2].copy_from_slice(p.as_bytes());
        full[2..flen].copy_from_slice(&b[..flen - 2]);
    } else if path.starts_with("~") {
        let sub = &path[1..].trim_start_matches('/');
        let base = "C:/Home";
        if sub.is_empty() {
            flen = base.len();
            full[..flen].copy_from_slice(base.as_bytes());
        } else {
            let total = base.len() + 1 + sub.len();
            flen = total.min(MAX_PATH);
            full[..base.len()].copy_from_slice(base.as_bytes());
            full[base.len()] = b'/';
            let rem = flen - (base.len() + 1);
            full[base.len() + 1..flen].copy_from_slice(&sub.as_bytes()[..rem]);
        }
    } else if path == ".." {
        if cwd == "C:/" || cwd == "C:" {
            flen = 3;
            full[..3].copy_from_slice(b"C:/");
        } else if let Some(i) = cwd.rfind('/') {
            let parent = if i <= 2 { "C:/" } else { &cwd[..i] };
            flen = parent.len();
            full[..flen].copy_from_slice(parent.as_bytes());
        }
    } else {
        // Relative to cwd
        let sub = path.trim_start_matches('/');
        let sep = if cwd.ends_with('/') { "" } else { "/" };
        let total = cwd.len() + sep.len() + sub.len();
        flen = total.min(MAX_PATH);
        let mut idx = 0;
        full[idx..idx + cwd.len()].copy_from_slice(cwd.as_bytes());
        idx += cwd.len();
        if !sep.is_empty() {
            full[idx..idx + sep.len()].copy_from_slice(sep.as_bytes());
            idx += sep.len();
        }
        let rem = flen - idx;
        full[idx..flen].copy_from_slice(&sub.as_bytes()[..rem]);
    }

    let full_str = core::str::from_utf8(&full[..flen]).unwrap_or("C:/Home");
    let (parent, name) = split_parent_name(full_str);
    (HeaplessString::from_str(parent), HeaplessString::from_str(name))
}

#[derive(Clone, Copy)]
pub struct HeaplessString {
    buf: [u8; MAX_PATH],
    len: usize,
}

impl HeaplessString {
    pub fn from_str(s: &str) -> Self {
        let mut buf = [0u8; MAX_PATH];
        let bytes = s.as_bytes();
        let len = bytes.len().min(MAX_PATH);
        buf[..len].copy_from_slice(&bytes[..len]);
        Self { buf, len }
    }

    pub fn as_str(&self) -> &str {
        core::str::from_utf8(&self.buf[..self.len]).unwrap_or("")
    }
}
