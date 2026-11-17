//! Which of a shop's machines can make a part, and why not.
//!
//! The designer's cadrs measures the part ([`PartSummary`], from the kernel) and sends the
//! summary with the request for a quote, so a shop, or a phone, can match it against machines
//! ([`MachineCaps`]) without the kernel. Every refusal says why, in words a person can act on
//! ("10 mm is thicker than this laser cuts mild steel (6 mm)").

use serde::{Deserialize, Serialize};

/// How a machine makes parts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Process {
    Waterjet,
    Laser,
    Plasma,
    /// A CNC router cutting sheet.
    Router,
    /// A mill: 3-axis or more.
    Mill,
    /// A lathe.
    Lathe,
    /// A press brake: bends sheet.
    PressBrake,
    /// A 3D printer.
    Printer,
}

impl Process {
    /// Cuts 2D profiles through sheet.
    pub fn cuts_profiles(self) -> bool {
        matches!(
            self,
            Process::Waterjet | Process::Laser | Process::Plasma | Process::Router
        )
    }

    /// A typical cutting speed in mild steel of `thickness` mm, in mm/min: a rough guide for a
    /// first quote when the shop hasn't given its own speeds. `None` for processes that don't
    /// cut profiles.
    pub fn typical_speed(self, thickness: f64) -> Option<f64> {
        let t = thickness.max(0.5);
        Some(match self {
            Process::Waterjet => 2500.0 / t.powf(0.9),
            Process::Laser => (12_000.0 / t).min(20_000.0),
            Process::Plasma => (8000.0 / t.powf(0.8)).min(10_000.0),
            Process::Router => 2000.0,
            _ => return None,
        })
    }

    pub fn label(self) -> &'static str {
        match self {
            Process::Waterjet => "waterjet",
            Process::Laser => "laser",
            Process::Plasma => "plasma",
            Process::Router => "router",
            Process::Mill => "mill",
            Process::Lathe => "lathe",
            Process::PressBrake => "press brake",
            Process::Printer => "3D printer",
        }
    }
}

/// How thick a machine cuts one material.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MaterialLimit {
    /// The material's name, as the shop's materials name it ("mild steel").
    pub material: String,
    /// The thickest it cuts, in mm.
    pub max_thickness: f64,
    /// Cutting speed at a few thicknesses, (mm, mm/min), for quoting; sorted by thickness.
    pub speeds: Vec<(f64, f64)>,
}

impl MaterialLimit {
    /// The cutting speed at `thickness`, interpolated between the known ones (mm/min).
    pub fn speed_at(&self, thickness: f64) -> Option<f64> {
        let speeds = &self.speeds;
        let first = speeds.first()?;
        if thickness <= first.0 {
            return Some(first.1);
        }
        for pair in speeds.windows(2) {
            let ((t0, s0), (t1, s1)) = (pair[0], pair[1]);
            if thickness <= t1 {
                let f = (thickness - t0) / (t1 - t0).max(1e-9);
                return Some(s0 + f * (s1 - s0));
            }
        }
        speeds.last().map(|s| s.1)
    }
}

/// What a machine can do.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MachineCaps {
    pub name: String,
    pub process: Process,
    /// The largest part it takes: for profile cutters the bed (x, y); for others the work
    /// envelope (x, y, z). In mm.
    pub envelope: [f64; 3],
    pub materials: Vec<MaterialLimit>,
    /// The cut's width, in mm (profile cutters).
    pub kerf: f64,
    /// The smallest hole it cuts cleanly, as a multiple of the thickness (plasma ~1.5, waterjet
    /// and laser ~0.5–1).
    pub min_hole_ratio: f64,
    /// The tolerance it holds, ± mm.
    pub tolerance: f64,
    /// Seconds to pierce once (profile cutters), for quoting.
    pub pierce_seconds: f64,
}

/// What the designer's cadrs measured on a part.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PartSummary {
    pub name: String,
    /// The part is a flat profile of constant thickness (cuttable from sheet): its thickness.
    pub flat_thickness: Option<f64>,
    /// The bounding box, in mm, largest first.
    pub size: [f64; 3],
    /// The smallest hole's diameter, in mm.
    pub min_hole: Option<f64>,
    /// The smallest inside corner radius, in mm.
    pub min_inside_radius: Option<f64>,
    /// The length of every cut around the profile, in mm.
    pub cut_length: f64,
    /// How many separate cuts (each one a pierce).
    pub pierces: u32,
    /// The profile's area (what the part takes of a sheet), in mm².
    pub area: f64,
    /// The material the designer asked for.
    pub material: Option<String>,
    /// The tightest tolerance drawn, ± mm.
    pub tolerance: Option<f64>,
}

/// Can this machine make that part?
#[derive(Clone, Debug, PartialEq)]
pub struct Fit {
    pub machine: String,
    /// Empty when it can.
    pub problems: Vec<String>,
}

impl Fit {
    pub fn ok(&self) -> bool {
        self.problems.is_empty()
    }
}

