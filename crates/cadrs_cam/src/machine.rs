//! Machines: what kind of cutting they do, their travel, rates and controller, and the post
//! settings ([`crate::post::PostSettings`]) a job starts from.
//!
//! The catalogue ([`catalog`]) is generic machines grouped by kind and controller (a 3-axis
//! mill on Fanuc, LinuxCNC or Haas-style controls, a GRBL router, a GRBL diode or CO₂ laser, a
//! LinuxCNC/QtPlasmaC or Mach3 plasma table, a waterjet). A job copies its machine, so editing
//! the job's travel or post settings never changes the catalogue.

use serde::{Deserialize, Serialize};

use crate::post::{Dialect, PostSettings};

/// What a machine does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MachineKind {
    /// A vertical machining centre or mill: rigid, flood coolant, tool changer.
    Mill,
    /// A CNC router: large bed, light cuts, usually manual tool changes.
    Router,
    /// A laser cutter or engraver (beam on/off, power).
    Laser,
    /// A plasma table (torch on/off, pierce, torch height control).
    Plasma,
    /// An abrasive waterjet (pierce, kerf).
    Waterjet,
}

impl MachineKind {
    pub const ALL: [MachineKind; 5] = [
        MachineKind::Mill,
        MachineKind::Router,
        MachineKind::Laser,
        MachineKind::Plasma,
        MachineKind::Waterjet,
    ];

    pub fn label(self) -> &'static str {
        match self {
            MachineKind::Mill => "Milling machine",
            MachineKind::Router => "CNC router",
            MachineKind::Laser => "Laser cutter",
            MachineKind::Plasma => "Plasma cutter",
            MachineKind::Waterjet => "Waterjet",
        }
    }

    /// The machine-kind icon (icon-rs).
    pub fn icon(self) -> &'static str {
        match self {
            MachineKind::Mill => "cam-machine",
            MachineKind::Router => "cam-router",
            MachineKind::Laser => "cam-laser",
            MachineKind::Plasma => "cam-plasma",
            MachineKind::Waterjet => "cam-waterjet",
        }
    }

    /// Cuts through sheet with a beam: profiles are cut in one pass at the bottom of the
    /// material, holes first, with pierces and lead-ins.
    pub fn is_cutting(self) -> bool {
        matches!(
            self,
            MachineKind::Laser | MachineKind::Plasma | MachineKind::Waterjet
        )
    }

    /// Spins a cutter.
    pub fn is_milling(self) -> bool {
        matches!(self, MachineKind::Mill | MachineKind::Router)
    }
}

/// Indexing rotary axes (a trunnion table or a tilting head): the machine tilts the tool to
/// a work plane and cuts 3-axis in it (3+2), its control working out the rotary axes from a
/// tilted working plane (`G68.2`).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Indexing {
    /// The most the tool may tilt from the table's vertical (degrees).
    pub max_tilt: f64,
    /// Its rotary axes' letters, the tilt's first (a trunnion's A and C).
    #[serde(default = "trunnion_axes")]
    pub rotary: [char; 2],
}

fn trunnion_axes() -> [char; 2] {
    ['A', 'C']
}

/// A machine a job runs on.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Machine {
    /// Who builds it ("Generic").
    pub builder: String,
    /// Its model name ("3-axis mill — Fanuc").
    pub model: String,
    pub kind: MachineKind,
    /// The controller's G-code dialect.
    pub dialect: Dialect,
    /// Travel along X, Y and Z (mm).
    pub travel: [f64; 3],
    /// Rapid rate (mm/min), for the time estimate and posts without rapid positioning.
    pub rapid_rate: f64,
    /// Fastest cutting feed (mm/min).
    pub max_feed: f64,
    /// Fastest spindle (rpm); a beam machine's S value at 100 % power.
    pub max_spindle: f64,
    /// Has an automatic tool changer (else the post stops for a manual change).
    pub tool_changer: bool,
    /// Rotary axes for 3+2 machining (`None`: 3 axes only).
    #[serde(default)]
    pub indexing: Option<Indexing>,
    /// The post settings a job starts from.
    pub post: PostSettings,
}

impl Machine {
    #[allow(clippy::too_many_arguments)]
    fn new(
        model: &str,
        kind: MachineKind,
        dialect: Dialect,
        travel: [f64; 3],
        rapid_rate: f64,
        max_feed: f64,
        max_spindle: f64,
        tool_changer: bool,
    ) -> Machine {
        Machine {
            builder: "Generic".into(),
            model: model.into(),
            kind,
            dialect,
            travel,
            rapid_rate,
            max_feed,
            max_spindle,
            tool_changer,
            indexing: None,
            post: PostSettings::for_dialect(dialect, kind),
        }
    }

