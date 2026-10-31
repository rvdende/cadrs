//! The Thread feature's dialog (`cadrs_core::thread`), in the applied-feature dialogs' chain
//! between [`crate::transform_ui`] and [`crate::surfacing_ui`]:
//!
//! - *Cylinder or hole edge*: the circular edge the thread starts from. Picking one fits the
//!   size to it (an external thread's major diameter, an internal thread's tap drill).
//! - **Standard** ISO (metric) or ANSI (inch); **Size**; **Pitch** (coarse and fine, as the hole
//!   dialog lists them). The Size's last entry, **Custom**, takes a *Major diameter* and a
//!   *Pitch* typed in (mm), as a cylinder no size fits gets.
//! - **Blind** with its *Length*, or **Up to end** (the whole cylinder).
//! - **Right-hand** / **Left-hand**.
//!
//! The toolbar's Thread button starts it with the selected edge.

use bevy::prelude::*;
use cadrs_core::document::EdgeRef;
use cadrs_core::hole::{HoleSpec, HoleStandard};
use cadrs_core::thread::{Cylinder, ThreadEnd, ThreadFeature};
use cadrs_core::{Feature, FeatureKind};
use cadrs_ui::i18n::{tr, tr_with};
use cadrs_ui::prelude::*;

use crate::ActiveDocument;
use crate::applied::{AppliedField, AppliedKind, AppliedSession, BeforeParts};
use crate::applied_dialog::{Role, body_column, index_of, list, number, opts, select_row};
use crate::parts::PartCache;
use crate::viewport::Pick;

/// The cylinder an edge pick bounds (else on the parts before the feature being edited: the
/// thread cuts the edge it starts from).
fn cylinder_of(world: &World, pick: Pick) -> Option<(EdgeRef, Cylinder)> {
    let Pick::Edge(part, edge) = pick else {
        return None;
    };
    let before = world.get_resource::<BeforeParts>();
    let cache = world.resource::<PartCache>();
    let solid = cache
        .part_with_edge(part, &edge)
        .or_else(|| before.and_then(|b| b.parts.iter().find(|p| p.solid.edge(&edge).is_some())))
        .map(|p| &p.solid)?;
    let i = solid.edges.iter().position(|e| e.name == edge)?;
    let c = cadrs_core::thread::cylinder(solid, i)?;
    let seed = solid.edges[i].midpoint();
    Some((EdgeRef { part, edge, seed }, c))
}

/// "12.5 mm".
fn mm(v: f64) -> String {
    let s = format!("{v:.3}");
    format!("{} mm", s.trim_end_matches('0').trim_end_matches('.'))
}

/// Takes a picked edge: the size fitted to its cylinder, a new thread's length to it.
fn take(x: &mut ThreadFeature, e: EdgeRef, c: &Cylinder, fit_length: bool) {
    x.edge = Some(e);
    x.fit_size(2.0 * c.radius, c.external);
    if fit_length {
        let d = x.major().unwrap_or(c.radius * 2.0);
        let l = c.height.min(2.0 * d);
        x.length = (l * 2.0).floor().max(1.0) / 2.0;
        x.length_expr = mm(x.length);
    }
}

/// A new feature's base name, parameters and first field, from the selection.
pub fn initial(
    world: &World,
    kind: AppliedKind,
    picked: &[Pick],
) -> Option<(&'static str, FeatureKind, AppliedField)> {
    if kind != AppliedKind::Thread {
        return None;
    }
    let mut x = ThreadFeature::default();
    let inch = world
        .get_resource::<ActiveDocument>()
        .is_some_and(|d| d.doc.units.length == cadrs_sketch::units::LengthUnit::Inch);
    if inch {
        x.set_standard(HoleStandard::Ansi);
    }
    let mut base = "Thread";
    if let Some((e, c)) = picked.iter().find_map(|p| cylinder_of(world, *p)) {
        take(&mut x, e, &c, true);
        base = ThreadFeature::base_name(c.external);
    }
    Some((base, FeatureKind::Thread(x), AppliedField::ThreadEdge))
}

