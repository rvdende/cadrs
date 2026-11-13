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
//! - A **Hypoid pair**'s member (`cadrs_core::gear::hypoid`): its *Axis* and its *Mate's axis*
//!   (optional; with both, the shaft angle, the offset and the hand are theirs), its teeth and
//!   its mate's, the module at the mean point, the spiral angle, **Cut as** its Gear or Pinion,
//!   the *Shaft angle* and *Offset* (without shafts), the face mill's *Cutter radius* and
//!   *Pressure shift*, *Backlash*, *End relief*, and (without shafts) where its pitch plane is.
//! - A **Worm drive**'s member (`cadrs_core::gear::worm`): **Cut as** its Wheel or its Worm (the
//!   two counts change places), the worm's *Starts* and the wheel's *Teeth*, the module, the
//!   worm's *Diameter quotient* and hand, the worm's *Length* or the wheel's *Face width*, the
//!   *Bore*, *Backlash* (taken off the worm), and where the drive is (the wheel's axis).
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

/// A pick into the gear's face field, or a hypoid member's shaft fields. Returns false if it
/// doesn't fit.
pub fn pick(world: &mut World, kind: &mut FeatureKind, field: AppliedField, pick: Pick) -> bool {
    if let (FeatureKind::Gear(x), AppliedField::GearShaft | AppliedField::GearMateShaft) =
        (&mut *kind, field)
    {
        let Some(features) = world
            .get_resource::<crate::ActiveDocument>()
            .and_then(|d| d.active_element())
            .map(|e| e.features().to_vec())
        else {
            return false;
        };
        let cache = world.resource::<PartCache>();
        let Some(a) = crate::pattern::axis_of(&features, cache, pick) else {
            return false;
        };
        let slot = if field == AppliedField::GearShaft {
            &mut x.shaft
        } else {
            &mut x.mate_shaft
        };
        *slot = if *slot == Some(a) { None } else { Some(a) };
        // Then the mate's.
        if field == AppliedField::GearShaft
            && x.shaft.is_some()
            && x.mate_shaft.is_none()
            && let Some(mut s) = world.get_resource_mut::<crate::applied::AppliedSession>()
        {
            s.field = AppliedField::GearMateShaft;
        }
        return true;
    }
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

/// The face (or a hypoid member's shafts), shown selected in the view while the dialog is
/// open.
pub fn references(kind: &FeatureKind, cache: &PartCache) -> Vec<Pick> {
    let FeatureKind::Gear(x) = kind else {
        return Vec::new();
    };
    let mut out: Vec<Pick> = x
        .face
        .iter()
        .map(|f| {
            cache
                .parts
                .iter()
                .find(|p| p.solid.faces.iter().any(|g| g.name == f.face))
                .map_or(Pick::Face(f.part, f.face), |p| Pick::Face(p.id, f.face))
        })
        .collect();
    out.extend(
        [&x.shaft, &x.mate_shaft]
            .into_iter()
            .flatten()
            .filter_map(|a| crate::transform_ui::axis_pick(cache, a)),
    );
    out
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
            "gear {:?} {:?} {} {} {} {:?} {:?} {} {} {}",
            x.gear_type,
            x.style,
            x.face.is_some(),
            x.left_hand,
            x.flip,
            x.spiral,
            x.member,
            x.shaft.is_some() && x.mate_shaft.is_some(),
            x.fast_preview,
            x.auto_teeth,
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
        (FeatureKind::Gear(x), Role::GearShaft) => Some(
            x.shaft
                .iter()
                .map(|a| crate::revolve::axis_label(features, a))
                .collect(),
        ),
        (FeatureKind::Gear(x), Role::GearMateShaft) => Some(
            x.mate_shaft
                .iter()
                .map(|a| crate::revolve::axis_label(features, a))
                .collect(),
        ),
        (k, r) => crate::surfacing_ui::items(features, cache, k, r),
    }
}