    fn indexing(mut self, max_tilt: f64) -> Machine {
        self.indexing = Some(Indexing {
            max_tilt,
            rotary: trunnion_axes(),
        });
        self
    }

    /// Whether it can cut in a work plane whose Z is `z` (setup coordinates): along the
    /// setup's Z on any machine, tilted within its limit with indexing.
    pub fn can_tilt_to(&self, z: crate::geom::P3) -> bool {
        let tilt = z[2].clamp(-1.0, 1.0).acos().to_degrees();
        tilt < 1e-6 || self.indexing.is_some_and(|i| tilt <= i.max_tilt + 1e-9)
    }

    /// "Generic 3-axis mill — Fanuc".
    pub fn title(&self) -> String {
        format!("{} {}", self.builder, self.model)
    }

    /// Problems with the machine's own settings.
    pub fn check(&self) -> Result<(), String> {
        if self.travel.iter().any(|t| t.is_nan() || *t <= 0.0) {
            return Err("Every axis needs some travel".into());
        }
        if self.rapid_rate.is_nan()
            || self.rapid_rate <= 0.0
            || self.max_feed.is_nan()
            || self.max_feed <= 0.0
        {
            return Err("The rapid rate and the fastest feed must be more than 0".into());
        }
        Ok(())
    }
}

/// The machines offered by Create machine, in list order.
pub fn catalog() -> Vec<Machine> {
    use Dialect::*;
    use MachineKind::*;
    vec![
        Machine::new(
            "3-axis mill — Fanuc",
            Mill,
            Fanuc,
            [762.0, 406.0, 508.0],
            25_000.0,
            12_000.0,
            10_000.0,
            true,
        ),
        Machine::new(
            "3-axis mill — Haas-style",
            Mill,
            Haas,
            [762.0, 406.0, 508.0],
            25_400.0,
            16_500.0,
            8_100.0,
            true,
        ),
        Machine::new(
            "3-axis mill — LinuxCNC",
            Mill,
            LinuxCnc,
            [450.0, 300.0, 400.0],
            6_000.0,
            4_000.0,
            6_000.0,
            false,
        ),
        Machine::new(
            "CNC router — GRBL",
            Router,
            Grbl,
            [800.0, 800.0, 100.0],
            5_000.0,
            5_000.0,
            24_000.0,
            false,
        ),
        Machine::new(
            "CNC router — Mach3/Mach4",
            Router,
            Mach3,
            [1_220.0, 2_440.0, 150.0],
            15_000.0,
            12_000.0,
            24_000.0,
            false,
        ),
        Machine::new(
            "Diode laser — GRBL",
            Laser,
            Grbl,
            [400.0, 400.0, 50.0],
            6_000.0,
            6_000.0,
            1_000.0,
            false,
        ),
        Machine::new(
            "CO₂ laser — GRBL",
            Laser,
            Grbl,
            [900.0, 600.0, 50.0],
            30_000.0,
            30_000.0,
            1_000.0,
            false,
        ),
        Machine::new(
            "Plasma table — LinuxCNC (QtPlasmaC)",
            Plasma,
            LinuxCnc,
            [1_220.0, 1_220.0, 100.0],
            20_000.0,
            12_000.0,
            1.0,
            false,
        ),
        Machine::new(
            "Plasma table — Mach3 THC",
            Plasma,
            Mach3,
            [1_220.0, 2_440.0, 100.0],
            15_000.0,
            10_000.0,
            1.0,
            false,
        ),
        Machine::new(
            "Waterjet — Fanuc",
            Waterjet,
            Fanuc,
            [1_500.0, 3_000.0, 150.0],
            10_000.0,
            5_000.0,
            1.0,
            false,
        ),
        Machine::new(
            "5-axis trunnion mill (3+2) — Fanuc",
            Mill,
            Fanuc,
            [650.0, 520.0, 475.0],
            30_000.0,
            15_000.0,
            15_000.0,
            true,
        )
        .indexing(110.0),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_covers_every_kind_and_checks() {
        let c = catalog();
        for k in MachineKind::ALL {
            assert!(c.iter().any(|m| m.kind == k), "{k:?}");
        }
        for m in &c {
            m.check().unwrap();
            assert!(m.post.decimals >= 3);
        }
        let titles: std::collections::HashSet<_> = c.iter().map(|m| m.title()).collect();
        assert_eq!(titles.len(), c.len());
    }
}
