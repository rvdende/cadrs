//! The Gear feature's dialog (`cadrs_core::gear`, issue #179), in the applied-feature dialogs'
//! chain between [`crate::thread_ui`] and [`crate::surfacing_ui`]:
//!
//! - *Cylinder, cone or flat face (optional)*: the face the teeth are cut into. Picking one sets
//!   the **Type** (a cylinder: Spur gear, a hole: Ring gear, a cone: Bevel gear, a flat face:
//!   Rack) and the number of teeth that brings the module nearest the one set; the field then
//!   shows the module the gear has there.
//! - **Type**, **Teeth**, **Module** (a gear made whole, or a rack), **Pressure angle**.
//! - **Tooth line** Straight, Helical or Herringbone, with its *Helix angle* and hand. A bevel's
//!   helical teeth: **Spiral** Face-milled (Gleason), **Cut as** the Gear of a pair or as the
//!   Pinion that matches it, with its *Cutter radius* (0: the cone distance at the middle of
//!   the face); or Equiangular.
//! - A gear made whole: its *Face width*, *Bore diameter*, *Rim thickness* (ring), *Pitch angle*
//!   (bevel), *Length* (rack), *Body thickness* (bevel, rack), then where it is: **X**, **Y**,
//!   **Z** and its **Axis**.
//! - *Rotation* about its axis; *Backlash*. A rack on a face: **Teeth across the face**.
//!
//! The toolbar's Gear button starts it with the selected face.

use bevy::prelude::*;
use cadrs_core::document::FaceRef;
use cadrs_core::gear::{
    BevelMember, GearAxis, GearFeature, GearType, SpiralKind, Target, ToothStyle,
};
use cadrs_core::{Feature, FeatureKind};
use cadrs_ui::OptionRow;
use cadrs_ui::i18n::{tr, tr_with};
use cadrs_ui::prelude::*;

use crate::applied::{AppliedField, AppliedKind, BeforeParts};
use crate::applied_dialog::{Role, body_column, index_of, list, number, opts, select_row};
use crate::parts::PartCache;
use crate::viewport::Pick;

/// What a gear can do with a picked face (on the parts before the feature being edited too: the
/// gear cuts the face it is on).
fn target_of(world: &World, pick: Pick) -> Option<(FaceRef, Target)> {
    let Pick::Face(part, face) = pick else {
        return None;
    };
    let before = world.get_resource::<BeforeParts>();
    let cache = world.resource::<PartCache>();
    let solid = cache
        .part_or_ghost_with_face(part, &face)
        .or_else(|| {
            before.and_then(|b| {
                b.parts
                    .iter()
                    .find(|p| p.solid.faces.iter().any(|f| f.name == face))
            })
        })
        .map(|p| &p.solid)?;
    let i = solid.faces.iter().position(|f| f.name == face)?;
    let target = cadrs_core::gear::target(solid, i)?;
    let seed = solid.face_point(i)?;
    Some((FaceRef { part, face, seed }, target))
}

/// A new feature's base name, parameters and first field, from the selection.
pub fn initial(
    world: &World,
    kind: AppliedKind,
    picked: &[Pick],
) -> Option<(&'static str, FeatureKind, AppliedField)> {
    if kind != AppliedKind::Gear {
        return None;
    }
    let mut x = GearFeature::default();
    if let Some((f, t)) = picked.iter().find_map(|p| target_of(world, *p)) {
        x.face = Some(f);
        x.fit(&t);
    }
    Some(("Gear", FeatureKind::Gear(x), AppliedField::GearFace))
}

/// A pick into the gear's face field. Returns false if it doesn't fit.
pub fn pick(world: &mut World, kind: &mut FeatureKind, field: AppliedField, pick: Pick) -> bool {
    let (FeatureKind::Gear(x), AppliedField::GearFace) = (kind, field) else {
        return false;
    };
    let Some((f, t)) = target_of(world, pick) else {
        return false;
    };
    if x.face.is_some_and(|g| g.face == f.face) {
        x.face = None;
    } else {
        x.face = Some(f);
        x.fit(&t);
    }
    true
}

