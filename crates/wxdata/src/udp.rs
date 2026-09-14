//! User-defined radar products (Phase C1): a small, safe expression language for combining a
//! gate's own moments and geometry into a custom derived value — GR2Analyst's user-defined
//! products, scoped to what this pass can build and verify.
//!
//! This is deliberately the *evaluator* half only. It answers "what does this formula compute at
//! one gate", which is enough to drive a live readout (the gate inspector) against real data. It
//! does not yet render a user-defined product as its own map layer — plugging a new value into
//! the polar per-tilt rendering pipeline (palettes, 3D, thresholds, all keyed by the fixed
//! [`crate::level2::Moment`] enum) is a separate, larger piece of work, called out as such in
//! `ROADMAP_NEW.md` rather than attempted here.
//!
//! Also out of scope for this pass, and noted for the same reason: the roadmap's vertical/layer
//! aggregate functions (`max_vertical`, `layer_mean`, height-crossing) and environmental-height
//! inputs (freezing level, -10C/-20C heights) — both need a whole column of tilts or external
//! model data, not just the one gate this evaluator sees.
//!
//! No native code ever runs: expressions parse to a fixed [`Expr`] tree and every node the parser
//! can produce is one this module's own evaluator interprets, so a malformed or malicious formula
//! is a parse or evaluation error, never a crash.
//!
//! # Grammar
//!
//! ```text
//! expr    := ternary
//! ternary := or ( '?' expr ':' expr )?
//! or      := and ( '||' and )*
//! and     := cmp ( '&&' cmp )*
//! cmp     := add ( ('<'|'<='|'>'|'>='|'=='|'!=') add )?
//! add     := mul ( ('+'|'-') mul )*
//! mul     := unary ( ('*'|'/') unary )*
//! unary   := ('-'|'!')? primary
//! primary := NUMBER | IDENT | IDENT '(' expr (',' expr)* ')' | '(' expr ')'
//! ```
//!
//! Identifiers are case-insensitive. Inputs: `REF`, `VEL`, `SW`, `ZDR`, `KDP`, `CC`, `RANGE_KM`
//! (ground range), `AZIMUTH_DEG`, `ELEVATION_DEG`, `BEAM_HEIGHT_M` (above radar), and
//! `BEAM_ALTITUDE_M` (above sea level when site elevation is known). Functions:
//! `min`, `max` (2 args), `mean` (2–8 args), `clamp` (3 args: value, low, high), `abs` (1 arg).
//! Comparisons and logical operators produce `1.0`
//! (true) or `0.0` (false); the ternary's condition treats any nonzero value as true.
//!
//! `None` (a moment absent at this gate — below threshold, range-folded, or simply not carried by
//! this radial) propagates through every operator: a formula referencing a missing input has no
//! value here, the same as the moments it is built from.

use std::fmt;

/// One input an expression can read at a gate. Case-insensitive on the way in; see
/// [`Input::parse`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Input {
    Reflectivity,
    Velocity,
    SpectrumWidth,
    DifferentialReflectivity,
    SpecificDifferentialPhase,
    CorrelationCoefficient,
    /// Ground range from the radar, km — matches what the gate inspector already labels "Ground
    /// range", not the slant range along the beam.
    RangeKm,
    AzimuthDeg,
    ElevationDeg,
    /// Beam center height above the radar, metres; not altitude above sea level.
    BeamHeightM,
    /// Approximate beam center altitude above sea level, metres; unavailable without site elevation.
    BeamAltitudeM,
}

impl Input {
    fn parse(name: &str) -> Option<Self> {
        Some(match name.to_ascii_uppercase().as_str() {
            "REF" => Self::Reflectivity,
            "VEL" => Self::Velocity,
            "SW" => Self::SpectrumWidth,
            "ZDR" => Self::DifferentialReflectivity,
            "KDP" => Self::SpecificDifferentialPhase,
            "CC" => Self::CorrelationCoefficient,
            "RANGE_KM" => Self::RangeKm,
            "AZIMUTH_DEG" => Self::AzimuthDeg,
            "ELEVATION_DEG" => Self::ElevationDeg,
            "BEAM_HEIGHT_M" => Self::BeamHeightM,
            "BEAM_ALTITUDE_M" => Self::BeamAltitudeM,
            _ => return None,
        })
    }

