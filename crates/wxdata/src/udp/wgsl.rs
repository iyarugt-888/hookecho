//! A user-defined product's formula as a WGSL compute shader (ROADMAP_NEW C1, "optional
//! AST-to-WGSL generation for parallel products"; 1010.md V17).
//!
//! The shader computes exactly what [`super::evaluate`] does, one gate per invocation. Each node
//! of the formula becomes a value and a "present" flag, and missing values follow the CPU's
//! rules:
//! - anything computed from a missing value is missing;
//! - a result that is not a finite number is missing;
//! - a zero divisor is missing;
//! - `clamp` with its bounds the wrong way round is missing;
//! - `if_missing` and `is_missing` read the flag itself.
//!
//! Both branches of a ternary are computed and the condition selects one. Nothing has side
//! effects, so that matches the CPU's evaluation of only the chosen branch.
//!
//! Results agree with the CPU to about 1 part in 100,000, not to the last bit: WGSL division
//! need only be within 2.5 units in the last place. A GPU compiler may also fold constants
//! together across an operation, which IEEE arithmetic would not. `x * 1e30 * 1e10` is then `x`
//! times infinity, missing even where `x` is 0. That only matters when an intermediate value
//! passes f32's 3.4e38, which no radar formula comes near.
//!
//! Only per-gate formulas translate. A formula with a vertical or layer function reads a whole
//! column, and [`super::Expr::to_wgsl`] returns `None` for it; it stays on the CPU.
//!
//! The data layout, which [`pack`] builds:
//! - `values`: each gate's inputs in [`GpuProgram::inputs`] order, `stride` of them per gate;
//! - `present`: a bit per input, set where that input has a value at the gate;
//! - `out_value` and `out_present`: the result, and whether there is one.
//!
//! Workgroups are 64 wide, dispatched in two dimensions so a whole volume fits under the
//! 65,535-per-dimension limit; [`GpuProgram::dispatch`] gives the size.

use super::{BinOp, Expr, ExprNode, Func, GateInputs, Input};
use std::fmt::Write as _;

/// Gates per workgroup.
pub const WORKGROUP: u32 = 64;
/// Most workgroups along one dispatch dimension.
const MAX_GROUPS_X: u32 = 65_535;

/// A formula translated for the GPU.
#[derive(Debug, Clone, PartialEq)]
pub struct GpuProgram {
    pub wgsl: String,
    /// The inputs the formula reads, in the order each gate's values are packed.
    pub inputs: Vec<Input>,
}

impl GpuProgram {
    /// Values per gate in the `values` buffer (at least one, so the buffer is never empty).
    pub fn stride(&self) -> usize {
        self.inputs.len().max(1)
    }

    /// Workgroups to dispatch for `gates` gates, as `(x, y)`.
    pub fn dispatch(gates: usize) -> (u32, u32) {
        let groups = (gates as u64).div_ceil(u64::from(WORKGROUP)).max(1);
        let x = groups.min(u64::from(MAX_GROUPS_X));
        (x as u32, groups.div_ceil(x) as u32)
    }

    /// `values` and `present` for `gates`, as the shader reads them.
    pub fn pack(&self, gates: &[GateInputs]) -> (Vec<f32>, Vec<u32>) {
        let stride = self.stride();
        let mut values = vec![0.0f32; gates.len() * stride];
        let mut present = vec![0u32; gates.len()];
        for (i, g) in gates.iter().enumerate() {
            for (k, input) in self.inputs.iter().enumerate() {
                if let Some(v) = g.get(*input) {
                    values[i * stride + k] = v;
                    present[i] |= 1 << k;
                }
            }
        }
        (values, present)
    }
}