/// The face, shown selected in the view while the dialog is open.
pub fn references(kind: &FeatureKind, cache: &PartCache) -> Vec<Pick> {
    let FeatureKind::Gear(x) = kind else {
        return Vec::new();
    };
    x.face
        .iter()
        .map(|f| {
            cache
                .parts
                .iter()
                .find(|p| p.solid.faces.iter().any(|g| g.name == f.face))
                .map_or(Pick::Face(f.part, f.face), |p| Pick::Face(p.id, f.face))
        })
        .collect()
}

// ---------------------------------------------------------------------------------------------
// The dialog

pub(crate) fn name(kind: &FeatureKind) -> Option<&'static str> {
    match kind {
        FeatureKind::Gear(_) => Some("gear"),
        k => crate::surfacing_ui::name(k),
    }
}

pub(crate) fn layout(kind: &FeatureKind) -> Option<String> {
    match kind {
        FeatureKind::Gear(x) => Some(format!(
            "gear {:?} {:?} {} {} {} {:?} {:?}",
            x.gear_type,
            x.style,
            x.face.is_some(),
            x.left_hand,
            x.flip,
            x.spiral,
            x.member
        )),
        k => crate::surfacing_ui::layout(k),
    }
}

/// "2.5 mm".
fn mm(v: f64) -> String {
    let s = format!("{v:.3}");
    format!("{} mm", s.trim_end_matches('0').trim_end_matches('.'))
}

pub(crate) fn items(
    features: &[Feature],
    cache: &PartCache,
    kind: &FeatureKind,
    role: Role,
) -> Option<Vec<String>> {
    match (kind, role) {
        (FeatureKind::Gear(x), Role::GearFace) => Some(
            x.face
                .iter()
                .map(|f| {
                    let maker = features
                        .iter()
                        .find(|g| g.id.0 == f.face.op)
                        .map_or_else(|| tr("model-unknown-part"), |g| g.name.clone());
                    let face = tr_with("common-face-of", &[("name", &maker)]);
                    // The module the gear has on the face, where the face is still there to
                    // measure (a rack's is its own).
                    let module = (x.gear_type != GearType::Rack)
                        .then(|| {
                            let solid = &cache.part_or_ghost_with_face(f.part, &f.face)?.solid;
                            let i = solid.faces.iter().position(|g| g.name == f.face)?;
                            x.module_on(&cadrs_core::gear::target(solid, i)?)
                        })
                        .flatten();
                    match module {
                        Some(m) => tr_with(
                            "model-gear-face-module",
                            &[("face", &face), ("module", &mm(m))],
                        ),
                        None => face,
                    }
                })
                .collect(),
        ),
        (k, r) => crate::surfacing_ui::items(features, cache, k, r),
    }
}

pub(crate) fn list_field(role: Role) -> Option<AppliedField> {
    match role {
        Role::GearFace => Some(AppliedField::GearFace),
        r => crate::surfacing_ui::list_field(r),
    }
}

fn type_key(t: GearType) -> &'static str {
    match t {
        GearType::Spur => "model-gear-spur",
        GearType::Ring => "model-gear-ring",
        GearType::Bevel => "model-gear-bevel",
        GearType::Rack => "model-gear-rack",
        GearType::Hypoid => "model-gear-hypoid",
    }
}

fn style_key(s: ToothStyle) -> &'static str {
    match s {
        ToothStyle::Straight => "model-gear-straight",
        ToothStyle::Helical => "model-gear-helical",
        ToothStyle::Herringbone => "model-gear-herringbone",
    }
}