    /// Every input name a formula can reference — for building an editor's autocomplete/help list.
    pub const ALL: [Input; 11] = [
        Input::Reflectivity,
        Input::Velocity,
        Input::SpectrumWidth,
        Input::DifferentialReflectivity,
        Input::SpecificDifferentialPhase,
        Input::CorrelationCoefficient,
        Input::RangeKm,
        Input::AzimuthDeg,
        Input::ElevationDeg,
        Input::BeamHeightM,
        Input::BeamAltitudeM,
    ];

    /// The exact spelling a formula uses for this input.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Reflectivity => "REF",
            Self::Velocity => "VEL",
            Self::SpectrumWidth => "SW",
            Self::DifferentialReflectivity => "ZDR",
            Self::SpecificDifferentialPhase => "KDP",
            Self::CorrelationCoefficient => "CC",
            Self::RangeKm => "RANGE_KM",
            Self::AzimuthDeg => "AZIMUTH_DEG",
            Self::ElevationDeg => "ELEVATION_DEG",
            Self::BeamHeightM => "BEAM_HEIGHT_M",
            Self::BeamAltitudeM => "BEAM_ALTITUDE_M",
        }
    }
}

/// The moments and geometry available at one gate. `None` means that input has no value here
/// (missing moment, below threshold, range-folded, or simply never sampled).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct GateInputs {
    pub reflectivity: Option<f32>,
    pub velocity: Option<f32>,
    pub spectrum_width: Option<f32>,
    pub differential_reflectivity: Option<f32>,
    pub specific_diff_phase: Option<f32>,
    pub correlation_coefficient: Option<f32>,
    pub range_km: Option<f32>,
    pub azimuth_deg: Option<f32>,
    pub elevation_deg: Option<f32>,
    pub beam_height_m: Option<f32>,
    pub beam_altitude_m: Option<f32>,
}

