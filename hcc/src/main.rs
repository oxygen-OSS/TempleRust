// -------------------------------------------------------------------------

//! hcc — an ahead-of-time compiler for the HolyC subset used by this OS.
//!
//! hcc reads an `.HC` source file plus the kernel's fixed-address API table
//! (`build/kernel_api.nm`) and emits a flat x86-64 blob with an HC01 header
//! — no linker, no object files, no relocations, exactly TempleOS AOT:
//!
//! ```text
//! +0  u32 magic 0x31304348 ("HC01")</think><tool_call>edit<arg_key>newString</arg_key><arg_value>// -------------------------------------------------------------------------

//! hcc — an ahead-of-time compiler for the HolyC subset used by this OS.
//!
//! hcc reads an `.HC` source file plus the kernel's fixed-address API table
//! (`build/kernel_api.nm`) and emits a flat x86-64 blob with an HC01 header
//! — no linker, no object files, no relocations, exactly TempleOS AOT:
//!
//! ```text
//! +0  u32 magic 0x31304348 ("HC01")
//! +4  u32 entry_offset   (blob offset of Main, or the first function)
//! +8  u32 total_len
//! +12 u32 data_offset    (zero-init globals, string literals)
//! +16 .. then code bytes .. then data bytes
//! ```
//!
//! Code is generated for its final load address (LOAD_BASE 0x200000, the
//! kernel loader's PROG_BASE). Calls are `CALL rel32` — to fixed `.hcapi`
//! kernel addresses for `Print`, `Snd`, `Sleep`, ... and rel32 app-to-app.
//! Apps run in ring 0 in one address space and may scribble anywhere,
//! including the framebuffer statics at 0x68000.
//!
//! Data model: everything is a signed-agnostic 64-bit integer in `rax`.
//! Binary ops go through push/pop; a compiler-tracked outstanding-push
//! counter makes stack alignment deterministic: one `sub rsp,8` when odd.
//! Locals live at fixed RBP offsets and are zeroed by the prologue.

use std::collections::HashMap;
use std::fs;
use std::process::exit;

mod lexparse {
    /// Token / lexer / parser subdream — kept in one module for simplicity.
    use super::*;

    pub fn lex(src: &[u8], name: &str) -> Vec<T> {
        match src.iter().position(|&b| b == 0) {
            None => Lex::new(src).run(),
            _ => {
                eprintln!("hcc: {name}: NUL byte in source is not supported");
                exit(1);
            }
        }
    }

    pub fn parse(src: &[u8], name: &str) -> (Vec<Global>, Vec<FnDef>, Vec<Vec<u8>>) {
        let ts = lex(src, name);
        let mut p = P { ts, i: 0, fns: vec![], globals: vec![], strings: vec![], strmap: HashMap::new() };
        p.program(name);
        (p.globals, p.fns, p.strings)
    }
}

// ======================================================== main data types ==

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum B {
    Void,
    U8,
    U16,
    U32,
    U64,
    I8,
    I16,
    I32,
    I64,
}