pub(crate) fn body(
    b: &mut ChildSpawner,
    t: &Theme,
    kind: &FeatureKind,
    field: AppliedField,
    items_of: &dyn Fn(Role) -> Vec<String>,
) {
    let FeatureKind::Gear(x) = kind else {
        crate::surfacing_ui::body(b, t, kind, field, items_of);
        return;
    };
    let on_face = x.face.is_some();
    let rack = x.gear_type == GearType::Rack;
    body_column(b, |b| {
        list(
            b,
            t,
            "gear-face-field",
            &tr("model-gear-face"),
            Role::GearFace,
            items_of(Role::GearFace),
            field == AppliedField::GearFace,
        );
        // On a face the type is the face's.
        let types = opts(
            &GearType::ALL,
            |g| tr(type_key(g)),
            |g| !on_face || g == x.gear_type,
        );
        select_row(
            b,
            t,
            "gear-type",
            &tr("common-type"),
            Role::GearType,
            &types,
            index_of(&GearType::ALL, &x.gear_type),
            None,
        );
        if x.gear_type == GearType::Hypoid {
            hypoid_body(b, t, x);
            return;
        }
        let mut num = |name: &str, key: &str, role: Role, text: &str| {
            number(b, t, name, &tr(key), role, text, None);
        };
        if !rack {
            num(
                "gear-teeth",
                "model-gear-teeth",
                Role::GearTeeth,
                &x.teeth_expr,
            );
        }
        if !on_face || rack {
            num(
                "gear-module",
                "model-gear-module",
                Role::GearModule,
                &x.module_expr,
            );
        }
        num(
            "gear-pressure-angle",
            "model-gear-pressure-angle",
            Role::GearPressureAngle,
            &x.pressure_angle_expr,
        );
        let styles = opts(
            &ToothStyle::ALL,
            |s| tr(style_key(s)),
            |s| !(x.gear_type == GearType::Bevel && s == ToothStyle::Herringbone),
        );
        select_row(
            b,
            t,
            "gear-style",
            &tr("model-gear-tooth-line"),
            Role::GearStyle,
            &styles,
            index_of(&ToothStyle::ALL, &x.style),
            None,
        );
        if x.style != ToothStyle::Straight {
            number(
                b,
                t,
                "gear-helix-angle",
                &tr("model-gear-helix-angle"),
                Role::GearHelixAngle,
                &x.helix_angle_expr,
                None,
            );
            let hands: Vec<(String, bool)> = HANDS.iter().map(|h| (tr(h), true)).collect();
            select_row(
                b,
                t,
                "gear-hand",
                &tr("common-direction"),
                Role::GearHand,
                &hands,
                usize::from(x.left_hand),
                None,
            );
            // A bevel's helical teeth: as a face-mill cutter leaves them, or equiangular.
            if x.gear_type == GearType::Bevel {
                let spirals = opts(
                    &SpiralKind::ALL,
                    |k| {
                        tr(match k {
                            SpiralKind::FaceMilled => "model-gear-face-milled",
                            SpiralKind::Equiangular => "model-gear-equiangular",
                        })
                    },
                    |_| true,
                );
                select_row(
                    b,
                    t,
                    "gear-spiral",
                    &tr("model-gear-spiral"),
                    Role::GearSpiral,
                    &spirals,
                    index_of(&SpiralKind::ALL, &x.spiral),
                    None,
                );
                if x.spiral == SpiralKind::FaceMilled {
                    // The gear of a pair is cut by the cutter itself; its pinion to run with it, with
                    // localised contact or as its exact conjugate.
                    let members = opts(
                        &BevelMember::ALL,
                        |k| {
                            tr(match k {
                                BevelMember::Gear => "model-gear-member-gear",
                                BevelMember::Pinion => "model-gear-member-pinion",
                                BevelMember::ConjugatePinion => "model-gear-member-conjugate",
                            })
                        },
                        |_| true,
                    );
                    select_row(
                        b,
                        t,
                        "gear-member",
                        &tr("model-gear-member"),
                        Role::GearMember,
                        &members,
                        index_of(&BevelMember::ALL, &x.member),
                        None,
                    );
                    number(
                        b,
                        t,
                        "gear-cutter",
                        &tr("model-gear-cutter-radius"),
                        Role::GearCutter,
                        &x.cutter_radius_expr,
                        None,
                    );
                    if x.member == BevelMember::Pinion {
                        number(
                            b,
                            t,
                            "gear-shaft-angle",
                            &tr("model-gear-shaft-angle"),
                            Role::GearShaftAngle,
                            &x.shaft_angle_expr,
                            None,
                        );
                    }
                }
            }
        }
        let mut num = |name: &str, key: &str, role: Role, text: &str| {
            number(b, t, name, &tr(key), role, text, None);
        };
        if !on_face {
            if x.gear_type == GearType::Bevel {
                num(
                    "gear-pitch-angle",
                    "model-gear-pitch-angle",
                    Role::GearPitchAngle,
                    &x.pitch_angle_expr,
                );
            }
            if rack {
                num(
                    "gear-length",
                    "model-length",
                    Role::GearLength,
                    &x.length_expr,
                );
            }
            num(
                "gear-width",
                "model-gear-face-width",
                Role::GearWidth,
                &x.width_expr,
            );
            match x.gear_type {
                GearType::Spur | GearType::Bevel => {
                    num("gear-bore", "model-gear-bore", Role::GearBore, &x.bore_expr)
                }
                GearType::Ring => num("gear-rim", "model-gear-rim", Role::GearRim, &x.rim_expr),
                GearType::Rack | GearType::Hypoid => {}
            }
            if matches!(x.gear_type, GearType::Bevel | GearType::Rack) {
                num(
                    "gear-height",
                    "model-gear-body",
                    Role::GearHeight,
                    &x.height_expr,
                );
            }
        }
        if !rack {
            num(
                "gear-rotation",
                "common-rotation",
                Role::GearRotation,
                &x.rotation_expr,
            );
        }
        num(
            "gear-backlash",
            "model-gear-backlash",
            Role::GearBacklash,
            &x.backlash_expr,
        );
        if on_face && rack {
            b.spawn(
                OptionRow::new("gear-across", tr("model-gear-across"))
                    .checked(x.flip)
                    .build(t),
            );
        }
        if !on_face {
            for (i, (name, label, role)) in [
                ("gear-x", "X", Role::GearX),
                ("gear-y", "Y", Role::GearY),
                ("gear-z", "Z", Role::GearZ),
            ]
            .into_iter()
            .enumerate()
            {
                number(b, t, name, label, role, &x.position_expr[i], None);
            }
            let axes = opts(&GearAxis::ALL, |a| a.label(), |_| true);
            select_row(
                b,
                t,
                "gear-axis",
                &tr("model-gear-axis"),
                Role::GearAxis,
                &axes,
                index_of(&GearAxis::ALL, &x.axis),
                None,
            );
            // A bevel gear: placed by its apex or its back face, its axis tilted about the apex
            // (a pair at any shaft angle).
            if x.gear_type == GearType::Bevel {
                b.spawn(
                    OptionRow::new("gear-at-apex", tr("model-gear-at-apex"))
                        .checked(x.at_apex)
                        .build(t),
                );
                number(
                    b,
                    t,
                    "gear-tilt",
                    &tr("model-gear-tilt"),
                    Role::GearTilt,
                    &x.tilt_expr,
                    None,
                );
            }
        }
    });
}