impl GateInputs {
    fn get(&self, input: Input) -> Option<f32> {
        match input {
            Input::Reflectivity => self.reflectivity,
            Input::Velocity => self.velocity,
            Input::SpectrumWidth => self.spectrum_width,
            Input::DifferentialReflectivity => self.differential_reflectivity,
            Input::SpecificDifferentialPhase => self.specific_diff_phase,
            Input::CorrelationCoefficient => self.correlation_coefficient,
            Input::RangeKm => self.range_km,
            Input::AzimuthDeg => self.azimuth_deg,
            Input::ElevationDeg => self.elevation_deg,
            Input::BeamHeightM => self.beam_height_m,
            Input::BeamAltitudeM => self.beam_altitude_m,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Lt,
    Le,
    Gt,
    Ge,
    Eq,
    Ne,
    And,
    Or,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Func {
    Min,
    Max,
    Mean,
    Clamp,
    Abs,
}

impl Func {
    fn parse(name: &str) -> Option<(Self, usize, usize)> {
        Some(match name.to_ascii_lowercase().as_str() {
            "min" => (Self::Min, 2, 2),
            "max" => (Self::Max, 2, 2),
            "mean" => (Self::Mean, 2, 8),
            "clamp" => (Self::Clamp, 3, 3),
            "abs" => (Self::Abs, 1, 1),
            _ => return None,
        })
    }
}

/// A parsed user-defined-product formula. Build one with [`parse`]; evaluate it at a gate with
/// [`evaluate`]. Never constructed directly by a caller outside this module — the grammar in the
/// module doc comment is the only way in, so every `Expr` a caller holds is one this evaluator
/// already knows how to interpret.
#[derive(Debug, Clone, PartialEq)]
pub struct Expr(ExprNode);

#[derive(Debug, Clone, PartialEq)]
enum ExprNode {
    Number(f32),
    Var(Input),
    Neg(Box<ExprNode>),
    Not(Box<ExprNode>),
    Bin(BinOp, Box<ExprNode>, Box<ExprNode>),
    Call(Func, Vec<ExprNode>),
    Ternary(Box<ExprNode>, Box<ExprNode>, Box<ExprNode>),
}

/// A formula that failed to parse — position is a 0-based character offset into the source, for
/// pointing an editor at the mistake.
#[derive(Debug, Clone, PartialEq)]
pub struct ParseError {
    pub message: String,
    pub position: usize,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} (at character {})", self.message, self.position)
    }
}

impl std::error::Error for ParseError {}

/// Parse a formula. Case-insensitive identifiers; whitespace anywhere between tokens.
pub fn parse(src: &str) -> Result<Expr, ParseError> {
    let tokens = tokenize(src)?;
    let mut p = Parser { tokens: &tokens, pos: 0, depth: 0 };
    let node = p.ternary()?;
    if p.pos != p.tokens.len() {
        return Err(p.error("unexpected trailing input"));
    }
    Ok(Expr(node))
}

/// Evaluate a parsed formula against one gate's inputs. `None` when the formula (or any input it
/// touches) has no value here — never a panic or a made-up number.
pub fn evaluate(expr: &Expr, inputs: &GateInputs) -> Option<f32> {
    eval_node(&expr.0, inputs)
}

fn truthy(v: f32) -> bool {
    v != 0.0
}

fn eval_node(node: &ExprNode, inputs: &GateInputs) -> Option<f32> {
    match node {
        ExprNode::Number(n) => Some(*n),
        ExprNode::Var(input) => inputs.get(*input),
        ExprNode::Neg(a) => eval_node(a, inputs).map(|v| -v),
        ExprNode::Not(a) => eval_node(a, inputs).map(|v| f32::from(!truthy(v))),
        ExprNode::Bin(op, a, b) => {
            let a = eval_node(a, inputs)?;
            let b = eval_node(b, inputs)?;
            Some(match op {
                BinOp::Add => a + b,
                BinOp::Sub => a - b,
                BinOp::Mul => a * b,
                BinOp::Div => a / b,
                BinOp::Lt => f32::from(a < b),
                BinOp::Le => f32::from(a <= b),
                BinOp::Gt => f32::from(a > b),
                BinOp::Ge => f32::from(a >= b),
                BinOp::Eq => f32::from(a == b),
                BinOp::Ne => f32::from(a != b),
                BinOp::And => f32::from(truthy(a) && truthy(b)),
                BinOp::Or => f32::from(truthy(a) || truthy(b)),
            })
        }
        ExprNode::Call(func, args) => {
            let vals: Option<Vec<f32>> = args.iter().map(|a| eval_node(a, inputs)).collect();
            let vals = vals?;
            Some(match (func, vals.as_slice()) {
                (Func::Min, [a, b]) => a.min(*b),
                (Func::Max, [a, b]) => a.max(*b),
                (Func::Mean, values) => values.iter().sum::<f32>() / values.len() as f32,
                (Func::Clamp, [x, lo, hi]) => x.clamp(*lo, *hi),
                (Func::Abs, [x]) => x.abs(),
                _ => unreachable!("Func::parse's arity matches evaluate's arm for it"),
            })
        }
        ExprNode::Ternary(cond, a, b) => {
            let cond = eval_node(cond, inputs)?;
            eval_node(if truthy(cond) { a } else { b }, inputs)
        }
    }
}

// --- tokenizer ---

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Number(f32),
    Ident(String),
    Plus,
    Minus,
    Star,
    Slash,
    Lt,
    Le,
    Gt,
    Ge,
    EqEq,
    Ne,
    AndAnd,
    OrOr,
    Bang,
    Question,
    Colon,
    Comma,
    LParen,
    RParen,
}

