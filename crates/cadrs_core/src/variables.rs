//! Variables (P3F.4; `intro-to-parametric-cad.md` P5.2, P5.3, X2): the **Variable** feature
//! (`#name = expression`, with a type and a description) and the expressions that name
//! variables in the other features' fields.
//!
//! - **Where expressions live.** Every numeric field of a feature keeps the text it was typed as
//!   next to the value it evaluated to (`depth_expr` beside `depth`, a hole's
//!   [`crate::hole::Length`], a pattern's `count_expr` beside `count`); a sketch keeps the
//!   expressions of its driving dimensions in [`cadrs_sketch::Sketch::expressions`]. [`slots`]
//!   visits them all.
//! - **Order.** Variables are features: a use sees only the variables above it in the list
//!   (after suppression and the rollback bar). [`check`] finds the uses that name a variable
//!   defined below them (**used before it is defined**) or nowhere, and the expressions that no
//!   longer evaluate; the rebuild fails those features with the reason (P3D.1 error states).
//! - **Propagation.** [`refresh`] evaluates the variables top to bottom and writes the value of
//!   every expression that names one, re-solving the sketches whose dimensions changed. The
//!   command layer runs it after each edit of a Part Studio ([`crate::commands::refresh_studio`]),
//!   so changing `#piston_d` in the Variable table is one undo step that updates every use.
//! - **Uses.** [`uses`] lists the variables a feature's expressions name (the feature list's
//!   `:variable` filter, Show dependencies).

use cadrs_sketch::units::{self, ParseError, Quantity, Units, VarValue};
use cadrs_sketch::SketchOp;
use serde::{Deserialize, Serialize};

use crate::document::{Feature, FeatureKind};
use crate::ids::FeatureId;

/// A Variable's type (Onshape's Length, Angle, Number and Any).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum VariableType {
    #[default]
    Length,
    Angle,
    Number,
    /// Whatever the expression gives.
    Any,
}

impl VariableType {
    pub const ALL: [VariableType; 4] = [VariableType::Length, VariableType::Angle, VariableType::Number, VariableType::Any];

    pub fn label(self) -> &'static str {
        match self {
            VariableType::Length => "Length",
            VariableType::Angle => "Angle",
            VariableType::Number => "Number",
            VariableType::Any => "Any",
        }
    }
}

/// What a variable's value measures, once evaluated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum VariableKind {
    /// Millimetres.
    #[default]
    Length,
    /// Degrees.
    Angle,
    Number,
}

impl VariableKind {
    pub fn quantity(self) -> Quantity {
        match self {
            VariableKind::Length => Quantity::Length,
            VariableKind::Angle => Quantity::Angle,
            VariableKind::Number => Quantity::Count,
        }
    }

    fn of(q: Quantity) -> Self {
        match q {
            Quantity::Length => VariableKind::Length,
            Quantity::Angle => VariableKind::Angle,
            Quantity::Count => VariableKind::Number,
        }
    }
}

/// A Variable feature: `#name = expr`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VariableFeature {
    /// The name, without the `#`.
    pub name: String,
    #[serde(default)]
    pub var_type: VariableType,
    /// The expression as typed ("40 mm", "#piston_d + 0.5 mm").
    pub expr: String,
    /// Its value when last evaluated: mm, degrees or a plain number.
    pub value: f64,
    /// What the value measures.
    #[serde(default)]
    pub kind: VariableKind,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
}

impl Default for VariableFeature {
    fn default() -> Self {
        Self {
            name: String::new(),
            var_type: VariableType::Length,
            expr: "0 mm".into(),
            value: 0.0,
            kind: VariableKind::Length,
            description: String::new(),
        }
    }
}

impl VariableFeature {
    /// A Length variable `#name = expr`, evaluated without other variables.
    pub fn length(name: &str, expr: &str) -> Self {
        let mut v = Self { name: name.into(), expr: expr.into(), ..Self::default() };
        let _ = v.evaluate(&Units::default(), &Vec::new());
        v
    }

