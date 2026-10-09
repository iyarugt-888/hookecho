//! Operand units and height datums for user-defined product formulas (1008.md C4, M3.2).
//!
//! Every input has a physical quantity: reflectivity in dBZ, ZDR in dB, velocity and spectrum
//! width in m/s, KDP in °/km, CC a ratio, range in km, angles in degrees, and heights in metres
//! measured from one of two datums — above the radar antenna (`BEAM_HEIGHT_M`, and every column
//! function's height) or above mean sea level (`BEAM_ALTITUDE_M` and the isotherm heights).
//! [`check`] walks a formula and reports, in plain words, each place it adds, subtracts, compares,
//! takes the min/max/mean of, or chooses between two different quantities, and each place it
//! mixes the two height datums — including an MSL height used as a layer bound, which column
//! functions read as above the antenna. Numbers take whatever unit they meet; a product or ratio
//! of two quantities becomes a derived unit that is not checked further. Nothing is refused: a
//! diagnostic is advice the editor and an import show beside the product.

use super::{BinOp, ExprNode, Func, Input};

/// What a sub-expression measures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quantity {
    Dbz,
    Db,
    MetresPerSecond,
    DegreesPerKm,
    Ratio,
    Km,
    Degrees,
    MetresAboveAntenna,
    MetresMsl,
    /// A plain number (or a truth value): takes the unit of what it meets.
    Number,
    /// A product, ratio or integral of quantities: not checked further.
    Derived,
}

impl Quantity {
    fn of(input: Input) -> Self {
        match input {
            Input::Reflectivity => Self::Dbz,
            Input::DifferentialReflectivity => Self::Db,
            Input::Velocity | Input::SpectrumWidth => Self::MetresPerSecond,
            Input::SpecificDifferentialPhase => Self::DegreesPerKm,
            Input::CorrelationCoefficient => Self::Ratio,
            Input::RangeKm => Self::Km,
            Input::AzimuthDeg | Input::ElevationDeg => Self::Degrees,
            Input::BeamHeightM => Self::MetresAboveAntenna,
            Input::BeamAltitudeM
            | Input::FreezingLevelM
            | Input::Minus10cHeightM
            | Input::Minus20cHeightM
            | Input::Minus30cHeightM
            | Input::Minus40cHeightM => Self::MetresMsl,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Dbz => "dBZ",
            Self::Db => "dB",
            Self::MetresPerSecond => "m/s",
            Self::DegreesPerKm => "°/km",
            Self::Ratio => "a ratio (CC)",
            Self::Km => "km",
            Self::Degrees => "degrees",
            Self::MetresAboveAntenna => "metres above the antenna",
            Self::MetresMsl => "metres above sea level",
            Self::Number => "a plain number",
            Self::Derived => "a derived unit",
        }
    }

    /// Whether `self` and `other` can be added or compared as they are.
    fn agrees(self, other: Self) -> bool {
        self == other
            || matches!(self, Self::Number | Self::Derived)
            || matches!(other, Self::Number | Self::Derived)
    }

    fn is_height(self) -> bool {
        matches!(self, Self::MetresAboveAntenna | Self::MetresMsl)
    }
}

/// One finding, worded for the analyst.
fn mismatch(what: &str, a: Quantity, b: Quantity) -> String {
    if a.is_height() && b.is_height() {
        format!(
            "{what} metres above the antenna with metres above sea level: they differ by the \
             antenna's altitude (use BEAM_ALTITUDE_M against isotherm heights)"
        )
    } else {
        format!("{what} {} with {}", a.label(), b.label())
    }
}

/// What a formula's value measures (a plain number for a truth value or a count).
pub(super) fn result(node: &ExprNode) -> Quantity {
    walk(node, &mut Vec::new())
}

impl Quantity {
    /// The quantity a product's units label names, when it names one this module knows.
    pub fn from_label(label: &str) -> Option<Self> {
        Some(match label.trim().to_ascii_lowercase().as_str() {
            "dbz" => Self::Dbz,
            "db" => Self::Db,
            "m/s" | "m s-1" | "ms-1" => Self::MetresPerSecond,
            "deg/km" | "\u{b0}/km" => Self::DegreesPerKm,
            "km" => Self::Km,
            "deg" | "degrees" | "\u{b0}" => Self::Degrees,
            _ => return None,
        })
    }
}

