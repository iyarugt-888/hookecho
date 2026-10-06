//! Attribute filters for imported GIS layers (ROADMAP_PARITY M4.3): a small, bounded expression
//! language over a feature's attributes.
//!
//! ```text
//! POP100 > 1000 and TYPE = "school"
//! not (STATUS = "closed") or `Site name` contains "fire"
//! OPEN = true and EMAIL is not missing
//! ```
//!
//! Comparisons are typed: a number compares with a number, text with text, true/false with a
//! boolean; anything else, and any attribute the feature does not have, is unknown rather than
//! false. Unknown carries through `and`/`or`/`not` the three-valued way, and a feature is shown only
//! when its filter is true, so a feature missing the attribute is not shown and is never treated
//! as zero or as an empty string. `=`/`!=` on text are exact; `contains` ignores case. Names with
//! spaces or punctuation go in backticks. The parser is bounded in length, nesting and size.

use serde_json::{Map, Value};

pub const MAX_LEN: usize = 1000;
pub const MAX_DEPTH: usize = 32;
pub const MAX_NODES: usize = 200;

#[derive(Debug, Clone, PartialEq)]
enum Lit {
    Num(f64),
    Text(String),
    Bool(bool),
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Op {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Contains,
}

#[derive(Debug, Clone, PartialEq)]
enum Node {
    Cmp(String, Op, Lit),
    Missing(String, bool),
    Not(Box<Node>),
    And(Box<Node>, Box<Node>),
    Or(Box<Node>, Box<Node>),
}

/// A parsed filter.
#[derive(Debug, Clone, PartialEq)]
pub struct Filter(Node);

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Ident(String),
    Lit(Lit),
    Op(Op),
    And,
    Or,
    Not,
    Is,
    Missing,
    Open,
    Close,
}

fn lex(src: &str) -> Result<Vec<Tok>, String> {
    let mut out = Vec::new();
    let c: Vec<char> = src.chars().collect();
    let mut i = 0;
    while i < c.len() {
        let ch = c[i];
        if ch.is_whitespace() {
            i += 1;
            continue;
        }
        match ch {
            '(' => {
                out.push(Tok::Open);
                i += 1;
            }
            ')' => {
                out.push(Tok::Close);
                i += 1;
            }
            '"' | '\'' | '`' => {
                let q = ch;
                let start = i + 1;
                let end = c[start..]
                    .iter()
                    .position(|&x| x == q)
                    .map(|p| start + p)
                    .ok_or_else(|| format!("unclosed {q} at column {}", i + 1))?;
                let text: String = c[start..end].iter().collect();
                out.push(if q == '`' {
                    Tok::Ident(text)
                } else {
                    Tok::Lit(Lit::Text(text))
                });
                i = end + 1;
            }
            '=' => {
                out.push(Tok::Op(Op::Eq));
                i += if c.get(i + 1) == Some(&'=') { 2 } else { 1 };
            }
            '!' if c.get(i + 1) == Some(&'=') => {
                out.push(Tok::Op(Op::Ne));
                i += 2;
            }
            '<' | '>' => {
                let eq = c.get(i + 1) == Some(&'=');
                out.push(Tok::Op(match (ch, eq) {
                    ('<', false) => Op::Lt,
                    ('<', true) => Op::Le,
                    ('>', false) => Op::Gt,
                    _ => Op::Ge,
                }));
                i += if eq { 2 } else { 1 };
            }
            _ if ch.is_ascii_digit() || ch == '-' || ch == '.' => {
                let start = i;
                i += 1;
                while i < c.len() {
                    let x = c[i];
                    let exponent_sign = matches!(x, '+' | '-') && matches!(c[i - 1], 'e' | 'E');
                    if x.is_ascii_digit() || matches!(x, '.' | 'e' | 'E') || exponent_sign {
                        i += 1;
                    } else {
                        break;
                    }
                }
                let text: String = c[start..i].iter().collect();
                let n: f64 = text
                    .parse()
                    .map_err(|_| format!("\u{201c}{text}\u{201d} is not a number"))?;
                if !n.is_finite() {
                    return Err(format!("\u{201c}{text}\u{201d} is not a finite number"));
                }
                out.push(Tok::Lit(Lit::Num(n)));
            }
            _ if ch.is_alphabetic() || ch == '_' => {
                let start = i;
                while i < c.len() && (c[i].is_alphanumeric() || c[i] == '_') {
                    i += 1;
                }
                let word: String = c[start..i].iter().collect();
                out.push(match word.to_ascii_lowercase().as_str() {
                    "and" => Tok::And,
                    "or" => Tok::Or,
                    "not" => Tok::Not,
                    "is" => Tok::Is,
                    "missing" => Tok::Missing,
                    "contains" => Tok::Op(Op::Contains),
                    "true" => Tok::Lit(Lit::Bool(true)),
                    "false" => Tok::Lit(Lit::Bool(false)),
                    _ => Tok::Ident(word),
                });
            }
            _ => {
                return Err(format!(
                    "unexpected \u{201c}{ch}\u{201d} at column {}",
                    i + 1
                ))
            }
        }
    }
    Ok(out)
}