/// A pick into the thread's edge field. Returns false if it doesn't fit.
pub fn pick(world: &mut World, kind: &mut FeatureKind, field: AppliedField, pick: Pick) -> bool {
    let (FeatureKind::Thread(x), AppliedField::ThreadEdge) = (kind, field) else {
        return false;
    };
    let Some((e, c)) = cylinder_of(world, pick) else {
        return false;
    };
    if x.edge.is_some_and(|f| f.edge == e.edge) {
        x.edge = None;
    } else {
        let first = x.edge.is_none();
        take(x, e, &c, first);
        if first {
            name_new(world, c.external);
        }
    }
    true
}

/// A new "Thread N" named for the edge it was given: "External thread 1", "Internal thread 1".
fn name_new(world: &mut World, external: bool) {
    let Some(s) = world.get_resource::<AppliedSession>().cloned() else {
        return;
    };
    let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() else {
        return;
    };
    let Some(el) = doc.doc.element(s.element) else {
        return;
    };
    if !s.is_new
        || !el
            .feature(s.feature)
            .is_some_and(|f| f.name.starts_with("Thread "))
    {
        return;
    }
    let name = el.next_feature_name(ThreadFeature::base_name(external));
    let _ = doc.execute(&cadrs_core::commands::RenameFeature {
        element: s.element,
        feature: s.feature,
        name,
    });
}

/// The edge, shown selected in the view while the dialog is open.
pub fn references(kind: &FeatureKind, cache: &PartCache) -> Vec<Pick> {
    let FeatureKind::Thread(x) = kind else {
        return Vec::new();
    };
    x.edge
        .iter()
        .filter_map(|e| {
            cache
                .parts
                .iter()
                .find(|p| p.solid.edge(&e.edge).is_some())
                .map(|p| Pick::Edge(p.id, e.edge))
                .or(Some(Pick::Edge(e.part, e.edge)))
        })
        .collect()
}

// ---------------------------------------------------------------------------------------------
// The dialog

pub(crate) fn name(kind: &FeatureKind) -> Option<&'static str> {
    match kind {
        FeatureKind::Thread(_) => Some("thread"),
        k => crate::surfacing_ui::name(k),
    }
}

pub(crate) fn layout(kind: &FeatureKind) -> Option<String> {
    match kind {
        FeatureKind::Thread(x) => Some(format!(
            "thread {:?} {} {} {:?} {}",
            x.standard, x.size, x.custom, x.end, x.clockwise
        )),
        k => crate::surfacing_ui::layout(k),
    }
}

fn op_name(features: &[Feature], op: cadrs_sketch::OpId) -> String {
    features
        .iter()
        .find(|f| f.id.0 == op)
        .map_or_else(|| tr("model-unknown-part"), |f| f.name.clone())
}

pub(crate) fn items(
    features: &[Feature],
    cache: &PartCache,
    kind: &FeatureKind,
    role: Role,
) -> Option<Vec<String>> {
    match (kind, role) {
        (FeatureKind::Thread(x), Role::ThreadEdge) => Some(
            x.edge
                .iter()
                .map(|e| {
                    tr_with(
                        "common-edge-of",
                        &[(
                            "name",
                            &op_name(features, crate::parts::edge_maker(features, &e.edge)),
                        )],
                    )
                })
                .collect(),
        ),
        (k, r) => crate::surfacing_ui::items(features, cache, k, r),
    }
}

pub(crate) fn list_field(role: Role) -> Option<AppliedField> {
    match role {
        Role::ThreadEdge => Some(AppliedField::ThreadEdge),
        r => crate::surfacing_ui::list_field(r),
    }
}

/// The handedness options' catalog keys.
const HANDS: [&str; 2] = ["model-thread-right-hand", "model-thread-left-hand"];