/// A note when a product's units label names a different quantity than its formula computes.
pub fn label_note(label: &str, computed: Quantity) -> Option<String> {
    let named = Quantity::from_label(label)?;
    (!named.agrees(computed)).then(|| {
        format!(
            "Labelled {} but the formula computes {}",
            label.trim(),
            computed.label()
        )
    })
}

/// The diagnostics for a formula, in the order the formula reads, each once.
pub(super) fn check(node: &ExprNode) -> Vec<String> {
    let mut out = Vec::new();
    walk(node, &mut out);
    let mut seen = std::collections::HashSet::new();
    out.retain(|d| seen.insert(d.clone()));
    out
}

/// The unit of `a` and `b` combined where they must agree; the concrete one of the two.
fn join(a: Quantity, b: Quantity) -> Quantity {
    match (a, b) {
        (Quantity::Number, x) | (x, Quantity::Number) => x,
        (Quantity::Derived, _) | (_, Quantity::Derived) => Quantity::Derived,
        (x, y) if x == y => x,
        _ => Quantity::Derived,
    }
}

fn walk(node: &ExprNode, out: &mut Vec<String>) -> Quantity {
    match node {
        ExprNode::Number(_) => Quantity::Number,
        ExprNode::Var(i) => Quantity::of(*i),
        ExprNode::Neg(x) => walk(x, out),
        ExprNode::Not(x) => {
            walk(x, out);
            Quantity::Number
        }
        ExprNode::Bin(op, a, b) => {
            let (qa, qb) = (walk(a, out), walk(b, out));
            match op {
                BinOp::Add | BinOp::Sub => {
                    if !qa.agrees(qb) {
                        let verb = if *op == BinOp::Add {
                            "Adds"
                        } else {
                            "Subtracts"
                        };
                        out.push(mismatch(verb, qa, qb));
                    }
                    join(qa, qb)
                }
                // Scaling a quantity by a coefficient is how an index weighs its terms
                // (`(REF-50)/2 + (1-CC)*100`), so a scaled quantity is not checked further.
                BinOp::Mul => match (qa, qb) {
                    (Quantity::Number, Quantity::Number) => Quantity::Number,
                    _ => Quantity::Derived,
                },
                BinOp::Div => match (qa, qb) {
                    (x, y) if x == y && x != Quantity::Derived => Quantity::Number,
                    _ => Quantity::Derived,
                },
                BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge | BinOp::Eq | BinOp::Ne => {
                    if !qa.agrees(qb) {
                        out.push(mismatch("Compares", qa, qb));
                    }
                    Quantity::Number
                }
                BinOp::And | BinOp::Or => Quantity::Number,
            }
        }
        ExprNode::Ternary(c, a, b) => {
            walk(c, out);
            let (qa, qb) = (walk(a, out), walk(b, out));
            if !qa.agrees(qb) {
                out.push(mismatch("Chooses between", qa, qb));
            }
            join(qa, qb)
        }
        ExprNode::Call(f, args) => call(*f, args, out),
    }
}

fn call(f: Func, args: &[ExprNode], out: &mut Vec<String>) -> Quantity {
    let q: Vec<Quantity> = args.iter().map(|a| walk(a, out)).collect();
    let first = q.first().copied().unwrap_or(Quantity::Number);
    let same = |out: &mut Vec<String>, what: &str, qs: &[Quantity]| {
        let mut acc = Quantity::Number;
        for &x in qs {
            if !acc.agrees(x) {
                out.push(mismatch(what, acc, x));
            }
            acc = join(acc, x);
        }
        acc
    };
    // Layer bounds are read against each tilt's height above the antenna.
    let bounds = |out: &mut Vec<String>, qs: &[Quantity]| {
        for &x in qs {
            if x == Quantity::MetresMsl {
                out.push(
                    "Uses a height above sea level as a layer bound: column functions read \
                     heights above the antenna, so the layer is off by the antenna's altitude"
                        .to_string(),
                );
            } else if !Quantity::MetresAboveAntenna.agrees(x) {
                out.push(format!(
                    "Uses {} as a layer bound, which is a height above the antenna in metres",
                    x.label()
                ));
            }
        }
    };
    match f {
        Func::Min | Func::Max | Func::Mean | Func::Clamp => same(out, "Mixes", &q),
        Func::Abs => first,
        Func::MaxVertical | Func::MinVertical | Func::MeanVertical => first,
        Func::MaxLayer | Func::MinLayer | Func::MeanLayer => {
            bounds(out, q.get(1..3).unwrap_or(&[]));
            first
        }
        Func::IntegralLayer => {
            bounds(out, q.get(1..3).unwrap_or(&[]));
            Quantity::Derived
        }
        Func::FirstCrossingHeight
        | Func::LastCrossingHeight
        | Func::FirstHeightAbove
        | Func::LastHeightAbove => {
            if let [a, b, ..] = q[..] {
                if !a.agrees(b) {
                    out.push(mismatch("Compares", a, b));
                }
            }
            Quantity::MetresAboveAntenna
        }
        Func::MaxHeight | Func::MinHeight => Quantity::MetresAboveAntenna,
        Func::CountAbove => {
            if let [a, b, ..] = q[..] {
                if !a.agrees(b) {
                    out.push(mismatch("Compares", a, b));
                }
            }
            Quantity::Number
        }
        Func::CountVertical | Func::FractionVertical => Quantity::Number,
    }
}