/// A hypoid pair's member: the pair as a designer states it (both members' teeth, the module
/// at the mean point, the shafts' angle and offset, the spiral, the face mill), which member
/// this is, and where the pair's pitch plane is.
fn hypoid_body(b: &mut ChildSpawner, t: &Theme, x: &cadrs_core::gear::GearFeature) {
    let num = |b: &mut ChildSpawner, name: &str, key: &str, role: Role, text: &str| {
        number(b, t, name, &tr(key), role, text, None);
    };
    num(
        b,
        "gear-teeth",
        "model-gear-teeth",
        Role::GearTeeth,
        &x.teeth_expr,
    );
    num(
        b,
        "gear-mate-teeth",
        "model-gear-mate-teeth",
        Role::GearMateTeeth,
        &x.mate_teeth_expr,
    );
    num(
        b,
        "gear-module",
        "model-gear-module",
        Role::GearModule,
        &x.module_expr,
    );
    num(
        b,
        "gear-pressure-angle",
        "model-gear-pressure-angle",
        Role::GearPressureAngle,
        &x.pressure_angle_expr,
    );
    num(
        b,
        "gear-helix-angle",
        "model-gear-spiral-angle",
        Role::GearHelixAngle,
        &x.helix_angle_expr,
    );
    let hands: Vec<(String, bool)> = HANDS.iter().map(|h| (tr(h), true)).collect();
    select_row(
        b,
        t,
        "gear-hand",
        &tr("common-direction"),
        Role::GearHand,
        &hands,
        usize::from(x.left_hand),
        None,
    );
    // The gear or the pinion (a hypoid pinion is generated, never cut as a conjugate).
    let members = opts(
        &BevelMember::ALL,
        |k| {
            tr(match k {
                BevelMember::Gear => "model-gear-member-gear",
                _ => "model-gear-member-hypoid-pinion",
            })
        },
        |k| k != BevelMember::ConjugatePinion,
    );
    select_row(
        b,
        t,
        "gear-member",
        &tr("model-gear-member"),
        Role::GearMember,
        &members,
        index_of(&BevelMember::ALL, &x.member),
        None,
    );
    num(
        b,
        "gear-shaft-angle",
        "model-gear-shaft-angle",
        Role::GearShaftAngle,
        &x.shaft_angle_expr,
    );
    num(
        b,
        "gear-offset",
        "model-gear-offset",
        Role::GearOffset,
        &x.offset_expr,
    );
    num(
        b,
        "gear-cutter",
        "model-gear-cutter-radius",
        Role::GearCutter,
        &x.cutter_radius_expr,
    );
    num(
        b,
        "gear-pressure-shift",
        "model-gear-pressure-shift",
        Role::GearPressureShift,
        &x.pressure_shift_expr,
    );
    num(
        b,
        "gear-backlash",
        "model-gear-backlash",
        Role::GearBacklash,
        &x.backlash_expr,
    );
    num(
        b,
        "gear-end-relief",
        "model-gear-end-relief",
        Role::GearEndRelief,
        &x.end_relief_expr,
    );
    for (i, (name, label, role)) in [
        ("gear-x", "X", Role::GearX),
        ("gear-y", "Y", Role::GearY),
        ("gear-z", "Z", Role::GearZ),
    ]
    .into_iter()
    .enumerate()
    {
        number(b, t, name, label, role, &x.position_expr[i], None);
    }
    let axes = opts(&GearAxis::ALL, |a| a.label(), |_| true);
    select_row(
        b,
        t,
        "gear-axis",
        &tr("model-gear-pitch-plane-normal"),
        Role::GearAxis,
        &axes,
        index_of(&GearAxis::ALL, &x.axis),
        None,
    );
}