struct Parser {
    toks: Vec<Tok>,
    pos: usize,
    nodes: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos)
    }

    fn next(&mut self) -> Option<Tok> {
        let t = self.toks.get(self.pos).cloned();
        self.pos += 1;
        t
    }

    fn node(&mut self, n: Node) -> Result<Node, String> {
        self.nodes += 1;
        if self.nodes > MAX_NODES {
            return Err(format!("more than {MAX_NODES} terms"));
        }
        Ok(n)
    }

    fn or(&mut self, depth: usize) -> Result<Node, String> {
        let mut left = self.and(depth)?;
        while self.peek() == Some(&Tok::Or) {
            self.pos += 1;
            let right = self.and(depth)?;
            left = self.node(Node::Or(Box::new(left), Box::new(right)))?;
        }
        Ok(left)
    }

    fn and(&mut self, depth: usize) -> Result<Node, String> {
        let mut left = self.not(depth)?;
        while self.peek() == Some(&Tok::And) {
            self.pos += 1;
            let right = self.not(depth)?;
            left = self.node(Node::And(Box::new(left), Box::new(right)))?;
        }
        Ok(left)
    }

    fn not(&mut self, depth: usize) -> Result<Node, String> {
        if depth > MAX_DEPTH {
            return Err(format!("nested more than {MAX_DEPTH} deep"));
        }
        if self.peek() == Some(&Tok::Not) {
            self.pos += 1;
            let inner = self.not(depth + 1)?;
            return self.node(Node::Not(Box::new(inner)));
        }
        self.term(depth)
    }

    fn term(&mut self, depth: usize) -> Result<Node, String> {
        match self.next() {
            Some(Tok::Open) => {
                let inner = self.or(depth + 1)?;
                match self.next() {
                    Some(Tok::Close) => Ok(inner),
                    _ => Err("a \u{201c}(\u{201d} is not closed".into()),
                }
            }
            Some(Tok::Ident(name)) => match self.next() {
                Some(Tok::Is) => {
                    let negated = self.peek() == Some(&Tok::Not);
                    if negated {
                        self.pos += 1;
                    }
                    match self.next() {
                        Some(Tok::Missing) => self.node(Node::Missing(name, !negated)),
                        _ => Err(format!("after \u{201c}{name} is\u{201d}, expected missing")),
                    }
                }
                Some(Tok::Op(op)) => match self.next() {
                    Some(Tok::Lit(lit)) => {
                        if op == Op::Contains && !matches!(lit, Lit::Text(_)) {
                            return Err("contains needs text in quotes".into());
                        }
                        self.node(Node::Cmp(name, op, lit))
                    }
                    _ => Err(format!(
                        "after \u{201c}{name}\u{201d} and its comparison, expected a value"
                    )),
                },
                _ => Err(format!(
                    "after \u{201c}{name}\u{201d}, expected a comparison"
                )),
            },
            Some(t) => Err(format!("unexpected {t:?}")),
            None => Err("the filter ends early".into()),
        }
    }
}

/// Parse a filter.
pub fn parse(src: &str) -> Result<Filter, String> {
    if src.len() > MAX_LEN {
        return Err(format!("longer than {MAX_LEN} characters"));
    }
    let toks = lex(src)?;
    if toks.is_empty() {
        return Err("empty".into());
    }
    let mut p = Parser {
        toks,
        pos: 0,
        nodes: 0,
    };
    let node = p.or(0)?;
    if p.pos < p.toks.len() {
        return Err(format!("unexpected {:?}", p.toks[p.pos]));
    }
    Ok(Filter(node))
}