pub(crate) fn body(
    b: &mut ChildSpawner,
    t: &Theme,
    kind: &FeatureKind,
    field: AppliedField,
    items_of: &dyn Fn(Role) -> Vec<String>,
) {
    let FeatureKind::Thread(x) = kind else {
        crate::surfacing_ui::body(b, t, kind, field, items_of);
        return;
    };
    body_column(b, |b| {
        list(
            b,
            t,
            "thread-edge-field",
            &tr("model-thread-edge"),
            Role::ThreadEdge,
            items_of(Role::ThreadEdge),
            field == AppliedField::ThreadEdge,
        );
        let standards = [("ISO".to_string(), true), ("ANSI".to_string(), true)];
        select_row(
            b,
            t,
            "thread-standard",
            &tr("model-standard"),
            Role::ThreadStandard,
            &standards,
            usize::from(x.standard == HoleStandard::Ansi),
            None,
        );
        let mut sizes: Vec<(String, bool)> = ThreadFeature::sizes(x.standard)
            .into_iter()
            .map(|s| (s, true))
            .collect();
        sizes.push((tr(CUSTOM), true));
        select_row(
            b,
            t,
            "thread-size",
            &tr("common-size"),
            Role::ThreadSize,
            &sizes,
            size_index(x),
            None,
        );
        if x.custom {
            number(
                b,
                t,
                "thread-diameter",
                &tr("model-thread-major-diameter"),
                Role::ThreadDiameter,
                &x.diameter_expr,
                None,
            );
            number(
                b,
                t,
                "thread-custom-pitch",
                &tr("model-pitch"),
                Role::ThreadCustomPitch,
                &x.custom_pitch_expr,
                None,
            );
        } else {
            let pitches: Vec<(String, bool)> = x
                .pitches()
                .iter()
                .map(|p| (HoleSpec::pitch_label(x.standard, p), true))
                .collect();
            select_row(
                b,
                t,
                "thread-pitch",
                &tr("model-pitch"),
                Role::ThreadPitch,
                &pitches,
                pitch_index(x),
                None,
            );
        }
        let ends = opts(
            &ThreadEnd::ALL,
            |e| {
                tr(match e {
                    ThreadEnd::Blind => "model-blind",
                    ThreadEnd::Full => "model-thread-up-to-end",
                })
            },
            |_| true,
        );
        select_row(
            b,
            t,
            "thread-end",
            "",
            Role::ThreadEnd,
            &ends,
            index_of(&ThreadEnd::ALL, &x.end),
            None,
        );
        if x.end == ThreadEnd::Blind {
            number(
                b,
                t,
                "thread-length",
                &tr("model-length"),
                Role::ThreadLength,
                &x.length_expr,
                None,
            );
        }
        let hands: Vec<(String, bool)> = HANDS.iter().map(|h| (tr(h), true)).collect();
        select_row(
            b,
            t,
            "thread-hand",
            &tr("common-direction"),
            Role::ThreadHand,
            &hands,
            usize::from(!x.clockwise),
            None,
        );
    });
}

/// The Size dropdown's last entry: a diameter and pitch typed in (its catalog key).
const CUSTOM: &str = "model-thread-custom";

fn size_index(x: &ThreadFeature) -> usize {
    let sizes = ThreadFeature::sizes(x.standard);
    if x.custom {
        return sizes.len();
    }
    sizes.iter().position(|s| *s == x.size).unwrap_or(0)
}

fn pitch_index(x: &ThreadFeature) -> usize {
    x.pitches().iter().position(|p| *p == x.pitch).unwrap_or(0)
}

// ---------------------------------------------------------------------------------------------
// Keeping it in step and input

pub(crate) fn number_text(kind: &FeatureKind, role: Role) -> Option<String> {
    match (kind, role) {
        (FeatureKind::Thread(x), Role::ThreadLength) => Some(x.length_expr.clone()),
        (FeatureKind::Thread(x), Role::ThreadDiameter) => Some(x.diameter_expr.clone()),
        (FeatureKind::Thread(x), Role::ThreadCustomPitch) => Some(x.custom_pitch_expr.clone()),
        (k, r) => crate::surfacing_ui::number_text(k, r),
    }
}

