//! A small reader for the top-level assignments of a Python module, enough for the tables of the AMS dashboard code
//! (`hyperspectral_dashboard_row_dependancy.py`): numbers, strings, `None` / `True` / `False`, lists, tuples, dicts, names
//! of earlier assignments, `a if cond else b`, `==` / `!=`, `+ - * /`. Anything else (a function call, an attribute) is
//! `Unknown`, which makes the value that contains it unknown but never stops the rest from being read. Nothing is run.

use std::cell::RefCell;
use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq)]
pub enum Val {
    Str(String),
    Num(f64),
    Bool(bool),
    None,
    List(Vec<Val>),
    Tuple(Vec<Val>),
    Dict(Vec<(Val, Val)>),
    Unknown,
}

impl Val {
    pub fn str(&self) -> Option<&str> {
        match self {
            Val::Str(s) => Some(s),
            _ => None,
        }
    }
    pub fn num(&self) -> Option<f64> {
        match self {
            Val::Num(n) => Some(*n),
            _ => None,
        }
    }
    /// Items of a list or a tuple.
    pub fn items(&self) -> Option<&[Val]> {
        match self {
            Val::List(v) | Val::Tuple(v) => Some(v),
            _ => None,
        }
    }
    pub fn dict(&self) -> Option<&[(Val, Val)]> {
        match self {
            Val::Dict(d) => Some(d),
            _ => None,
        }
    }
    pub fn get(&self, key: &str) -> Option<&Val> {
        self.dict()?.iter().find(|(k, _)| k.str() == Some(key)).map(|(_, v)| v)
    }
    pub fn is_unknown(&self) -> bool {
        matches!(self, Val::Unknown)
    }
}

/// The top-level assignments of a module, by name (the text on the right of the `=`), evaluated on demand.
pub struct Module {
    rhs: HashMap<String, String>,
    memo: RefCell<HashMap<String, Val>>,
    busy: RefCell<Vec<String>>,
}