/// The handedness options' catalog keys (a thread's).
const HANDS: [&str; 2] = ["model-thread-right-hand", "model-thread-left-hand"];

// ---------------------------------------------------------------------------------------------
// Keeping it in step and input

pub(crate) fn number_text(kind: &FeatureKind, role: Role) -> Option<String> {
    let FeatureKind::Gear(x) = kind else {
        return crate::surfacing_ui::number_text(kind, role);
    };
    Some(
        match role {
            Role::GearTeeth => &x.teeth_expr,
            Role::GearModule => &x.module_expr,
            Role::GearPressureAngle => &x.pressure_angle_expr,
            Role::GearHelixAngle => &x.helix_angle_expr,
            Role::GearWidth => &x.width_expr,
            Role::GearBore => &x.bore_expr,
            Role::GearRim => &x.rim_expr,
            Role::GearPitchAngle => &x.pitch_angle_expr,
            Role::GearLength => &x.length_expr,
            Role::GearHeight => &x.height_expr,
            Role::GearBacklash => &x.backlash_expr,
            Role::GearX => &x.position_expr[0],
            Role::GearY => &x.position_expr[1],
            Role::GearZ => &x.position_expr[2],
            Role::GearRotation => &x.rotation_expr,
            Role::GearCutter => &x.cutter_radius_expr,
            Role::GearShaftAngle => &x.shaft_angle_expr,
            Role::GearTilt => &x.tilt_expr,
            Role::GearMateTeeth => &x.mate_teeth_expr,
            Role::GearOffset => &x.offset_expr,
            Role::GearPressureShift => &x.pressure_shift_expr,
            Role::GearEndRelief => &x.end_relief_expr,
            _ => return None,
        }
        .clone(),
    )
}