fn tokenize(src: &str) -> Result<Vec<(Token, usize)>, ParseError> {
    let chars: Vec<char> = src.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let start = i;
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        if c.is_ascii_digit() || (c == '.' && chars.get(i + 1).is_some_and(char::is_ascii_digit)) {
            let mut j = i;
            while j < chars.len() && (chars[j].is_ascii_digit() || chars[j] == '.') {
                j += 1;
            }
            if j < chars.len() && (chars[j] == 'e' || chars[j] == 'E') {
                let mut k = j + 1;
                if k < chars.len() && (chars[k] == '+' || chars[k] == '-') {
                    k += 1;
                }
                if k < chars.len() && chars[k].is_ascii_digit() {
                    j = k;
                    while j < chars.len() && chars[j].is_ascii_digit() {
                        j += 1;
                    }
                }
            }
            let text: String = chars[i..j].iter().collect();
            let n: f32 = text.parse().map_err(|_| ParseError {
                message: format!("'{text}' is not a valid number"),
                position: start,
            })?;
            out.push((Token::Number(n), start));
            i = j;
            continue;
        }
        if c.is_ascii_alphabetic() || c == '_' {
            let mut j = i;
            while j < chars.len() && (chars[j].is_ascii_alphanumeric() || chars[j] == '_') {
                j += 1;
            }
            out.push((Token::Ident(chars[i..j].iter().collect()), start));
            i = j;
            continue;
        }
        macro_rules! two {
            ($next:expr, $tok:expr, $else_tok:expr) => {{
                if chars.get(i + 1) == Some(&$next) {
                    out.push(($tok, start));
                    i += 2;
                } else {
                    out.push(($else_tok, start));
                    i += 1;
                }
            }};
        }
        match c {
            '+' => {
                out.push((Token::Plus, start));
                i += 1;
            }
            '-' => {
                out.push((Token::Minus, start));
                i += 1;
            }
            '*' => {
                out.push((Token::Star, start));
                i += 1;
            }
            '/' => {
                out.push((Token::Slash, start));
                i += 1;
            }
            '?' => {
                out.push((Token::Question, start));
                i += 1;
            }
            ':' => {
                out.push((Token::Colon, start));
                i += 1;
            }
            ',' => {
                out.push((Token::Comma, start));
                i += 1;
            }
            '(' => {
                out.push((Token::LParen, start));
                i += 1;
            }
            ')' => {
                out.push((Token::RParen, start));
                i += 1;
            }
            '<' => two!('=', Token::Le, Token::Lt),
            '>' => two!('=', Token::Ge, Token::Gt),
            '!' => two!('=', Token::Ne, Token::Bang),
            '=' => {
                if chars.get(i + 1) == Some(&'=') {
                    out.push((Token::EqEq, start));
                    i += 2;
                } else {
                    return Err(ParseError {
                        message: "'=' is not an operator here — did you mean '=='?".into(),
                        position: start,
                    });
                }
            }
            '&' => {
                if chars.get(i + 1) == Some(&'&') {
                    out.push((Token::AndAnd, start));
                    i += 2;
                } else {
                    return Err(ParseError { message: "'&' must be '&&'".into(), position: start });
                }
            }
            '|' => {
                if chars.get(i + 1) == Some(&'|') {
                    out.push((Token::OrOr, start));
                    i += 2;
                } else {
                    return Err(ParseError { message: "'|' must be '||'".into(), position: start });
                }
            }
            other => {
                return Err(ParseError {
                    message: format!("unexpected character '{other}'"),
                    position: start,
                })
            }
        }
    }
    Ok(out)
}

// --- recursive-descent parser ---

/// Recursion-depth ceiling for nested parentheses/ternaries/function calls — deterministic, so a
/// pasted or generated formula fails with a clear parse error instead of a stack overflow. Far
/// beyond anything a hand-typed formula needs.
const MAX_EXPR_DEPTH: usize = 64;
const MAX_FUNCTION_ARGS: usize = 8;