/// Splits `src` into top-level statements: a statement starts at column 0 and runs to the end of its brackets.
fn statements(src: &str) -> Vec<String> {
    let mut out = vec![];
    let mut cur = String::new();
    let (mut depth, mut quote): (i32, Option<char>) = (0, None);
    for line in src.lines() {
        if cur.is_empty() && (line.is_empty() || line.starts_with([' ', '\t', '#'])) {
            continue;
        }
        let mut chars = line.chars().peekable();
        let mut text = String::new();
        while let Some(c) = chars.next() {
            match quote {
                Some(q) => {
                    text.push(c);
                    if c == '\\' {
                        if let Some(n) = chars.next() {
                            text.push(n);
                        }
                    } else if c == q {
                        quote = None;
                    }
                }
                None => match c {
                    '#' => break,
                    '"' | '\'' => {
                        quote = Some(c);
                        text.push(c);
                    }
                    '(' | '[' | '{' => {
                        depth += 1;
                        text.push(c);
                    }
                    ')' | ']' | '}' => {
                        depth -= 1;
                        text.push(c);
                    }
                    _ => text.push(c),
                },
            }
        }
        if !cur.is_empty() {
            cur.push('\n');
        }
        cur.push_str(&text);
        // a line ending in a backslash continues too
        if depth <= 0 && quote.is_none() && !text.trim_end().ends_with('\\') {
            out.push(std::mem::take(&mut cur));
            depth = 0;
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// `NAME = expr` or `A, B = x, y` at the start of a statement.
fn split_assign(st: &str) -> Option<(Vec<String>, String)> {
    let b = st.as_bytes();
    let mut i = 0;
    // targets: identifiers separated by commas
    let mut names = vec![];
    loop {
        while i < b.len() && b[i] == b' ' {
            i += 1;
        }
        let s = i;
        while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
            i += 1;
        }
        if s == i || b[s].is_ascii_digit() {
            return None;
        }
        names.push(st[s..i].to_string());
        while i < b.len() && b[i] == b' ' {
            i += 1;
        }
        if i < b.len() && b[i] == b',' {
            i += 1;
            continue;
        }
        break;
    }
    if i < b.len() && b[i] == b'=' && b.get(i + 1) != Some(&b'=') {
        Some((names, st[i + 1..].trim().to_string()))
    } else {
        None
    }
}

impl Module {
    pub fn parse(src: &str) -> Module {
        let mut rhs = HashMap::new();
        for st in statements(src) {
            if let Some((names, expr)) = split_assign(&st) {
                if names.len() == 1 {
                    rhs.insert(names[0].clone(), expr);
                } else {
                    // A, B = x, y: each name takes the element at its place
                    for (k, n) in names.iter().enumerate() {
                        rhs.insert(n.clone(), format!("({expr})[{k}]"));
                    }
                }
            }
        }
        Module { rhs, memo: RefCell::new(HashMap::new()), busy: RefCell::new(vec![]) }
    }

    #[allow(dead_code)]
    pub fn has(&self, name: &str) -> bool {
        self.rhs.contains_key(name)
    }

    /// The value of a top-level name (`Unknown` when it is not there or cannot be worked out).
    pub fn get(&self, name: &str) -> Val {
        if let Some(v) = self.memo.borrow().get(name) {
            return v.clone();
        }
        if self.busy.borrow().iter().any(|b| b == name) {
            return Val::Unknown;
        }
        let Some(text) = self.rhs.get(name) else { return Val::Unknown };
        self.busy.borrow_mut().push(name.to_string());
        let v = Parser { s: text.chars().collect(), i: 0, m: self }.top();
        self.busy.borrow_mut().pop();
        self.memo.borrow_mut().insert(name.to_string(), v.clone());
        v
    }
}

struct Parser<'a> {
    s: Vec<char>,
    i: usize,
    m: &'a Module,
}

impl Parser<'_> {
    fn ws(&mut self) {
        while self.i < self.s.len() && (self.s[self.i].is_whitespace() || self.s[self.i] == '\\') {
            self.i += 1;
        }
    }
    fn peek(&mut self) -> Option<char> {
        self.ws();
        self.s.get(self.i).copied()
    }
    fn eat(&mut self, t: &str) -> bool {
        self.ws();
        let tc: Vec<char> = t.chars().collect();
        if self.s[self.i..].starts_with(&tc) {
            self.i += tc.len();
            true
        } else {
            false
        }
    }
    /// A keyword (`if`, `else`) as a whole word.
    fn kw(&mut self, w: &str) -> bool {
        self.ws();
        let wc: Vec<char> = w.chars().collect();
        if self.s[self.i..].starts_with(&wc) && !self.s.get(self.i + wc.len()).is_some_and(|c| c.is_alphanumeric() || *c == '_') {
            self.i += wc.len();
            true
        } else {
            false
        }
    }

    fn top(&mut self) -> Val {
        let v = self.expr_list();
        self.ws();
        if self.i < self.s.len() {
            Val::Unknown
        } else {
            v
        }
    }

    /// `a, b` without brackets is a tuple.
    fn expr_list(&mut self) -> Val {
        let first = self.expr();
        if self.peek() == Some(',') {
            let mut v = vec![first];
            while self.eat(",") {
                if matches!(self.peek(), None | Some(')') | Some(']') | Some('}')) {
                    break;
                }
                v.push(self.expr());
            }
            Val::Tuple(v)
        } else {
            first
        }
    }

    fn expr(&mut self) -> Val {
        let a = self.cmp();
        if self.kw("if") {
            let c = self.cmp();
            if !self.kw("else") {
                return Val::Unknown;
            }
            let b = self.expr();
            return match c {
                Val::Bool(true) => a,
                Val::Bool(false) => b,
                _ => Val::Unknown,
            };
        }
        a
    }

    fn cmp(&mut self) -> Val {
        let a = self.sum();
        let eq = if self.eat("==") {
            Some(true)
        } else if self.eat("!=") {
            Some(false)
        } else {
            None
        };
        match eq {
            Some(e) => {
                let b = self.sum();
                if a.is_unknown() || b.is_unknown() {
                    Val::Unknown
                } else {
                    Val::Bool((a == b) == e)
                }
            }
            None => a,
        }
    }

    fn sum(&mut self) -> Val {
        let mut a = self.term();
        loop {
            let plus = if self.eat("+") {
                true
            } else if self.peek() == Some('-') {
                self.i += 1;
                false
            } else {
                break;
            };
            let b = self.term();
            a = match (a, b, plus) {
                (Val::Num(x), Val::Num(y), true) => Val::Num(x + y),
                (Val::Num(x), Val::Num(y), false) => Val::Num(x - y),
                (Val::Str(x), Val::Str(y), true) => Val::Str(x + &y),
                _ => Val::Unknown,
            };
        }
        a
    }

    fn term(&mut self) -> Val {
        let mut a = self.factor();
        loop {
            let mul = if self.eat("*") {
                true
            } else if self.peek() == Some('/') {
                self.i += 1;
                false
            } else {
                break;
            };
            let b = self.factor();
            a = match (a, b, mul) {
                (Val::Num(x), Val::Num(y), true) => Val::Num(x * y),
                (Val::Num(x), Val::Num(y), false) if y != 0.0 => Val::Num(x / y),
                _ => Val::Unknown,
            };
        }
        a
    }

    fn factor(&mut self) -> Val {
        if self.peek() == Some('-') {
            self.i += 1;
            return match self.factor() {
                Val::Num(x) => Val::Num(-x),
                _ => Val::Unknown,
            };
        }
        if self.peek() == Some('+') {
            self.i += 1;
        }
        let mut a = self.atom();
        // `.attr`, `(call)` and `[subscript]` after a value
        loop {
            match self.peek() {
                Some('.') if !self.s.get(self.i + 1).is_some_and(|c| c.is_ascii_digit()) => {
                    self.i += 1;
                    while self.i < self.s.len() && (self.s[self.i].is_alphanumeric() || self.s[self.i] == '_') {
                        self.i += 1;
                    }
                    a = Val::Unknown;
                }
                Some('(') => {
                    self.skip_group('(', ')');
                    a = Val::Unknown;
                }
                Some('[') => {
                    let save = self.i;
                    self.i += 1;
                    let idx = self.expr();
                    if self.eat("]") {
                        a = match (&a, &idx) {
                            (Val::List(v) | Val::Tuple(v), Val::Num(k)) if *k >= 0.0 => v.get(*k as usize).cloned().unwrap_or(Val::Unknown),
                            _ => Val::Unknown,
                        };
                    } else {
                        self.i = save;
                        self.skip_group('[', ']');
                        a = Val::Unknown;
                    }
                }
                _ => break,
            }
        }
        a
    }

    fn skip_group(&mut self, open: char, close: char) {
        let mut depth = 0;
        let mut quote: Option<char> = None;
        while self.i < self.s.len() {
            let c = self.s[self.i];
            self.i += 1;
            match quote {
                Some(q) => {
                    if c == '\\' {
                        self.i += 1;
                    } else if c == q {
                        quote = None;
                    }
                }
                None if c == '"' || c == '\'' => quote = Some(c),
                None if c == open => depth += 1,
                None if c == close => {
                    depth -= 1;
                    if depth == 0 {
                        return;
                    }
                }
                _ => {}
            }
        }
    }

    fn atom(&mut self) -> Val {
        let Some(c) = self.peek() else { return Val::Unknown };
        match c {
            '(' => {
                self.i += 1;
                if self.eat(")") {
                    return Val::Tuple(vec![]);
                }
                let first = self.expr();
                if self.peek() == Some(',') {
                    let mut v = vec![first];
                    while self.eat(",") {
                        if self.peek() == Some(')') {
                            break;
                        }
                        v.push(self.expr());
                    }
                    self.eat(")");
                    Val::Tuple(v)
                } else if self.eat(")") {
                    first
                } else {
                    Val::Unknown
                }
            }
            '[' => {
                self.i += 1;
                let mut v = vec![];
                while self.peek().is_some() && self.peek() != Some(']') {
                    v.push(self.expr());
                    if !self.eat(",") {
                        break;
                    }
                }
                if self.eat("]") {
                    Val::List(v)
                } else {
                    Val::Unknown
                }
            }
            '{' => {
                self.i += 1;
                let mut d = vec![];
                while self.peek().is_some() && self.peek() != Some('}') {
                    let k = self.expr();
                    if !self.eat(":") {
                        return Val::Unknown;
                    }
                    let v = self.expr();
                    d.push((k, v));
                    if !self.eat(",") {
                        break;
                    }
                }
                if self.eat("}") {
                    Val::Dict(d)
                } else {
                    Val::Unknown
                }
            }
            '"' | '\'' => self.string(),
            c if c.is_ascii_digit() || c == '.' => self.number(),
            c if c.is_alphabetic() || c == '_' => {
                let s = self.i;
                while self.i < self.s.len() && (self.s[self.i].is_alphanumeric() || self.s[self.i] == '_') {
                    self.i += 1;
                }
                let word: String = self.s[s..self.i].iter().collect();
                // a prefixed string: r"..", b"..", f".." (f-strings are not worked out)
                if matches!(word.as_str(), "r" | "R" | "u" | "b") && matches!(self.s.get(self.i), Some('"') | Some('\'')) {
                    return self.string();
                }
                match word.as_str() {
                    "True" => Val::Bool(true),
                    "False" => Val::Bool(false),
                    "None" => Val::None,
                    "not" => match self.factor() {
                        Val::Bool(b) => Val::Bool(!b),
                        _ => Val::Unknown,
                    },
                    _ => self.m.get(&word),
                }
            }
            _ => Val::Unknown,
        }
    }

    fn number(&mut self) -> Val {
        let s = self.i;
        while self.i < self.s.len() && (self.s[self.i].is_ascii_digit() || matches!(self.s[self.i], '.' | '_' | 'e' | 'E')) {
            // a sign directly after an exponent belongs to the number
            if matches!(self.s[self.i], 'e' | 'E') && matches!(self.s.get(self.i + 1), Some('-') | Some('+')) {
                self.i += 1;
            }
            self.i += 1;
        }
        let t: String = self.s[s..self.i].iter().filter(|c| **c != '_').collect();
        t.parse::<f64>().map(Val::Num).unwrap_or(Val::Unknown)
    }

    /// One or more adjacent string literals.
    fn string(&mut self) -> Val {
        let mut out = String::new();
        loop {
            let Some(&q) = self.s.get(self.i) else { break };
            if q != '"' && q != '\'' {
                break;
            }
            self.i += 1;
            while self.i < self.s.len() && self.s[self.i] != q {
                let c = self.s[self.i];
                self.i += 1;
                if c == '\\' {
                    let n = self.s.get(self.i).copied().unwrap_or('\\');
                    self.i += 1;
                    match n {
                        'n' => out.push('\n'),
                        't' => out.push('\t'),
                        '\\' => out.push('\\'),
                        '"' => out.push('"'),
                        '\'' => out.push('\''),
                        o => {
                            out.push('\\');
                            out.push(o);
                        }
                    }
                } else {
                    out.push(c);
                }
            }
            self.i += 1;
            self.ws();
            if !matches!(self.s.get(self.i), Some('"') | Some('\'')) {
                break;
            }
        }
        Val::Str(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SRC: &str = r##"
import os
FE3_METRIC = "D900"
W900_REF, W900_HALF = 900.0, 30.0    # +-30 nm
EDGE_FRAC = 0.24         # strip
ROWS = {   # key -> (column, ok flag, label, colourbar label)
    "a": ("D2200", "2200_ok", "White mica\nD2200", "depth"),
    # a comment inside the dict
    "b": (("D900", "900_ok", "Fe3+\nD900", "depth") if FE3_METRIC == "D900"
          else ("red_slope", None, "Fe3+\nR750/R550", "ratio")),
    "c": ("W2200_shift", "W2200_ok", "Al-OH pos.\nW2200 shift", "Δnm"),
}
SHORT = {"b": "Fe³⁺\n" + ("D900" if FE3_METRIC == "D900" else "R750/550")}
DIV = {"w900": ["#7f0000", "#d6452e"], "x": "PuOr_r"}
HALF = {"w900": W900_HALF}
RANGE = (1545.0, 1565.0)
def f(x):
    return x
CALLED = np.cross(a, b)
NEG = -1.5e-3
"##;

    #[test]
    fn tables_with_comments_conditions_and_names_are_read() {
        let m = Module::parse(SRC);
        let rows = m.get("ROWS");
        let a = rows.get("a").unwrap().items().unwrap();
        assert_eq!(a[0].str(), Some("D2200"));
        assert_eq!(a[2].str(), Some("White mica\nD2200"));
        let b = rows.get("b").unwrap().items().unwrap();
        assert_eq!((b[0].str(), b[3].str()), (Some("D900"), Some("depth")), "the conditional picks the branch that is true");
        assert_eq!(rows.get("c").unwrap().items().unwrap()[3].str(), Some("Δnm"));
        assert_eq!(rows.dict().unwrap().len(), 3);
        assert_eq!(m.get("SHORT").get("b").unwrap().str(), Some("Fe³⁺\nD900"));
        assert_eq!(m.get("W900_HALF").num(), Some(30.0));
        assert_eq!(m.get("W900_REF").num(), Some(900.0));
        assert_eq!(m.get("HALF").get("w900").unwrap().num(), Some(30.0));
        assert_eq!(m.get("DIV").get("w900").unwrap().items().unwrap().len(), 2);
        assert_eq!(m.get("RANGE").items().unwrap()[1].num(), Some(1565.0));
        assert_eq!(m.get("NEG").num(), Some(-0.0015));
        assert_eq!(m.get("EDGE_FRAC").num(), Some(0.24));
    }

    #[test]
    fn what_cannot_be_worked_out_is_unknown_and_the_rest_still_reads() {
        let m = Module::parse(SRC);
        assert!(m.get("CALLED").is_unknown());
        assert!(m.get("not_there").is_unknown());
        assert!(!m.has("f"), "functions are not assignments");
        // a cycle does not hang
        let c = Module::parse("A = B + 1\nB = A + 1\n");
        assert!(c.get("A").is_unknown());
    }
}
