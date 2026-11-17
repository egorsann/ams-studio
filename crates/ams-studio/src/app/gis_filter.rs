//! Layer filters in the syntax QGIS uses for them: the SQL `WHERE` condition of the layer's *Filter* (query builder)
//! dialog, which is also what a QGIS project stores in its data source as `subset=...`.
//!
//! ```text
//! "HOLEID" IN ('PRB001', 'PRB002')          "DEPTH" > 120 AND "TYPE" = 'core'
//! "HOLEID" LIKE 'PRIB%'                     NOT ("STATUS" IS NULL) OR lower("NAME") = 'x'
//! "DIP" BETWEEN 10 AND 45                   "A" || '-' || "B" = 'x-y'
//! ```
//!
//! Field names are written in double quotes (bare names work too). A double-quoted word that is not a field is taken as
//! text, as SQLite does (`"HOLEID" LIKE "PRIB%"`). Text is in single quotes (`''` for a quote). Operators: `= != <> < >
//! <= >=`, `AND OR NOT`, `IN (..)`, `NOT IN`, `LIKE` / `ILIKE` (`%` and `_`, case-insensitive like SQLite), `IS [NOT]
//! NULL`, `BETWEEN .. AND ..`, `+ - * / %`, `||` (join text), parentheses. Functions: `lower upper length trim abs round
//! floor ceil coalesce concat substr left right to_int to_real to_string`. A feature is shown when the condition is true.
//!
//! Pure, tested without a window; the layer code in `gis_ui.rs` applies the result to a layer.

use super::gis_vector::AttrVal;

#[derive(Clone, Debug, PartialEq)]
pub enum Val {
    Null,
    Num(f64),
    Text(String),
    Bool(bool),
}

impl Val {
    fn from_attr(a: &AttrVal) -> Val {
        match a {
            AttrVal::Null => Val::Null,
            AttrVal::Num(n) => Val::Num(*n),
            AttrVal::Text(t) => Val::Text(t.clone()),
        }
    }

    fn num(&self) -> Option<f64> {
        match self {
            Val::Num(n) => Some(*n),
            Val::Bool(b) => Some(f64::from(u8::from(*b))),
            Val::Text(t) => t.trim().replace(',', ".").parse().ok(),
            Val::Null => None,
        }
    }

    fn text(&self) -> String {
        match self {
            Val::Null => String::new(),
            Val::Num(n) if n.fract() == 0.0 && n.abs() < 1e15 => format!("{}", *n as i64),
            Val::Num(n) => format!("{n}"),
            Val::Text(t) => t.clone(),
            Val::Bool(b) => if *b { "1" } else { "0" }.to_string(),
        }
    }