pub(crate) fn select_index(kind: &FeatureKind, role: Role) -> Option<usize> {
    let FeatureKind::Gear(x) = kind else {
        return crate::surfacing_ui::select_index(kind, role);
    };
    Some(match role {
        Role::GearType => index_of(&GearType::ALL, &x.gear_type),
        Role::GearStyle => index_of(&ToothStyle::ALL, &x.style),
        Role::GearHand => usize::from(x.left_hand),
        Role::GearAxis => index_of(&GearAxis::ALL, &x.axis),
        Role::GearSpiral => index_of(&SpiralKind::ALL, &x.spiral),
        Role::GearMember => index_of(&BevelMember::ALL, &x.member),
        _ => return None,
    })
}

pub(crate) fn select(k: &mut FeatureKind, role: Role, i: usize) {
    let FeatureKind::Gear(x) = k else {
        crate::surfacing_ui::select(k, role, i);
        return;
    };
    match role {
        Role::GearType => {
            x.gear_type = GearType::ALL[i.min(GearType::ALL.len() - 1)];
            // A hypoid pair's teeth are spiral, its pinion generated.
            if x.gear_type == GearType::Hypoid {
                x.style = ToothStyle::Helical;
                if x.member == BevelMember::ConjugatePinion {
                    x.member = BevelMember::Pinion;
                }
            }
            if x.gear_type == GearType::Bevel && x.style == ToothStyle::Herringbone {
                x.style = ToothStyle::Helical;
            }
        }
        Role::GearStyle => x.style = ToothStyle::ALL[i.min(ToothStyle::ALL.len() - 1)],
        Role::GearHand => x.left_hand = i == 1,
        Role::GearAxis => x.axis = GearAxis::ALL[i.min(GearAxis::ALL.len() - 1)],
        Role::GearSpiral => x.spiral = SpiralKind::ALL[i.min(SpiralKind::ALL.len() - 1)],
        Role::GearMember => x.member = BevelMember::ALL[i.min(BevelMember::ALL.len() - 1)],
        _ => {}
    }
}

pub(crate) fn tab(k: &mut FeatureKind, role: Role, i: usize) {
    crate::surfacing_ui::tab(k, role, i)
}

pub(crate) fn is_checkbox(name: &str) -> bool {
    name == "gear-across-checkbox"
        || name == "gear-at-apex-checkbox"
        || crate::surfacing_ui::is_checkbox(name)
}

pub(crate) fn checkbox(k: &mut FeatureKind, name: &str, on: bool) -> Option<&'static str> {
    match (k, name) {
        (FeatureKind::Gear(x), "gear-across-checkbox") => {
            x.flip = on;
            Some("Teeth across the face")
        }
        (FeatureKind::Gear(x), "gear-at-apex-checkbox") => {
            x.at_apex = on;
            Some("Position at the apex")
        }
        (k, n) => crate::surfacing_ui::checkbox(k, n, on),
    }
}

/// Numbers that may be zero or of any sign: the bore (none at 0), the backlash, the position
/// and the rotation.
pub(crate) fn is_signed(role: Role) -> bool {
    matches!(
        role,
        Role::GearBore
            | Role::GearBacklash
            | Role::GearX
            | Role::GearY
            | Role::GearZ
            | Role::GearRotation
            | Role::GearCutter
            | Role::GearTilt
            | Role::GearOffset
            | Role::GearPressureShift
            | Role::GearEndRelief
    ) || crate::surfacing_ui::is_signed(role)
}

pub(crate) fn is_plain(role: Role) -> bool {
    crate::surfacing_ui::is_plain(role)
}

pub(crate) fn is_angle(role: Role) -> bool {
    matches!(
        role,
        Role::GearPressureAngle
            | Role::GearHelixAngle
            | Role::GearPitchAngle
            | Role::GearRotation
            | Role::GearShaftAngle
            | Role::GearTilt
            | Role::GearPressureShift
    ) || crate::surfacing_ui::is_angle(role)
}