    /// The value as expressions see it.
    pub fn var_value(&self) -> VarValue {
        VarValue { value: self.value, quantity: self.kind.quantity() }
    }

    /// Evaluates the expression with the variables above it (`vars`), setting the value.
    pub fn evaluate(&mut self, units: &Units, vars: &[(String, VarValue)]) -> Result<(), ParseError> {
        let (value, kind) = eval_variable(self.var_type, &self.expr, units, vars)?;
        self.value = value;
        self.kind = kind;
        Ok(())
    }

    /// Why it can't be defined, if it can't.
    pub fn problem(&self) -> Option<&'static str> {
        if self.name.is_empty() {
            return Some("Enter a variable name");
        }
        if !units::is_variable_name(&self.name) {
            return Some("A name is letters, digits and _ (starting with a letter)");
        }
        if self.expr.trim().is_empty() {
            return Some("Enter a value");
        }
        None
    }

    /// The value with its unit, as the Variable table shows it ("40.5 mm").
    pub fn display(&self, units: &Units) -> String {
        match self.kind {
            VariableKind::Length => format!("{} {}", units.value(self.value, Quantity::Length), units.length.symbol()),
            VariableKind::Angle => format!("{} deg", units.value(self.value, Quantity::Angle)),
            VariableKind::Number => {
                let s = format!("{:.6}", self.value);
                s.trim_end_matches('0').trim_end_matches('.').to_string()
            }
        }
    }
}

/// A variable's expression evaluated as its type asks: its value and what it measures.
pub fn eval_variable(t: VariableType, expr: &str, units: &Units, vars: &[(String, VarValue)]) -> Result<(f64, VariableKind), ParseError> {
    Ok(match t {
        VariableType::Length => (units.eval_vars(expr, Quantity::Length, &vars)?, VariableKind::Length),
        VariableType::Angle => (units.eval_vars(expr, Quantity::Angle, &vars)?, VariableKind::Angle),
        VariableType::Number => (units.eval_vars(expr, Quantity::Count, &vars)?, VariableKind::Number),
        VariableType::Any => {
            let v = units::eval_any(expr, units.length.mm(), &vars)?;
            (v.value, VariableKind::of(v.quantity))
        }
    })
}

/// One numeric field of a feature: its label, the text it was typed as, its value and what it
/// measures. Counts are carried as `f64` and written back rounded.
pub struct Slot<'a> {
    pub label: &'static str,
    pub expr: &'a mut String,
    pub value: &'a mut f64,
    pub quantity: Quantity,
}