    fn truth(&self) -> Option<bool> {
        match self {
            Val::Null => None,
            Val::Bool(b) => Some(*b),
            Val::Num(n) => Some(*n != 0.0),
            Val::Text(t) => t.trim().parse::<f64>().ok().map(|n| n != 0.0),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Num(f64),
    Str(String),
    /// A double-quoted word: a field, or text when there is no such field.
    Quoted(String),
    Word(String),
    Op(&'static str),
    LParen,
    RParen,
    Comma,
}

fn lex(src: &str) -> Result<Vec<Tok>, String> {
    let c: Vec<char> = src.chars().collect();
    let mut i = 0;
    let mut out = vec![];
    while i < c.len() {
        let ch = c[i];
        match ch {
            _ if ch.is_whitespace() => i += 1,
            '\'' | '"' => {
                let q = ch;
                let mut s = String::new();
                i += 1;
                loop {
                    match c.get(i) {
                        None => return Err(format!("A {q} is not closed.")),
                        Some(&x) if x == q => {
                            if c.get(i + 1) == Some(&q) {
                                s.push(q);
                                i += 2;
                            } else {
                                i += 1;
                                break;
                            }
                        }
                        Some(&x) => {
                            s.push(x);
                            i += 1;
                        }
                    }
                }
                out.push(if q == '\'' { Tok::Str(s) } else { Tok::Quoted(s) });
            }
            '0'..='9' | '.' if ch.is_ascii_digit() || c.get(i + 1).is_some_and(|d| d.is_ascii_digit()) => {
                let start = i;
                while i < c.len() && (c[i].is_ascii_digit() || c[i] == '.') {
                    i += 1;
                }
                if i < c.len() && (c[i] == 'e' || c[i] == 'E') && c.get(i + 1).is_some_and(|d| d.is_ascii_digit() || *d == '-' || *d == '+') {
                    i += 2;
                    while i < c.len() && c[i].is_ascii_digit() {
                        i += 1;
                    }
                }
                let t: String = c[start..i].iter().collect();
                out.push(Tok::Num(t.parse().map_err(|_| format!("{t} is not a number."))?));
            }
            '(' => {
                out.push(Tok::LParen);
                i += 1;
            }
            ')' => {
                out.push(Tok::RParen);
                i += 1;
            }
            ',' => {
                out.push(Tok::Comma);
                i += 1;
            }
            _ if ch.is_alphabetic() || ch == '_' => {
                let start = i;
                while i < c.len() && (c[i].is_alphanumeric() || c[i] == '_' || c[i] == '.') {
                    i += 1;
                }
                out.push(Tok::Word(c[start..i].iter().collect()));
            }
            _ => {
                let two: String = c[i..(i + 2).min(c.len())].iter().collect();
                let op = match two.as_str() {
                    "<>" => Some("<>"),
                    "!=" => Some("<>"),
                    "<=" => Some("<="),
                    ">=" => Some(">="),
                    "||" => Some("||"),
                    "==" => Some("="),
                    _ => None,
                };
                if let Some(op) = op {
                    out.push(Tok::Op(op));
                    i += 2;
                    continue;
                }
                let op = match ch {
                    '=' => "=",
                    '<' => "<",
                    '>' => ">",
                    '+' => "+",
                    '-' => "-",
                    '*' => "*",
                    '/' => "/",
                    '%' => "%",
                    other => return Err(format!("Unexpected character {other}.")),
                };
                out.push(Tok::Op(op));
                i += 1;
            }
        }
    }
    Ok(out)
}

#[derive(Clone, Debug, PartialEq)]
pub enum Expr {
    Lit(Val),
    /// A double-quoted word.
    Quoted(String),
    /// A bare word that has to be a field.
    Field(String),
    Neg(Box<Expr>),
    Not(Box<Expr>),
    Bin(&'static str, Box<Expr>, Box<Expr>),
    And(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
    In(Box<Expr>, Vec<Expr>, bool),
    Like(Box<Expr>, Box<Expr>, bool),
    IsNull(Box<Expr>, bool),
    Between(Box<Expr>, Box<Expr>, Box<Expr>, bool),
    Call(String, Vec<Expr>),
}

struct P {
    t: Vec<Tok>,
    i: usize,
}

fn is_word(t: Option<&Tok>, w: &str) -> bool {
    matches!(t, Some(Tok::Word(x)) if x.eq_ignore_ascii_case(w))
}

impl P {
    fn peek(&self) -> Option<&Tok> {
        self.t.get(self.i)
    }

    fn word(&mut self, w: &str) -> bool {
        if is_word(self.peek(), w) {
            self.i += 1;
            true
        } else {
            false
        }
    }

    fn or(&mut self) -> Result<Expr, String> {
        let mut l = self.and()?;
        while self.word("or") {
            l = Expr::Or(Box::new(l), Box::new(self.and()?));
        }
        Ok(l)
    }

    fn and(&mut self) -> Result<Expr, String> {
        let mut l = self.not()?;
        while self.word("and") {
            l = Expr::And(Box::new(l), Box::new(self.not()?));
        }
        Ok(l)
    }

    fn not(&mut self) -> Result<Expr, String> {
        if self.word("not") {
            return Ok(Expr::Not(Box::new(self.not()?)));
        }
        self.cmp()
    }

    fn cmp(&mut self) -> Result<Expr, String> {
        let l = self.concat()?;
        loop {
            let negated = is_word(self.peek(), "not") && (is_word(self.t.get(self.i + 1), "in") || is_word(self.t.get(self.i + 1), "like") || is_word(self.t.get(self.i + 1), "ilike") || is_word(self.t.get(self.i + 1), "between"));
            if negated {
                self.i += 1;
            }
            if self.word("in") {
                if self.peek() != Some(&Tok::LParen) {
                    return Err("IN needs a list in parentheses.".into());
                }
                self.i += 1;
                let mut items = vec![];
                loop {
                    items.push(self.or()?);
                    match self.peek() {
                        Some(Tok::Comma) => self.i += 1,
                        Some(Tok::RParen) => {
                            self.i += 1;
                            break;
                        }
                        _ => return Err("The list of IN is not closed with a parenthesis.".into()),
                    }
                }
                return Ok(Expr::In(Box::new(l), items, negated));
            }
            if self.word("like") || self.word("ilike") {
                return Ok(Expr::Like(Box::new(l), Box::new(self.concat()?), negated));
            }
            if self.word("between") {
                let a = self.concat()?;
                if !self.word("and") {
                    return Err("BETWEEN needs AND between its two limits.".into());
                }
                let b = self.concat()?;
                return Ok(Expr::Between(Box::new(l), Box::new(a), Box::new(b), negated));
            }
            if self.word("is") {
                let neg = self.word("not");
                if self.word("null") {
                    return Ok(Expr::IsNull(Box::new(l), neg));
                }
                if self.word("true") {
                    return Ok(Expr::Bin(if neg { "<>" } else { "=" }, Box::new(l), Box::new(Expr::Lit(Val::Bool(true)))));
                }
                if self.word("false") {
                    return Ok(Expr::Bin(if neg { "<>" } else { "=" }, Box::new(l), Box::new(Expr::Lit(Val::Bool(false)))));
                }
                return Err("IS must be followed by NULL, NOT NULL, TRUE or FALSE.".into());
            }
            if let Some(Tok::Op(op @ ("=" | "<>" | "<" | ">" | "<=" | ">="))) = self.peek().cloned() {
                self.i += 1;
                let r = self.concat()?;
                return Ok(Expr::Bin(op, Box::new(l), Box::new(r)));
            }
            return Ok(l);
        }
    }

    fn concat(&mut self) -> Result<Expr, String> {
        let mut l = self.add()?;
        while self.peek() == Some(&Tok::Op("||")) {
            self.i += 1;
            l = Expr::Bin("||", Box::new(l), Box::new(self.add()?));
        }
        Ok(l)
    }

    fn add(&mut self) -> Result<Expr, String> {
        let mut l = self.mul()?;
        while let Some(Tok::Op(op @ ("+" | "-"))) = self.peek().cloned() {
            self.i += 1;
            l = Expr::Bin(op, Box::new(l), Box::new(self.mul()?));
        }
        Ok(l)
    }

    fn mul(&mut self) -> Result<Expr, String> {
        let mut l = self.unary()?;
        while let Some(Tok::Op(op @ ("*" | "/" | "%"))) = self.peek().cloned() {
            self.i += 1;
            l = Expr::Bin(op, Box::new(l), Box::new(self.unary()?));
        }
        Ok(l)
    }

    fn unary(&mut self) -> Result<Expr, String> {
        match self.peek() {
            Some(Tok::Op("-")) => {
                self.i += 1;
                Ok(Expr::Neg(Box::new(self.unary()?)))
            }
            Some(Tok::Op("+")) => {
                self.i += 1;
                self.unary()
            }
            _ => self.primary(),
        }
    }

    fn primary(&mut self) -> Result<Expr, String> {
        let Some(t) = self.peek().cloned() else { return Err("The condition ends too early.".into()) };
        self.i += 1;
        match t {
            Tok::Num(n) => Ok(Expr::Lit(Val::Num(n))),
            Tok::Str(s) => Ok(Expr::Lit(Val::Text(s))),
            Tok::Quoted(s) => Ok(Expr::Quoted(s)),
            Tok::LParen => {
                let e = self.or()?;
                if self.peek() != Some(&Tok::RParen) {
                    return Err("A parenthesis is not closed.".into());
                }
                self.i += 1;
                Ok(e)
            }
            Tok::Word(w) => {
                if self.peek() == Some(&Tok::LParen) {
                    self.i += 1;
                    let mut args = vec![];
                    if self.peek() == Some(&Tok::RParen) {
                        self.i += 1;
                    } else {
                        loop {
                            args.push(self.or()?);
                            match self.peek() {
                                Some(Tok::Comma) => self.i += 1,
                                Some(Tok::RParen) => {
                                    self.i += 1;
                                    break;
                                }
                                _ => return Err(format!("The call of {w}( is not closed.")),
                            }
                        }
                    }
                    let name = w.to_lowercase();
                    if !FUNCTIONS.contains(&name.as_str()) {
                        return Err(format!("Unknown function {w}."));
                    }
                    return Ok(Expr::Call(name, args));
                }
                match w.to_lowercase().as_str() {
                    "null" => Ok(Expr::Lit(Val::Null)),
                    "true" => Ok(Expr::Lit(Val::Bool(true))),
                    "false" => Ok(Expr::Lit(Val::Bool(false))),
                    "and" | "or" | "in" | "like" | "is" | "between" => Err(format!("{} is in the wrong place.", w.to_uppercase())),
                    _ => Ok(Expr::Field(w)),
                }
            }
            Tok::RParen => Err("A closing parenthesis has nothing to close.".into()),
            Tok::Comma => Err("A comma is in the wrong place.".into()),
            Tok::Op(o) => Err(format!("{o} is in the wrong place.")),
        }
    }
}

const FUNCTIONS: [&str; 15] = ["lower", "upper", "length", "trim", "abs", "round", "floor", "ceil", "coalesce", "concat", "substr", "left", "right", "to_int", "to_real"];

/// Reads a condition. `Ok(None)` for an empty one (everything is shown).
pub fn parse(src: &str) -> Result<Option<Expr>, String> {
    let t = lex(src)?;
    if t.is_empty() {
        return Ok(None);
    }
    let mut p = P { t, i: 0 };
    let e = p.or()?;
    if p.i < p.t.len() {
        return Err(format!("Something is left over after the condition: {}.", describe(&p.t[p.i])));
    }
    Ok(Some(e))
}

fn describe(t: &Tok) -> String {
    match t {
        Tok::Num(n) => format!("{n}"),
        Tok::Str(s) => format!("'{s}'"),
        Tok::Quoted(s) => format!("\"{s}\""),
        Tok::Word(w) => w.clone(),
        Tok::Op(o) => (*o).to_string(),
        Tok::LParen => "(".into(),
        Tok::RParen => ")".into(),
        Tok::Comma => ",".into(),
    }
}

/// `%` and `_` pattern match, case-insensitive.
fn like(text: &str, pat: &str) -> bool {
    let t: Vec<char> = text.to_lowercase().chars().collect();
    let p: Vec<char> = pat.to_lowercase().chars().collect();
    fn go(t: &[char], p: &[char]) -> bool {
        match p.first() {
            None => t.is_empty(),
            Some('%') => (0..=t.len()).any(|k| go(&t[k..], &p[1..])),
            Some('_') => !t.is_empty() && go(&t[1..], &p[1..]),
            Some(c) => t.first() == Some(c) && go(&t[1..], &p[1..]),
        }
    }
    go(&t, &p)
}

/// Compares two values: numbers numerically (also text that reads as a number against a number), text as text.
fn compare(a: &Val, b: &Val) -> Option<std::cmp::Ordering> {
    if matches!(a, Val::Null) || matches!(b, Val::Null) {
        return None;
    }
    let numeric = matches!(a, Val::Num(_) | Val::Bool(_)) || matches!(b, Val::Num(_) | Val::Bool(_));
    if numeric {
        if let (Some(x), Some(y)) = (a.num(), b.num()) {
            return x.partial_cmp(&y);
        }
    }
    Some(a.text().cmp(&b.text()))
}

/// Evaluates against one feature; `get` gives the value of a field by name (`None`: there is no such field).
pub fn eval(e: &Expr, get: &dyn Fn(&str) -> Option<Val>) -> Result<Val, String> {
    let b = |x: bool| Val::Bool(x);
    Ok(match e {
        Expr::Lit(v) => v.clone(),
        Expr::Quoted(n) => get(n).unwrap_or_else(|| Val::Text(n.clone())),
        Expr::Field(n) => get(n).ok_or_else(|| format!("There is no field {n}."))?,
        Expr::Neg(x) => eval(x, get)?.num().map_or(Val::Null, |n| Val::Num(-n)),
        Expr::Not(x) => eval(x, get)?.truth().map_or(Val::Null, |t| b(!t)),
        Expr::And(l, r) => match (eval(l, get)?.truth(), eval(r, get)?.truth()) {
            (Some(false), _) | (_, Some(false)) => b(false),
            (Some(true), Some(true)) => b(true),
            _ => Val::Null,
        },
        Expr::Or(l, r) => match (eval(l, get)?.truth(), eval(r, get)?.truth()) {
            (Some(true), _) | (_, Some(true)) => b(true),
            (Some(false), Some(false)) => b(false),
            _ => Val::Null,
        },
        Expr::IsNull(x, neg) => b(matches!(eval(x, get)?, Val::Null) != *neg),
        Expr::In(x, items, neg) => {
            let v = eval(x, get)?;
            if matches!(v, Val::Null) {
                return Ok(Val::Null);
            }
            let mut found = false;
            for it in items {
                if compare(&v, &eval(it, get)?) == Some(std::cmp::Ordering::Equal) {
                    found = true;
                    break;
                }
            }
            b(found != *neg)
        }
        Expr::Like(x, p, neg) => {
            let (v, p) = (eval(x, get)?, eval(p, get)?);
            if matches!(v, Val::Null) || matches!(p, Val::Null) {
                return Ok(Val::Null);
            }
            b(like(&v.text(), &p.text()) != *neg)
        }
        Expr::Between(x, lo, hi, neg) => {
            let (v, lo, hi) = (eval(x, get)?, eval(lo, get)?, eval(hi, get)?);
            match (compare(&v, &lo), compare(&v, &hi)) {
                (Some(a), Some(c)) => b((a != std::cmp::Ordering::Less && c != std::cmp::Ordering::Greater) != *neg),
                _ => Val::Null,
            }
        }
        Expr::Bin(op, l, r) => {
            let (a, c) = (eval(l, get)?, eval(r, get)?);
            match *op {
                "=" | "<>" | "<" | ">" | "<=" | ">=" => match compare(&a, &c) {
                    None => Val::Null,
                    Some(o) => b(match *op {
                        "=" => o.is_eq(),
                        "<>" => o.is_ne(),
                        "<" => o.is_lt(),
                        ">" => o.is_gt(),
                        "<=" => o.is_le(),
                        _ => o.is_ge(),
                    }),
                },
                "||" => {
                    if matches!(a, Val::Null) || matches!(c, Val::Null) {
                        Val::Null
                    } else {
                        Val::Text(a.text() + &c.text())
                    }
                }
                _ => match (a.num(), c.num()) {
                    (Some(x), Some(y)) => match *op {
                        "+" => Val::Num(x + y),
                        "-" => Val::Num(x - y),
                        "*" => Val::Num(x * y),
                        "/" => {
                            if y == 0.0 {
                                Val::Null
                            } else {
                                Val::Num(x / y)
                            }
                        }
                        _ => {
                            if y == 0.0 {
                                Val::Null
                            } else {
                                Val::Num(x % y)
                            }
                        }
                    },
                    _ => Val::Null,
                },
            }
        }
        Expr::Call(name, args) => {
            let v: Vec<Val> = args.iter().map(|a| eval(a, get)).collect::<Result<_, _>>()?;
            let arg = |i: usize| v.get(i).cloned().unwrap_or(Val::Null);
            let text_fn = |f: &dyn Fn(String) -> Val| if matches!(arg(0), Val::Null) { Val::Null } else { f(arg(0).text()) };
            let num_fn = |f: &dyn Fn(f64) -> f64| arg(0).num().map_or(Val::Null, |n| Val::Num(f(n)));
            match name.as_str() {
                "lower" => text_fn(&|t| Val::Text(t.to_lowercase())),
                "upper" => text_fn(&|t| Val::Text(t.to_uppercase())),
                "trim" => text_fn(&|t| Val::Text(t.trim().to_string())),
                "length" => text_fn(&|t| Val::Num(t.chars().count() as f64)),
                "abs" => num_fn(&f64::abs),
                "floor" => num_fn(&f64::floor),
                "ceil" => num_fn(&f64::ceil),
                "round" => {
                    let k = 10f64.powi(arg(1).num().unwrap_or(0.0) as i32);
                    num_fn(&|n| (n * k).round() / k)
                }
                "to_int" => num_fn(&f64::trunc),
                "to_real" => arg(0).num().map_or(Val::Null, Val::Num),
                "coalesce" => v.iter().find(|x| !matches!(x, Val::Null)).cloned().unwrap_or(Val::Null),
                "concat" => Val::Text(v.iter().map(|x| x.text()).collect()),
                "substr" => text_fn(&|t| {
                    let chars: Vec<char> = t.chars().collect();
                    let start = (arg(1).num().unwrap_or(1.0) as i64 - 1).clamp(0, chars.len() as i64) as usize;
                    let len = arg(2).num().map_or(chars.len(), |n| n.max(0.0) as usize);
                    Val::Text(chars[start..(start + len).min(chars.len())].iter().collect())
                }),
                "left" => text_fn(&|t| Val::Text(t.chars().take(arg(1).num().unwrap_or(0.0).max(0.0) as usize).collect())),
                "right" => text_fn(&|t| {
                    let n = t.chars().count();
                    let k = arg(1).num().unwrap_or(0.0).max(0.0) as usize;
                    Val::Text(t.chars().skip(n.saturating_sub(k)).collect())
                }),
                _ => return Err(format!("Unknown function {name}.")),
            }
        }
    })
}

/// Which features pass the condition: one flag per row. A condition that cannot be read, or names a field that is not
/// there, is an error with the reason (nothing is filtered then).
pub fn mask(src: &str, fields: &[String], rows: &[Vec<AttrVal>]) -> Result<Vec<bool>, String> {
    let Some(expr) = parse(src)? else { return Ok(vec![true; rows.len()]) };
    // a bare word or a call that names an unknown field is reported once, before the rows are visited
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let get = |n: &str| fields.iter().position(|f| f == n).or_else(|| fields.iter().position(|f| f.eq_ignore_ascii_case(n))).map(|i| row.get(i).map_or(Val::Null, Val::from_attr));
        out.push(eval(&expr, &get)?.truth() == Some(true));
    }
    if rows.is_empty() {
        // an empty table still gets its field names checked
        let get = |n: &str| fields.iter().any(|f| f == n).then_some(Val::Null);
        eval(&expr, &get)?;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fields() -> Vec<String> {
        ["HOLEID", "DEPTH", "TYPE", "NOTE"].iter().map(|s| s.to_string()).collect()
    }

    fn rows() -> Vec<Vec<AttrVal>> {
        let t = |s: &str| AttrVal::Text(s.to_string());
        vec![
            vec![t("PRB001"), AttrVal::Num(12.5), t("core"), AttrVal::Null],
            vec![t("PRB002"), AttrVal::Num(130.0), t("core"), t("x-y")],
            vec![t("PRIB77"), AttrVal::Num(300.0), t("chip"), t("")],
            vec![t("zk9"), AttrVal::Null, t("Core"), t("a")],
        ]
    }

    fn pass(src: &str) -> Vec<bool> {
        mask(src, &fields(), &rows()).unwrap_or_else(|e| panic!("{src}: {e}"))
    }

    #[test]
    fn in_lists_like_and_the_double_quoted_pattern_of_a_real_project() {
        assert_eq!(pass("\"HOLEID\" IN ('PRB001', 'zk9')"), vec![true, false, false, true]);
        // the list over several lines with CR LF, as a project stores it
        assert_eq!(pass("\"HOLEID\" IN (\r\n'PRB002',\r\n'PRIB77'\r\n)"), vec![false, true, true, false]);
        assert_eq!(pass("\"HOLEID\" NOT IN ('PRB001')"), vec![false, true, true, true]);
        // LIKE is case-insensitive like SQLite; "PRIB%" is no field, so it is text
        assert_eq!(pass("\"HOLEID\" LIKE \"PRIB%\""), vec![false, false, true, false]);
        assert_eq!(pass("\"HOLEID\" like 'prb00_'"), vec![true, true, false, false]);
        assert_eq!(pass("\"HOLEID\" NOT LIKE 'PR%'"), vec![false, false, false, true]);
    }

    #[test]
    fn comparisons_logic_null_and_arithmetic() {
        assert_eq!(pass("\"DEPTH\" > 100 AND \"TYPE\" = 'core'"), vec![false, true, false, false]);
        assert_eq!(pass("\"DEPTH\" < 20 OR \"TYPE\" = 'chip'"), vec![true, false, true, false]);
        assert_eq!(pass("NOT (\"DEPTH\" > 100)"), vec![true, false, false, false]);
        // a null never matches a comparison; IS NULL finds it
        assert_eq!(pass("\"DEPTH\" IS NULL"), vec![false, false, false, true]);
        assert_eq!(pass("\"DEPTH\" IS NOT NULL AND \"DEPTH\" <> 130"), vec![true, false, true, false]);
        assert_eq!(pass("\"DEPTH\" BETWEEN 10 AND 200"), vec![true, true, false, false]);
        assert_eq!(pass("\"DEPTH\" * 2 > 250 AND \"DEPTH\" - 100 < 50"), vec![false, true, false, false]);
        assert_eq!(pass("\"DEPTH\" != 12.5"), vec![false, true, true, false]);
        assert_eq!(pass("\"TYPE\" = 'core'"), vec![true, true, false, false]);
    }

    #[test]
    fn functions_concat_and_bare_field_names() {
        assert_eq!(pass("lower(\"TYPE\") = 'core'"), vec![true, true, false, true]);
        assert_eq!(pass("length(\"HOLEID\") = 6 AND left(\"HOLEID\", 3) = 'PRB'"), vec![true, true, false, false]);
        assert_eq!(pass("\"HOLEID\" || '-' || \"TYPE\" = 'PRB001-core'"), vec![true, false, false, false]);
        assert_eq!(pass("coalesce(\"NOTE\", 'none') = 'none'"), vec![true, false, false, false]);
        assert_eq!(pass("round(\"DEPTH\") = 13"), vec![true, false, false, false]);
        assert_eq!(pass("DEPTH > 100"), vec![false, true, true, false]);
        // the text of a number compares as a number
        assert_eq!(pass("\"DEPTH\" = '130'"), vec![false, true, false, false]);
        // empty: everything
        assert_eq!(pass("  "), vec![true; 4]);
    }

    #[test]
    fn mistakes_are_reported_with_a_reason() {
        for (src, why) in [
            ("\"HOLEID\" IN ('a'", "not closed"),
            ("\"HOLEID\" = 'a", "not closed"),
            ("\"DEPTH\" >", "ends too early"),
            ("NOFIELD = 3", "no field"),
            ("frobnicate(1) = 1", "Unknown function"),
            ("\"DEPTH\" > 3 4", "left over"),
            ("\"DEPTH\" BETWEEN 1 2", "AND"),
            ("\"HOLEID\" ~ 'a'", "Unexpected character"),
        ] {
            let e = mask(src, &fields(), &rows()).unwrap_err();
            assert!(e.contains(why), "{src}: {e}");
        }
    }
}
