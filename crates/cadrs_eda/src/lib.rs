//! cadrs_eda: electronics design for cadrs (schematics, symbols, footprints, board layouts).
//! No Bevy, and no other tool's format: importers and exporters (`cadrs_kicad`) convert to and
//! from this model, and nothing here depends on them.
//!
//! - [`units`]: `i64` nanometre lengths, Y up.
//! - [`symbol`] / [`schematic`]: library symbols and the sheets they are placed on.
//! - [`footprint`] / [`board`]: footprints and the layouts they are placed on.
//! - [`Design`]: one board's schematic and layout together; [`Component`]: a part's symbol,
//!   footprint and 3D model.

pub mod assign;
pub mod board;
pub mod board_edit;
pub mod bom;
pub mod connectivity;
pub mod copper;
pub mod drc;
pub mod expr;
pub mod fab;
pub mod lib_edit;
pub mod model3d;
pub mod erc;
pub mod font;
pub mod getting_started;
pub mod forward;
pub mod outline;
pub mod power_monitor;
pub mod zone;
pub mod sch_edit;
pub mod footprint;
pub mod geom;
pub mod graphics;
pub mod layer;
pub mod library;
pub mod poly;
pub mod render;
pub mod schematic;
pub mod stdlib;
pub mod symbol;
pub mod units;
pub mod view;
pub mod wrl;

use serde::{Deserialize, Serialize};

/// A board's design: its schematic and its layout.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Design {
    pub schematic: schematic::Schematic,
    pub board: board::Board,
}

impl Design {
    /// A new design: one empty A4 sheet, an empty two-layer board.
    pub fn new() -> Design {
        let mut d = Design::default();
        d.ensure_sheet();
        d
    }

    /// Gives a design without a schematic (a board imported alone) its first, empty sheet:
    /// schematic editing works on sheet 0.
    pub fn ensure_sheet(&mut self) {
        if self.schematic.sheets.is_empty() {
            self.schematic.sheets.push(schematic::Sheet { id: uuid::Uuid::new_v4(), name: "Root".into(), ..Default::default() });
        }
    }
}

/// A part: the symbol drawn on schematics, the footprint placed on boards, and (in the
/// footprint) its 3D models.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Component {
    pub name: String,
    pub symbol: Option<symbol::Symbol>,
    pub footprint: Option<footprint::Footprint>,
}

impl Component {
    pub fn new(name: impl Into<String>) -> Component {
        Component { name: name.into(), symbol: None, footprint: None }
    }
}
