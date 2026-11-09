//! Tiny recursive-descent integer evaluator, so the REPL can feel a
//! little bit like HolyC.

pub fn eval(s: &str) -> Option<i64> {
    let mut p = Parser {
        b: s.as_bytes(),
        i: 0,
    };
    let v = p.expr()?;
    p.ws();
    if p.i == p.b.len() {
        Some(v)
    } else {
        None
    }
}

struct Parser<'a> {
    b: &'a [u8],
    i: usize,
}

impl Parser<'_> {
    fn ws(&mut self) {
        while self.i < self.b.len() && self.b[self.i].is_ascii_whitespace() {
            self.i += 1;
        }
    }

    fn peek(&mut self) -> Option<u8> {
        self.ws();
        self.b.get(self.i).copied()
    }

    fn eat(&mut self, c: u8) -> bool {
        if self.peek() == Some(c) {
            self.i += 1;
            true
        } else {
            false
        }
    }

    fn expr(&mut self) -> Option<i64> {
        let mut v = self.term()?;
        loop {
            match self.peek() {
                Some(b'+') => {
                    self.i += 1;
                    v = v.checked_add(self.term()?)?;
                }
                Some(b'-') => {
                    self.i += 1;
                    v = v.checked_sub(self.term()?)?;
                }
                _ => return Some(v),
            }
        }
    }

    fn term(&mut self) -> Option<i64> {
        let mut v = self.factor()?;
        loop {
            match self.peek() {
                Some(b'*') => {
                    self.i += 1;
                    v = v.checked_mul(self.factor()?)?;
                }
                Some(b'/') => {
                    self.i += 1;
                    let d = self.factor()?;
                    if d == 0 {
                        return None;
                    }
                    v = v.checked_div(d)?;
                }
                Some(b'%') => {
                    self.i += 1;
                    let d = self.factor()?;
                    if d == 0 {
                        return None;
                    }
                    v = v.checked_rem(d)?;
                }
                _ => return Some(v),
            }
        }
    }

    fn factor(&mut self) -> Option<i64> {
        match self.peek()? {
            b'-' => {
                self.i += 1;
                Some(self.factor()?.checked_neg()?)
            }
            b'+' => {
                self.i += 1;
                self.factor()
            }
            b'(' => {
                self.i += 1;
                let v = self.expr()?;
                if !self.eat(b')') {
                    return None;
                }
                Some(v)
            }
            c if c.is_ascii_digit() => {
                let mut v: i64 = 0;
                while let Some(c) = self.peek() {
                    if !c.is_ascii_digit() {
                        break;
                    }
                    self.i += 1;
                    v = v.checked_mul(10)?.checked_add((c - b'0') as i64)?;
                }
                Some(v)
            }
            _ => None,
        }
    }
}
