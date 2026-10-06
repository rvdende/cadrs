//! Library symbols: the drawing and pins of a part, as placed on a schematic.
//!
//! Coordinates are the symbol's own, Y up, origin at its anchor. A symbol may have several
//! units (the gates of a quad op-amp) and an alternate body style (De Morgan): every item says
//! which unit and style it belongs to, 0 meaning all.

use crate::graphics::{Shape, Text};
use crate::units::{Nm, Pt};
use serde::{Deserialize, Serialize};

/// A pin's electrical type, used by ERC.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PinType {
    Input,
    Output,
    Bidirectional,
    TriState,
    #[default]
    Passive,
    Free,
    Unspecified,
    PowerIn,
    PowerOut,
    OpenCollector,
    OpenEmitter,
    NoConnect,
}

/// How a pin is drawn.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PinShape {
    #[default]
    Line,
    Inverted,
    Clock,
    InvertedClock,
    InputLow,
    ClockLow,
    OutputLow,
    EdgeClockHigh,
    NonLogic,
}

/// A pin. `at` is where wires connect; the pin runs `length` from there in direction `angle`
/// (degrees counter-clockwise, 0 = +X) towards the body.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Pin {
    pub number: String,
    pub name: String,
    pub kind: PinType,
    pub shape: PinShape,
    pub at: Pt,
    pub angle: f64,
    pub length: Nm,
    pub visible: bool,
    /// Text heights of the name and number.
    pub name_size: Nm,
    pub number_size: Nm,
    pub unit: u32,
    pub style: u32,
}

impl Pin {
    /// The end of the pin at the body.
    pub fn inner_end(&self) -> Pt {
        self.at + Pt::new(self.length, 0).rotated(self.angle)
    }
}

/// A drawing item of a symbol, in a unit and body style (0 = all).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SymbolGraphic {
    pub item: SymbolItem,
    pub unit: u32,
    pub style: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum SymbolItem {
    Shape(Shape),
    Text(Text),
}

/// A named value of a symbol or placed part (reference, value, footprint, datasheet, …) and
/// where it is drawn.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Field {
    pub name: String,
    pub text: Text,
    /// Draw "name: value" rather than the value alone.
    pub show_name: bool,
}

impl Field {
    pub fn value(&self) -> &str {
        &self.text.text
    }
}

/// The well-known field names.
pub mod fields {
    pub const REFERENCE: &str = "Reference";
    pub const VALUE: &str = "Value";
    pub const FOOTPRINT: &str = "Footprint";
    pub const DATASHEET: &str = "Datasheet";
    pub const DESCRIPTION: &str = "Description";
}

/// A library symbol.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Symbol {
    /// Its id in the design, `library:name`.
    pub id: String,
    pub fields: Vec<Field>,
    pub keywords: String,
    /// Footprint name filters (`C_*`) for picking a footprint.
    pub footprint_filters: Vec<String>,
    /// How many units; units are interchangeable when `units_swappable`.
    pub unit_count: u32,
    pub units_swappable: bool,
    pub unit_names: Vec<(u32, String)>,
    /// Has a De Morgan alternate body (style 2).
    pub has_alternate: bool,
    /// A power symbol: its value names a global net (GND, +3V3).
    pub power: bool,
    pub show_pin_numbers: bool,
    pub show_pin_names: bool,
    /// Pin names inside the body this far from the pin's inner end; 0 puts them above the pin.
    pub pin_name_offset: Nm,
    pub in_bom: bool,
    pub on_board: bool,
    pub graphics: Vec<SymbolGraphic>,
    pub pins: Vec<Pin>,
}

impl Symbol {
    pub fn field(&self, name: &str) -> Option<&Field> {
        self.fields.iter().find(|f| f.name == name)
    }

    /// The pins drawn for `unit` in body `style`.
    pub fn unit_pins(&self, unit: u32, style: u32) -> impl Iterator<Item = &Pin> {
        self.pins.iter().filter(move |p| (p.unit == 0 || p.unit == unit) && (p.style == 0 || p.style == style))
    }

    /// The graphics drawn for `unit` in body `style`.
    pub fn unit_graphics(&self, unit: u32, style: u32) -> impl Iterator<Item = &SymbolGraphic> {
        self.graphics.iter().filter(move |g| (g.unit == 0 || g.unit == unit) && (g.style == 0 || g.style == style))
    }

    /// The name after the library prefix.
    pub fn name(&self) -> &str {
        self.id.rsplit_once(':').map_or(&self.id, |(_, n)| n)
    }
}