pub(crate) fn number_label(role: Role) -> Option<&'static str> {
    Some(match role {
        Role::GearTeeth => "Teeth",
        Role::GearModule => "Module",
        Role::GearPressureAngle => "Pressure angle",
        Role::GearHelixAngle => "Helix angle",
        Role::GearWidth => "Face width",
        Role::GearBore => "Bore diameter",
        Role::GearRim => "Rim thickness",
        Role::GearPitchAngle => "Pitch angle",
        Role::GearLength => "Length",
        Role::GearHeight => "Body thickness",
        Role::GearBacklash => "Backlash",
        Role::GearX => "X",
        Role::GearY => "Y",
        Role::GearZ => "Z",
        Role::GearRotation => "Rotation",
        Role::GearCutter => "Cutter radius",
        Role::GearShaftAngle => "Shaft angle",
        Role::GearTilt => "Tilt",
        Role::GearMateTeeth => "Mate's teeth",
        Role::GearOffset => "Offset",
        Role::GearPressureShift => "Pressure shift",
        Role::GearEndRelief => "End relief",
        r => return crate::surfacing_ui::number_label(r),
    })
}

pub(crate) fn set_number(k: &mut FeatureKind, role: Role, v: f64, expr: String) {
    let FeatureKind::Gear(x) = k else {
        crate::surfacing_ui::set_number(k, role, v, expr);
        return;
    };
    let (value, text): (&mut f64, &mut String) = match role {
        Role::GearTeeth => {
            x.teeth = v.round().max(0.0) as u32;
            x.teeth_expr = expr;
            return;
        }
        Role::GearModule => (&mut x.module, &mut x.module_expr),
        Role::GearPressureAngle => (&mut x.pressure_angle, &mut x.pressure_angle_expr),
        Role::GearHelixAngle => (&mut x.helix_angle, &mut x.helix_angle_expr),
        Role::GearWidth => (&mut x.width, &mut x.width_expr),
        Role::GearBore => (&mut x.bore, &mut x.bore_expr),
        Role::GearRim => (&mut x.rim, &mut x.rim_expr),
        Role::GearPitchAngle => (&mut x.pitch_angle, &mut x.pitch_angle_expr),
        Role::GearLength => (&mut x.length, &mut x.length_expr),
        Role::GearHeight => (&mut x.height, &mut x.height_expr),
        Role::GearBacklash => (&mut x.backlash, &mut x.backlash_expr),
        Role::GearX => (&mut x.position[0], &mut x.position_expr[0]),
        Role::GearY => (&mut x.position[1], &mut x.position_expr[1]),
        Role::GearZ => (&mut x.position[2], &mut x.position_expr[2]),
        Role::GearRotation => (&mut x.rotation, &mut x.rotation_expr),
        Role::GearCutter => (&mut x.cutter_radius, &mut x.cutter_radius_expr),
        Role::GearShaftAngle => (&mut x.shaft_angle, &mut x.shaft_angle_expr),
        Role::GearTilt => (&mut x.tilt, &mut x.tilt_expr),
        Role::GearMateTeeth => {
            x.mate_teeth = v.round().max(0.0) as u32;
            x.mate_teeth_expr = expr;
            return;
        }
        Role::GearOffset => (&mut x.offset, &mut x.offset_expr),
        Role::GearPressureShift => (&mut x.pressure_shift, &mut x.pressure_shift_expr),
        Role::GearEndRelief => (&mut x.end_relief, &mut x.end_relief_expr),
        _ => return,
    };
    *value = v;
    *text = expr;
}

pub(crate) fn flip(k: &mut FeatureKind, role: Role) {
    crate::surfacing_ui::flip(k, role)
}

/// The face's ✕.
pub(crate) fn remove(k: &mut FeatureKind, role: Role, i: usize) {
    match (k, role) {
        (FeatureKind::Gear(x), Role::GearFace) => x.face = None,
        (k, r) => crate::surfacing_ui::remove(k, r, i),
    }
}