pub(crate) fn select_index(kind: &FeatureKind, role: Role) -> Option<usize> {
    let FeatureKind::Thread(x) = kind else {
        return crate::surfacing_ui::select_index(kind, role);
    };
    Some(match role {
        Role::ThreadStandard => usize::from(x.standard == HoleStandard::Ansi),
        Role::ThreadSize => size_index(x),
        Role::ThreadPitch => pitch_index(x),
        Role::ThreadEnd => index_of(&ThreadEnd::ALL, &x.end),
        Role::ThreadHand => usize::from(!x.clockwise),
        _ => return None,
    })
}

pub(crate) fn select(k: &mut FeatureKind, role: Role, i: usize) {
    let FeatureKind::Thread(x) = k else {
        crate::surfacing_ui::select(k, role, i);
        return;
    };
    match role {
        Role::ThreadStandard => x.set_standard(if i == 1 {
            HoleStandard::Ansi
        } else {
            HoleStandard::Iso
        }),
        Role::ThreadSize => match ThreadFeature::sizes(x.standard).get(i) {
            Some(s) => x.set_size(s),
            None => x.set_custom(),
        },
        Role::ThreadPitch => {
            if let Some(p) = x.pitches().get(i) {
                x.pitch = p.clone();
            }
        }
        Role::ThreadEnd => x.end = ThreadEnd::ALL[i.min(ThreadEnd::ALL.len() - 1)],
        Role::ThreadHand => x.clockwise = i == 0,
        _ => {}
    }
}

pub(crate) fn tab(k: &mut FeatureKind, role: Role, i: usize) {
    crate::surfacing_ui::tab(k, role, i)
}

pub(crate) fn is_checkbox(name: &str) -> bool {
    crate::surfacing_ui::is_checkbox(name)
}

pub(crate) fn checkbox(k: &mut FeatureKind, name: &str, on: bool) -> Option<&'static str> {
    crate::surfacing_ui::checkbox(k, name, on)
}

pub(crate) fn is_signed(role: Role) -> bool {
    crate::surfacing_ui::is_signed(role)
}

pub(crate) fn is_plain(role: Role) -> bool {
    crate::surfacing_ui::is_plain(role)
}

pub(crate) fn is_angle(role: Role) -> bool {
    crate::surfacing_ui::is_angle(role)
}

pub(crate) fn number_label(role: Role) -> Option<&'static str> {
    match role {
        Role::ThreadLength => Some("Length"),
        Role::ThreadDiameter => Some("Major diameter"),
        Role::ThreadCustomPitch => Some("Pitch"),
        r => crate::surfacing_ui::number_label(r),
    }
}

pub(crate) fn set_number(k: &mut FeatureKind, role: Role, v: f64, expr: String) {
    match (k, role) {
        (FeatureKind::Thread(x), Role::ThreadLength) => {
            x.length = v;
            x.length_expr = expr;
        }
        (FeatureKind::Thread(x), Role::ThreadDiameter) => {
            x.diameter = v;
            x.diameter_expr = expr;
        }
        (FeatureKind::Thread(x), Role::ThreadCustomPitch) => {
            x.custom_pitch = v;
            x.custom_pitch_expr = expr;
        }
        (k, r) => crate::surfacing_ui::set_number(k, r, v, expr),
    }
}

pub(crate) fn flip(k: &mut FeatureKind, role: Role) {
    crate::surfacing_ui::flip(k, role)
}

/// The edge's ✕.
pub(crate) fn remove(k: &mut FeatureKind, role: Role, i: usize) {
    match (k, role) {
        (FeatureKind::Thread(x), Role::ThreadEdge) => x.edge = None,
        (k, r) => crate::surfacing_ui::remove(k, r, i),
    }
}