/// Visits every numeric field of a feature that keeps its expression (not a sketch's
/// dimensions, see [`sketch_expressions`]).
pub fn slots(kind: &mut FeatureKind, f: &mut dyn FnMut(Slot<'_>)) {
    use Quantity::{Angle as A, Count as C, Length as L};
    let mut s = |label: &'static str, expr: &mut String, value: &mut f64, quantity: Quantity| {
        f(Slot { label, expr, value, quantity })
    };
    let count = |label: &'static str, expr: &mut String, n: &mut u32, s: &mut dyn FnMut(&'static str, &mut String, &mut f64, Quantity)| {
        let mut v = *n as f64;
        s(label, expr, &mut v, C);
        *n = v.round().max(0.0) as u32;
    };
    match kind {
        FeatureKind::Extrude(e) => {
            s("Depth", &mut e.depth_expr, &mut e.depth, L);
            if let Some(o) = &mut e.offset {
                s("Offset distance", &mut o.expr, &mut o.value, L);
            }
            if let Some(o) = &mut e.start_offset {
                s("Starting offset", &mut o.expr, &mut o.value, L);
            }
            if let Some(c) = &mut e.second {
                s("Second depth", &mut c.depth_expr, &mut c.depth, L);
                if let Some(o) = &mut c.offset {
                    s("Second offset distance", &mut o.expr, &mut o.value, L);
                }
            }
            s("Thickness 1", &mut e.thin.thickness1_expr, &mut e.thin.thickness1, L);
            s("Thickness 2", &mut e.thin.thickness2_expr, &mut e.thin.thickness2, L);
            if let Some(d) = &mut e.draft {
                s("Draft angle", &mut d.expr, &mut d.angle, A);
            }
        }
        FeatureKind::Revolve(r) => {
            s("Revolve angle", &mut r.angle_expr, &mut r.angle, A);
            if let Some(o) = &mut r.offset {
                s("Offset angle", &mut o.expr, &mut o.value, A);
            }
            if let Some(c) = &mut r.second {
                s("Second angle", &mut c.depth_expr, &mut c.depth, A);
                if let Some(o) = &mut c.offset {
                    s("Second offset angle", &mut o.expr, &mut o.value, A);
                }
            }
            s("Thickness 1", &mut r.thin.thickness1_expr, &mut r.thin.thickness1, L);
            s("Thickness 2", &mut r.thin.thickness2_expr, &mut r.thin.thickness2, L);
        }
        FeatureKind::Boolean(b) => {
            if let Some(o) = &mut b.offset {
                s("Offset distance", &mut o.expr, &mut o.distance, L);
            }
        }
        FeatureKind::Fillet(x) => {
            s("Radius", &mut x.size_expr, &mut x.size, L);
            s("Second radius", &mut x.second_expr, &mut x.second, L);
            for v in &mut x.vertices {
                s("Vertex radius", &mut v.expr, &mut v.radius, L);
            }
            for p in &mut x.edge_points {
                s("Point radius", &mut p.expr, &mut p.radius, L);
            }
            let q = if x.partial_bound == crate::applied::PartialBound::Length { L } else { C };
            s("First bound", &mut x.partial_first_expr, &mut x.partial_first, q);
            s("Second bound", &mut x.partial_second_expr, &mut x.partial_second, q);
        }
        FeatureKind::Chamfer(x) => {
            s("Distance", &mut x.distance_expr, &mut x.distance, L);
            s("Distance 2", &mut x.distance2_expr, &mut x.distance2, L);
            s("Angle", &mut x.angle_expr, &mut x.angle, A);
        }
        FeatureKind::Shell(x) => s("Thickness", &mut x.thickness_expr, &mut x.thickness, L),
        FeatureKind::Hole(h) => {
            let sp = &mut h.spec;
            for (label, l, q) in [
                ("Hole diameter", &mut sp.diameter, L),
                ("Hole depth", &mut sp.depth, L),
                ("Tip angle", &mut sp.tip_angle, A),
                ("Counterbore diameter", &mut sp.cbore_diameter, L),
                ("Counterbore depth", &mut sp.cbore_depth, L),
                ("Countersink diameter", &mut sp.csink_diameter, L),
                ("Countersink angle", &mut sp.csink_angle, A),
                ("Tapped depth", &mut sp.tapped_depth, L),
            ] {
                s(label, &mut l.expr, &mut l.value, q);
            }
            if let Some(l) = &mut sp.end_offset {
                s("Offset", &mut l.expr, &mut l.value, L);
            }
        }
        FeatureKind::Plane(x) => {
            s("Offset", &mut x.offset_expr, &mut x.offset, L);
            s("Angle", &mut x.angle_expr, &mut x.angle, A);
        }
        FeatureKind::Loft(x) => {
            s("Start magnitude", &mut x.start_magnitude_expr, &mut x.start_magnitude, C);
            s("End magnitude", &mut x.end_magnitude_expr, &mut x.end_magnitude, C);
        }
        FeatureKind::Pattern(x) => {
            s("Distance", &mut x.first.distance_expr, &mut x.first.distance, L);
            count("Instance count", &mut x.first.count_expr, &mut x.first.count, &mut s);
            s("Second distance", &mut x.second.distance_expr, &mut x.second.distance, L);
            count("Second instance count", &mut x.second.count_expr, &mut x.second.count, &mut s);
            s("Angle", &mut x.angle_expr, &mut x.angle, A);
        }
        FeatureKind::Draft(x) => s("Draft angle", &mut x.angle_expr, &mut x.angle, A),
        FeatureKind::Transform(x) => {
            s("Distance", &mut x.distance_expr, &mut x.distance, L);
            s("X distance", &mut x.dx_expr, &mut x.dx, L);
            s("Y distance", &mut x.dy_expr, &mut x.dy, L);
            s("Z distance", &mut x.dz_expr, &mut x.dz, L);
            s("Angle", &mut x.angle_expr, &mut x.angle, A);
            s("Scale", &mut x.scale_expr, &mut x.scale, C);
        }
        FeatureKind::MateConnector(x) => {
            for (i, label) in ["X translation", "Y translation", "Z translation"].into_iter().enumerate() {
                s(label, &mut x.offset_expr[i], &mut x.offset[i], L);
            }
            s("Rotation", &mut x.rotation_expr, &mut x.rotation, A);
        }
        FeatureKind::Thicken(x) => {
            s("Thickness 1", &mut x.thickness1_expr, &mut x.thickness1, L);
            s("Thickness 2", &mut x.thickness2_expr, &mut x.thickness2, L);
        }
        FeatureKind::Helix(x) => {
            s("Revolutions", &mut x.revolutions_expr, &mut x.revolutions, C);
            s("Pitch", &mut x.pitch_expr, &mut x.pitch, L);
            s("Height", &mut x.height_expr, &mut x.height, L);
            s("Radius", &mut x.radius_expr, &mut x.radius, L);
            s("Start angle", &mut x.start_angle_expr, &mut x.start_angle, A);
        }
        FeatureKind::SheetMetalModel(x) => {
            let e = &mut x.exprs;
            let p = &mut x.params;
            s("Thickness", &mut e.thickness, &mut p.thickness, L);
            s("Bend radius", &mut e.bend_radius, &mut p.bend_radius, L);
            s("Default bend K Factor", &mut e.k_factor, &mut p.k_factor, C);
            s("Rolled K Factor", &mut e.rolled_k_factor, &mut p.rolled_k_factor, C);
            s("Bend allowance", &mut e.bend_allowance, &mut p.bend_allowance, L);
            s("Bend deduction", &mut e.bend_deduction, &mut p.bend_deduction, L);
            s("Minimal gap", &mut e.minimal_gap, &mut p.minimal_gap, L);
            s("Corner relief scale", &mut e.corner_relief_scale, &mut p.corner_relief.scale, C);
            s("Corner relief size", &mut e.corner_relief_size, &mut p.corner_relief.size, L);
            s("Bend relief depth scale", &mut e.bend_relief_depth_scale, &mut p.bend_relief.depth_scale, C);
            s("Bend relief width scale", &mut e.bend_relief_width_scale, &mut p.bend_relief.width_scale, C);
            s("Clearance from input", &mut x.clearance_expr, &mut x.clearance, L);
            s("Depth", &mut x.depth_expr, &mut x.depth, L);
            if let Some(c) = &mut x.second {
                s("Second depth", &mut c.depth_expr, &mut c.depth, L);
            }
        }
        FeatureKind::SheetMetal(x) => {
            for (label, expr, value, angle) in x.exprs_mut() {
                s(label, expr, value, if angle { A } else { L });
            }
        }
        FeatureKind::Variable(_)
        | FeatureKind::Fill(_)
        | FeatureKind::Sketch(_)
        | FeatureKind::DeletePart(_)
        | FeatureKind::Sweep(_)
        | FeatureKind::Split(_)
        | FeatureKind::Mirror(_)
        | FeatureKind::Import(_)
        | FeatureKind::Derived(_)
        | FeatureKind::Composite(_) => {}
    }
}

