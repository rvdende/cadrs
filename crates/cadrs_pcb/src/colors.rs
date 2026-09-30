//! Display classes and colours of PCB bodies (X5): a green board, components coloured by the
//! kind of package their name suggests, keep areas dark and translucent.
//!
//! The palette follows the course's viewport (`v3-interface-poster.png`, P3H.3 judge): strong,
//! saturated boxes (red, yellow, cyan, blue, with white for small discretes) on a light green
//! board.

use serde::{Deserialize, Serialize};

/// sRGB and opacity (255 opaque).
pub type Rgba = [u8; 4];

/// What a package looks like it is, from its name ([`component_kind`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ComponentKind {
    /// QFP, QFN, BGA, SOIC, TSSOP, ...: cyan.
    Ic,
    /// Chip resistors, capacitors, inductors (0603, 1206, ...): yellow.
    Passive,
    /// SOT, SOD, diodes and transistors: white.
    Discrete,
    /// Headers, connectors, sockets: blue.
    Connector,
    /// Crystals and oscillators: red.
    Crystal,
    /// Buttons and switches: orange.
    Switch,
    /// Anything else: red (the course's generic box).
    Other,
}

/// What a body of a [`crate::BoardGeometry`] is, for its colour and for Sync.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BodyClass {
    Board,
    Component(ComponentKind),
    /// A placeholder box for a component whose package isn't in the library.
    Placeholder,
    KeepOut,
    KeepIn,
    /// An other outline (heatsink, ...).
    Other,
}

/// The PCB green of the course's screenshots (`v3-interface-poster.png`,
/// `v4-import-idf-poster.png`): a light, strong green (its lit edges are nearly lime).
pub const BOARD_GREEN: Rgba = [36, 178, 36, 255];
/// Keep-outs: dark and mostly opaque, so they read as dark patches on the green board
/// (`ex1-step11-synced-board-keepouts.png`: nearly black green over the board).
pub const KEEPOUT: Rgba = [12, 28, 18, 185];
pub const KEEPIN: Rgba = [20, 45, 90, 150];

impl BodyClass {
    pub fn color(self) -> Rgba {
        match self {
            BodyClass::Board => BOARD_GREEN,
            BodyClass::Component(k) => k.color(),
            BodyClass::Placeholder => [220, 60, 200, 255],
            BodyClass::KeepOut => KEEPOUT,
            BodyClass::KeepIn => KEEPIN,
            BodyClass::Other => [150, 150, 160, 160],
        }
    }

    pub fn is_keep(self) -> bool {
        matches!(self, BodyClass::KeepOut | BodyClass::KeepIn)
    }

    pub fn is_component(self) -> bool {
        matches!(self, BodyClass::Component(_) | BodyClass::Placeholder)
    }
}

impl ComponentKind {
    pub fn color(self) -> Rgba {
        match self {
            ComponentKind::Ic => [0, 190, 230, 255],
            ComponentKind::Passive => [240, 210, 20, 255],
            ComponentKind::Discrete => [235, 235, 235, 255],
            ComponentKind::Connector => [25, 70, 220, 255],
            ComponentKind::Crystal => [225, 20, 20, 255],
            ComponentKind::Switch => [240, 120, 20, 255],
            ComponentKind::Other => [225, 20, 20, 255],
        }
    }
}