#[cfg(test)]
mod tests {
    use crate::udp::parse;

    fn diag(src: &str) -> Vec<String> {
        parse(src).unwrap().unit_diagnostics()
    }

    #[test]
    fn consistent_formulas_have_nothing_to_say() {
        for ok in [
            "REF >= 50 && ZDR <= 1 && CC <= 0.97 ? REF : 0/0",
            "clamp((REF-50)/2 + (1-CC)*100 + (1-ZDR)*4, 0, 40)",
            "BEAM_ALTITUDE_M >= MINUS10C_HEIGHT_M && REF >= 45 ? REF : 0/0",
            "max_layer(REF, 0, 3000)",
            "first_height_above(REF, 50)",
            "max_vertical(REF, BEAM_ALTITUDE_M >= FREEZING_LEVEL_M) - 10",
            "abs(VEL) / SW",
            "count_above(REF, 45) * 2",
        ] {
            assert!(diag(ok).is_empty(), "{ok}: {:?}", diag(ok));
        }
    }

    #[test]
    fn a_units_label_that_names_another_quantity_is_noted() {
        use super::{label_note, Quantity};
        let q = |src: &str| parse(src).unwrap().result_quantity();
        assert_eq!(q("REF >= 50 ? REF : 0/0"), Quantity::Dbz);
        assert_eq!(q("abs(VEL)"), Quantity::MetresPerSecond);
        assert_eq!(
            q("first_height_above(REF, 50)"),
            Quantity::MetresAboveAntenna
        );
        assert_eq!(
            label_note("dBZ", q("abs(VEL)")).as_deref(),
            Some("Labelled dBZ but the formula computes m/s")
        );
        assert_eq!(label_note("dBZ", q("REF")), None);
        assert_eq!(
            label_note("index", q("abs(VEL)")),
            None,
            "an unknown label says nothing"
        );
        assert_eq!(
            label_note("m/s", q("REF / 2")),
            None,
            "a scaled value is not checked"
        );
    }

    #[test]
    fn mixed_quantities_and_datums_are_named() {
        assert_eq!(diag("REF + ZDR"), ["Adds dBZ with dB"]);
        assert_eq!(diag("VEL > REF ? 1 : 0"), ["Compares m/s with dBZ"]);
        assert_eq!(
            diag("CC > 0.9 ? REF : VEL"),
            ["Chooses between dBZ with m/s"]
        );
        assert_eq!(diag("max(REF, KDP)"), ["Mixes dBZ with °/km"]);
        let datum = diag("BEAM_HEIGHT_M >= FREEZING_LEVEL_M ? REF : 0/0");
        assert_eq!(datum.len(), 1);
        assert!(
            datum[0].contains("differ by the antenna's altitude"),
            "{datum:?}"
        );
        let layer = diag("max_layer(REF, FREEZING_LEVEL_M, MINUS20C_HEIGHT_M)");
        assert_eq!(layer.len(), 1, "one finding however many bounds: {layer:?}");
        assert!(layer[0].contains("layer bound"));
        assert_eq!(
            diag("max_layer(REF, 0, RANGE_KM)"),
            ["Uses km as a layer bound, which is a height above the antenna in metres"]
        );
        // A height of a column extremum is above the antenna too.
        assert_eq!(
            diag("first_height_above(REF, 50) > FREEZING_LEVEL_M").len(),
            1
        );
    }
}