/// A sketch's dimension expressions: (dimension, expression, what it measures).
pub fn sketch_expressions(g: &cadrs_sketch::Sketch) -> Vec<(cadrs_sketch::DimensionId, String, Quantity)> {
    g.expressions
        .iter()
        .filter_map(|(id, e)| Some((*id, e.clone(), g.dimensions.get(*id)?.kind.quantity())))
        .collect()
}

/// Every expression a feature holds that names a variable, with its label.
pub fn expressions(f: &Feature) -> Vec<(&'static str, String)> {
    match &f.kind {
        FeatureKind::Sketch(sk) => sketch_expressions(&sk.geometry)
            .into_iter()
            .map(|(_, e, _)| ("Dimension", e))
            .collect(),
        FeatureKind::Variable(v) => vec![("Value", v.expr.clone())],
        k => {
            let mut k = k.clone();
            let mut out = Vec::new();
            slots(&mut k, &mut |s| {
                if s.expr.contains('#') {
                    out.push((s.label, s.expr.clone()));
                }
            });
            out
        }
    }
}

/// The variables a feature's expressions name, in order (a Variable's own name not included).
pub fn uses(f: &Feature) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for (_, e) in expressions(f) {
        for n in units::variable_names(&e) {
            if !out.contains(&n) {
                out.push(n);
            }
        }
    }
    out
}