/// Guesses a package's kind from its name (case-insensitive; the first rule that matches):
/// crystals (`CRYSTAL`, `XTAL`, `HC49`, `OSC`), switches (`BUTTON`, `BTN`, `SW`), connectors
/// (`HDR`, `HEADER`, `CONN`, `USB`, `JST`, `SOCKET`), discretes (`SOT`, `SOD`, `DO-`, `TO-`,
/// `DIODE`, `LED`), ICs (`QFP`, `QFN`, `BGA`, `SOIC`, `SOP`, `SSOP`, `TSSOP`, `DIP`, `PLCC`,
/// `LCC`, `DFN`, `CSP`), passives (an imperial chip size `0201`, `0402`, `0603`, `0805`, `1206`,
/// `1210`, `2010`, `2512`, or `RES`, `CAP`, `IND`).
pub fn component_kind(package: &str) -> ComponentKind {
    let n = package.to_ascii_uppercase();
    let has = |keys: &[&str]| keys.iter().any(|k| n.contains(k));
    if has(&["CRYSTAL", "XTAL", "HC49", "OSC"]) {
        ComponentKind::Crystal
    } else if has(&["BUTTON", "BTN", "SWITCH"]) || n.starts_with("SW") {
        ComponentKind::Switch
    } else if has(&["HDR", "HEADER", "CONN", "USB", "JST", "SOCKET"]) {
        ComponentKind::Connector
    } else if has(&["SOT", "SOD", "DO-", "TO-", "DIODE", "LED"]) {
        ComponentKind::Discrete
    } else if has(&["QFP", "QFN", "BGA", "SOIC", "SOP", "DIP", "PLCC", "LCC", "DFN", "CSP"]) {
        ComponentKind::Ic
    } else if has(&["0201", "0402", "0603", "0805", "1206", "1210", "2010", "2512", "RES", "CAP", "IND"]) {
        ComponentKind::Passive
    } else {
        ComponentKind::Other
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn package_kinds() {
        use ComponentKind::*;
        for (name, kind) in [
            ("QFP100_600MIL", Ic),
            ("QFN64_400MIL", Ic),
            ("uBGA48_7.4X7.1", Ic),
            ("TSSOP_20", Ic),
            ("0603R", Passive),
            ("1206C", Passive),
            ("1210_SR73K2E", Passive),
            ("SOT23", Discrete),
            ("SOD123F", Discrete),
            ("HDR_1X20", Connector),
            ("CRYSTAL_HC49", Crystal),
            ("CRYSTAL_CX_4V", Crystal),
            ("BUTTON_EVQPUA02", Switch),
            ("extractor", Other),
        ] {
            assert_eq!(component_kind(name), kind, "{name}");
        }
    }

    /// HSV saturation and value of a colour (0–1).
    fn sat_val(c: Rgba) -> (f64, f64) {
        let [r, g, b] = [c[0], c[1], c[2]].map(|v| v as f64 / 255.0);
        let (mx, mn) = (r.max(g).max(b), r.min(g).min(b));
        (if mx > 0.0 { (mx - mn) / mx } else { 0.0 }, mx)
    }

    #[test]
    fn colours_are_saturated_like_the_course() {
        use ComponentKind::*;
        // Every coloured kind is strong and bright (the course's red, yellow, cyan and blue
        // boxes); only small discretes are white.
        for k in [Ic, Passive, Connector, Crystal, Switch, Other] {
            let (s, v) = sat_val(k.color());
            assert!(s >= 0.85 && v >= 0.85, "{k:?}: saturation {s:.2}, value {v:.2}");
        }
        let (s, v) = sat_val(Discrete.color());
        assert!(s < 0.1 && v > 0.9);
        // Red, yellow, cyan and blue are all there.
        let hue = |c: Rgba| {
            let [r, g, b] = [c[0], c[1], c[2]].map(|v| v as f64);
            (3f64.sqrt() * (g - b)).atan2(2.0 * r - g - b).to_degrees().rem_euclid(360.0)
        };
        let hues: Vec<f64> = [Crystal, Passive, Ic, Connector].map(|k| hue(k.color())).to_vec();
        assert!(hues[0] < 10.0 || hues[0] > 350.0, "red {hues:?}");
        assert!((40.0..70.0).contains(&hues[1]), "yellow {hues:?}");
        assert!((170.0..215.0).contains(&hues[2]), "cyan {hues:?}");
        assert!((215.0..250.0).contains(&hues[3]), "blue {hues:?}");
        // A light, saturated green board.
        let (s, v) = sat_val(BOARD_GREEN);
        assert!(s > 0.75 && v > 0.65 && BOARD_GREEN[1] > BOARD_GREEN[0] * 3);
    }
}