impl Expr {
    /// This formula as a compute shader, or `None` when it reads a column (a vertical or layer
    /// function), which only the CPU evaluates.
    pub fn to_wgsl(&self) -> Option<GpuProgram> {
        if self.uses_column() {
            return None;
        }
        let inputs = self.inputs();
        // One bit each in `present`.
        if inputs.len() > 32 {
            return None;
        }
        let mut body = String::new();
        let mut next = 0usize;
        let root = emit(&self.0, &inputs, &mut body, &mut next)?;
        let wgsl = format!(
            "{HEADER}\
             @compute @workgroup_size({WORKGROUP})\n\
             fn main(@builtin(global_invocation_id) id: vec3<u32>, \
             @builtin(num_workgroups) groups: vec3<u32>) {{\n\
             \x20   let i = id.y * groups.x * {WORKGROUP}u + id.x;\n\
             \x20   if (i >= params.count) {{ return; }}\n\
             \x20   let base = i * params.stride;\n\
             \x20   let mask = present[i];\n\
             {body}\
             \x20   out_value[i] = select(0.0, v{root}, ok{root});\n\
             \x20   out_present[i] = select(0u, 1u, ok{root});\n\
             }}\n"
        );
        Some(GpuProgram { wgsl, inputs })
    }
}

const HEADER: &str = "struct Params { count: u32, stride: u32 }
@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> values: array<f32>;
@group(0) @binding(2) var<storage, read> present: array<u32>;
@group(0) @binding(3) var<storage, read_write> out_value: array<f32>;
@group(0) @binding(4) var<storage, read_write> out_present: array<u32>;

// Not infinite and not NaN: the exponent bits are not all set.
fn finite(x: f32) -> bool { return (bitcast<u32>(x) & 0x7f800000u) != 0x7f800000u; }
fn truthy(x: f32) -> bool { return x != 0.0; }
fn b2f(b: bool) -> f32 { return select(0.0, 1.0, b); }

";

/// A WGSL `f32` literal for `n`, or `None` for one that is not finite.
fn literal(n: f32) -> Option<String> {
    n.is_finite().then(|| format!("{n:e}f"))
}

/// Emit `node` as `let vN` and `let okN` lines into `out`; returns `N`.
fn emit(node: &ExprNode, inputs: &[Input], out: &mut String, next: &mut usize) -> Option<usize> {
    // The value and the presence expressions, before the finiteness check every node gets.
    let (value, ok) = match node {
        ExprNode::Number(n) => (literal(*n)?, "true".to_string()),
        ExprNode::Var(input) => {
            let k = inputs.iter().position(|i| i == input)?;
            (
                format!("values[base + {k}u]"),
                format!("((mask >> {k}u) & 1u) != 0u"),
            )
        }
        ExprNode::Neg(a) => {
            let a = emit(a, inputs, out, next)?;
            (format!("-v{a}"), format!("ok{a}"))
        }
        ExprNode::Not(a) => {
            let a = emit(a, inputs, out, next)?;
            (format!("b2f(!truthy(v{a}))"), format!("ok{a}"))
        }
        ExprNode::Bin(op, a, b) => {
            let (a, b) = (emit(a, inputs, out, next)?, emit(b, inputs, out, next)?);
            let both = format!("ok{a} && ok{b}");
            match op {
                BinOp::Add => (format!("v{a} + v{b}"), both),
                BinOp::Sub => (format!("v{a} - v{b}"), both),
                BinOp::Mul => (format!("v{a} * v{b}"), both),
                // The CPU's x / 0 is infinite or NaN, so missing; a GPU need not produce either,
                // so the divisor is checked instead.
                BinOp::Div => (format!("v{a} / v{b}"), format!("{both} && v{b} != 0.0")),
                BinOp::Lt => (format!("b2f(v{a} < v{b})"), both),
                BinOp::Le => (format!("b2f(v{a} <= v{b})"), both),
                BinOp::Gt => (format!("b2f(v{a} > v{b})"), both),
                BinOp::Ge => (format!("b2f(v{a} >= v{b})"), both),
                BinOp::Eq => (format!("b2f(v{a} == v{b})"), both),
                BinOp::Ne => (format!("b2f(v{a} != v{b})"), both),
                BinOp::And => (format!("b2f(truthy(v{a}) && truthy(v{b}))"), both),
                BinOp::Or => (format!("b2f(truthy(v{a}) || truthy(v{b}))"), both),
            }
        }
        ExprNode::Call(Func::IfMissing, args) => {
            let (x, f) = (
                emit(&args[0], inputs, out, next)?,
                emit(&args[1], inputs, out, next)?,
            );
            (
                format!("select(v{f}, v{x}, ok{x})"),
                format!("ok{x} || ok{f}"),
            )
        }
        ExprNode::Call(Func::IsMissing, args) => {
            let x = emit(&args[0], inputs, out, next)?;
            (format!("b2f(!ok{x})"), "true".to_string())
        }
        ExprNode::Call(func, args) => {
            let ids: Vec<usize> = args
                .iter()
                .map(|a| emit(a, inputs, out, next))
                .collect::<Option<_>>()?;
            let all = ids
                .iter()
                .map(|k| format!("ok{k}"))
                .collect::<Vec<_>>()
                .join(" && ");
            match (func, ids.as_slice()) {
                (Func::Min, [a, b]) => (format!("min(v{a}, v{b})"), all),
                (Func::Max, [a, b]) => (format!("max(v{a}, v{b})"), all),
                (Func::Mean, ids) => {
                    let sum = ids
                        .iter()
                        .map(|k| format!("v{k}"))
                        .collect::<Vec<_>>()
                        .join(" + ");
                    (format!("({sum}) / {}.0", ids.len()), all)
                }
                (Func::Clamp, [x, lo, hi]) => (
                    format!("clamp(v{x}, v{lo}, v{hi})"),
                    format!("{all} && v{lo} <= v{hi}"),
                ),
                (Func::Abs, [x]) => (format!("abs(v{x})"), all),
                // Column functions were turned away above; nothing else parses.
                _ => return None,
            }
        }
        ExprNode::Ternary(c, a, b) => {
            let (c, a, b) = (
                emit(c, inputs, out, next)?,
                emit(a, inputs, out, next)?,
                emit(b, inputs, out, next)?,
            );
            (
                format!("select(v{b}, v{a}, truthy(v{c}))"),
                format!("ok{c} && select(ok{b}, ok{a}, truthy(v{c}))"),
            )
        }
    };
    let n = *next;
    *next += 1;
    let _ = writeln!(out, "    let v{n}: f32 = {value};");
    let _ = writeln!(out, "    let ok{n}: bool = ({ok}) && finite(v{n});");
    Some(n)
}