/// Matches a part against one machine.
pub fn fit(machine: &MachineCaps, part: &PartSummary) -> Fit {
    let mut problems = Vec::new();
    let process = machine.process.label();
    if machine.process.cuts_profiles() {
        match part.flat_thickness {
            None => problems.push(format!(
                "it is not a flat profile of one thickness, which a {process} needs"
            )),
            Some(thickness) => {
                match part.material.as_deref() {
                    None => problems.push("no material was asked for".into()),
                    Some(material) => match machine
                        .materials
                        .iter()
                        .find(|m| m.material.eq_ignore_ascii_case(material))
                    {
                        None => problems.push(format!("this {process} doesn't cut {material}")),
                        Some(limit) if thickness > limit.max_thickness => problems.push(format!(
                            "{} mm is thicker than this {process} cuts {material} ({} mm)",
                            round(thickness),
                            round(limit.max_thickness)
                        )),
                        Some(_) => {}
                    },
                }
                if let Some(hole) = part.min_hole {
                    let smallest = machine.min_hole_ratio * thickness;
                    if hole < smallest {
                        problems.push(format!(
                            "a {} mm hole is smaller than this {process} cuts in {} mm ({} mm)",
                            round(hole),
                            round(thickness),
                            round(smallest)
                        ));
                    }
                }
                if let Some(radius) = part.min_inside_radius
                    && radius < machine.kerf / 2.0
                {
                    problems.push(format!(
                        "a {} mm inside corner is tighter than the {} mm cut allows",
                        round(radius),
                        round(machine.kerf)
                    ));
                }
            }
        }
        let [x, y, _] = machine.envelope;
        let [a, b, _] = part.size;
        let fits = (a <= x.max(y) && b <= x.min(y)) || (a <= x && b <= y);
        if !fits {
            problems.push(format!(
                "{} × {} mm doesn't fit the {} × {} mm bed",
                round(a),
                round(b),
                round(x),
                round(y)
            ));
        }
    } else {
        let mut envelope = machine.envelope;
        envelope.sort_by(|a, b| b.total_cmp(a));
        if part.size.iter().zip(envelope).any(|(p, m)| *p > m) {
            problems.push(format!(
                "{} × {} × {} mm doesn't fit its {} × {} × {} mm",
                round(part.size[0]),
                round(part.size[1]),
                round(part.size[2]),
                round(envelope[0]),
                round(envelope[1]),
                round(envelope[2])
            ));
        }
    }
    if let Some(tolerance) = part.tolerance
        && tolerance < machine.tolerance
    {
        problems.push(format!(
            "±{} mm is tighter than the ±{} mm it holds",
            round(tolerance),
            round(machine.tolerance)
        ));
    }
    Fit {
        machine: machine.name.clone(),
        problems,
    }
}

fn round(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub fn waterjet() -> MachineCaps {
        MachineCaps {
            name: "Flow Mach 500".into(),
            process: Process::Waterjet,
            envelope: [3000.0, 1500.0, 150.0],
            materials: vec![
                MaterialLimit {
                    material: "mild steel".into(),
                    max_thickness: 100.0,
                    speeds: vec![(3.0, 900.0), (10.0, 300.0), (50.0, 40.0)],
                },
                MaterialLimit {
                    material: "glass".into(),
                    max_thickness: 25.0,
                    speeds: vec![(10.0, 1200.0)],
                },
            ],
            kerf: 1.0,
            min_hole_ratio: 0.5,
            tolerance: 0.1,
            pierce_seconds: 4.0,
        }
    }

    pub fn bracket() -> PartSummary {
        PartSummary {
            name: "bracket".into(),
            flat_thickness: Some(10.0),
            size: [200.0, 80.0, 10.0],
            min_hole: Some(8.5),
            min_inside_radius: Some(3.0),
            cut_length: 900.0,
            pierces: 3,
            area: 12_000.0,
            material: Some("Mild Steel".into()),
            tolerance: None,
        }
    }

    #[test]
    fn a_waterjet_cuts_a_steel_bracket() {
        assert!(fit(&waterjet(), &bracket()).ok());
    }

    #[test]
    fn every_problem_is_named() {
        let mut part = bracket();
        part.flat_thickness = Some(120.0);
        part.min_hole = Some(20.0);
        part.size = [4000.0, 100.0, 120.0];
        part.tolerance = Some(0.01);
        let fit = fit(&waterjet(), &part);
        assert_eq!(fit.problems.len(), 4, "{:?}", fit.problems);
        assert!(fit.problems[0].contains("thicker"));
        assert!(fit.problems[1].contains("hole"));
        assert!(fit.problems[2].contains("bed"));
        assert!(fit.problems[3].contains("tighter"));
    }

    #[test]
    fn a_3d_part_needs_a_mill() {
        let mut part = bracket();
        part.flat_thickness = None;
        part.size = [100.0, 60.0, 40.0];
        assert!(!fit(&waterjet(), &part).ok());
        let mill = MachineCaps {
            name: "VMC".into(),
            process: Process::Mill,
            envelope: [40.0, 500.0, 300.0],
            materials: vec![],
            kerf: 0.0,
            min_hole_ratio: 0.0,
            tolerance: 0.02,
            pierce_seconds: 0.0,
        };
        assert!(fit(&mill, &part).ok());
    }

    #[test]
    fn small_holes_and_tight_corners() {
        let mut part = bracket();
        part.min_hole = Some(4.0);
        part.min_inside_radius = Some(0.2);
        let fit = fit(&waterjet(), &part);
        assert_eq!(fit.problems.len(), 2, "{:?}", fit.problems);
    }

    #[test]
    fn speed_interpolates() {
        let steel = &waterjet().materials[0];
        assert_eq!(steel.speed_at(1.0), Some(900.0));
        assert_eq!(steel.speed_at(6.5), Some(600.0));
        assert_eq!(steel.speed_at(80.0), Some(40.0));
    }

    #[test]
    fn typical_speeds_slow_down_with_thickness() {
        let thin = Process::Waterjet.typical_speed(3.0).unwrap();
        let thick = Process::Waterjet.typical_speed(30.0).unwrap();
        assert!(thin > 5.0 * thick);
        assert_eq!(Process::Mill.typical_speed(10.0), None);
    }
}