pub(crate) fn list_field(role: Role) -> Option<AppliedField> {
    match role {
        Role::GearFace => Some(AppliedField::GearFace),
        Role::GearShaft => Some(AppliedField::GearShaft),
        Role::GearMateShaft => Some(AppliedField::GearMateShaft),
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
        GearType::Worm => "model-gear-worm",
        GearType::Pair => "model-gear-pair",
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
        // (A hypoid pair's member is made whole: it takes no face.)
        if !matches!(
            x.gear_type,
            GearType::Hypoid | GearType::Worm | GearType::Pair
        ) {
            list(
                b,
                t,
                "gear-face-field",
                &tr("model-gear-face"),
                Role::GearFace,
                items_of(Role::GearFace),
                field == AppliedField::GearFace,
            );
        }
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
            hypoid_body(b, t, x, field, items_of);
            return;
        }
        if x.gear_type == GearType::Worm {
            worm_body(b, t, x);
            return;
        }
        if x.gear_type == GearType::Pair {
            pair_body(b, t, x, field, items_of);
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
                GearType::Rack | GearType::Hypoid | GearType::Worm | GearType::Pair => {}
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
fn hypoid_body(
    b: &mut ChildSpawner,
    t: &Theme,
    x: &cadrs_core::gear::GearFeature,
    field: AppliedField,
    items_of: &dyn Fn(Role) -> Vec<String>,
) {
    let num = |b: &mut ChildSpawner, name: &str, key: &str, role: Role, text: &str| {
        number(b, t, name, &tr(key), role, text, None);
    };
    // Its shaft and its mate's (optional): with both, the pair is laid on them.
    list(
        b,
        t,
        "gear-shaft-field",
        &tr("model-gear-shaft"),
        Role::GearShaft,
        items_of(Role::GearShaft),
        field == AppliedField::GearShaft,
    );
    list(
        b,
        t,
        "gear-mate-shaft-field",
        &tr("model-gear-mate-shaft"),
        Role::GearMateShaft,
        items_of(Role::GearMateShaft),
        field == AppliedField::GearMateShaft,
    );
    let on_shafts = x.shaft.is_some() && x.mate_shaft.is_some();
    fast_preview_row(b, t, x);
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
    // (On its shafts, the side of the offset sets the hand.)
    if !on_shafts {
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
    }
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
    if !on_shafts {
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
    }
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
    if on_shafts {
        return;
    }
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

/// A worm drive's member: the worm's starts and the wheel's teeth, the module, the worm's
/// diameter quotient, its hand, which member this is, its length or face width, its bore, and
/// where the drive is (the wheel's axis its axis, the worm's along its x).
fn worm_body(b: &mut ChildSpawner, t: &Theme, x: &cadrs_core::gear::GearFeature) {
    let num = |b: &mut ChildSpawner, name: &str, key: &str, role: Role, text: &str| {
        number(b, t, name, &tr(key), role, text, None);
    };
    let members = opts(
        &BevelMember::ALL,
        |k| {
            tr(match k {
                BevelMember::Gear => "model-gear-member-wheel",
                _ => "model-gear-member-worm",
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
    let worm = x.member != BevelMember::Gear;
    if !worm {
        fast_preview_row(b, t, x);
    }
    let (own, mate) = if worm {
        ("model-gear-starts", "model-gear-mate-teeth")
    } else {
        ("model-gear-teeth", "model-gear-worm-starts")
    };
    num(b, "gear-teeth", own, Role::GearTeeth, &x.teeth_expr);
    num(
        b,
        "gear-mate-teeth",
        mate,
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
        "gear-quotient",
        "model-gear-worm-quotient",
        Role::GearQuotient,
        &x.worm_quotient_expr,
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
    if worm {
        num(
            b,
            "gear-length",
            "model-length",
            Role::GearLength,
            &x.length_expr,
        );
    } else {
        num(
            b,
            "gear-width",
            "model-gear-face-width",
            Role::GearWidth,
            &x.width_expr,
        );
    }
    num(
        b,
        "gear-bore",
        "model-gear-bore",
        Role::GearBore,
        &x.bore_expr,
    );
    num(
        b,
        "gear-backlash",
        "model-gear-backlash",
        Role::GearBacklash,
        &x.backlash_expr,
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
        &tr("model-gear-wheel-axis"),
        Role::GearAxis,
        &axes,
        index_of(&GearAxis::ALL, &x.axis),
        None,
    );
}

/// **Fast preview**: a generated gear shown as its blank alone, at once (to move its shafts
/// and see the pair), its teeth cut once it's off.
fn fast_preview_row(b: &mut ChildSpawner, t: &Theme, x: &cadrs_core::gear::GearFeature) {
    b.spawn(
        OptionRow::new("gear-fast-preview", tr("model-gear-fast-preview"))
            .checked(x.fast_preview)
            .build(t),
    );
}

/// A gear pair: its shafts (with both, the shaft angle and offset are theirs), its ratio, its
/// tooth counts (chosen to suit, or typed), what it makes of them (a hypoid pair or a worm
/// drive, and its teeth), the module, the spiral and pressure angles, the hand, the backlash,
/// Fast preview; without shafts, the shaft angle and offset and where it is.
fn pair_body(
    b: &mut ChildSpawner,
    t: &Theme,
    x: &cadrs_core::gear::GearFeature,
    field: AppliedField,
    items_of: &dyn Fn(Role) -> Vec<String>,
) {
    let num = |b: &mut ChildSpawner, name: &str, key: &str, role: Role, text: &str| {
        number(b, t, name, &tr(key), role, text, None);
    };
    list(
        b,
        t,
        "gear-shaft-field",
        &tr("model-gear-pair-gear-shaft"),
        Role::GearShaft,
        items_of(Role::GearShaft),
        field == AppliedField::GearShaft,
    );
    list(
        b,
        t,
        "gear-mate-shaft-field",
        &tr("model-gear-pair-pinion-shaft"),
        Role::GearMateShaft,
        items_of(Role::GearMateShaft),
        field == AppliedField::GearMateShaft,
    );
    let on_shafts = x.shaft.is_some() && x.mate_shaft.is_some();
    num(
        b,
        "gear-ratio",
        "model-gear-ratio",
        Role::GearRatio,
        &x.ratio_expr,
    );
    b.spawn(
        OptionRow::new("gear-auto-teeth", tr("model-gear-auto-teeth"))
            .checked(x.auto_teeth)
            .build(t),
    );
    if !x.auto_teeth {
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
            "model-gear-pinion-teeth",
            Role::GearMateTeeth,
            &x.mate_teeth_expr,
        );
    }
    // What the pair makes of it (as typed: on shafts the view shows it where it stands).
    if !on_shafts {
        let text = pair_made_text(x);
        b.spawn((
            PairMade,
            Name::new("gear-pair-made"),
            t.text(
                text,
                t.font_sm,
                bevy::text::FontWeight::NORMAL,
                t.muted_foreground,
            ),
        ));
    }
    num(
        b,
        "gear-module",
        "model-gear-module",
        Role::GearModule,
        &x.module_expr,
    );
    num(
        b,
        "gear-helix-angle",
        "model-gear-spiral-angle",
        Role::GearHelixAngle,
        &x.helix_angle_expr,
    );
    num(
        b,
        "gear-pressure-angle",
        "model-gear-pressure-angle",
        Role::GearPressureAngle,
        &x.pressure_angle_expr,
    );
    if !on_shafts {
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
    }
    num(
        b,
        "gear-backlash",
        "model-gear-backlash",
        Role::GearBacklash,
        &x.backlash_expr,
    );
    fast_preview_row(b, t, x);
    if on_shafts {
        return;
    }
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
            Role::GearQuotient => &x.worm_quotient_expr,
            Role::GearRatio => &x.ratio_expr,
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
            // A worm drive: its wheel to a single-start worm.
            if x.gear_type == GearType::Worm {
                if x.member == BevelMember::ConjugatePinion {
                    x.member = BevelMember::Pinion;
                }
                let starts = if x.member == BevelMember::Gear {
                    &mut x.mate_teeth
                } else {
                    &mut x.teeth
                };
                if *starts > 8 {
                    *starts = 1;
                }
                x.teeth_expr = x.teeth.to_string();
                x.mate_teeth_expr = x.mate_teeth.to_string();
            }
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
        Role::GearMember => {
            let was_gear = x.member == BevelMember::Gear;
            x.member = BevelMember::ALL[i.min(BevelMember::ALL.len() - 1)];
            // A pair's other member: its teeth and its mate's change places, so the numbers
            // are still the pair's.
            if matches!(x.gear_type, GearType::Hypoid | GearType::Worm)
                && was_gear != (x.member == BevelMember::Gear)
            {
                std::mem::swap(&mut x.teeth, &mut x.mate_teeth);
                std::mem::swap(&mut x.teeth_expr, &mut x.mate_teeth_expr);
            }
        }
        _ => {}
    }
}

pub(crate) fn tab(k: &mut FeatureKind, role: Role, i: usize) {
    crate::surfacing_ui::tab(k, role, i)
}

pub(crate) fn is_checkbox(name: &str) -> bool {
    name == "gear-across-checkbox"
        || name == "gear-at-apex-checkbox"
        || name == "gear-fast-preview-checkbox"
        || name == "gear-auto-teeth-checkbox"
        || crate::surfacing_ui::is_checkbox(name)
}

pub(crate) fn checkbox(k: &mut FeatureKind, name: &str, on: bool) -> Option<&'static str> {
    match (k, name) {
        (FeatureKind::Gear(x), "gear-across-checkbox") => {
            x.flip = on;
            Some("Teeth across the face")
        }
        (FeatureKind::Gear(x), "gear-auto-teeth-checkbox") => {
            x.auto_teeth = on;
            Some("Auto teeth")
        }
        (FeatureKind::Gear(x), "gear-fast-preview-checkbox") => {
            x.fast_preview = on;
            Some("Fast preview")
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
        Role::GearQuotient => "Diameter quotient",
        Role::GearRatio => "Ratio",
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
        Role::GearQuotient => (&mut x.worm_quotient, &mut x.worm_quotient_expr),
        Role::GearRatio => (&mut x.ratio, &mut x.ratio_expr),
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
        (FeatureKind::Gear(x), Role::GearShaft) => x.shaft = None,
        (FeatureKind::Gear(x), Role::GearMateShaft) => x.mate_shaft = None,
        (k, r) => crate::surfacing_ui::remove(k, r, i),
    }
}

/// The line under a gear pair's ratio: what it makes of it.
#[derive(Component)]
pub(crate) struct PairMade;

/// What a gear pair makes of its ratio, at its shaft angle and offset as typed.
fn pair_made_text(x: &cadrs_core::gear::GearFeature) -> String {
    let (family, members) = cadrs_core::gear::pair_members(x, x.shaft_angle, x.offset);
    match family {
        cadrs_core::gear::PairFamily::Hypoid => tr_with(
            "model-gear-pair-hypoid",
            &[
                ("pinion", &members[1].teeth.to_string()),
                ("gear", &members[0].teeth.to_string()),
            ],
        ),
        cadrs_core::gear::PairFamily::Worm => tr_with(
            "model-gear-pair-worm",
            &[
                ("starts", &members[1].teeth.to_string()),
                ("gear", &members[0].teeth.to_string()),
            ],
        ),
    }
}

/// Keeps the gear pair's line in step with its ratio (changed from the view, or by undo).
pub(crate) fn update_pair_made(
    session: Option<Res<crate::applied::AppliedSession>>,
    doc: Option<Res<crate::ActiveDocument>>,
    over: Res<crate::parts::PartOverride>,
    mut q: Query<&mut Text, With<PairMade>>,
) {
    if q.is_empty() {
        return;
    }
    let Some(s) = session.as_deref() else {
        return;
    };
    let dragged = over
        .applied
        .as_ref()
        .filter(|(f, _)| *f == s.feature)
        .map(|(_, k)| k.clone());
    let kind = dragged.or_else(|| {
        Some(
            doc.as_deref()?
                .doc
                .element(s.element)?
                .feature(s.feature)?
                .kind
                .clone(),
        )
    });
    let Some(FeatureKind::Gear(x)) = kind else {
        return;
    };
    let text = pair_made_text(&x);
    for mut t in &mut q {
        if t.0 != text {
            t.0 = text.clone();
        }
    }
}

// ---------------------------------------------------------------------------------------------
// A gear pair in the view: its pitch circles, its ratio, its size

/// A shaft as the view has it: a sketch line's (its own way), a cylinder's (toward its middle),
/// a straight edge's.
fn shaft_of(
    features: &[Feature],
    cache: &PartCache,
    a: &cadrs_core::document::AxisRef,
) -> Option<cadrs_core::gear::Shaft> {
    use cadrs_core::document::AxisRef;
    match a {
        AxisRef::SketchCurve { sketch, curve } => {
            let sk = features.iter().find(|f| f.id == *sketch)?.sketch()?;
            let frame = sk.plane?.frame();
            let g = &sk.geometry;
            match g.curves.get(*curve)?.kind {
                cadrs_sketch::CurveKind::Line { a, b } => {
                    let (p, q) = (frame.to_world(g.pos(a)), frame.to_world(g.pos(b)));
                    Some((p, [q[0] - p[0], q[1] - p[1], q[2] - p[2]], None))
                }
                _ => None,
            }
        }
        AxisRef::Face(f) => {
            let face = cache.parts.iter().find_map(|p| p.solid.face(&f.face))?;
            let (o, d) = face.axis?;
            Some((o, d, face.center))
        }
        AxisRef::Edge(e) => {
            let edge = cache.parts.iter().find_map(|p| p.solid.edge(&e.edge))?;
            let (p, q) = (edge.points.first()?, edge.points.last()?);
            Some((*p, [q[0] - p[0], q[1] - p[1], q[2] - p[2]], None))
        }
        AxisRef::Connector(_) => None,
    }
}

/// The gear pair being edited, as it stands now (as dragged while dragged): its feature, its
/// parameters, and its [`cadrs_core::gear::PairInfo`].
fn pair_now(
    session: Option<&crate::applied::AppliedSession>,
    doc: Option<&crate::ActiveDocument>,
    cache: &PartCache,
    knob: &PairKnob,
) -> Option<(
    cadrs_core::FeatureId,
    cadrs_core::gear::GearFeature,
    cadrs_core::gear::PairInfo,
)> {
    let s = session.filter(|s| s.kind == crate::applied::AppliedKind::Gear)?;
    let el = doc?.doc.element(s.element)?;
    let kind = match knob.drag.as_ref() {
        Some(d) => &d.kind,
        None => &el.feature(s.feature)?.kind,
    };
    let FeatureKind::Gear(x) = kind else {
        return None;
    };
    let x = x.clone();
    if x.gear_type != GearType::Pair {
        return None;
    }
    let features = el.features();
    let shafts = match (&x.shaft, &x.mate_shaft) {
        (Some(a), Some(b)) => Some([shaft_of(features, cache, a)?, shaft_of(features, cache, b)?]),
        _ => None,
    };
    let info = cadrs_core::gear::pair_info(&x, shafts.as_ref())?;
    Some((s.feature, x, info))
}

/// The gear pair's size knob (on its gear's pitch circle: dragged out or in, the module
/// follows), and its label's place.
#[derive(Resource, Debug, Default)]
pub struct PairKnob {
    /// The knob on screen (window px).
    pub at: Option<Vec2>,
    pub hovered: bool,
    pub drag: Option<KnobDrag>,
    /// The gear's pitch circle (centre, axis, radius) as last drawn.
    circle: Option<([f64; 3], [f64; 3], f64)>,
    /// The gear's teeth.
    teeth: u32,
}

#[derive(Debug, Clone)]
pub struct KnobDrag {
    pub kind: FeatureKind,
}

const KNOB_GRAB: f32 = 9.0;

/// The pair's label (its ratio and teeth), over the gear.
#[derive(Component)]
pub(crate) struct PairLabel;

#[derive(Component)]
pub(crate) struct PairLabelText;

fn v3(a: [f64; 3]) -> Vec3 {
    Vec3::new(a[0] as f32, a[1] as f32, a[2] as f32)
}

/// Draws the pair's pitch circles and the knob, and places its label.
#[allow(clippy::too_many_arguments)]
pub(crate) fn draw_pair(
    session: Option<Res<crate::applied::AppliedSession>>,
    doc: Option<Res<crate::ActiveDocument>>,
    cache: Res<PartCache>,
    view: Res<crate::viewport::ViewportView>,
    rect: Res<crate::viewport::ViewportRect>,
    theme: Res<Theme>,
    mut knob: ResMut<PairKnob>,
    mut g: Gizmos<crate::transform_ui::RotateRingGizmos>,
    q_area: Query<Entity, With<crate::viewport::ViewportArea>>,
    mut q_label: Query<(Entity, &mut Node, &mut Visibility), With<PairLabel>>,
    mut q_text: Query<&mut Text, With<PairLabelText>>,
    mut commands: Commands,
) {
    let found = pair_now(session.as_deref(), doc.as_deref(), &cache, &knob);
    let Some((_, x, info)) = found else {
        knob.at = None;
        knob.circle = None;
        for (e, ..) in &q_label {
            commands.entity(e).try_despawn();
        }
        return;
    };
    let hot = knob.hovered || knob.drag.is_some();
    let blue = Color::srgb_u8(0x4a, 0x8c, 0xf0);
    let orange = Color::srgb_u8(0xff, 0xb4, 0x5a);
    // Each member's pitch circle.
    for (i, (c, a, r)) in info.circles.iter().enumerate() {
        let (c, a) = (v3(*c), v3(*a).normalize_or_zero());
        let iso = Isometry3d::new(c, Quat::from_rotation_arc(Vec3::Z, a));
        let col = if i == 0 && hot {
            orange
        } else {
            blue.with_alpha(0.7)
        };
        g.circle(iso, *r as f32, col).resolution(96);
    }
    // The knob on the gear's circle, on the side toward the pinion's.
    let (c, a, r) = info.circles[0];
    let (cv, av) = (v3(c), v3(a).normalize_or_zero());
    let toward = v3(info.circles[1].0) - cv;
    let out = (toward - av * toward.dot(av)).normalize_or(av.any_orthonormal_vector());
    let knob_p = cv + out * r as f32;
    let col = if hot { orange } else { blue };
    let k = 6.0 * view.view.scale;
    g.sphere(Isometry3d::from_translation(knob_p), k, col);
    knob.at = Some(rect.to_screen(view.view.project(knob_p)));
    knob.circle = Some((c, a, r));
    knob.teeth = info.teeth[0];
    // The label, over the gear's circle (the side away from the pinion).
    let label_at = rect.to_screen(view.view.project(cv - out * r as f32)) - rect.0.min;
    let text = format!(
        "{} : 1  ·  {} / {} {}  ·  {}",
        cadrs_core::hole::fmt(x.ratio),
        info.teeth[1],
        info.teeth[0],
        tr("model-gear-teeth").to_lowercase(),
        match info.family {
            cadrs_core::gear::PairFamily::Hypoid => tr("model-gear-hypoid"),
            cadrs_core::gear::PairFamily::Worm => tr("model-gear-worm"),
        }
    );
    if let Some((_, mut node, mut vis)) = q_label.iter_mut().next() {
        node.left = Val::Px(label_at.x + 8.0);
        node.top = Val::Px(label_at.y - 12.0);
        vis.set_if_neq(Visibility::Inherited);
        if let Some(mut t) = q_text.iter_mut().next()
            && t.0 != text
        {
            t.0 = text;
        }
        return;
    }
    let Some(area) = q_area.iter().next() else {
        return;
    };
    let t = &*theme;
    let e = commands
        .spawn((
            Name::new("gear-pair-label"),
            PairLabel,
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(label_at.x + 8.0),
                top: Val::Px(label_at.y - 12.0),
                padding: UiRect::axes(Val::Px(6.0), Val::Px(3.0)),
                column_gap: Val::Px(6.0),
                align_items: AlignItems::Center,
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(4.0)),
                ..default()
            },
            BackgroundColor(t.popover),
            BorderColor::all(t.border),
            DespawnOnExit(crate::AppState::Document),
            bevy::ui_widgets::observe(
                |_: On<bevy::picking::events::PointerClick>, mut commands: Commands| {
                    commands.queue(focus_ratio);
                },
            ),
        ))
        .observe(
            |mut ev: On<bevy::picking::events::PointerScroll>, mut commands: Commands| {
                ev.propagate(false);
                let by = if ev.y > 0.0 {
                    1
                } else if ev.y < 0.0 {
                    -1
                } else {
                    0
                };
                if by != 0 {
                    commands.queue(move |w: &mut World| step_ratio(w, by));
                }
            },
        )
        .with_children(|l| {
            let step =
                |l: &mut ChildSpawnerCommands, name: &'static str, glyph: &'static str, by: i32| {
                    l.spawn((
                        Name::new(name),
                        Node {
                            width: Val::Px(18.0),
                            height: Val::Px(18.0),
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            border_radius: BorderRadius::all(Val::Px(3.0)),
                            ..default()
                        },
                        BackgroundColor(t.secondary),
                        bevy::ui_widgets::observe(
                            move |mut ev: On<bevy::picking::events::PointerClick>,
                                  mut commands: Commands| {
                                // (Not the label's click: that focuses the Ratio field.)
                                ev.propagate(false);
                                commands.queue(move |w: &mut World| step_ratio(w, by));
                            },
                        ),
                    ))
                    .with_children(|b| {
                        b.spawn((
                            Pickable::IGNORE,
                            t.text(glyph, t.font_sm, bevy::text::FontWeight::BOLD, t.foreground),
                        ));
                    });
                };
            step(l, "gear-pair-ratio-down", "−", -1);
            l.spawn((
                PairLabelText,
                Pickable::IGNORE,
                t.text(
                    text,
                    t.font_sm,
                    bevy::text::FontWeight::MEDIUM,
                    t.foreground,
                ),
            ));
            step(l, "gear-pair-ratio-up", "+", 1);
        })
        .id();
    commands.entity(area).add_child(e);
}

/// The ratios the pair's label steps through (its − and +, the wheel over it).
const RATIOS: [f64; 22] = [
    1.0, 1.25, 1.5, 2.0, 2.5, 3.0, 3.5, 4.0, 5.0, 6.0, 7.5, 8.0, 10.0, 12.0, 15.0, 20.0, 25.0,
    30.0, 40.0, 50.0, 60.0, 80.0,
];

/// The gear pair's ratio a step up (`by` 1) or down (−1) [`RATIOS`] from where it is: one
/// command.
fn step_ratio(world: &mut World, by: i32) {
    crate::applied::change_kind(world, "Gear ratio", |k| {
        let FeatureKind::Gear(x) = k else {
            return;
        };
        let r = x.ratio;
        let next = if by > 0 {
            RATIOS.iter().copied().find(|&v| v > r + 1e-9)
        } else {
            RATIOS.iter().rev().copied().find(|&v| v < r - 1e-9)
        };
        if let Some(v) = next {
            x.ratio = v;
            x.ratio_expr = cadrs_core::hole::fmt(v);
        }
    });
}

/// Puts the cursor in the dialog's Ratio field (the label clicked).
fn focus_ratio(world: &mut World) {
    let mut q = world.query::<(Entity, &Name)>();
    let Some(root) = q
        .iter(world)
        .find(|(_, n)| n.as_str() == "gear-ratio")
        .map(|(e, _)| e)
    else {
        return;
    };
    let mut q = world.query::<(Entity, &cadrs_ui::NumberFieldEdit)>();
    let edit = q.iter(world).find(|(_, e)| e.0 == root).map(|(e, _)| e);
    if let Some(e) = edit {
        world
            .resource_mut::<bevy::input_focus::InputFocus>()
            .set(e, bevy::input_focus::FocusCause::Pressed);
    }
}

/// Grabs, drags and releases the size knob: the gear's pitch radius where the pointer's ray
/// meets its plane, the module following (to a hundredth of a millimetre).
#[allow(clippy::too_many_arguments)]
pub(crate) fn pair_knob_pointer(
    mut inputs: MessageReader<bevy::picking::pointer::PointerInput>,
    session: Option<Res<crate::applied::AppliedSession>>,
    doc: Option<Res<crate::ActiveDocument>>,
    mut knob: ResMut<PairKnob>,
    view: Res<crate::viewport::ViewportView>,
    rect: Res<crate::viewport::ViewportRect>,
    drag: Res<crate::viewport::ViewportDrag>,
    mut over: ResMut<crate::parts::PartOverride>,
    mut grab: ResMut<crate::assembly::ViewportGrab>,
    mut commands: Commands,
) {
    use bevy::picking::pointer::{PointerAction, PointerButton, PointerId};
    let at = knob.at;
    let near = |p: Vec2| at.is_some_and(|k| k.distance(p) <= KNOB_GRAB);
    knob.hovered = knob.drag.is_some() || near(drag.pointer());
    let (Some(s), Some((c, a, _))) = (session.as_ref(), knob.circle) else {
        inputs.clear();
        return;
    };
    let current = doc
        .as_ref()
        .and_then(|d| Some(d.doc.element(s.element)?.feature(s.feature)?.kind.clone()));
    for input in inputs.read() {
        if input.pointer_id != PointerId::Mouse {
            continue;
        }
        let pos = input.location.position;
        match input.action {
            PointerAction::Press(PointerButton::Primary) => {
                if near(pos)
                    && let Some(k @ FeatureKind::Gear(_)) = current.as_ref()
                {
                    knob.drag = Some(KnobDrag { kind: k.clone() });
                    grab.0 = true;
                }
            }
            PointerAction::Move { .. } => {
                let teeth = knob.teeth.max(1) as f64;
                let Some(d) = knob.drag.as_mut() else {
                    continue;
                };
                let (o, dir) = view.view.ray(rect.offset(pos));
                let (cv, av) = (v3(c), v3(a).normalize_or_zero());
                let den = dir.dot(av);
                if den.abs() < 1e-4 {
                    continue;
                }
                let t = (cv - o).dot(av) / den;
                let r = (o + dir * t - cv).length() as f64;
                let m = ((2.0 * r / teeth) * 100.0).round() / 100.0;
                if let FeatureKind::Gear(x) = &mut d.kind
                    && m > 0.0
                    && (x.module - m).abs() > 1e-9
                {
                    x.module = m;
                    x.module_expr = format!("{} mm", cadrs_core::hole::fmt(m));
                }
                let want = Some((s.feature, d.kind.clone()));
                if over.applied != want {
                    over.applied = want;
                }
            }
            PointerAction::Release(PointerButton::Primary) if knob.drag.is_some() => {
                commands.queue(|world: &mut World| {
                    world.resource_mut::<crate::parts::PartOverride>().applied = None;
                    let Some(d) = world.resource_mut::<PairKnob>().drag.take() else {
                        return;
                    };
                    if crate::applied::current(world).is_some_and(|f| f.kind != d.kind) {
                        crate::applied::set(world, d.kind, "Drag size");
                    }
                });
            }
            PointerAction::Cancel => {
                knob.drag = None;
                over.applied = None;
            }
            _ => {}
        }
    }
}