/// The variables defined by the features, top to bottom, each evaluated with those above it
/// (a later definition of a name wins below it).
pub fn defined(features: &[Feature], units: &Units) -> Vec<(String, VarValue)> {
    let mut env: Vec<(String, VarValue)> = Vec::new();
    for f in features {
        if let FeatureKind::Variable(v) = &f.kind {
            if v.problem().is_some() {
                continue;
            }
            let value = eval_variable(v.var_type, &v.expr, units, &env)
                .map(|(value, kind)| VarValue { value, quantity: kind.quantity() })
                .unwrap_or_else(|_| v.var_value());
            env.push((v.name.clone(), value));
        }
    }
    env
}

/// Why an expression can't be used here: a name defined only below, not at all, or an
/// evaluation error.
fn why(label: &str, expr: &str, q: Quantity, units: &Units, env: &[(String, VarValue)], later: &[String]) -> Option<String> {
    for n in units::variable_names(expr) {
        if !env.iter().any(|(k, _)| *k == n) {
            return Some(if later.contains(&n) {
                format!("{label}: #{n} is used before it is defined; move its Variable above this feature")
            } else {
                format!("{label}: #{n} is not defined")
            });
        }
    }
    match units.eval_vars(expr, q, &env) {
        Ok(_) => None,
        Err(e) => Some(format!("{label}: {e}")),
    }
}

