//! Ported from springrep's tests/many.rs (MIT, https://github.com/springcraft-ai/springrep):
//! booleans of many solids at once, against closed forms and against the same booleans one at a
//! time. springrep's `difference_all(target, tools)` and `union_all(parts)` are cadrs's
//! `Kernel::boolean` with a target and many tools.

mod springrep_port;

use std::f64::consts::PI;

use cadrs_kernel::native::NativeKernel;
use cadrs_kernel::*;
use springrep_port::*;

fn at(x: f64, y: f64, z: f64) -> Plane {
    frame(P3::new(x, y, z), V3::z(), V3::x())
}

fn assert_many(k: &NativeKernel, b: BodyId, vol: f64, ar: f64) {
    assert_solid_rel(k, b, vol, ar, 1e-9);
}

fn holes(
    k: &mut NativeKernel,
    nx: usize,
    ny: usize,
    first: f64,
    pitch: f64,
    r: f64,
) -> Vec<BodyId> {
    let mut out = Vec::new();
    for i in 0..nx {
        for j in 0..ny {
            let f = at(first + i as f64 * pitch, first + j as f64 * pitch, -1.0);
            out.push(cylinder(k, &f, r, 4.0));
        }
    }
    out
}

/// One body from a boolean of a target and many tools.
fn many(k: &mut NativeKernel, op: BoolOp, target: BodyId, tools: &[BodyId]) -> BodyId {
    let r = k.boolean(op, target, tools).unwrap();
    match r.bodies.len() {
        1 => r.bodies[0],
        _ => k.compound(&r.bodies).unwrap().first().unwrap(),
    }
}

#[test]
fn a_plate_less_sixteen_holes_apart_is_the_plate_drilled() {
    let mut k = NativeKernel::new();
    let plate = pblock(&mut k, &world(), [20.0, 20.0, 2.0]);
    let tools = holes(&mut k, 4, 4, 2.5, 5.0, 1.0);
    let got = many(&mut k, BoolOp::Subtract, plate, &tools);
    let top = 400.0 - 16.0 * PI;
    assert_many(
        &k,
        got,
        top * 2.0,
        2.0 * top + 160.0 + 16.0 * 2.0 * PI * 2.0,
    );
    assert_eq!(solids(&k, Some(got)), 1);
}

#[test]
fn holes_that_overlap_cut_as_they_do_one_at_a_time() {
    let mut k = NativeKernel::new();
    let plate = pblock(&mut k, &world(), [12.0, 8.0, 2.0]);
    let tools = holes(&mut k, 4, 3, 2.0, 1.5, 1.0);
    let got = many(&mut k, BoolOp::Subtract, plate, &tools);
    let mut one = plate;
    for &t in &tools {
        one = difference(&mut k, one, t);
    }
    let (v, a) = (volume(&k, one), area(&k, one));
    assert_many(&k, got, v, a);
}

#[test]
fn blocks_in_an_overlapping_row_unite_into_one_bar() {
    let mut k = NativeKernel::new();
    let parts: Vec<BodyId> = (0..5)
        .map(|i| pblock(&mut k, &at(0.5 * i as f64, 0.0, 0.0), [1.0, 1.0, 1.0]))
        .collect();
    let got = many(&mut k, BoolOp::Union, parts[0], &parts[1..]);
    assert_many(&k, got, 3.0, 2.0 * (3.0 + 3.0 + 1.0));
    assert_eq!(solids(&k, Some(got)), 1);
    let mut one = parts[0];
    for &p in &parts[1..] {
        one = union(&mut k, one, p);
    }
    let (v, a) = (volume(&k, one), area(&k, one));
    assert_many(&k, got, v, a);
}

#[test]
fn solids_apart_unite_as_themselves() {
    let mut k = NativeKernel::new();
    let parts: Vec<BodyId> = (0..3)
        .map(|i| sphere(&mut k, &at(5.0 * i as f64, 0.0, 0.0), 1.0))
        .collect();
    let got = many(&mut k, BoolOp::Union, parts[0], &parts[1..]);
    assert_many(&k, got, 3.0 * 4.0 / 3.0 * PI, 3.0 * 4.0 * PI);
    assert_eq!(solids(&k, Some(got)), 3);
}

#[test]
fn nothing_united_is_nothing_and_nothing_taken_away_leaves_the_solid() {
    // (springrep's union of no solids: cadrs's boolean always has a target, so no counterpart.)
    let mut k = NativeKernel::new();
    let plate = pblock(&mut k, &world(), [2.0, 2.0, 2.0]);
    let got = many(&mut k, BoolOp::Subtract, plate, &[]);
    assert_many(&k, got, 8.0, 24.0);
}

#[test]
fn apart_solids_are_combined_in_one_boolean() {
    // springrep's `apart_groups` (how it batches tools that don't overlap into one boolean) is
    // internal to its booleans; cadrs exposes no counterpart.
}