#[cfg(test)]
mod tests {
    use super::super::parse;
    use super::*;

    #[test]
    fn a_column_formula_stays_on_the_cpu() {
        assert!(parse("max_vertical(REF)").unwrap().to_wgsl().is_none());
        assert!(parse("REF > 40 ? max_layer(REF, 0, 3000) : 0")
            .unwrap()
            .to_wgsl()
            .is_none());
    }

    #[test]
    fn a_gate_formula_reads_only_its_own_inputs_in_order() {
        let p = parse("ZDR < 1 && REF > 55 ? REF : if_missing(KDP, 0)")
            .unwrap()
            .to_wgsl()
            .unwrap();
        assert_eq!(p.inputs.len(), 3);
        assert!(p.wgsl.contains("@compute @workgroup_size(64)"));
        let gate = GateInputs {
            reflectivity: Some(60.0),
            differential_reflectivity: Some(0.5),
            ..Default::default()
        };
        let (values, present) = p.pack(&[gate, GateInputs::default()]);
        assert_eq!(values.len(), 2 * p.stride());
        let k_ref = p
            .inputs
            .iter()
            .position(|i| *i == Input::Reflectivity)
            .unwrap();
        assert_eq!(values[k_ref], 60.0);
        assert_eq!(present[1], 0);
        assert_eq!(present[0].count_ones(), 2);
    }

    #[test]
    fn a_whole_volume_dispatches_under_the_limit() {
        assert_eq!(GpuProgram::dispatch(0), (1, 1));
        assert_eq!(GpuProgram::dispatch(64), (1, 1));
        assert_eq!(GpuProgram::dispatch(65), (2, 1));
        let (x, y) = GpuProgram::dispatch(18_000_000);
        assert!(x <= MAX_GROUPS_X && u64::from(x) * u64::from(y) * 64 >= 18_000_000);
    }
}