/// The features whose expressions can't be evaluated where they are (P3F.4): a variable used
/// above its definition ("used before it is defined"), one defined nowhere, or an expression
/// that no longer evaluates (a Length variable in an angle field). `features` are the ones
/// that build (suppressed and rolled-back ones left out), in order.
pub fn check(features: &[Feature], units: &Units) -> Vec<(FeatureId, String)> {
    // Fast path: nothing names a variable and nothing defines one.
    let names: Vec<String> = features
        .iter()
        .filter_map(|f| match &f.kind {
            FeatureKind::Variable(v) => Some(v.name.clone()),
            _ => None,
        })
        .collect();
    let mut out = Vec::new();
    let mut env: Vec<(String, VarValue)> = Vec::new();
    for (i, f) in features.iter().enumerate() {
        let later: Vec<String> = features[i + 1..]
            .iter()
            .filter_map(|g| match &g.kind {
                FeatureKind::Variable(v) => Some(v.name.clone()),
                _ => None,
            })
            .collect();
        match &f.kind {
            FeatureKind::Variable(v) => {
                if let Some(p) = v.problem() {
                    out.push((f.id, p.to_string()));
                    continue;
                }
                let mut value = v.var_value();
                match (units::variable_names(&v.expr).iter().find(|n| !env.iter().any(|(k, _)| k == *n)), eval_variable(v.var_type, &v.expr, units, &env)) {
                    (Some(n), _) => out.push((
                        f.id,
                        if later.contains(n) || (*n != v.name && names.contains(n)) {
                            format!("#{n} is used before it is defined; move its Variable above this one")
                        } else {
                            format!("#{n} is not defined")
                        },
                    )),
                    (None, Err(e)) => out.push((f.id, e.to_string())),
                    (None, Ok((x, kind))) => value = VarValue { value: x, quantity: kind.quantity() },
                }
                env.push((v.name.clone(), value));
            }
            FeatureKind::Sketch(sk) => {
                for (_, e, q) in sketch_expressions(&sk.geometry) {
                    if let Some(w) = why("Dimension", &e, q, units, &env, &later) {
                        out.push((f.id, w));
                        break;
                    }
                }
            }
            k => {
                if names.is_empty() && !has_hash(k) {
                    continue;
                }
                let mut k = k.clone();
                let mut first: Option<String> = None;
                slots(&mut k, &mut |s| {
                    if first.is_none() && s.expr.contains('#') {
                        first = why(s.label, s.expr, s.quantity, units, &env, &later);
                    }
                });
                if let Some(w) = first {
                    out.push((f.id, w));
                }
            }
        }
    }
    out
}

/// Whether any of a feature's fields names a variable (cheaply: without cloning a sketch).
fn has_hash(k: &FeatureKind) -> bool {
    match k {
        FeatureKind::Sketch(sk) => !sk.geometry.expressions.is_empty(),
        FeatureKind::Variable(_) => true,
        k => {
            let mut k = k.clone();
            let mut any = false;
            slots(&mut k, &mut |s| any |= s.expr.contains('#'));
            any
        }
    }
}

/// Re-evaluates the variables and every expression that names one, top to bottom, writing the
/// values (P3F.4: a changed `#piston_d` reaches the bore dimension, the depths and the counts).
/// Sketch dimensions that change are set through the sketch's own edit (and solve). Features
/// that fail to evaluate keep their last values ([`check`] reports them). Returns whether
/// anything changed.
pub fn refresh(features: &mut [Feature], suppressed: &[FeatureId], units: &Units) -> bool {
    if !features.iter().any(|f| has_hash(&f.kind)) {
        return false;
    }
    let mut changed = false;
    let mut env: Vec<(String, VarValue)> = Vec::new();
    for f in features.iter_mut() {
        if suppressed.contains(&f.id) {
            continue;
        }
        match &mut f.kind {
            FeatureKind::Variable(v) => {
                let before = (v.value, v.kind);
                if v.problem().is_none() && v.evaluate(units, &env).is_ok() {
                    changed |= before != (v.value, v.kind);
                }
                if v.problem().is_none() {
                    env.push((v.name.clone(), v.var_value()));
                }
            }
            FeatureKind::Sketch(sk) => {
                let g = &mut sk.geometry;
                let mut ops = Vec::new();
                for (id, e, q) in sketch_expressions(g) {
                    let Ok(value) = units.eval_vars(&e, q, &env) else { continue };
                    if g.dimensions.get(id).is_some_and(|d| (d.value - value).abs() > 1e-12 && !d.driven) {
                        ops.push(SketchOp::SetDimensionValue { id, value });
                    }
                }
                if !ops.is_empty() {
                    let before = g.clone();
                    if SketchOp::Batch(ops).apply(g).is_ok() {
                        changed = true;
                    } else {
                        *g = before;
                    }
                }
            }
            k => {
                slots(k, &mut |s| {
                    if s.expr.contains('#')
                        && let Ok(v) = units.eval_vars(s.expr, s.quantity, &env)
                        && (*s.value - v).abs() > 1e-12
                    {
                        *s.value = v;
                        changed = true;
                    }
                });
            }
        }
    }
    changed
}
