//! Localised bearing contact for a face-milled spiral bevel pair: the pinion's machine-tool
//! settings that NASA CR-4259 (F. L. Litvin and H.-T. Lee, *Generation and Tooth Contact
//! Analysis of Spiral Bevel Gears With Predesigned Parabolic Functions of Transmission
//! Errors*, 1989) synthesises, its chapter 3 and the first program of its Appendix C.
//!
//! The gear is cut as Gleason cut it: both sides of a tooth space at once, by a cutter whose
//! blades are straight, on a machine whose cradle rolls with the gear as its crown gear would
//! ([`super::FaceMill::new`]). Cut by that same cutter from the other side of the pitch plane,
//! a pinion would be its exact conjugate: the two would touch along lines, and the least
//! misalignment under load would carry the contact to an edge of a tooth. Instead each side of
//! the pinion is cut on a machine of its own, chosen at the mean contact point M so that
//!
//! - the pinion's flank touches the gear's at M, a point (its contact is localised: the pair
//!   meets in an ellipse that travels across the tooth, of the half-length asked for under the
//!   elastic approach of the paint the teeth are checked with);
//! - the path of that ellipse on the gear's tooth runs at the angle asked for to its root;
//! - the gear ratio's derivative there is the one asked for: the transmission error is a
//!   parabola of the pinion's turn, the gear lagging, which absorbs the linear error a
//!   misalignment brings instead of letting the contact jump (the report's chapter 1).
//!
//! [`synthesise`] gives each side's machine: the cutter's blade angle and tip radius, the
//! radial setting, the cradle angle, the ratio of roll, the machining offset and the sliding
//! base. It is the report's main program step for step (the equation numbers are its), for a
//! left-hand gear: a right-hand pair is the mirror image of a left-hand one with the same
//! settings. It has been checked against the program (`straight_blade.f`, compiled) on the
//! report's example and on the gears sample's pair (tests).

use super::{Vec3, add, cross, dot, rot, scale, sub};
use std::f64::consts::{FRAC_PI_2, PI};

/// A pair's blanks, its gear's cutter, and the contact asked for. Angles in radians.
#[derive(Debug, Clone, Copy)]
pub(super) struct Design {
    /// The pinion's teeth over the gear's.
    pub ratio: f64,
    /// The shaft angle.
    pub shaft: f64,
    /// The root angles of the pinion and of the gear.
    pub root: [f64; 2],
    /// The gear's spiral angle at the middle of its face (its size: the pair is left hand).
    pub beta: f64,
    /// The gear's cutter: its average diameter (to the middle of the blades' tips), the
    /// width of its blades' tips (the point width) and the blades' angle (the pressure
    /// angle).
    pub cutter: f64,
    pub point: f64,
    pub alpha: f64,
    /// The cone distance at the middle of the face.
    pub mean: f64,
    /// The cone distance to put the mean contact point at (so the bearing is centred on the
    /// face), or None for where the program puts it: where each side of the gear's tooth
    /// crosses the pitch line with the cradle at its setting, half a slot from the cutter's
    /// average circle (so toward the toe on one side and the heel on the other).
    pub centre: Option<f64>,
    /// For the gear's convex side and its concave side: the angle from the gear's root to
    /// the path of contact on its tooth, the derivative of the gear ratio m′₂₁ (over m₂₁:
    /// as the program takes it), and the contact ellipse's half-length.
    pub path: [f64; 2],
    pub m21_prime: [f64; 2],
    pub ellipse: [f64; 2],
    /// The elastic approach the ellipse is measured at.
    pub approach: f64,
}

/// One member's machine for one side (the report's Table 1, with the cutter).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Machine {
    /// The blade's angle from the cutter's axis (acute an inside blade, obtuse an outside
    /// one) and the cutter's radius to the blade's tip.
    pub psi: f64,
    pub tip: f64,
    /// The radial setting and the cradle angle.
    pub radial: f64,
    pub q: f64,
    /// The ratio of roll: the cradle's turn over the work's.
    pub roll: f64,
    /// The machining offset E_m and the sliding base L_m (none for the gear).
    pub offset: f64,
    pub base: f64,
}