struct Parser<'a> {
    tokens: &'a [(Token, usize)],
    pos: usize,
    /// Current recursive-descent nesting depth — see [`MAX_EXPR_DEPTH`].
    depth: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos).map(|(t, _)| t)
    }

    fn pos_at(&self, idx: usize) -> usize {
        self.tokens.get(idx).map_or_else(
            || self.tokens.last().map_or(0, |(_, p)| p + 1),
            |(_, p)| *p,
        )
    }

    fn error(&self, message: &str) -> ParseError {
        ParseError { message: message.to_string(), position: self.pos_at(self.pos) }
    }

    fn advance(&mut self) -> Option<&Token> {
        let t = self.tokens.get(self.pos).map(|(t, _)| t);
        if t.is_some() {
            self.pos += 1;
        }
        t
    }

    fn eat(&mut self, tok: &Token) -> Result<(), ParseError> {
        if self.peek() == Some(tok) {
            self.pos += 1;
            Ok(())
        } else {
            Err(self.error(&format!("expected {tok:?}")))
        }
    }

    /// Every nested sub-expression — parenthesized, a ternary branch, or a function argument —
    /// re-enters here, so this one choke point bounds all three kinds of nesting at once.
    fn ternary(&mut self) -> Result<ExprNode, ParseError> {
        self.depth += 1;
        if self.depth > MAX_EXPR_DEPTH {
            self.depth -= 1;
            return Err(self.error("expression nested too deeply"));
        }
        let result = self.ternary_inner();
        self.depth -= 1;
        result
    }

    fn ternary_inner(&mut self) -> Result<ExprNode, ParseError> {
        let cond = self.or()?;
        if self.peek() == Some(&Token::Question) {
            self.pos += 1;
            let a = self.ternary()?;
            self.eat(&Token::Colon)?;
            let b = self.ternary()?;
            return Ok(ExprNode::Ternary(Box::new(cond), Box::new(a), Box::new(b)));
        }
        Ok(cond)
    }

    fn or(&mut self) -> Result<ExprNode, ParseError> {
        let mut lhs = self.and()?;
        while self.peek() == Some(&Token::OrOr) {
            self.pos += 1;
            let rhs = self.and()?;
            lhs = ExprNode::Bin(BinOp::Or, Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn and(&mut self) -> Result<ExprNode, ParseError> {
        let mut lhs = self.cmp()?;
        while self.peek() == Some(&Token::AndAnd) {
            self.pos += 1;
            let rhs = self.cmp()?;
            lhs = ExprNode::Bin(BinOp::And, Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn cmp(&mut self) -> Result<ExprNode, ParseError> {
        let lhs = self.add()?;
        let op = match self.peek() {
            Some(Token::Lt) => BinOp::Lt,
            Some(Token::Le) => BinOp::Le,
            Some(Token::Gt) => BinOp::Gt,
            Some(Token::Ge) => BinOp::Ge,
            Some(Token::EqEq) => BinOp::Eq,
            Some(Token::Ne) => BinOp::Ne,
            _ => return Ok(lhs),
        };
        self.pos += 1;
        let rhs = self.add()?;
        Ok(ExprNode::Bin(op, Box::new(lhs), Box::new(rhs)))
    }

    fn add(&mut self) -> Result<ExprNode, ParseError> {
        let mut lhs = self.mul()?;
        loop {
            let op = match self.peek() {
                Some(Token::Plus) => BinOp::Add,
                Some(Token::Minus) => BinOp::Sub,
                _ => return Ok(lhs),
            };
            self.pos += 1;
            let rhs = self.mul()?;
            lhs = ExprNode::Bin(op, Box::new(lhs), Box::new(rhs));
        }
    }

    fn mul(&mut self) -> Result<ExprNode, ParseError> {
        let mut lhs = self.unary()?;
        loop {
            let op = match self.peek() {
                Some(Token::Star) => BinOp::Mul,
                Some(Token::Slash) => BinOp::Div,
                _ => return Ok(lhs),
            };
            self.pos += 1;
            let rhs = self.unary()?;
            lhs = ExprNode::Bin(op, Box::new(lhs), Box::new(rhs));
        }
    }

    fn unary(&mut self) -> Result<ExprNode, ParseError> {
        match self.peek() {
            Some(Token::Minus) => {
                self.pos += 1;
                Ok(ExprNode::Neg(Box::new(self.unary()?)))
            }
            Some(Token::Bang) => {
                self.pos += 1;
                Ok(ExprNode::Not(Box::new(self.unary()?)))
            }
            _ => self.primary(),
        }
    }

    fn primary(&mut self) -> Result<ExprNode, ParseError> {
        match self.advance().cloned() {
            Some(Token::Number(n)) => Ok(ExprNode::Number(n)),
            Some(Token::LParen) => {
                let inner = self.ternary()?;
                self.eat(&Token::RParen)?;
                Ok(inner)
            }
            Some(Token::Ident(name)) => {
                if self.peek() == Some(&Token::LParen) {
                    self.pos += 1;
                    let mut args = Vec::new();
                    if self.peek() != Some(&Token::RParen) {
                        loop {
                            args.push(self.ternary()?);
                            if args.len() > MAX_FUNCTION_ARGS {
                                return Err(self.error("too many function arguments"));
                            }
                            if self.peek() == Some(&Token::Comma) {
                                self.pos += 1;
                            } else {
                                break;
                            }
                        }
                    }
                    self.eat(&Token::RParen)?;
                    let Some((func, min_arity, max_arity)) = Func::parse(&name) else {
                        return Err(ParseError {
                            message: format!("'{name}' is not a known function"),
                            position: self.pos_at(self.pos.saturating_sub(1)),
                        });
                    };
                    if !(min_arity..=max_arity).contains(&args.len()) {
                        let arity = if min_arity == max_arity {
                            min_arity.to_string()
                        } else {
                            format!("{min_arity} to {max_arity}")
                        };
                        return Err(ParseError {
                            message: format!(
                                "{name} takes {arity} argument{}, got {}",
                                if max_arity == 1 { "" } else { "s" },
                                args.len()
                            ),
                            position: self.pos_at(self.pos.saturating_sub(1)),
                        });
                    }
                    return Ok(ExprNode::Call(func, args));
                }
                let Some(input) = Input::parse(&name) else {
                    return Err(ParseError {
                        message: format!("'{name}' is not a known input or function"),
                        position: self.pos_at(self.pos.saturating_sub(1)),
                    });
                };
                Ok(ExprNode::Var(input))
            }
            Some(other) => Err(ParseError {
                message: format!("unexpected {other:?}"),
                position: self.pos_at(self.pos.saturating_sub(1)),
            }),
            None => Err(self.error("expected an expression")),
        }
    }
}

/// A saved user-defined product: a name, the source formula, and enough display metadata to show
/// its value sensibly. Serializable so a set of these can be written to disk — see
/// `crates/hookecho/src/udp_store.rs` for where the app keeps them; syncing and exporting them are
/// not implemented yet.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ProductDef {
    pub name: String,
    pub units: String,
    pub expression: String,
}

impl ProductDef {
    /// Parse this definition's formula. Re-parses from source every call rather than caching the
    /// tree — formulas are edited far more often than evaluated in a tight loop (each evaluation
    /// is one gate, from a user click), so there is no hot path this would speed up, and it keeps
    /// `ProductDef` itself plain data with no derived state to fall out of sync.
    pub fn compile(&self) -> Result<Expr, ParseError> {
        parse(&self.expression)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inputs() -> GateInputs {
        GateInputs {
            reflectivity: Some(45.0),
            velocity: Some(-12.0),
            spectrum_width: Some(3.0),
            differential_reflectivity: Some(2.5),
            specific_diff_phase: Some(1.2),
            correlation_coefficient: Some(0.98),
            range_km: Some(80.0),
            azimuth_deg: Some(270.0),
            elevation_deg: Some(0.5),
            beam_height_m: Some(1200.0),
            beam_altitude_m: Some(1500.0),
        }
    }

    fn eval(src: &str) -> Option<f32> {
        evaluate(&parse(src).unwrap(), &inputs())
    }

    #[test]
    fn arithmetic_and_precedence() {
        assert_eq!(eval("2 + 3 * 4"), Some(14.0));
        assert_eq!(eval("(2 + 3) * 4"), Some(20.0));
        assert_eq!(eval("10 / 4 - 1"), Some(1.5));
        assert_eq!(eval("-5 + 2"), Some(-3.0));
    }

    #[test]
    fn inputs_read_the_gate() {
        assert_eq!(eval("REF"), Some(45.0));
        assert_eq!(eval("REF - 40"), Some(5.0));
        assert_eq!(eval("VEL * 2"), Some(-24.0));
    }

    #[test]
    fn comparisons_and_logic_are_one_or_zero() {
        assert_eq!(eval("REF > 40"), Some(1.0));
        assert_eq!(eval("REF > 50"), Some(0.0));
        assert_eq!(eval("REF > 40 && CC > 0.95"), Some(1.0));
        assert_eq!(eval("REF > 40 && CC > 0.99"), Some(0.0));
        assert_eq!(eval("REF > 90 || CC > 0.9"), Some(1.0));
        assert_eq!(eval("!(REF > 90)"), Some(1.0));
    }

    #[test]
    fn ternary_selects_a_branch() {
        assert_eq!(eval("REF > 40 ? REF : 0"), Some(45.0));
        assert_eq!(eval("REF > 90 ? REF : 0"), Some(0.0));
        // Right-associative: a ? b : (c ? d : e).
        assert_eq!(eval("0 ? 1 : 1 ? 2 : 3"), Some(2.0));
    }

    #[test]
    fn functions_min_max_mean_clamp_abs() {
        assert_eq!(eval("max(REF, 50)"), Some(50.0));
        assert_eq!(eval("min(REF, 50)"), Some(45.0));
        assert_eq!(eval("mean(REF, 50)"), Some(47.5));
        assert_eq!(eval("mean(REF, 50, 55)"), Some(50.0));
        assert_eq!(eval("clamp(REF, 0, 40)"), Some(40.0));
        assert_eq!(eval("abs(VEL)"), Some(12.0));
    }

    #[test]
    fn identifiers_are_case_insensitive() {
        assert_eq!(eval("ref"), Some(45.0));
        assert_eq!(eval("Ref + vel"), Some(33.0));
    }

    #[test]
    fn a_missing_input_makes_the_whole_expression_missing() {
        let mut i = inputs();
        i.reflectivity = None;
        let expr = parse("REF + 1").unwrap();
        assert_eq!(evaluate(&expr, &i), None);
        // A missing input under a function is missing too, not silently skipped.
        let expr = parse("max(REF, 10)").unwrap();
        assert_eq!(evaluate(&expr, &i), None);
        let expr = parse("mean(REF, 10, 20)").unwrap();
        assert_eq!(evaluate(&expr, &i), None);
    }

    #[test]
    fn geometry_inputs_are_readable() {
        assert_eq!(eval("RANGE_KM"), Some(80.0));
        assert_eq!(eval("AZIMUTH_DEG"), Some(270.0));
        assert_eq!(eval("ELEVATION_DEG"), Some(0.5));
        assert_eq!(eval("BEAM_HEIGHT_M"), Some(1200.0));
        assert_eq!(eval("BEAM_ALTITUDE_M"), Some(1500.0));
    }

    #[test]
    fn an_unknown_identifier_is_a_parse_error_not_a_silent_zero() {
        let err = parse("BANANA + 1").unwrap_err();
        assert!(err.message.contains("BANANA"), "message: {}", err.message);
    }

    #[test]
    fn wrong_arity_is_a_parse_error() {
        let err = parse("max(REF)").unwrap_err();
        assert!(err.message.contains("2 arguments"), "message: {}", err.message);
        let err = parse("clamp(REF, 0)").unwrap_err();
        assert!(err.message.contains("3 arguments"), "message: {}", err.message);
        let err = parse("mean(REF)").unwrap_err();
        assert!(err.message.contains("2 to 8 arguments"), "message: {}", err.message);
        assert!(parse("mean(1,2,3,4,5,6,7,8,9)").is_err());
    }

    #[test]
    fn unknown_function_is_a_parse_error() {
        let err = parse("banana(REF)").unwrap_err();
        assert!(err.message.contains("banana"), "message: {}", err.message);
    }

    #[test]
    fn unbalanced_parens_are_a_parse_error_not_a_panic() {
        assert!(parse("(REF + 1").is_err());
        assert!(parse("REF + 1)").is_err());
        assert!(parse("").is_err());
        assert!(parse("REF +").is_err());
        assert!(parse("REF REF").is_err());
    }

    #[test]
    fn a_bad_number_literal_is_a_parse_error() {
        assert!(parse("1.2.3").is_err());
    }

    #[test]
    fn an_unknown_character_is_a_parse_error() {
        assert!(parse("REF ~ 1").is_err());
        assert!(parse("REF = 1").is_err());
        assert!(parse("REF & 1").is_err());
        assert!(parse("REF | 1").is_err());
    }

    #[test]
    fn division_by_zero_is_infinity_not_a_panic() {
        // Floats saturate to +/-inf on divide-by-zero rather than panicking; a formula that hits
        // this reads a very large or NaN-like number rather than crashing the app either way.
        assert_eq!(eval("REF / 0"), Some(f32::INFINITY));
    }

    #[test]
    fn product_def_compiles_its_own_expression() {
        let def = ProductDef {
            name: "Hail signature".into(),
            units: "dBZ".into(),
            expression: "REF > 55 && ZDR < 1 ? REF : 0".into(),
        };
        let expr = def.compile().unwrap();
        assert_eq!(evaluate(&expr, &inputs()), Some(0.0), "this gate's ZDR is 2.5, not < 1");
    }

    #[test]
    fn product_def_reports_its_own_syntax_error() {
        let def = ProductDef {
            name: "broken".into(),
            units: "".into(),
            expression: "REF +".into(),
        };
        assert!(def.compile().is_err());
    }

    #[test]
    fn deeply_nested_parens_are_a_parse_error_not_a_stack_overflow() {
        // A hand-typed formula won't nest this deep, but a pasted or generated one might. Found
        // by this exact test: the first version of the parser had no depth limit and crashed the
        // process (stack overflow) on input like this rather than returning a `ParseError`.
        let src = format!("{}1{}", "(".repeat(2_000), ")".repeat(2_000));
        let err = parse(&src).unwrap_err();
        assert!(err.message.contains("nested too deeply"), "message: {}", err.message);
    }

    #[test]
    fn ordinary_nesting_well_under_the_limit_still_works() {
        let src = format!("{}1{}", "(".repeat(10), ")".repeat(10));
        assert_eq!(evaluate(&parse(&src).unwrap(), &inputs()), Some(1.0));
    }
}