impl B {
    fn size(self) -> u64 {
        match self {
            B::U8 | B::I8 => 1,
            B::U16 | B::I16 => 2,
            B::U32 | B::I32 => 4,
            B::U64 | B::I64 | B::Void => 8,
        }
    }
    fn signed(self) -> bool {
        matches!(self, B::I8 | B::I16 | B::I32 | B::I64)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Ty {
    B(B),
    P(B),
}

impl Ty {
    fn base(self) -> B {
        match self {
            Ty::B(b) | Ty::P(b) => b,
        }
    }
    fn is_ptr(self) -> bool {
        matches!(self, Ty::P(_))
    }
    fn bytes(self) -> u64 {
        match self {
            Ty::B(b) => b.size(),
            Ty::P(_) => 8,
        }
    }
}

fn load_shape(ty: Ty) -> u64 {
    // 1 byte / 2 word / 4 dword / 8 qword load widths.
    match ty {
        Ty::B(b) => b.size(),
        Ty::P(_) => 8,
    }
}

// =============================================================== constants ==

const LOAD_BASE: u64 = 0x200_000;
const HDR_MAGIC: u32 = 0x3130_4348;
const HDR_LEN: u64 = 16;

fn fail(msg: &str, line: u32) -> ! {
    eprintln!("hcc: error: {} (line {})", msg, line);
    exit(1);
}

// ==================================================================== lexer ==

#[derive(Clone, Debug)]
enum Tok {
    Idx(String),
    N(u64),
    Ch(u8),
    S(Vec<u8>),
    P(String),
}

#[derive(Clone, Debug)]
struct T {
    k: Tok,
    ln: u32,
}

struct Lex {
    b: Vec<u8>,
    i: usize,
    ln: u32,
}

impl Lex {
    fn new(src: &[u8]) -> Self {
        Self { b: src.to_vec(), i: 0, ln: 1 }
    }
    fn err(&self, m: &str) -> ! {
        fail(m, self.ln)
    }
    fn eof(&self) -> bool {
        self.i >= self.b.len()
    }
    fn peek(&self) -> u8 {
        if self.eof() { 0 } else { self.b[self.i] }
    }
    fn peek2(&self) -> u8 {
        if self.i + 1 >= self.b.len() { 0 } else { self.b[self.i + 1] }
    }
    fn next(&mut self) -> u8 {
        let c = self.peek();
        self.i += 1;
        if c == b'\n' {
            self.ln += 1;
        }
        c
    }
    fn skip_ws(&mut self) {
        loop {
            let c = self.peek();
            if c.is_ascii_whitespace() {
                self.next();
            } else if c == b'/' && self.peek2() == b'/' {
                while !self.eof() && self.peek() != b'\n' {
                    self.next();
                }
            } else if c == b'/' && self.peek2() == b'*' {
                self.next();
                self.next();
                loop {
                    if self.eof() {
                        self.err("unterminated /* comment");
                    }
                    if self.peek() == b'*' && self.peek2() == b'/' {
                        self.next();
                        self.next();
                        break;
                    }
                    self.next();
                }
            } else {
                break;
            }
        }
    }
    fn escape(&mut self) -> u8 {
        let s = self.next();
        match s {
            b'n' => b'\n',
            b'r' => b'\r',
            b't' => b'\t',
            b'0' => 0,
            b'\\' => b'\\',
            b'"' => b'"',
            b'\'' => b'\'',
            b'x' => {
                let mut v: u64 = 0;
                let mut any = false;
                while self.peek().is_ascii_hexdigit() {
                    let d = (self.peek() as char).to_digit(16).unwrap() as u64;
                    v = v * 16 + d;
                    self.next();
                    any = true;
                }
                if !any {
                    self.err("\\x with no digits");
                }
                v as u8
            }
            other => other,
        }
    }

    fn run(mut self) -> Vec<T> {
        let mut out = Vec::new();
        loop {
            self.skip_ws();
            if self.eof() {
                break;
            }
            let ln = self.ln;
            let c = self.peek();
            if c == b'#' {
                while !self.eof() && self.peek() != b'\n' {
                    self.next();
                }
                continue;
            }
            let k = if c.is_ascii_alphabetic() || c == b'_' {
                let mut s = Vec::new();
                while self.peek().is_ascii_alphanumeric() || self.peek() == b'_' {
                    s.push(self.next());
                }
                Tok::Idx(String::from_utf8_lossy(&s).into_owned())
            } else if c.is_ascii_digit() {
                let mut v: u64 = 0;
                if c == b'0' && (self.peek2() == b'x' || self.peek2() == b'X') {
                    self.next();
                    self.next();
                    if !self.peek().is_ascii_hexdigit() {
                        self.err("hex digits expected after 0x");
                    }
                    while self.peek().is_ascii_hexdigit() {
                        let d = (self.peek() as char).to_digit(16).unwrap() as u64;
                        v = v * 16 + d;
                        self.next();
                    }
                } else {
                    while self.peek().is_ascii_digit() {
                        v = v * 10 + (self.next() - b'0') as u64;
                    }
                }
                Tok::N(v)
            } else if c == b'"' {
                self.next();
                let mut bs = Vec::new();
                while self.peek() != b'"' {
                    if self.eof() || self.peek() == b'\n' {
                        self.err("unterminated string");
                    }
                    if self.peek() == b'\\' {
                        self.next();
                        let e = self.escape();
                        bs.push(e);
                    } else {
                        bs.push(self.next());
                    }
                }
                self.next();
                bs.push(0);
                Tok::S(bs)
            } else if c == b'\'' {
                self.next();
                let v = if self.peek() == b'\\' {
                    self.next();
                    self.escape()
                } else {
                    self.next()
                };
                if self.next() != b'\'' {
                    self.err("unterminated char literal");
                }
                Tok::Ch(v)
            } else {
                let two = [c, self.peek2()];
                let is_two = matches!(&two[..], b"<<" | b">>" | b"==" | b"!=" | b"<=" | b">="
                    | b"&&" | b"||" | b"++" | b"--" | b"+=" | b"-=" | b"*=" | b"/=" | b"%="
                    | b"&=" | b"|=" | b"^=");
                if is_two {
                    self.next();
                    self.next();
                    Tok::P(String::from_utf8_lossy(two.as_slice()).into_owned())
                } else if b"(){}[];,=+-*/%<>!~&|".contains(&c) {
                    self.next();
                    Tok::P((c as char).to_string())
                } else {
                    self.err(&format!("unexpected character {:?}", c as char));
                }
            };
            out.push(T { k, ln });
        }
        out
    }
}

// ======================================================================= AST ==

#[derive(Clone, Debug)]
enum Expr {
    Lit(u64),
    Str(u32),
    Id(String),
    Call(String, Vec<Expr>),
    Cast(Ty, Box<Expr>),
    Neg(Box<Expr>),
    Not(Box<Expr>),
    Inv(Box<Expr>),
    Deref(Box<Expr>),
    Index(Box<Expr>, Box<Expr>),
    Bin(String, Box<Expr>, Box<Expr>),
    Log(bool, Box<Expr>, Box<Expr>),
    Asg(Option<String>, Box<Expr>, Box<Expr>),
    Inc(Box<Expr>, i64),
}

#[derive(Clone, Debug)]
struct Decl {
    name: String,
    ty: Ty,
    elems: u64,
    init: Option<Expr>,
    line: u32,
}

#[derive(Clone, Debug)]
enum St {
    Empty,
    Expr(Expr),
    Decl(Decl),
    If(Expr, Box<St>, Option<Box<St>>),
    While(Expr, Box<St>),
    For(Box<St>, Option<Expr>, Option<Expr>, Box<St>),
    Ret(Option<Expr>),
    Brk,
    Cont,
    Blk(Vec<St>),
}

#[derive(Clone, Debug)]
struct FnDef {
    name: String,
    ret: B,
    line: u32,
    params: Vec<(String, Ty)>,
    body: Vec<St>,
}

#[derive(Debug)]
struct Global {
    name: String,
    ty: Ty,
    elems: u64,
    init: Option<Expr>,
    line: u32,
}

// ==================================================================== parser ==

const TYPES: [&str; 9] = ["U0", "U8", "U16", "U32", "U64", "I8", "I16", "I32", "I64"];

struct P {
    ts: Vec<T>,
    i: usize,
    fns: Vec<FnDef>,
    globals: Vec<Global>,
    strings: Vec<Vec<u8>>,
    strmap: HashMap<Vec<u8>, u32>,
}

impl P {
    fn peek(&self) -> &T {
        &self.ts[self.i.min(self.ts.len() - 1)]
    }
    fn at_pun(&self, p: &str) -> bool {
        matches!(&self.peek().k, Tok::P(x) if x == p)
    }
    fn at_id(&self) -> bool {
        matches!(self.peek().k, Tok::Idx(_))
    }
    fn at_type(&self) -> bool {
        matches!(&self.peek().k, Tok::Idx(x) if TYPES.contains(&x.as_str()))
    }
    fn eat(&mut self, p: &str) -> bool {
        if self.at_pun(p) {
            self.i += 1;
            true
        } else {
            false
        }
    }
    fn want(&mut self, p: &str) {
        let ln = self.peek().ln;
        if !self.eat(p) {
            fail(&format!("expected `{p}`"), ln);
        }
    }
    fn is_kw(&self, kw: &str) -> bool {
        matches!(&self.peek().k, Tok::Idx(s) if s == kw)
    }

    fn ident(&mut self) -> String {
        let ln = self.peek().ln;
        let s = match &self.peek().k {
            Tok::Idx(s) => s.clone(),
            t => fail(&format!("expected identifier, found {:?}", t), ln),
        };
        self.i += 1;
        s
    }

    fn type_base(&mut self) -> B {
        let ln = self.peek().ln;
        let s = match &self.peek().k {
            Tok::Idx(s) => s.clone(),
            t => fail(&format!("expected type, found {:?}", t), ln),
        };
        let b = match s.as_str() {
            "U0" => B::Void,
            "U8" => B::U8,
            "U16" => B::U16,
            "U32" => B::U32,
            "U64" => B::U64,
            "I8" => B::I8,
            "I16" => B::I16,
            "I32" => B::I32,
            "I64" => B::I64,
            _ => fail(&format!("unknown type `{s}`"), ln),
        };
        self.i += 1;
        b
    }

    fn intern(&mut self, bs: Vec<u8>) -> u32 {
        if let Some(&id) = self.strmap.get(&bs) {
            return id;
        }
        let id = self.strings.len() as u32;
        self.strings.push(bs.clone());
        self.strmap.insert(bs, id);
        id
    }

    fn const_expr(&mut self) -> u64 {
        let e = self.assign_expr();
        const_val(&e).unwrap_or_else(|| {
            fail("expected compile-time constant expression", self.peek().ln)
        })
    }

    /// declarator after base type: `*p[10] = 0`
    fn decl_one(&mut self, base: B, line: u32) -> Decl {
        let mut ptr = false;
        while self.eat("*") {
            ptr = true;
        }
        let name = self.ident();
        let mut elems = 0u64;
        if self.eat("[") {
            elems = self.const_expr();
            self.want("]");
        }
        let mut init = None;
        if self.eat("=") {
            init = Some(self.assign_expr());
        }
        Decl { name, ty: if ptr { Ty::P(base) } else { Ty::B(base) }, elems, init, line }
    }

    fn stmt_or_block(&mut self, loop_ok: bool) -> St {
        if self.at_pun("{") {
            self.i += 1;
            let mut v = Vec::new();
            self.block_body(&mut v, loop_ok);
            self.want("}");
            return St::Blk(v);
        }
        self.statement(loop_ok)
    }

    fn block_body(&mut self, v: &mut Vec<St>, loop_ok: bool) {
        while !self.at_pun("}") && self.i < self.ts.len() {
            v.push(self.statement(loop_ok));
        }
    }

    fn statement(&mut self, loop_ok: bool) -> St {
        let ln = self.peek().ln;
        if self.at_type() {
            let b = self.type_base();
            let mut decls = Vec::new();
            loop {
                let d = self.decl_one(b, ln);
                if d.init.is_some() && d.elems > 0 {
                    fail("array initializers are not supported (use assignment loops)", ln);
                }
                decls.push(d);
                if !self.eat(",") {
                    break;
                }
            }
            self.want(";");
            if decls.len() == 1 {
                return St::Decl(decls.remove(0));
            }
            return St::Blk(decls.into_iter().map(St::Decl).collect());
        }
        if self.eat(";") {
            return St::Empty;
        }
        if self.is_kw("if") {
            self.i += 1;
            self.want("(");
            let c = self.assign_expr();
            self.want(")");
            let then = Box::new(self.stmt_or_block(loop_ok));
            let els = if self.is_kw("else") {
                self.i += 1;
                Some(Box::new(self.stmt_or_block(loop_ok)))
            } else {
                None
            };
            return St::If(c, then, els);
        }
        if self.is_kw("while") {
            self.i += 1;
            self.want("(");
            let c = self.assign_expr();
            self.want(")");
            let body = Box::new(self.stmt_or_block(true));
            return St::While(c, body);
        }
        if self.is_kw("for") {
            self.i += 1;
            self.want("(");
            let init: St = if self.eat(";") {
                St::Empty
            } else if self.at_type() {
                let b = self.type_base();
                let d = self.decl_one(b, ln);
                self.want(";");
                St::Decl(d)
            } else {
                let e = self.assign_expr();
                self.want(";");
                St::Expr(e)
            };
            let cond = if self.at_pun(";") { None } else { Some(self.assign_expr()) };
            self.want(";");
            let post = if self.at_pun(")") { None } else { Some(self.assign_expr()) };
            self.want(")");
            let body = Box::new(self.stmt_or_block(true));
            return St::For(Box::new(init), cond, post, body);
        }
        if self.is_kw("return") {
            self.i += 1;
            let e = if self.at_pun(";") { None } else { Some(self.assign_expr()) };
            self.want(";");
            return St::Ret(e);
        }
        if self.is_kw("break") {
            self.i += 1;
            self.want(";");
            if !loop_ok {
                fail("break outside of a loop", ln);
            }
            return St::Brk;
        }
        if self.is_kw("continue") {
            self.i += 1;
            self.want(";");
            if !loop_ok {
                fail("continue outside of a loop", ln);
            }
            return St::Cont;
        }
        let mut e = self.assign_expr();
        if let Expr::Str(_) = e {
            e = Expr::Call("Print".into(), vec![e]);
        }
        self.want(";");
        St::Expr(e)
    }

    // ---- expressions ----

    fn postfix(&mut self) -> Expr {
        let mut e = self.primary();
        loop {
            if self.at_pun("[") {
                self.i += 1;
                let ix = self.assign_expr();
                self.want("]");
                e = Expr::Index(Box::new(e), Box::new(ix));
            } else if self.at_pun("++") {
                self.i += 1;
                e = Expr::Inc(Box::new(e), 1);
            } else if self.at_pun("--") {
                self.i += 1;
                e = Expr::Inc(Box::new(e), -1);
            } else if self.at_pun("->") || self.at_pun(".") {
                fail("structs not supported", self.peek().ln);
            } else {
                break;
            }
        }
        e
    }

    fn primary(&mut self) -> Expr {
        let ln = self.peek().ln;
        match self.peek().k.clone() {
            Tok::N(v) => {
                self.i += 1;
                Expr::Lit(v)
            }
            Tok::Ch(v) => {
                self.i += 1;
                Expr::Lit(v as u64)
            }
            Tok::S(bs) => {
                self.i += 1;
                let id = self.intern(bs);
                Expr::Str(id)
            }
            Tok::Idx(name) => {
                self.i += 1;
                if self.at_pun("(") {
                    self.i += 1;
                    let mut args = Vec::new();
                    if !self.at_pun(")") {
                        loop {
                            args.push(self.assign_expr());
                            if !self.eat(",") {
                                break;
                            }
                        }
                    }
                    self.want(")");
                    Expr::Call(name, args)
                } else {
                    Expr::Id(name)
                }
            }
            _ => self.unary(),
        }
    }

    fn unary(&mut self) -> Expr {
        let _ = 0;
        match self.peek().k.clone() {
            Tok::P(x) if x == "(" => {
                self.i += 1;
                if self.at_type() {
                    let b = self.type_base();
                    let mut ptr = false;
                    while self.eat("*") {
                        ptr = true;
                    }
                    self.want(")");
                    let ty = if ptr { Ty::P(b) } else { Ty::B(b) };
                    let e = self.unary();
                    return Expr::Cast(ty, Box::new(e));
                }
                let e = self.assign_expr();
                self.want(")");
                e
            }
            Tok::P(x) if x == "-" => {
                self.i += 1;
                Expr::Neg(Box::new(self.unary()))
            }
            Tok::P(x) if x == "!" => {
                self.i += 1;
                Expr::Not(Box::new(self.unary()))
            }
            Tok::P(x) if x == "~" => {
                self.i += 1;
                Expr::Inv(Box::new(self.unary()))
            }
            Tok::P(x) if x == "*" => {
                self.i += 1;
                Expr::Deref(Box::new(self.unary()))
            }
            Tok::P(x) if x == "+" => {
                self.i += 1;
                self.unary()
            }
            _ => self.postfix(),
        }
    }

    fn bin_expr(&mut self, min_bp: u8) -> Expr {
        let mut lhs = self.unary();
        loop {
            let op = match &self.peek().k {
                Tok::P(p) => p.clone(),
                _ => break,
            };
            let bp = match bin_bp(&op) {
                Some(bp) if bp >= min_bp => bp,
                _ => break,
            };
            self.i += 1;
            let rhs = self.bin_expr(bp + 1);
            if op == "&&" || op == "||" {
                lhs = Expr::Log(op == "&&", Box::new(lhs), Box::new(rhs));
            } else {
                lhs = Expr::Bin(op, Box::new(lhs), Box::new(rhs));
            }
        }
        lhs
    }

    fn assign_expr(&mut self) -> Expr {
        let lhs = self.bin_expr(0);
        let op = match &self.peek().k {
            Tok::P(p)
                if matches!(
                    p.as_str(),
                    "=" | "+=" | "-=" | "*=" | "/=" | "%=" | "&=" | "|=" | "^=" | "<<=" | ">>="
                ) =>
            {
                Some(p.clone())
            }
            _ => None,
        };
        if op.is_none() {
            return lhs;
        }
        self.i += 1;
        let rhs = self.assign_expr();
        Expr::Asg(op, Box::new(lhs), Box::new(rhs))
    }

    fn program(&mut self, fname: &str) {
        while self.i < self.ts.len() {
            let ln = self.peek().ln;
            if !self.at_type() {
                fail("top level expects a `U0/U8/...` declaration", ln);
            }
            let b = self.type_base();
            let mut ptr = false;
            while self.eat("*") {
                ptr = true;
            }
            let name = self.ident();
            if self.at_pun("(") {
                // function definition
                if b == B::Void && ptr {
                    fail("U0* return type not supported", ln);
                }
                self.i += 1;
                let mut params = Vec::new();
                if !self.at_pun(")") {
                    loop {
                        let pln = self.peek().ln;
                        let pb = self.type_base();
                        let mut pptr = false;
                        while self.eat("*") {
                            pptr = true;
                        }
                        let pname = self.ident();
                        params.push((pname, if pptr { Ty::P(pb) } else { Ty::B(pb) }));
                        if !self.eat(",") {
                            break;
                        }
                        if self.at_pun(")") {
                            break;
                        }
                    }
                }
                self.want(")");
                self.want("{");
                let mut body = Vec::new();
                self.block_body(&mut body, false);
                self.want("}");
                self.fns.push(FnDef { name: name.clone(), ret: b, line: ln, params, body });
            } else {
                // global declaration
                let mut d = self.global_declarator(&name, b, ptr, ln);
                self.globals.push(d);
                while self.eat(",") {
                    let gln = self.peek().ln;
                    let _ = gln;
                    let mut gptr = false;
                    while self.eat("*") {
                        gptr = true;
                    }
                    let gname = self.ident();
                    d = self.global_declarator(&gname, b, gptr, ln);
                    self.globals.push(d);
                }
                self.want(";");
            }
        }
        // fixup: declarations with ptr handled through decl Ty construction
        let _ = fname;
    }

    fn global_declarator(&mut self, name: &str, base: B, ptr: bool, line: u32) -> Global {
        let mut elems = 0u64;
        if self.eat("[") {
            elems = self.const_expr();
            self.want("]");
        }
        let mut init = None;
        if self.eat("=") {
            init = Some(self.assign_expr());
        }
        Global {
            name: name.to_string(),
            ty: if ptr { Ty::P(base) } else { Ty::B(base) },
            elems,
            init,
            line,
        }
    }
}

fn bin_bp(op: &str) -> Option<u8> {
    Some(match op {
        "||" => 1,
        "&&" => 2,
        "|" => 3,
        "^" => 4,
        "&" => 5,
        "==" | "!=" => 6,
        "<" | "<=" | ">" | ">=" => 7,
        "<<" | ">>" => 8,
        "+" | "-" => 9,
        "*" | "/" | "%" => 10,
        _ => return None,
    })
}

// ================================================================= codegen ==

enum Fix {
    Label { at: u32, end: u32, lbl: u32 },       // rel32 field at `at..at+4`, instr ends at `end`
    CallFn { at: u32, end: u32, idx: usize },    // E8 rel32 call to internal fn
    CallApi { at: u32, end: u32, addr: u64 },    // E8 rel32 into fixed kernel address
    Abs { at: u32, addr: Abs },                  // 8-byte movabs imm64 (address of data)
}

enum Abs {
    Str(u32),  // string id
    Glob(usize), // global index
}

struct Slot {
    ty: Ty,
    elems: u64, // 0 => scalar
    off: i64,   // negative: bytes from rbp to slot start
}

struct Cg {
    code: Vec<u8>,
    foff: Vec<u64>, // fn index -> code offset (set when the fn is emitted)

    fns: Vec<FnDef>,
    fmap: HashMap<String, usize>,

    api: HashMap<String, u64>,
    api_bare: HashMap<String, u64>,
    // fixed-address kernel statics apps read directly: name -> (addr, bytes)
    api_statics: HashMap<String, (u64, u64)>,

    lmap: HashMap<String, Slot>,
    slot_next: i64,
    frame: u64,
    push_n: u64, // outstanding 64-bit pushes (parity = alignment state)
    line: u32,

    globals: Vec<Global>,
    gmap: HashMap<String, usize>,

    strings: Vec<Vec<u8>>,

    fixes: Vec<Fix>,
    labels: HashMap<u32, u64>,
    next_label: u32,
}

const ARG_REGBITS: [u8; 6] = [7, 6, 2, 1, 0, 1]; // rdi rsi rdx rcx r8 r9

impl Cg {
    fn new(
        fns: Vec<FnDef>,
        globals: Vec<Global>,
        strings: Vec<Vec<u8>>,
        api: HashMap<String, u64>,
    ) -> Self {
        let mut fmap = HashMap::new();
        for (i, f) in fns.iter().enumerate() {
            if f.params.len() > 6 {
                fail(&format!("fn `{}` has {} params; max 6", f.name, f.params.len()), f.line);
            }
            fmap.entry(f.name.clone()).or_insert(i);
        }
        let mut api_bare = HashMap::new();
        for k in api.keys() {
            if let Some(bare) = k.strip_prefix("HC_") {
                api_bare.entry(bare.to_string()).or_insert(api[k]);
            }
        }
        let mut api_statics = HashMap::new();
        for (name, bytes) in [("HC_FBADDR", 8u64), ("HC_FBW", 4), ("HC_FBH", 4), ("HC_FBPITCH", 4)] {
            if let Some(&a) = api.get(name) {
                api_statics.insert(name.strip_prefix("HC_").unwrap().to_string(), (a, bytes));
            }
        }
        let mut gmap = HashMap::new();
        for (i, g) in globals.iter().enumerate() {
            if gmap.insert(g.name.clone(), i).is_some() {
                fail(&format!("duplicate global `{}`", g.name), g.line);
            }
        }
        Cg {
            code: Vec::new(),
            foff: Vec::new(),
            fns,
            fmap,
            api,
            api_bare,
            api_statics,
            lmap: HashMap::new(),
            slot_next: -8,
            frame: 0,
            push_n: 0,
            line: 0,
            globals,
            gmap,
            strings,
            fixes: Vec::new(),
            labels: HashMap::new(),
            next_label: 1,
        }
    }

    fn le(&mut self, lbl: u32) {
        // Current position becomes the target of `lbl`.
        self.labels.insert(lbl, self.code.len() as u64);
    }
    fn lab(&mut self) -> u32 {
        self.next_label += 1;
        self.next_label - 1
    }

    // ---- raw byte helpers ----

    fn b(&mut self, x: u8) {
        self.code.push(x);
    }
    fn bs(&mut self, xs: &[u8]) {
        self.code.extend_from_slice(xs);
    }
    fn r32(&mut self, v: i32) {
        self.code.extend_from_slice(&v.to_le_bytes());
    }
    fn imm64(&mut self, v: u64) {
        self.code.extend_from_slice(&v.to_le_bytes());
    }

    fn push_rax(&mut self) {
        self.b(0x50);
        self.push_n += 1;
    }
    fn pop_rcx(&mut self) {
        self.b(0x59);
        self.push_n -= 1;
    }
    fn pop_rax(&mut self) {
        self.b(0x58);
        self.push_n -= 1;
    }

    fn jmp32(&mut self, lbl: u32) {
        self.b(0xE9);
        let at = self.code.len() as u32;
        self.r32(0);
        let end = self.code.len() as u32;
        self.fixes.push(Fix::Label { at, end, lbl });
    }
    fn jcc32(&mut self, cc: u8, lbl: u32) {
        self.bs(&[0x0F, 0x80 | cc]);
        let at = self.code.len() as u32;
        self.r32(0);
        let end = self.code.len() as u32;
        self.fixes.push(Fix::Label { at, end, lbl });
    }

    fn movabs(&mut self, v: u64) {
        self.bs(&[0x48, 0xB8]);
        self.imm64(v);
    }

    // ---- label-aware control flow for internal calls ----

    fn sub_rsp_imm(&mut self, v: i32) {
        if (-128..=127).contains(&v) {
            self.bs(&[0x48, 0x83, 0xEC, v as u8]);
        } else {
            self.bs(&[0x48, 0x81, 0xEC]);
            self.r32(v);
        }
    }
    fn add_rsp_imm(&mut self, v: i32) {
        if v == 0 {
            return;
        }
        if (-128..=127).contains(&v) {
            self.bs(&[0x48, 0x83, 0xC4, v as u8]);
        } else {
            self.bs(&[0x48, 0x81, 0xC4]);
            self.r32(v);
        }
    }

    fn lea_local(&mut self, off: i64) {
        self.bs(&[0x48, 0x8D, 0x85]); // lea rax, [rbp+disp32]
        self.r32(off as i32);
    }

    fn movabs_g(&mut self, g: usize) {
        self.bs(&[0x48, 0xB8]);
        let at = self.code.len() as u32;
        self.imm64(0);
        self.fixes.push(Fix::Abs { at, addr: Abs::Glob(g) });
    }
    fn movabs_str(&mut self, s: u32) {
        self.bs(&[0x48, 0xB8]);
        let at = self.code.len() as u32;
        self.imm64(0);
        self.fixes.push(Fix::Abs { at, addr: Abs::Str(s) });
    }

    fn test_rax(&mut self) {
        self.bs(&[0x48, 0x85, 0xC0]);
    }
    fn xchg(&mut self) {
        self.bs(&[0x48, 0x91]); // xchg rax, rcx
    }
    fn mov_rcx_rax(&mut self) {
        self.bs(&[0x48, 0x89, 0xC1]);
    }
    fn mov_rdx_rcx(&mut self) {
        self.bs(&[0x48, 0x89, 0xCA]);
    }
    fn mov_rax_rdx(&mut self) {
        self.bs(&[0x48, 0x89, 0xD0]);
    }
    fn shl_rax(&mut self, k: u8) {
        self.bs(&[0x48, 0xC1, 0xE0, k]);
    }
    fn add_rax_imm(&mut self, v: i64) {
        if (-128..=127).contains(&v) {
            self.bs(&[0x48, 0x83, 0xC0, v as u8]);
        } else {
            self.bs(&[0x48, 0x81, 0xC0]);
            self.r32(v as i32);
        }
    }

    /// XOR a register with itself (self-zeroing). Set `ex` for r8/r9: both
    /// the reg and rm fields extend, so REX = W|R|B = 0x4D.
    fn zero_reg(&mut self, bits: u8, ex: bool) {
        let rex = if ex { 0x4D } else { 0x48 };
        self.bs(&[rex, 0x31, 0xC0 | (bits << 3) | bits]);
    }

    // ---- memory ops ----

    /// mov rax ← [rbp+disp32] (sizes/sign per ty).
    fn load_local(&mut self, off: i64, ty: Ty) {
        match load_shape(ty) {
            1 => {
                if ty.base().signed() {
                    self.bs(&[0x48, 0x0F, 0xBF, 0x85]); // movsx  rax, word  — handled below for 1?
                } else {
                    self.bs(&[0x48, 0x0F, 0xB6, 0x85]); // movzx rax, byte [rbp+d]
                }
                self.r32(off as i32);
            }
            2 => {
                if ty.base().signed() {
                    self.bs(&[0x48, 0x0F, 0xBF, 0x85]); // movsx rax, word
                } else {
                    self.bs(&[0x66, 0x8B, 0x85]); // mov ax, [..] zero-extends
                }
                self.r32(off as i32);
            }
            4 => {
                if ty.base().signed() {
                    self.bs(&[0x48, 0x63, 0x85]); // movsxd rax, dword [rbp+d]
                } else {
                    self.bs(&[0x8B, 0x85]); // mov eax, [rbp+d] zero-extends
                }
                self.r32(off as i32);
            }
            _ => {
                self.bs(&[0x48, 0x8B, 0x85]);
                self.r32(off as i32);
            }
        }
    }

    /// mov [rbp+disp32] ← rax.
    fn store_local(&mut self, off: i64, ty: Ty) {
        match ty.bytes() {
            1 => {
                self.bs(&[0x88, 0x85]);
                self.r32(off as i32);
            }
            2 => {
                self.bs(&[0x66, 0x89, 0x85]);
                self.r32(off as i32);
            }
            4 => {
                self.bs(&[0x89, 0x85]);
                self.r32(off as i32);
            }
            _ => {
                self.bs(&[0x48, 0x89, 0x85]);
                self.r32(off as i32);
            }
        }
    }

    /// mov rax ← [base], base = 1(rcx) | 2(rdx)
    fn load_ptr(&mut self, base: u8, ty: Ty) {
        debug_assert!(base == 1 || base == 2);
        match load_shape(ty) {
            1 => {
                if ty.base().signed() {
                    self.bs(&[0x48, 0x0F, 0xBE, base]); // movsx rax, byte
                } else {
                    self.bs(&[0x48, 0x0F, 0xB6, base]); // movzx rax, byte
                }
            }
            2 => {
                if ty.base().signed() {
                    self.bs(&[0x48, 0x0F, 0xBF, base]); // movsx rax, word
                } else {
                    self.bs(&[0x66, 0x8B, base]);
                }
            }
            4 => {
                if ty.base().signed() {
                    self.bs(&[0x48, 0x63, base]); // movsxd rax, dword
                } else {
                    self.bs(&[0x8B, base]); // mov eax, [base] zero-extends
                }
            }
            _ => {
                self.bs(&[0x48, 0x8B, base]);
            }
        }
    }

    /// mov [base] ← rax, base = 1(rcx) | 2(rdx)
    fn store_ptr(&mut self, base: u8, ty: Ty) {
        debug_assert!(base == 1 || base == 2);
        match ty.bytes() {
            1 => self.bs(&[0x88, base]),
            2 => self.bs(&[0x66, 0x89, base]),
            4 => self.bs(&[0x89, base]),
            _ => self.bs(&[0x48, 0x89, base]),
        }
    }

    /// lhs in rcx, rhs in rax → result rax. All widths 64-bit.
    fn apply_bin(&mut self, op: &str) {
        match op {
            "+" => self.bs(&[0x48, 0x01, 0xC8]),
            "-" => {
                self.xchg();
                self.bs(&[0x48, 0x29, 0xC8]);
            }
            "*" => self.bs(&[0x48, 0x0F, 0xAF, 0xC8]),
            "&" => self.bs(&[0x48, 0x21, 0xC8]),
            "|" => self.bs(&[0x48, 0x09, 0xC8]),
            "^" => self.bs(&[0x48, 0x31, 0xC8]),
            "<<" => {
                self.xchg();
                self.bs(&[0x48, 0xD3, 0xE0]); // shl rax, cl
            }
            ">>" => {
                self.xchg();
                self.bs(&[0x48, 0xD3, 0xF8]); // sar rax, cl (signed semantics)
            }
            "/" => {
                self.xchg();
                self.bs(&[0x48, 0x99]); // cqo
                self.bs(&[0x48, 0xF7, 0xF9]); // idiv rcx
            }
            "%" => {
                self.xchg();
                self.bs(&[0x48, 0x99]);
                self.bs(&[0x48, 0xF7, 0xF9]);
                self.mov_rax_rdx();
            }
            "==" | "!=" | "<" | "<=" | ">" | ">=" => {
                self.xchg();
                self.bs(&[0x48, 0x39, 0xC8]); // cmp rax, rcx
                let cc: u8 = match op {
                    "==" => 0x94,
                    "!=" => 0x95,
                    "<" => 0x9C,
                    "<=" => 0x9E,
                    ">" => 0x9F,
                    _ => 0x9D,
                };
                self.bs(&[0x0F, cc, 0xC0]); // setcc al
                self.bs(&[0x48, 0x0F, 0xB6, 0xC0]); // movzx rax, al
            }
            other => fail(&format!("internal: bad op {other}"), self.line),
        }
    }

    // ---- types ----

    fn ety(&self, e: &Expr) -> Ty {
        match e {
            Expr::Lit(_) | Expr::Neg(_) | Expr::Not(_) | Expr::Inv(_) | Expr::Bin(..) | Expr::Log(..) => Ty::B(B::I64),
            Expr::Str(_) => Ty::P(B::U8),
            Expr::Cast(t, _) => *t,
            Expr::Id(name) => {
                if let Some(slot) = self.lmap.get(name) {
                    if slot.elems > 0 {
                        Ty::P(slot.ty.base())
                    } else {
                        slot.ty
                    }
                } else if let Some(&g) = self.gmap.get(name) {
                    let g = &self.globals[g];
                    if g.elems > 0 {
                        Ty::P(g.ty.base())
                    } else {
                        g.ty
                    }
                } else if let Some((_, 8)) = self.api_statics.get(name) {
                    Ty::P(B::U8) // FBADDR is a pointer
                } else if self.api_statics.contains_key(name) {
                    Ty::B(B::U32)
                } else {
                    Ty::B(B::I64)
                }
            }
            Expr::Call(name, _) => {
                if let Some(&fi) = self.fmap.get(name) {
                    Ty::B(self.fns[fi].ret)
                } else {
                    Ty::B(B::I64)
                }
            }
            Expr::Deref(x) => {
                let t = self.ety(x);
                if t.is_ptr() {
                    Ty::B(t.base())
                } else {
                    Ty::B(B::U64)
                }
            }
            Expr::Index(a, _) => {
                let t = self.ety(a);
                if t.is_ptr() {
                    Ty::B(t.base())
                } else {
                    Ty::B(B::U64)
                }
            }
            Expr::Asg(_, l, _) => self.ety(l),
            Expr::Inc(x, _) => self.ety(x),
        }
    }

    // ---- call pipeline ----

    fn resolve_call(&self, name: &str) -> Option<Res> {
        if let Some(&fi) = self.fmap.get(name) {
            return Some(Res::Fn(fi));
        }
        if let Some(&a) = self.api.get(name) {
            return Some(Res::Api(a));
        }
        if let Some(&a) = self.api_bare.get(name) {
            return Some(Res::Api(a));
        }
        if let Some(&a) = self.api_bare.get(&name.to_uppercase()) {
            return Some(Res::Api(a));
        }
        None
    }

    fn emit_call(&mut self, name: &str, args: &[Expr]) {
        let res = self.resolve_call(name).unwrap_or_else(|| {
            fail(
                &format!("unknown function `{name}` (not defined here and not a kernel API)"),
                self.line,
            )
        });

        let n = args.len();
        if n > 6 {
            fail(&format!("`{name}` takes at most 6 args"), self.line);
        }
        if let Res::Fn(fi) = res {
            if self.fns[fi].params.len() != n {
                fail(
                    &format!("`{name}` takes {} args, got {}", self.fns[fi].params.len(), n),
                    self.line,
                );
            }
        }

        for a in args {
            self.eval(a);
            self.push_rax();
        }
        let odd = self.push_n % 2 == 1;
        if odd {
            self.sub_rsp_imm(8); // align down: rsp%16==0 for the call
        }
        // arg k (0-based) sits at [rsp + 8*(n-1-k) + pad]
        let pad = if odd { 8 } else { 0 };
        for k in 0..n {
            let disp = (8 * (n - 1 - k)) as i64 + pad;
            let (rex, rb) = arg_reg(k);
            self.bs(&[rex, 0x8B, 0x40 | (rb << 3) | 0x04, 0x24]);
            self.b(disp as u8);
        }
        // zero arg regs the caller doesn't supply (TempleOS-style determinism).
        for r in n..6 {
            let (_, rb) = arg_reg(r);
            self.zero_reg(rb, r >= 4); // r8/r9 need the REX.R|B form
        }
        match res {
            Res::Fn(fi) => self.call_fn(fi),
            Res::Api(a) => self.call_api(a),
        }
        // release args (+ the alignment pad if we added one). The counter
        // must forget the n arg pushes too, or its parity skews by n mod 2
        // per call and every later call site lands 8 bytes off — which is
        // exactly how an SSE prologue (#GP) dies QEMU-side.
        let rel = 8 * n as i32 + if odd { 8 } else { 0 };
        self.add_rsp_imm(rel);
        if self.push_n < n as u64 {
            fail(
                &format!(
                    "internal: releasing {} arg pushes with push_n={} ({})",
                    n, self.push_n, name
                ),
                self.line,
            );
        }
        self.push_n -= n as u64;
    }

    fn call_fn(&mut self, idx: usize) {
        self.b(0xE8);
        let at = self.code.len() as u32;
        self.r32(0);
        let end = self.code.len() as u32;
        self.fixes.push(Fix::CallFn { at, end, idx });
    }
    fn call_api(&mut self, addr: u64) {
        let here = LOAD_BASE + HDR_LEN + self.code.len() as u64 + 5;
        let rel = (addr as i64) - (here as i64);
        self.b(0xE8);
        self.r32(rel as i32);
    }

    // ---- locals ----

    fn slot_alloc(&mut self, name: &str, ty: Ty, elems: u64) -> i64 {
        let size = if elems == 0 { 8u64 } else { elems * ty.bytes() };
        let pad = (size + 7) & !7;
        self.slot_next -= pad as i64;
        let off = self.slot_next;
        self.lmap.insert(name.to_string(), Slot { ty, elems, off });
        off
    }

    fn collect_locals(&mut self, stmts: &[St]) {
        for s in stmts {
            match s {
                St::Decl(d) => {
                    self.slot_alloc(&d.name, d.ty, d.elems);
                }
                St::Blk(v) => self.collect_locals(v),
                St::If(_, t, e) => {
                    self.collect_locals(std::slice::from_ref(t));
                    if let Some(e) = e {
                        self.collect_locals(std::slice::from_ref(e));
                    }
                }
                St::While(_, b) => self.collect_locals(std::slice::from_ref(b)),
                St::For(init, _, _, b) => {
                    self.collect_locals(std::slice::from_ref(init));
                    self.collect_locals(std::slice::from_ref(b));
                }
                _ => {}
            }
        }
    }

    // ---- prologue/epilogue ----

    fn epilogue(&mut self) {
        // mov rsp, rbp ; pop rbp ; ret
        self.bs(&[0x48, 0x89, 0xEC]);
        self.b(0x5D);
        self.b(0xC3);
    }

    fn emit_fn(&mut self, fi: usize) {
        let f = self.fns[fi].clone();
        self.foff.push(self.code.len() as u64);
        self.frame = 0;
        self.push_n = 0;
        self.lmap.clear();
        self.slot_next = -48; // slots start below the 48-byte arg save area
        self.line = f.line;

        // Slot all locals first so the frame size is fixed before emitting.
        self.collect_locals(&f.body);
        for (pname, pty) in &f.params {
            self.slot_alloc(pname, *pty, 0);
        }
        let slots = ((self.slot_next.abs() as u64) + 15) & !15;
        self.frame = 48 + slots; // 48-byte arg save area + slot region
        if self.frame > 0x100_000 {
            fail(&format!("fn `{}` local frame too large ({})", f.name, f.line), f.line);
        }

        // Prologue. Layout below rbp: [rsp..rsp+48) saves the six arg
        // registers, [rsp+48..rsp+48+slots) are the value slots.
        //
        // The zeroing pass (`rep stosb`) clobbers rdi and rcx, so params are
        // stashed with MOVs (not pushes) BEFORE it runs and copied from the
        // save area afterwards. push_n stays 0 through the prologue, so the
        // call-site parity math is unaffected.
        self.b(0x55); // push rbp
        self.bs(&[0x48, 0x89, 0xE5]); // mov rbp, rsp
        self.bs(&[0x48, 0x81, 0xEC]); // sub rsp, imm32
        self.r32(self.frame as i32);
        // save arg regs: mov [rsp+8k], argreg_k   (mod=01 disp8, rm=SIB)
        let save: [&[u8]; 6] = [
            &[0x48, 0x89, 0x7C, 0x24], // rdi:  reg=111, SIB
            &[0x48, 0x89, 0x74, 0x24], // rsi:  reg=110
            &[0x48, 0x89, 0x54, 0x24], // rdx:  reg=010
            &[0x48, 0x89, 0x4C, 0x24], // rcx:  reg=001
            &[0x4C, 0x89, 0x44, 0x24], // r8:   reg=000, REX.R
            &[0x4C, 0x89, 0x4C, 0x24], // r9:   reg=001, REX.R
        ];
        for (k, enc) in save.iter().enumerate() {
            self.bs(enc);
            self.b((8 * k) as u8);
        }
        // zero the slot region: lea rdi,[rsp+48] ; mov ecx,slots ; rep stosb
        if slots > 0 {
            self.bs(&[0x48, 0x8D, 0xBC, 0x24]); // lea rdi, [rsp+disp32]
            self.r32(48);
            self.bs(&[0x48, 0x31, 0xC0]); // xor rax, rax
            self.bs(&[0xFC]); // cld
            self.bs(&[0xB9]); // mov ecx, imm32
            self.r32(slots as i32);
            self.bs(&[0xF3, 0xAA]); // rep stosb
        }
        // copy params from the save area into their zeroed slots
        for (k, (pname, _)) in f.params.iter().enumerate() {
            let Some(off) = self.lmap.get(pname).map(|s| s.off) else {
                fail(&format!("param slot missing: {pname}"), f.line);
            };
            // mov rax, [rsp + 8*k]
            self.bs(&[0x48, 0x8B, 0x44, 0x24, (8 * k) as u8]);
            // mov [rbp+off], rax
            self.bs(&[0x48, 0x89, 0x85]);
            self.r32(off as i32);
        }

        // body
        for s in &f.body {
            self.emit_stmt(s, None, None);
        }
        // implicit return
        self.bs(&[0x48, 0x31, 0xC0]); // xor rax, rax
        self.epilogue();
    }

    // ---- statements ----

    fn emit_stmt(&mut self, s: &St, brk: Option<u32>, cnt: Option<u32>) {
        match s {
            St::Empty => {}
            St::Expr(e) => self.eval(e),
            St::Decl(d) => {
                if let Some(init) = &d.init {
                    let Some(off) = self.lmap.get(&d.name).map(|s| s.off) else {
                        fail(&format!("deployed unknown local `{}`", d.name), self.line);
                    };
                    self.eval(init);
                    self.store_local(off, d.ty);
                }
            }
            St::Blk(v) => {
                for x in v {
                    self.emit_stmt(x, brk, cnt);
                }
            }
            St::If(c, t, e) => {
                self.eval(c);
                self.test_rax();
                let lelse = self.lab();
                self.jcc32(0x4, lelse); // je
                self.emit_stmt(t, brk, cnt);
                if let Some(els) = e {
                    let lend = self.lab();
                    self.jmp32(lend);
                    self.le(lelse);
                    self.emit_stmt(els, brk, cnt);
                    self.le(lend);
                } else {
                    self.le(lelse);
                }
            }
            St::While(c, body) => {
                let ltop = self.lab();
                let lend = self.lab();
                self.le(ltop);
                self.eval(c);
                self.test_rax();
                self.jcc32(0x4, lend);
                self.emit_stmt(body, Some(lend), Some(ltop));
                self.jmp32(ltop);
                self.le(lend);
            }
            St::For(init, cond, post, body) => {
                self.emit_stmt(init, brk, cnt);
                let ltop = self.lab();
                let lcont = self.lab();
                let lend = self.lab();
                self.le(ltop);
                if let Some(c) = cond {
                    self.eval(c);
                    self.test_rax();
                    self.jcc32(0x4, lend);
                }
                self.emit_stmt(body, Some(lend), Some(lcont));
                self.le(lcont);
                if let Some(p) = post {
                    self.eval(p);
                }
                self.jmp32(ltop);
                self.le(lend);
            }
            St::Ret(e) => {
                if let Some(e) = e {
                    self.eval(e);
                }
                self.epilogue();
            }
            St::Brk => {
                let t = brk.unwrap_or_else(|| fail("break outside loop", self.line));
                self.jmp32(t);
            }
            St::Cont => {
                let t = cnt.unwrap_or_else(|| fail("continue outside loop", self.line));
                self.jmp32(t);
            }
        }
    }

    // ---- expressions ----

    /// Leaves value in rax. Stack balanced except pushes made directly.
    fn eval(&mut self, e: &Expr) {
        // Debug invariant: a complete expression must not leak stack pushes.
        // Even WITH the parity pad, the *count* must return to its entry
        // value, otherwise the frame slowly skews and call sites misalign.
        let pn0 = self.push_n;
        let ev = self.emit_eval(e);
        if self.push_n != pn0 {
            self.push_n = pn0;
            fail(
                "internal: expression leaked stack pushes (push_n imbalance)",
                self.line,
            );
        }
        let _ = ev;
    }

    fn emit_eval(&mut self, e: &Expr) {
        match e {
            Expr::Lit(v) => self.movabs(*v),
            Expr::Str(id) => self.movabs_str(*id),
            Expr::Id(name) => {
                let this = name.clone();
                if let Some(slot) = self.lmap.get(&this) {
                    let (ty, elems, off) = (slot.ty, slot.elems, slot.off);
                    if elems > 0 {
                        self.lea_local(off); // arrays decompose to their address
                    } else {
                        self.load_local(off, ty);
                    }
                } else if let Some(&g) = self.gmap.get(&this) {
                    if self.globals[g].elems > 0 {
                        self.movabs_g(g); // decayed array address
                    } else {
                        let ty = self.globals[g].ty;
                        self.movabs_g(g);
                        self.mov_rcx_rax();
                        self.load_ptr(1, ty);
                    }
                } else if let Some(&(addr, bytes)) = self.api_statics.get(&this) {
                    self.bs(&[0x48, 0xB8]);
                    self.imm64(addr); // direct load address, no fixup needed
                    if bytes == 8 {
                        self.mov_rcx_rax();
                        self.load_ptr(1, Ty::B(B::U64));
                    } else {
                        self.mov_rcx_rax();
                        self.load_ptr(1, Ty::B(B::U32));
                    }
                } else if self.resolve_call(&this).is_some() {
                    // Bare API name used as an expression (`key = Key;`) is a
                    // zero-argument call, HolyC-style.
                    self.emit_call(&this, &[]);
                } else {
                    let ln = self.line;
                    fail(&format!("unknown identifier `{this}`"), ln);
                }
            }
            Expr::Call(n, args) => {
                let v: Vec<Expr> = args.clone();
                let name = n.clone();
                self.emit_call(&name, &v);
            }
            Expr::Cast(_, x) => self.eval(x),
            Expr::Neg(x) => {
                self.eval(x);
                self.bs(&[0x48, 0xF7, 0xD8]); // neg rax
            }
            Expr::Not(x) => {
                self.eval(x);
                self.test_rax();
                self.bs(&[0x0F, 0x94, 0xC0]); // sete al
                self.bs(&[0x48, 0x0F, 0xB6, 0xC0]); // movzx rax, al
            }
            Expr::Inv(x) => {
                self.eval(x);
                self.bs(&[0x48, 0xF7, 0xD0]); // not rax
            }
            Expr::Deref(x) => {
                let t = self.ety(x);
                self.eval(x);
                self.mov_rcx_rax();
                let elem = if t.is_ptr() { t.base() } else { B::U64 };
                self.load_ptr(1, Ty::B(elem));
            }
            Expr::Index(a, i) => {
                let at = self.ety(a);
                let esz = if at.is_ptr() { at.base().size() } else { 1 };
                self.eval(a);
                self.push_rax();
                self.eval(i);
                self.scale_rax(esz);
                self.pop_rcx();
                self.bs(&[0x48, 0x01, 0xC8]); // add rax, rcx
                self.mov_rcx_rax();
                let elem = if at.is_ptr() { at.base() } else { B::U64 };
                self.load_ptr(1, Ty::B(elem));
            }
            Expr::Bin(op, l, r) => {
                let lt = self.ety(l);
                let rt = self.ety(r);
                let op = op.clone();
                if lt.is_ptr() && (op == "+" || op == "-") {
                    // ptr +- int
                    self.eval(l);
                    self.push_rax();
                    self.eval(r);
                    self.scale_rax(lt.base().size());
                    self.pop_rcx();
                    if op == "+" {
                        self.bs(&[0x48, 0x01, 0xC8]); // add rax, rcx
                    } else {
                        self.bs(&[0x48, 0xF7, 0xD8]); // neg rax
                        self.bs(&[0x48, 0x01, 0xC8]);
                    }
                } else if rt.is_ptr() && op == "+"
                    && !(self.ety(l).is_ptr())
                {
                    // int + ptr
                    self.eval(l);
                    self.push_rax();
                    self.eval(r);
                    // rax = ptr, rcx = int; scale int (int in rcx)
                    self.pop_rcx();
                    self.bs(&[0x48, 0x91]); // xchg rax, rcx: rax=int, rcx=ptr
                    self.scale_rax(rt.base().size());
                    self.bs(&[0x48, 0x01, 0xC8]); // add rax, rcx (ptr)
                } else {
                    self.eval(l);
                    self.push_rax();
                    self.eval(r);
                    self.pop_rcx();
                    self.apply_bin(&op);
                }
            }
            Expr::Log(is_and, l, r) => {
                let is_and = *is_and;
                let lfalse = self.lab();
                let lend = self.lab();
                self.eval(l);
                self.test_rax();
                if is_and {
                    self.jcc32(0x4, lfalse); // je
                    self.eval(r);
                    self.test_rax();
                    self.jcc32(0x4, lfalse);
                    self.movabs(1);
                    self.jmp32(lend);
                    self.le(lfalse);
                    self.movabs(0);
                    self.le(lend);
                } else {
                    self.jcc32(0x5, lfalse); // jne → true
                    self.eval(r);
                    self.test_rax();
                    self.jcc32(0x5, lfalse);
                    self.movabs(0);
                    self.jmp32(lend);
                    self.le(lfalse);
                    self.movabs(1);
                    self.le(lend);
                }
            }
            Expr::Asg(op, l, r) => {
                let opv = op.clone();
                self.assign(opv, l, r);
            }
            Expr::Inc(x, d) => self.incr(x, *d),
        }
    }

    fn scale_rax(&mut self, sz: u64) {
        match sz {
            1 => {}
            2 => self.shl_rax(1),
            4 => self.shl_rax(2),
            _ => self.shl_rax(3),
        }
    }

    fn assign(&mut self, op: Option<String>, l: &Expr, r: &Expr) {
        match l {
            Expr::Id(name) => {
                if let Some(slot) = self.lmap.get(name) {
                    let (ty, elems, off) = (slot.ty, slot.elems, slot.off);
                    if elems > 0 {
                        fail("array assignment not supported", self.line);
                    }
                    match op {
                        None => {
                            self.eval(r);
                            self.store_local(off, ty);
                        }
                        Some(o) if o == "=" => {
                            self.eval(r);
                            self.store_local(off, ty);
                        }
                        Some(o) => {
                            self.load_local(off, ty);
                            self.push_rax();
                            self.eval(r);
                            self.pop_rcx();
                            let comp = o.trim_end_matches('=').to_string();
                            self.apply_bin(&comp);
                            self.store_local(off, ty);
                        }
                    }
                } else if let Some(&g) = self.gmap.get(name) {
                    if self.globals[g].elems > 0 {
                        fail("array assignment not supported", self.line);
                    }
                    let ty = self.globals[g].ty;
                    self.movabs_g(g); // rax = address
                    // Address-holding protocol: the address lives on the
                    // stack across the RHS eval (which needs rcx as its own
                    // scratch and may call), then pops back into rcx.
                    self.push_rax(); // addr
                    match op.as_deref() {
                        None | Some("=") => {
                            self.eval(r);
                            self.pop_rcx(); // addr
                            self.store_ptr(1, ty);
                        }
                        Some(comp_op) => {
                            self.mov_rcx_rax();
                            self.load_ptr(1, ty); // old value → rax
                            self.push_rax(); // old
                            self.eval(r);
                            self.pop_rcx(); // old (lhs)
                            self.apply_bin(comp_op.trim_end_matches('='));
                            self.pop_rcx(); // addr
                            self.store_ptr(1, ty);
                        }
                    }
                } else {
                    let ln = self.line;
                    fail(&format!("unknown variable `{name}`"), ln);
                }
            }
            Expr::Deref(p) => {
                let pt = self.ety(p);
                let elem = if pt.is_ptr() { pt.base() } else { B::U64 };
                self.eval(p); // rax = address
                self.push_rax(); // addr
                match op.as_deref() {
                    None | Some("=") => {
                        self.eval(r);
                        self.pop_rcx(); // addr
                        self.store_ptr(1, Ty::B(elem));
                    }
                    Some(comp_op) => {
                        self.mov_rcx_rax();
                        self.load_ptr(1, Ty::B(elem)); // old value → rax
                        self.push_rax(); // old
                        self.eval(r);
                        self.pop_rcx(); // old (lhs)
                        self.apply_bin(comp_op.trim_end_matches('='));
                        self.pop_rcx(); // addr
                        self.store_ptr(1, Ty::B(elem));
                    }
                }
            }
            Expr::Index(base, ix) => {
                let bt = self.ety(base);
                let elem = if bt.is_ptr() { bt.base() } else { B::U64 };
                self.eval(base);
                self.push_rax();
                self.eval(ix);
                self.scale_rax(elem.size());
                self.pop_rcx();
                self.bs(&[0x48, 0x01, 0xC8]); // add rax, rcx → address
                self.push_rax(); // addr
                match op.as_deref() {
                    None | Some("=") => {
                        self.eval(r);
                        self.pop_rcx(); // addr
                        self.store_ptr(1, Ty::B(elem));
                    }
                    Some(comp_op) => {
                        self.mov_rcx_rax();
                        self.load_ptr(1, Ty::B(elem)); // old value → rax
                        self.push_rax(); // old
                        self.eval(r);
                        self.pop_rcx(); // old (lhs)
                        self.apply_bin(comp_op.trim_end_matches('='));
                        self.pop_rcx(); // addr
                        self.store_ptr(1, Ty::B(elem));
                    }
                }
            }
            other => {
                let ln = self.line;
                fail(&format!("invalid assignment target: {other:?}"), ln);
            }
        }
    }

    fn incr(&mut self, x: &Expr, d: i64) {
        match x {
            Expr::Id(name) => {
                if let Some(slot) = self.lmap.get(name) {
                    let (ty, elems, off) = (slot.ty, slot.elems, slot.off);
                    if elems > 0 {
                        fail("++/-- not supported on arrays", self.line);
                    }
                    let step = (if ty.is_ptr() { ty.base().size() } else { 1 }) as i64 * d;
                    self.load_local(off, ty);
                    self.push_rax();
                    self.add_rax_imm(step);
                    self.store_local(off, ty);
                    self.pop_rax();
                } else if let Some(&g) = self.gmap.get(name) {
                    if self.globals[g].elems > 0 {
                        fail("++/-- not supported on arrays", self.line);
                    }
                    let ty = self.globals[g].ty;
                    let step = (if ty.is_ptr() { ty.base().size() } else { 1 }) as i64 * d;
                    self.movabs_g(g);
                    self.mov_rcx_rax();
                    self.mov_rdx_rcx();
                    self.load_ptr(2, ty);
                    self.push_rax();
                    self.add_rax_imm(step);
                    self.store_ptr(2, ty);
                    self.pop_rax();
                } else {
                    let ln = self.line;
                    fail(&format!("++/-- on unknown `{name}`"), ln);
                }
            }
            Expr::Deref(p) => {
                let pt = self.ety(p);
                let elem = if pt.is_ptr() { pt.base() } else { B::U64 };
                let step = elem.size() as i64 * d;
                self.eval(p);
                self.mov_rcx_rax();
                self.mov_rdx_rcx();
                self.load_ptr(2, Ty::B(elem));
                self.push_rax();
                self.add_rax_imm(step);
                self.store_ptr(2, Ty::B(elem));
                self.pop_rax();
            }
            Expr::Index(base, ix) => {
                let bt = self.ety(base);
                let elem = if bt.is_ptr() { bt.base() } else { B::U64 };
                let step = elem.size() as i64 * d;
                self.eval(base);
                self.push_rax();
                self.eval(ix);
                self.scale_rax(elem.size());
                self.pop_rcx();
                self.bs(&[0x48, 0x01, 0xC8]);
                self.mov_rcx_rax();
                self.mov_rdx_rcx();
                self.load_ptr(2, Ty::B(elem));
                self.push_rax();
                self.add_rax_imm(step);
                self.store_ptr(2, Ty::B(elem));
                self.pop_rax();
            }
            other => {
                let ln = self.line;
                fail(&format!("++/-- not supported on {other:?}"), ln);
            }
        }
    }
}

enum Res {
    Fn(usize),
    Api(u64),
}

/// (REX byte, reg field bits) for a SysV integer arg register.
fn arg_reg(k: usize) -> (u8, u8) {
    match k {
        // rdi/rsi/rdx/rcx need no extension: REX = W only (0x48). A 0x49 here
        // would set REX.B and reinterpret rbp as r13 / rsp as r12!
        0 => (0x48, 7), // rdi
        1 => (0x48, 6), // rsi
        2 => (0x48, 2), // rdx
        3 => (0x48, 1), // rcx
        4 => (0x4C, 0), // r8: REX.W|R (reg field extended)
        _ => (0x4C, 1), // r9: REX.W|R
    }
}

// ================================================================ finalize ==

struct Final<'g> {
    code_len: u64,
    data: Vec<u8>,
    goff: Vec<u64>,
    soff: Vec<u64>,
    globals: &'g [Global],
}

fn finalize(mut cg: Cg) -> Vec<u8> {
    let code_len = cg.code.len() as u64;

    // --- assemble data section --
    let mut data: Vec<u8> = Vec::new();
    let mut goff = Vec::new();
    for g in &cg.globals {
        while data.len() % 8 != 0 {
            data.push(0);
        }
        goff.push(data.len() as u64);
        let size = if g.elems > 0 { g.elems * g.ty.bytes() } else { g.ty.bytes() };
        data.extend(core::iter::repeat_n(0u8, size as usize));
    }
    let mut soff = vec![0u64; cg.strings.len()];
    for (i, s) in cg.strings.iter().enumerate() {
        soff[i] = data.len() as u64;
        data.extend_from_slice(s);
    }

    let fin = Final { code_len, data: data.clone(), goff, soff, globals: &cg.globals };

    // --- bake global initializers ---
    for (gi, g) in cg.globals.iter().enumerate() {
        if let Some(init) = &g.init {
            let start = fin.goff[gi] as usize;
            let v = match init {
                Expr::Neg(inner) => const_val(inner).map(|x| (x as i64).wrapping_neg() as u64),
                Expr::Cast(_, inner) => const_val(inner),
                other => const_val(other),
            };
            match (init, v) {
                (Expr::Str(sid), _) => {
                    let a = LOAD_BASE + HDR_LEN + fin.code_len + fin.soff[*sid as usize];
                    data[start..start + 8].copy_from_slice(&a.to_le_bytes());
                }
                (_, Some(v)) => {
                    let bs: Vec<u8> = match fin.globals[gi].ty.bytes() {
                        1 => vec![v as u8],
                        2 => (v as u16).to_le_bytes().to_vec(),
                        4 => (v as u32).to_le_bytes().to_vec(),
                        _ => v.to_le_bytes().to_vec(),
                    };
                    data[start..start + bs.len()].copy_from_slice(&bs);
                }
                _ => {
                    fail("global initializers must be constant or a string literal", g.line);
                }
            }
        }
    }

    let data_off = HDR_LEN + code_len;

    // --- patch fixes ---
    for fix in cg.fixes.drain(..) {
        match fix {
            Fix::Label { at, end, lbl } => {
                let Some(target) = cg.labels.get(&lbl) else {
                    panic!("hcc: internal: label {lbl} never placed");
                };
                let rel = *target as i64 - (end as i64);
                cg.code[at as usize..at as usize + 4].copy_from_slice(&(rel as i32).to_le_bytes());
            }
            Fix::CallFn { at, end, idx } => {
                // Both ends live inside the code section: target IP =
                // LOAD_BASE + HDR_LEN + foff[idx], source IP = LOAD_BASE +
                // HDR_LEN + end. HDR_LEN cancels — adding it here would call
                // 16 bytes past every internal function.
                let rel = cg.foff[idx] as i64 - end as i64;
                cg.code[at as usize..at as usize + 4].copy_from_slice(&(rel as i32).to_le_bytes());
            }
            Fix::CallApi { at, end, addr } => {
                let rel = (addr as i64) - (LOAD_BASE + HDR_LEN + end as u64) as i64;
                cg.code[at as usize..at as usize + 4].copy_from_slice(&(rel as i32).to_le_bytes());
            }
            Fix::Abs { at, addr } => {
                let a = match addr {
                    Abs::Str(s) => LOAD_BASE + HDR_LEN + fin.code_len + fin.soff[s as usize],
                    Abs::Glob(g) => LOAD_BASE + HDR_LEN + fin.code_len + fin.goff[g],
                };
                cg.code[at as usize..at as usize + 8].copy_from_slice(&a.to_le_bytes());
            }
        }
    }

    // --- blob ---
    let entry_idx = cg
        .fmap
        .get("Main")
        .copied()
        .unwrap_or_else(|| {
            cg.fns
                .iter()
                .position(|f| f.name.eq_ignore_ascii_case("main"))
                .unwrap_or(0)
        });
    let entry_off = HDR_LEN + cg.foff[entry_idx];

    let mut blob: Vec<u8> = Vec::with_capacity((HDR_LEN + code_len + data.len() as u64) as usize);
    blob.extend_from_slice(&HDR_MAGIC.to_le_bytes());
    blob.extend_from_slice(&(entry_off as u32).to_le_bytes());
    blob.extend_from_slice(&((HDR_LEN + code_len + data.len() as u64) as u32).to_le_bytes());
    blob.extend_from_slice(&(data_off as u32).to_le_bytes());
    blob.extend_from_slice(&cg.code);
    blob.extend_from_slice(&data);
    blob
}

fn const_val(e: &Expr) -> Option<u64> {
    Some(match e {
        Expr::Lit(v) => *v,
        Expr::Neg(x) => (const_val(x)? as i64).wrapping_neg() as u64,
        Expr::Inv(x) => !(const_val(x)?),
        Expr::Not(x) => (const_val(x)? == 0) as u64,
        Expr::Cast(_, x) => const_val(x)?,
        Expr::Bin(op, a, b) => {
            let (x, y) = (const_val(a)? as i64, const_val(b)? as i64);
            (match op.as_str() {
                "+" => x.wrapping_add(y),
                "-" => x.wrapping_sub(y),
                "*" => x.wrapping_mul(y),
                "/" => {
                    if y == 0 {
                        0
                    } else {
                        x / y
                    }
                }
                "%" => {
                    if y == 0 {
                        0
                    } else {
                        x % y
                    }
                }
                "<<" => x << (y & 63),
                ">>" => x >> (y & 63),
                "&" => x & y,
                "|" => x | y,
                "^" => x ^ y,
                "<" => (x < y) as i64,
                "<=" => (x <= y) as i64,
                ">" => (x > y) as i64,
                ">=" => (x >= y) as i64,
                "==" => (x == y) as i64,
                "!=" => (x != y) as i64,
                _ => return None,
            }) as u64
        }
        Expr::Log(is_and, a, b) => {
            let (x, y) = (const_val(a)?, const_val(b)?);
            (if *is_and { x != 0 && y != 0 } else { x != 0 || y != 0 }) as u64
        }
        _ => return None,
    })
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: hcc <in.HC> <out.bin> [--api <kernel_api.nm>]");
        std::process::exit(1);
    }
    let src = fs::read(&args[1]).unwrap_or_else(|e| {
        eprintln!("hcc: reading {}: {e}", args[1]);
        exit(1);
    });
    let mut api = HashMap::new();
    if let Some(pos) = args.iter().position(|a| a == "--api") {
        let path = args.get(pos + 1).unwrap_or_else(|| {
            eprintln!("hcc: --api needs a file argument");
            exit(1);
        });
        let nm = fs::read_to_string(path).unwrap_or_else(|e| {
            eprintln!("hcc: reading {}: {e}", path);
            exit(1);
        });
        for line in nm.lines() {
            let f: Vec<&str> = line.split_whitespace().collect();
            if f.len() == 2 && f[0].starts_with("HC_") {
                if let Ok(a) = u64::from_str_radix(f[1].trim_start_matches("0x"), 16) {
                    api.insert(f[0].to_string(), a);
                }
            }
        }
    }
    let (globals, fns, strings) = lexparse::parse(&src, &args[1]);
    let mut cg = Cg::new(fns, globals, strings, api);
    for fi in 0..cg.fns.len() {
        cg.emit_fn(fi);
    }
    let blob = finalize(cg);
    fs::write(&args[2], &blob).unwrap_or_else(|e| {
        eprintln!("hcc: writing {}: {e}", args[2]);
        exit(1);
    });
}
