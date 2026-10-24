//! Starting feeds and speeds for a solid carbide cutter in a work material: the spindle speed
//! from the material's surface speed, the feed from its chip load per tooth (a fraction of the
//! diameter), the depth and width of cut as fractions of the diameter. Conservative figures for
//! a hobby or light production machine; the operator tunes them from there.

use serde::{Deserialize, Serialize};

use crate::tool::{CuttingData, Tool, ToolKind};

/// What is being cut.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum WorkMaterial {
    Aluminium,
    Brass,
    MildSteel,
    StainlessSteel,
    Plastic,
    Hardwood,
    Softwood,
    Mdf,
}

/// One material's figures for carbide.
struct Figures {
    /// Surface speed (m/min).
    vc: f64,
    /// Chip load per tooth as a fraction of the diameter.
    fz: f64,
    /// Depth of cut, width of cut (fractions of the diameter).
    ap: f64,
    ae: f64,
    /// Plunge feed as a fraction of the cutting feed.
    plunge: f64,
}

impl WorkMaterial {
    pub const ALL: [WorkMaterial; 8] = [
        WorkMaterial::Aluminium,
        WorkMaterial::Brass,
        WorkMaterial::MildSteel,
        WorkMaterial::StainlessSteel,
        WorkMaterial::Plastic,
        WorkMaterial::Hardwood,
        WorkMaterial::Softwood,
        WorkMaterial::Mdf,
    ];

    pub fn label(self) -> &'static str {
        match self {
            WorkMaterial::Aluminium => "Aluminium",
            WorkMaterial::Brass => "Brass",
            WorkMaterial::MildSteel => "Mild steel",
            WorkMaterial::StainlessSteel => "Stainless steel",
            WorkMaterial::Plastic => "Plastic (acrylic, acetal)",
            WorkMaterial::Hardwood => "Hardwood",
            WorkMaterial::Softwood => "Softwood",
            WorkMaterial::Mdf => "MDF, plywood",
        }
    }

    fn figures(self) -> Figures {
        let f = |vc, fz, ap, ae, plunge| Figures {
            vc,
            fz,
            ap,
            ae,
            plunge,
        };
        match self {
            WorkMaterial::Aluminium => f(300.0, 0.010, 0.5, 0.4, 0.3),
            WorkMaterial::Brass => f(200.0, 0.008, 0.4, 0.4, 0.3),
            WorkMaterial::MildSteel => f(120.0, 0.005, 0.25, 0.3, 0.25),
            WorkMaterial::StainlessSteel => f(70.0, 0.004, 0.2, 0.3, 0.25),
            WorkMaterial::Plastic => f(250.0, 0.012, 0.5, 0.4, 0.4),
            WorkMaterial::Hardwood => f(400.0, 0.015, 1.0, 0.45, 0.4),
            WorkMaterial::Softwood => f(500.0, 0.020, 1.0, 0.45, 0.4),
            WorkMaterial::Mdf => f(450.0, 0.018, 1.0, 0.45, 0.4),
        }
    }
}

/// `tool`'s cutting data in `material`, its spindle no faster than `max_rpm` and its feed no
/// faster than `max_feed` (the chip load kept when the spindle is limited). The diameter cut
/// is the tool's full one (a chamfer or engraving tool's widest).
pub fn cutting_data(
    tool: &Tool,
    material: WorkMaterial,
    max_rpm: f64,
    max_feed: f64,
) -> CuttingData {
    let m = material.figures();
    let d = tool.diameter.max(0.1);
    // A tap at a quarter of the surface speed, fed its pitch a turn.
    let vc = if tool.kind == ToolKind::Tap {
        m.vc * 0.25
    } else {
        m.vc
    };
    let rpm = (vc * 1000.0 / (std::f64::consts::PI * d)).min(max_rpm.max(1.0));
    // A drill feeds about two teeth' worth a turn.
    let teeth = match tool.kind {
        ToolKind::Drill => 2.0,
        _ => tool.flutes.max(1) as f64,
    };
    let fz = (m.fz * d).clamp(0.005, 0.25);
    // A tap's feed is its pitch a turn: slowed to the machine's feed, it turns slower too.
    let (rpm, feed) = if tool.kind == ToolKind::Tap {
        let pitch = tool.pitch.max(1e-6);
        let rpm = rpm.min(max_feed.max(1.0) / pitch).floor().max(1.0);
        (rpm, rpm * pitch)
    } else {
        (rpm, (rpm * teeth * fz).min(max_feed.max(1.0)))
    };
    CuttingData {
        spindle_rpm: rpm.round(),
        feed: if tool.kind == ToolKind::Tap {
            feed
        } else {
            feed.round()
        },
        plunge_feed: (feed * m.plunge).round(),
        step_down: round2(m.ap * d),
        step_over: round2(m.ae * d),
        material: Some(material),
    }
}

fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_six_mm_end_mill_in_aluminium() {
        let mut t = Tool::new(ToolKind::EndMill, 6.0);
        t.flutes = 3;
        // 300 m/min on 6 mm: 15 915 rpm; 3 × 0.06 mm a tooth.
        let c = cutting_data(&t, WorkMaterial::Aluminium, 24_000.0, 10_000.0);
        assert_eq!(c.spindle_rpm, 15_915.0);
        assert_eq!(c.feed, (15_915.494_f64 * 3.0 * 0.06).round());
        assert_eq!(c.step_down, 3.0);
        assert_eq!(c.material, Some(WorkMaterial::Aluminium));
        // A 10 000 rpm spindle: the same chip load, a slower feed.
        let slow = cutting_data(&t, WorkMaterial::Aluminium, 10_000.0, 10_000.0);
        assert_eq!(slow.spindle_rpm, 10_000.0);
        assert_eq!(slow.feed, 1_800.0);
        // Steel: slower, lighter.
        let steel = cutting_data(&t, WorkMaterial::MildSteel, 24_000.0, 10_000.0);
        assert!(steel.spindle_rpm < c.spindle_rpm && steel.feed < c.feed);
        assert!(steel.step_down < c.step_down);
        // The machine's feed limit.
        let capped = cutting_data(&t, WorkMaterial::Softwood, 24_000.0, 2_000.0);
        assert_eq!(capped.feed, 2_000.0);
        // A tap stays at its pitch a turn, slowed to the machine's feed.
        let mut tap = Tool::new(ToolKind::Tap, 8.0);
        tap.pitch = 1.25;
        let c = cutting_data(&tap, WorkMaterial::Softwood, 24_000.0, 1_000.0);
        assert_eq!(c.spindle_rpm, 800.0);
        assert!((c.feed - c.spindle_rpm * 1.25).abs() < 1e-9);
    }
}
