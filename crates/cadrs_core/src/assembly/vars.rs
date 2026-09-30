//! Variables in an assembly (P3F.4; `intro-to-assemblies.md` A1.8 "Variable table",
//! `intro-to-parametric-cad.md` P5.2): Variable features in the Mate Features list
//! ([`MateKind::Variable`]) and mate offsets typed as expressions naming them
//! ([`MateFeature::exprs`]). As in a Part Studio, a mate sees the variables above it; one naming a
//! variable below it, or none, is reported by [`check`].
//!
//! [`refresh`] re-evaluates the variables top to bottom and writes every offset expression's value
//! into its mate. The mate commands run it ([`super::commands::AddMateFeature`],
//! [`super::commands::SetMateFeature`]); the app solves the placements for the refreshed mates
//! and passes them in (see `variables_ui` in the app).

use cadrs_sketch::units::{self, Quantity, Units, VarValue};

use super::Assembly;
use super::mate::{MateId, MateKind, OffsetSlot};
use crate::variables::eval_variable;

fn slot_quantity(s: OffsetSlot) -> Quantity {
    if s == OffsetSlot::Angle { Quantity::Angle } else { Quantity::Length }
}

/// The assembly's variables, top to bottom, each evaluated with those above it.
pub fn defined(asm: &Assembly, units: &Units) -> Vec<(String, VarValue)> {
    let mut env: Vec<(String, VarValue)> = Vec::new();
    for f in asm.mates.iter().filter(|f| !f.suppressed) {
        if let MateKind::Variable(v) = &f.kind
            && v.problem().is_none()
        {
            let value = eval_variable(v.var_type, &v.expr, units, &env)
                .map(|(value, kind)| VarValue { value, quantity: kind.quantity() })
                .unwrap_or_else(|_| v.var_value());
            env.push((v.name.clone(), value));
        }
    }
    env
}

/// Re-evaluates the variables and the offsets typed as expressions naming them, writing the
/// values (translation mm, rotation radians). Returns whether anything changed.
pub fn refresh(asm: &mut Assembly, units: &Units) -> bool {
    if !asm.mates.iter().any(|f| matches!(f.kind, MateKind::Variable(_)) || !f.exprs.is_empty()) {
        return false;
    }
    let mut changed = false;
    let mut env: Vec<(String, VarValue)> = Vec::new();
    for f in asm.mates.iter_mut().filter(|f| !f.suppressed) {
        let exprs = f.exprs.clone();
        match &mut f.kind {
            MateKind::Variable(v) if v.problem().is_none() => {
                let before = (v.value, v.kind);
                if v.evaluate(units, &env).is_ok() {
                    changed |= before != (v.value, v.kind);
                }
                env.push((v.name.clone(), v.var_value()));
            }
            MateKind::Mate(m) => {
                let Some(o) = &mut m.offset else { continue };
                for (slot, e) in &exprs {
                    let Ok(v) = units.eval_vars(e, slot_quantity(*slot), &env) else { continue };
                    let target = match slot {
                        OffsetSlot::X => &mut o.translation[0],
                        OffsetSlot::Y => &mut o.translation[1],
                        OffsetSlot::Z => &mut o.translation[2],
                        OffsetSlot::Angle => &mut o.angle,
                    };
                    let v = if *slot == OffsetSlot::Angle { v.to_radians() } else { v };
                    if (*target - v).abs() > 1e-12 {
                        *target = v;
                        changed = true;
                    }
                }
            }
            _ => {}
        }
    }
    changed
}

/// The mate features whose expressions can't be evaluated where they are: a variable named
/// above its definition, or not defined at all.
pub fn check(asm: &Assembly, units: &Units) -> Vec<(MateId, String)> {
    let mut out = Vec::new();
    let mut env: Vec<(String, VarValue)> = Vec::new();
    let live: Vec<&super::mate::MateFeature> = asm.mates.iter().filter(|f| !f.suppressed).collect();
    for (i, f) in live.iter().enumerate() {
        let later = |n: &str| live[i + 1..].iter().any(|g| matches!(&g.kind, MateKind::Variable(v) if v.name == n));
        let texts: Vec<(&str, String, Quantity)> = match &f.kind {
            MateKind::Variable(v) => vec![("Value", v.expr.clone(), Quantity::Count)],
            _ => f.exprs.iter().map(|(s, e)| ("Offset", e.clone(), slot_quantity(*s))).collect(),
        };
        let mut why = None;
        for (label, e, q) in texts {
            if let Some(n) = units::variable_names(&e).into_iter().find(|n| !env.iter().any(|(k, _)| k == n)) {
                why = Some(if later(&n) {
                    format!("{label}: #{n} is used before it is defined; move its Variable above this feature")
                } else {
                    format!("{label}: #{n} is not defined")
                });
                break;
            }
            if q != Quantity::Count
                && let Err(err) = units.eval_vars(&e, q, &env)
            {
                why = Some(format!("{label}: {err}"));
                break;
            }
        }
        if let Some(w) = why {
            out.push((f.id, w));
        }
        if let MateKind::Variable(v) = &f.kind
            && v.problem().is_none()
        {
            let value = eval_variable(v.var_type, &v.expr, units, &env)
                .map(|(value, kind)| VarValue { value, quantity: kind.quantity() })
                .unwrap_or_else(|_| v.var_value());
            env.push((v.name.clone(), value));
        }
    }
    out
}