/// The machines of one side of the gear's teeth (its convex side or its concave one) and the
/// pinion's that meets it, and the mean contact point M.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Side {
    pub gear: Machine,
    pub pinion: Machine,
    /// M in the gear's pitch frame S_f (the report's Fig. 18: z along the pitch line from
    /// the apex, x square to the pitch plane away from the gear).
    pub contact: Vec3,
    /// How far the cradle has turned from the gear's setting when its cutter cuts M (φ_G,
    /// none where the program puts M), and where round the gear's and the pinion's cutters
    /// M is cut (θ_G, θ_P).
    pub phase: f64,
    pub theta: [f64; 2],
}

/// The pair's blanks and its two sides: [convex, concave] of the gear's teeth.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Pair {
    /// The pitch angles and dedendum angles: [pinion, gear].
    pub pitch: [f64; 2],
    pub dedendum: [f64; 2],
    pub sides: [Side; 2],
}

fn det(a: Vec3, b: Vec3, c: Vec3) -> f64 {
    dot(a, cross(b, c))
}

/// `p` turned by `a` about the unit vector `k` (the program's ROTATE).
fn turn(p: Vec3, a: f64, k: Vec3) -> Vec3 {
    rot(p, k, a)
}

/// The pinion's machines for a left-hand gear (the report's section 3, its program's lines
/// 131–768), or why there are none.
pub(super) fn synthesise(d: &Design) -> Result<Pair, &'static str> {
    let m21 = d.ratio;
    // Pitch angles (3.1), (3.2); dedendum angles (3.3); the gear's ratio of roll (3.7).
    let mut mu2 = (d.shaft.sin() / (m21 + d.shaft.cos())).atan();
    if mu2 < 0.0 {
        mu2 += PI;
    }
    let mu1 = d.shaft - mu2;
    let (d1, d2) = (mu1 - d.root[0], mu2 - d.root[1]);
    let (sn1, cs1, sn2, cs2) = (d1.sin(), d1.cos(), d2.sin(), d2.cos());
    let (tn1, tn2) = (d1.tan(), d2.tan());
    let (sm1, cm1, sm2, cm2) = (mu1.sin(), mu1.cos(), mu2.sin(), mu2.cos());
    let (sr1, cr1, sr2, cr2) = (
        d.root[0].sin(),
        d.root[0].cos(),
        d.root[1].sin(),
        d.root[1].cos(),
    );
    let mg2 = sm2 / cs2;
    let (a, adia, beta) = (d.mean, d.cutter, d.beta);
    // The gear's radial (3.9) and cradle angle (3.10): the cutter's average circle through
    // the middle of the pitch line, at the spiral angle to it.
    let sg = (adia * adia / 4.0 + a * a * cs2 * cs2 - a * adia * cs2 * beta.sin()).sqrt();
    let qg = ((a * a * cs2 * cs2 + sg * sg - adia * adia / 4.0) / (2.0 * a * sg * cs2)).acos();
    let mut sides = [None, None];
    for (i, slot) in sides.iter_mut().enumerate() {
        let convex = i == 0;
        // The gear's blade (section 2.2) and its tip radius (3.8).
        let psig = if convex { d.alpha } else { PI - d.alpha };
        let (sps, cps) = psig.sin_cos();
        let ctps = cps / sps;
        let rg = if convex {
            (adia - d.point) / 2.0
        } else {
            (adia + d.point) / 2.0
        };
        // The gear's flank with the cutter at the cradle angle `q` (its setting, or as the
        // cradle has turned from it while cutting: q − φ_G): θ_G where it crosses the line
        // midway between the roots (3.14), (3.15), u_G there from the equation of meshing
        // (2.43), and the point, its normal and the cutter's first principal direction there
        // ((2.1), (2.2), (2.9)) in S_f = S_p of the gear as it is then, [M_pm][M_mc] of (2.25),
        // (2.26).
        let locate = |q: f64| -> Option<(f64, f64, Vec3, Vec3, Vec3)> {
            let (sq, cq) = q.sin_cos();
            let to_p = |b: Vec3, at: f64| -> Vec3 {
                let m = [b[0], cq * b[1] - sq * b[2], sq * b[1] + cq * b[2]];
                let m = [m[0], m[1] - at * sg * sq, m[2] + at * sg * cq];
                [cs2 * m[0] - sn2 * m[2], m[1], sn2 * m[0] + cs2 * m[2]]
            };
            let ug_at = |th: f64| {
                rg * ctps * cps
                    - sg * ((mg2 - sr2) * cps * th.sin() - sq * cr2 * sps) / (cr2 * (th - q).sin())
            };
            let miss = |th: f64| {
                let u = ug_at(th);
                let b = to_p(
                    [rg * ctps - u * cps, u * sps * th.sin(), u * sps * th.cos()],
                    1.0,
                );
                b[0] - b[2] * (tn1 - tn2) / 2.0
            };
            let mut th = q - beta + FRAC_PI_2;
            for _ in 0..60 {
                let step = 1e-7;
                let slope = (miss(th + step) - miss(th - step)) / (2.0 * step);
                if !(slope.is_finite() && slope != 0.0) {
                    return None;
                }
                let dt = miss(th) / slope;
                th -= dt;
                if dt.abs() < 1e-14 {
                    let u = ug_at(th);
                    let (st, ct) = th.sin_cos();
                    let b = to_p([rg * ctps - u * cps, u * sps * st, u * sps * ct], 1.0);
                    let n = to_p([sps, cps * st, cps * ct], 0.0);
                    let e = to_p([0.0, ct, -st], 0.0);
                    return Some((th, u, b, n, e));
                }
            }
            None
        };
        // The mean contact point: with the cradle at its setting (the program), or as far
        // round as puts it at the cone distance asked for.
        let lost = "No mean contact point on the gear's flank";
        let q = match d.centre {
            None => qg,
            Some(c) => {
                let off = |q: f64| locate(q).map(|m| m.2[2] - c).ok_or(lost);
                let (mut q0, mut q1) = (qg, qg + 0.01);
                let (mut f0, mut f1) = (off(q0)?, off(q1)?);
                for _ in 0..40 {
                    if f1.abs() < 1e-11 * c || f1 == f0 {
                        break;
                    }
                    let q2 = q1 - f1 * (q1 - q0) / (f1 - f0);
                    (q0, f0, q1) = (q1, f1, q2);
                    f1 = off(q1)?;
                }
                if f1.abs() > 1e-9 * c {
                    return Err(lost);
                }
                q1
            }
        };
        let (th, ug, bf, nf, egi) = locate(q).ok_or(lost)?;
        let taug = th - q;
        // The pinion's blade angle (3.83) and τ_P (3.84)–(3.86).
        let mut psip = (cs1 * nf[0] - sn1 * nf[2]).asin();
        if convex {
            psip = PI - psip;
        }
        let (spp, cpp) = psip.sin_cos();
        let taup = (nf[1] / cpp).atan2((nf[0] - cs1 * spp) / (-sn1 * cpp));
        let (stp, ctp) = taup.sin_cos();
        // The gear's principal curvatures at M from its cutter's (2.10), (2.12) and the
        // relative motion of cutting, (3.17)–(3.31).
        let kgi = -ctps / ug;
        let kgii = 0.0;
        let egii = turn(egi, FRAC_PI_2, nf);
        let w2 = [-sm2, 0.0, cm2];
        let wg = [-mg2 * cs2, 0.0, -mg2 * sn2];
        let w2g = sub(w2, wg);
        let vt2 = cross(w2, bf);
        let vtg = cross(wg, bf);
        let vt2g = sub(vt2, vtg);
        let (vgi, vgii) = (dot(egi, vt2g), dot(egii, vt2g));
        let a13 = -kgi * vgi - det(w2g, nf, egi);
        let a23 = -kgii * vgii - det(w2g, nf, egii);
        let a33 = kgi * vgi * vgi + kgii * vgii * vgii
            - det(nf, w2g, vt2g)
            - dot(nf, sub(cross(w2, vtg), cross(wg, vt2)));
        let p = a23 * a23 - a13 * a13 + (kgi - kgii) * a33;
        let sigdbl = (2.0 * a13 * a23 / p).atan();
        let sigma = 0.5 * sigdbl;
        let t1 = p / (a33 * sigdbl.cos());
        let t2 = kgi + kgii - (a13 * a13 + a23 * a23) / a33;
        let (k2i, k2ii) = ((t1 + t2) / 2.0, (t2 - t1) / 2.0);
        let e2i = turn(egi, -sigma, nf);
        let e2ii = turn(e2i, FRAC_PI_2, nf);
        // The direction of the path of contact (3.44).
        let tnetag = (d.path[i] + sigma).tan();
        // The pair's meshing at M, (3.32)–(3.43), with the derivative of the gear ratio.
        let w2 = [-m21 * sm2, 0.0, m21 * cm2];
        let w1 = [-sm1, 0.0, -cm1];
        let w12 = sub(w1, w2);
        let vt2 = cross(w2, bf);
        let vt1 = cross(w1, bf);
        let vt12 = sub(vt1, vt2);
        let (v2i, v2ii) = (dot(vt12, e2i), dot(vt12, e2ii));
        let a13 = -k2i * v2i - det(w12, nf, e2i);
        let a23 = -k2ii * v2ii - det(w12, nf, e2ii);
        let a33 = k2i * v2i * v2i + k2ii * v2ii * v2ii
            - det(nf, w12, vt12)
            - dot(nf, sub(cross(w1, vt2), cross(w2, vt1)))
            + d.m21_prime[i] * det(nf, w2, bf);
        // The path of contact on the pinion (3.53), and its principal curvatures from the
        // contact ellipse asked for, (3.66)–(3.79).
        let tnetap =
            ((a33 + a13 * v2i) * tnetag - a13 * v2ii) / (a33 + a23 * (v2ii - v2i * tnetag));
        let tt = tnetap * tnetap;
        let n3 = (1.0 + tt) * a33;
        let n1 = (a13 * a13 - (a23 * tnetap) * (a23 * tnetap)) / n3;
        let n2 = (a23 + a13 * tnetap) * (a13 + a23 * tnetap) / n3;
        let axia = d.approach / (d.ellipse[i] * d.ellipse[i]);
        let ks2 = k2i + k2ii;
        let ks1 = ks2
            - ((4.0 * axia * axia - n1 * n1 - n2 * n2) * (1.0 + tt)
                / (-2.0 * axia * (1.0 + tt) + n1 * (tt - 1.0) - 2.0 * n2 * tnetap));
        let a11 = tt / (1.0 + tt) * (ks2 - ks1) + n1;
        let a12 = -tnetap / (1.0 + tt) * (ks2 - ks1) + n2;
        let a22 = 1.0 / (1.0 + tt) * (ks2 - ks1) - n1;
        let sigdbl = (2.0 * a12 / (k2i - k2ii - a11 + a22)).atan();
        let sigm12 = 0.5 * sigdbl;
        let g1 = 2.0 * a12 / sigdbl.sin();
        let (k1i, k1ii) = (0.5 * (ks1 + g1), 0.5 * (ks1 - g1));
        let e1i = turn(e2i, -sigm12, nf);
        // The pinion's cutter: its principal direction (3.92), the one nearer the gear's
        // cutter's; its curvature (3.94), (2.12); its u_P (3.95) and tip radius (3.99), (3.100).
        let mut epi = [sn1 * stp, ctp, cs1 * stp];
        if dot(egi, epi).clamp(-1.0, 1.0).acos() > PI / 4.0 {
            epi = scale(epi, -1.0);
        }
        let epii = turn(epi, FRAC_PI_2, nf);
        let snsigm = (e1i[1] * epi[2] - e1i[2] * epi[1]) / nf[0];
        let cssigm = dot(e1i, epi);
        let kpii = 0.0;
        let kpi = k1i * k1ii / (k1i * snsigm * snsigm + k1ii * cssigm * cssigm);
        let a11 = kpi - k1i * cssigm * cssigm - k1ii * snsigm * snsigm;
        let a12 = (k1i - k1ii) * snsigm * cssigm;
        let up = cpp / (kpi * spp);
        let bmx = -bf[0] * cs1 + bf[2] * sn1;
        let rp = (bmx + up * cpp) * spp / cpp;
        // The pinion's ratio of roll (3.107)–(3.145).
        let ([nx, ny, nz], [bx, by, bz]) = (nf, bf);
        let c11 = (ny * epi[2] - nz * epi[1]) * cs1 + (ny * epi[0] - nx * epi[1]) * sn1;
        let c12 = (nz * epi[1] - ny * epi[2]) * sm1 + (ny * epi[0] - nx * epi[1]) * cm1;
        let c22 = -(ny * epii[2] - nz * epii[1]) * sm1 + (ny * epii[0] - nx * epii[1]) * cm1;
        let t4 = (by * cr1) / (epii[0] * cs1 - epii[2] * sn1);
        let t1 = -c11 / kpi;
        let t2 = (a11 * kpii * t4 + a11 * c22 - a12 * c12) / (a12 * kpi);
        let u1 = scale(epi, t1);
        let u2 = add(scale(epi, t2), scale(epii, t4));
        let ([u11, u21, u31], [u12, u22, u32]) = (u1, u2);
        let v2 = (u22 * cs1 - u21 * sm1) * nz
            - (u11 * cm1 + u12 * sn1 + u32 * cs1 - u31 * sm1) * ny
            + (u21 * cm1 + u22 * sn1) * nx;
        let v3 = u22 * cm1 * nx + (u32 * sm1 - u12 * cm1) * ny - u22 * sm1 * nz;
        let k = bz * sm1 - bx * cm1;
        let h1 = [
            -u21 * cm1 + sn1 * k,
            u11 * cm1 - u31 * sm1 + by * sr1,
            u21 * sm1 + cs1 * k,
        ];
        let h2 = [
            (k - u22) * cm1,
            -(by - u12 * cm1 + u32 * sm1),
            -(k - u22) * sm1,
        ];
        let (f1, f2) = (dot(nf, h1), dot(nf, h2));
        let y2 = a12 * (2.0 * kpi * t1 * t2 - v2 - f1);
        let y3 =
            a12 * (kpi * t2 * t2 + kpii * t4 * t4 - v3 - f2) - (kpi * t2 + c12) * (kpii * t4 + c22);
        let mp1 = -y3 / y2;
        // The machining offset and sliding base (3.146), (3.147); the radial and cradle
        // angle (3.150), (3.151).
        let vx = u11 * mp1 + u12;
        let vy = u21 * mp1 + u22;
        let em = (by * cm1 - vx) / (mp1 * sn1) + by;
        let lm = (bx * cm1 - bz * sm1 + vy) / mp1 + bx * sn1 + bz * cs1;
        let z1 = -by + em - up * spp * stp;
        let z2 = bx * sn1 + bz * cs1 - lm - up * spp * ctp;
        let sp = z1.hypot(z2);
        // (The program's ATAN(Z1/Z2), in its quadrant: the cradle angle passes 90° as the
        // pinion's pitch angle gets small, at a shaft angle near 40° for 24 and 18 teeth.)
        let qp = z1.atan2(z2);
        let side = Side {
            gear: Machine {
                psi: psig,
                tip: rg,
                radial: sg,
                q: qg,
                roll: mg2,
                offset: 0.0,
                base: 0.0,
            },
            pinion: Machine {
                psi: psip,
                tip: rp,
                radial: sp,
                q: qp,
                roll: mp1,
                offset: em,
                base: lm,
            },
            contact: bf,
            phase: qg - q,
            theta: [th, taup - qp],
        };
        let numbers = [psip, rp, sp, qp, mp1, em, lm, taug];
        if !numbers.iter().all(|x| x.is_finite()) || rp <= 0.0 || mp1 <= 0.0 {
            return Err("The pinion's machine settings don't exist for this pair");
        }
        *slot = Some(side);
    }
    let [Some(convex), Some(concave)] = sides else {
        unreachable!("both sides are set or the function has returned");
    };
    Ok(Pair {
        pitch: [mu1, mu2],
        dedendum: [d1, d2],
        sides: [convex, concave],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The report's example (its Tables 2 and 3) and the settings its program computes for it
    /// (`straight_blade.f` compiled with gfortran, unit 72; Table 6 of the report).
    #[test]
    fn the_settings_of_the_reports_example() {
        let d = Design {
            ratio: 10.0 / 41.0,
            shaft: FRAC_PI_2,
            root: [
                (12.0 + 1.0 / 60.0_f64).to_radians(),
                (72.0 + 25.0 / 60.0_f64).to_radians(),
            ],
            beta: 35f64.to_radians(),
            cutter: 6.0,
            point: 0.08,
            alpha: 20f64.to_radians(),
            mean: 3.226,
            centre: None,
            path: [90f64.to_radians(), 75f64.to_radians()],
            m21_prime: [-3.5e-3, 5.2e-3],
            ellipse: [0.171, 0.181],
            approach: 0.00025,
        };
        let pair = synthesise(&d).unwrap();
        // (ψ_P °, r_P, s_P, q_P °, m_P1, E_m, L_m) and the gear's (s_G, q_G °).
        let want = [
            [
                163.443_912_459,
                2.964_692_434_56,
                2.993_309_116_44,
                63.186_945_134_4,
                0.228_996_215_031,
                0.174_043_780_772,
                0.021_230_832_121_3,
            ],
            [
                22.990_666_967_7,
                3.070_373_500_74,
                2.697_834_107_55,
                54.191_028_320_3,
                0.253_472_686_553,
                -0.244_586_918_835,
                0.052_131_774_190_9,
            ],
        ];
        for (side, w) in pair.sides.iter().zip(want) {
            let p = side.pinion;
            let got = [
                p.psi.to_degrees(),
                p.tip,
                p.radial,
                p.q.to_degrees(),
                p.roll,
                p.offset,
                p.base,
            ];
            for (g, w) in got.iter().zip(w) {
                assert!(
                    (g - w).abs() < 1e-9 * w.abs().max(1.0),
                    "{got:?} against {w:?}"
                );
            }
            assert!((side.gear.radial - 2.877_979_542_7).abs() < 1e-9);
            assert!((side.gear.q.to_degrees() - 58.636_487_666_2).abs() < 1e-8);
        }
    }

    /// The gears sample's pair as the Gear feature designs it (16 and 40 teeth, module 2 mm,
    /// face 12 mm, 35°; the cutter's radius the mean cone distance), with the mean contact
    /// point where the program puts it.
    fn sample() -> Design {
        Design {
            ratio: 0.4,
            shaft: FRAC_PI_2,
            root: [0.322_541_590_248_000_6, 1.132_325_162_818_167_2],
            beta: 35f64.to_radians(),
            cutter: 74.162_636_914_152_06,
            point: 0.651_267_885_290_483_6,
            alpha: 20f64.to_radians(),
            mean: 37.081_318_457_076_03,
            centre: None,
            path: [90f64.to_radians(), 75f64.to_radians()],
            m21_prime: [-0.005_463_414_634_146_339, 0.008_117_073_170_731_704],
            ellipse: [1.8, 1.908],
            approach: 0.00635,
        }
    }

    /// The sample against the program run on the same inputs: its gear's convex side (the
    /// program stops before the concave side: its TCA, a fixed ±30° of the pinion's turn, goes
    /// far past this pinion's ±11.25° and finds the surfaces crossing there).
    #[test]
    fn the_settings_of_the_gears_samples_pair() {
        let p = synthesise(&sample()).unwrap().sides[0].pinion;
        let got = [p.psi, p.tip, p.radial, p.q, p.roll, p.offset, p.base];
        let want = [
            2.856_415_030_304_815,
            41.017_169_368_626_3,
            37.878_622_058_403_15,
            1.274_597_142_376_838,
            0.364_857_224_193_464,
            1.859_329_454_726_073,
            0.580_627_194_394_843,
        ];
        for (g, w) in got.iter().zip(want) {
            assert!(
                (g - w).abs() < 1e-11 * w.abs().max(1.0),
                "{got:?} against {want:?}"
            );
        }
    }

    /// Centred, the mean contact point is at the middle of the face on both sides, still
    /// midway between the roots (on the pitch plane). (The program's TCA routines, run on
    /// these settings with the gear's φ_G there, find the two surfaces tangent at M to 1e-14
    /// and a parabolic transmission error of the report's level: −7.3″ and −9.7″ on the
    /// convex side where the teeth change over, +11.7″ and +14.1″ on the concave side.)
    #[test]
    fn a_centred_mean_contact_point_is_at_the_middle_of_the_face() {
        let d = Design {
            centre: Some(sample().mean),
            ..sample()
        };
        let pair = synthesise(&d).unwrap();
        let program = synthesise(&sample()).unwrap();
        for (side, uncentred) in pair.sides.iter().zip(program.sides) {
            let m = side.contact;
            assert!((m[2] - d.mean).abs() < 1e-9 && m[0].abs() < 1e-9, "{m:?}");
            // The program's M is half a slot along the pitch line from it.
            assert!((uncentred.contact[2] - d.mean).abs() > 1.5);
            assert!(side.phase.abs() > 0.01);
        }
    }
}