fn cmp(value: Option<&Value>, op: Op, lit: &Lit) -> Option<bool> {
    let ord = |o: std::cmp::Ordering| match op {
        Op::Eq => o.is_eq(),
        Op::Ne => o.is_ne(),
        Op::Lt => o.is_lt(),
        Op::Le => o.is_le(),
        Op::Gt => o.is_gt(),
        Op::Ge => o.is_ge(),
        Op::Contains => false,
    };
    match (value?, lit) {
        (Value::Null, _) => None,
        (Value::Number(n), Lit::Num(x)) if op != Op::Contains => {
            Some(ord(n.as_f64()?.partial_cmp(x)?))
        }
        (Value::String(s), Lit::Text(t)) if op == Op::Contains => {
            Some(s.to_lowercase().contains(&t.to_lowercase()))
        }
        (Value::String(s), Lit::Text(t)) => Some(ord(s.as_str().cmp(t.as_str()))),
        (Value::Bool(b), Lit::Bool(x)) if matches!(op, Op::Eq | Op::Ne) => {
            Some((b == x) == (op == Op::Eq))
        }
        _ => None,
    }
}

fn eval(n: &Node, props: &Map<String, Value>) -> Option<bool> {
    match n {
        Node::Cmp(k, op, lit) => cmp(props.get(k), *op, lit),
        Node::Missing(k, missing) => {
            let absent = props.get(k).is_none_or(Value::is_null);
            Some(absent == *missing)
        }
        Node::Not(a) => eval(a, props).map(|v| !v),
        Node::And(a, b) => match (eval(a, props), eval(b, props)) {
            (Some(false), _) | (_, Some(false)) => Some(false),
            (Some(true), Some(true)) => Some(true),
            _ => None,
        },
        Node::Or(a, b) => match (eval(a, props), eval(b, props)) {
            (Some(true), _) | (_, Some(true)) => Some(true),
            (Some(false), Some(false)) => Some(false),
            _ => None,
        },
    }
}

impl Filter {
    /// Whether a feature with these attributes is shown: only when the filter is true for it.
    pub fn shows(&self, props: &Map<String, Value>) -> bool {
        eval(&self.0, props) == Some(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn props(v: Value) -> Map<String, Value> {
        v.as_object().unwrap().clone()
    }

    #[test]
    fn typed_comparisons_and_logic() {
        let school = props(serde_json::json!({
            "TYPE": "school", "POP": 1200, "OPEN": true, "Site name": "Fire Station 4"
        }));
        let shows = |f: &str| parse(f).unwrap().shows(&school);
        assert!(shows("POP > 1000 and TYPE = \"school\""));
        assert!(shows("POP >= 1200 and POP <= 1200.0 and POP != 5"));
        assert!(!shows("POP < 1000"));
        assert!(shows("OPEN = true and not (OPEN = false)"));
        assert!(
            shows("`Site name` contains 'FIRE'"),
            "contains ignores case"
        );
        assert!(!shows("TYPE = 'School'"), "= is exact");
        assert!(shows("TYPE = 'x' or POP = 1.2e3"));
        assert!(shows("EMAIL is missing and TYPE is not missing"));
    }

    #[test]
    fn a_missing_or_mistyped_attribute_is_unknown_never_zero_or_empty() {
        let f = props(serde_json::json!({"POP": "1200", "NAME": null}));
        for src in [
            "POP > 1000", // text against a number
            "POP < 1000",
            "AREA = 0",       // absent
            "NAME = ''",      // null
            "not (AREA > 5)", // unknown stays unknown under not
        ] {
            assert!(!parse(src).unwrap().shows(&f), "{src}");
        }
        // Unknown or true is true; unknown and false is false.
        assert!(parse("AREA > 5 or POP = '1200'").unwrap().shows(&f));
        assert!(!parse("AREA > 5 and POP = '1200'").unwrap().shows(&f));
        assert!(
            parse("NAME is missing").unwrap().shows(&f),
            "null counts as missing"
        );
    }

    #[test]
    fn bad_filters_say_why_and_are_bounded() {
        for (src, why) in [
            ("POP >", "expected a value"),
            ("POP 5", "expected a comparison"),
            ("(POP > 5", "not closed"),
            ("NAME = 'x", "unclosed"),
            ("POP > 5 POP", "unexpected"),
            ("NAME contains 5", "needs text"),
            ("POP > 1e999", "finite"),
            ("", "empty"),
            ("POP # 3", "unexpected"),
        ] {
            let e = parse(src).unwrap_err();
            assert!(e.contains(why), "{src}: {e}");
        }
        let deep = format!("{}A = 1{}", "(".repeat(40), ")".repeat(40));
        assert!(parse(&deep).unwrap_err().contains("deep"));
        let nots = format!("{}A = 1", "not ".repeat(40));
        assert!(parse(&nots).unwrap_err().contains("deep"));
        let wide = vec!["A = 1"; 110].join(" or ");
        assert!(parse(&wide).unwrap_err().contains("terms"));
        assert!(parse(&"A".repeat(MAX_LEN + 1))
            .unwrap_err()
            .contains("longer"));
    }
}
