//! Schematics: placed symbols, wires, junctions, labels and text on a sheet.
//!
//! Sheet coordinates are millimetres-as-nanometres with Y up; the page spans (0, 0) to
//! (width, height), its bottom-left corner at the origin.

use crate::graphics::{Color, Shape, Stroke, Text};
use crate::symbol::{Field, Pin, Symbol, fields};
use crate::units::{Nm, Pt, Size, normalize_deg};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A paper size.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Paper {
    /// "A4", "A3", "USLetter", … or "User".
    pub name: String,
    pub size: Size,
}

impl Default for Paper {
    fn default() -> Self {
        Paper { name: "A4".into(), size: Size::mm(297.0, 210.0) }
    }
}

/// The title block's text.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TitleBlock {
    pub title: String,
    pub date: String,
    pub revision: String,
    pub company: String,
    pub comments: Vec<String>,
}

/// Mirroring of a placed symbol, applied before its rotation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Mirror {
    #[default]
    None,
    /// About the X axis (upside down).
    X,
    /// About the Y axis (left-right).
    Y,
}

/// Where a symbol sits: symbol coordinates → sheet coordinates.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Placement {
    pub at: Pt,
    /// 0, 90, 180 or 270 degrees counter-clockwise.
    pub angle: f64,
    pub mirror: Mirror,
}

impl Placement {
    pub fn apply(&self, p: Pt) -> Pt {
        let m = match self.mirror {
            Mirror::None => p,
            Mirror::X => Pt::new(p.x, -p.y),
            Mirror::Y => Pt::new(-p.x, p.y),
        };
        self.at + m.rotated(self.angle)
    }

    /// A direction (degrees) in symbol coordinates, in sheet coordinates.
    pub fn apply_angle(&self, deg: f64) -> f64 {
        let m = match self.mirror {
            Mirror::None => deg,
            Mirror::X => -deg,
            Mirror::Y => 180.0 - deg,
        };
        normalize_deg(m + self.angle)
    }

    /// Text in symbol coordinates, on the sheet: anchor and direction transformed; a mirror
    /// turns the text's "up" over, so its vertical alignment swaps (the text itself is never
    /// drawn mirrored).
    pub fn text(&self, t: &Text) -> Text {
        let mut out = t.clone();
        out.at = self.apply(t.at);
        out.angle = self.apply_angle(t.angle);
        if self.mirror != Mirror::None {
            use crate::graphics::VAlign;
            out.style.v_align = match t.style.v_align {
                VAlign::Top => VAlign::Bottom,
                VAlign::Bottom => VAlign::Top,
                c => c,
            };
        }
        out
    }
}

/// A symbol placed on the sheet.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlacedSymbol {
    pub id: Uuid,
    /// The [`Symbol::id`] in [`Schematic::symbols`].
    pub symbol: String,
    pub placement: Placement,
    pub unit: u32,
    /// Body style: 1 normal, 2 De Morgan.
    pub style: u32,
    /// Fields in sheet coordinates (Reference, Value, Footprint, …).
    pub fields: Vec<Field>,
    pub in_bom: bool,
    pub on_board: bool,
    pub dnp: bool,
    pub exclude_from_sim: bool,
    /// Ids of its pins by number, kept so links to pins survive edits.
    pub pin_ids: Vec<(String, Uuid)>,
}

impl PlacedSymbol {
    pub fn field(&self, name: &str) -> Option<&Field> {
        self.fields.iter().find(|f| f.name == name)
    }

    pub fn field_mut(&mut self, name: &str) -> Option<&mut Field> {
        self.fields.iter_mut().find(|f| f.name == name)
    }

    pub fn footprint(&self) -> &str {
        self.field(fields::FOOTPRINT).map_or("", Field::value)
    }

    pub fn reference(&self) -> &str {
        self.field(fields::REFERENCE).map_or("", Field::value)
    }

    pub fn value(&self) -> &str {
        self.field(fields::VALUE).map_or("", Field::value)
    }
}

/// A wire or bus segment.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Wire {
    pub id: Uuid,
    pub a: Pt,
    pub b: Pt,
    pub stroke: Stroke,
}

/// A dot joining crossing wires.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Junction {
    pub id: Uuid,
    pub at: Pt,
    /// 0 = the default size.
    pub diameter: Nm,
    pub color: Option<Color>,
}

/// An X marking a pin as deliberately unconnected.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NoConnect {
    pub id: Uuid,
    pub at: Pt,
}

/// The outline of a global or hierarchical label.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LabelShape {
    Input,
    Output,
    Bidirectional,
    TriState,
    #[default]
    Passive,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LabelKind {
    /// Names a net on this sheet.
    #[default]
    Local,
    /// Names a net across every sheet.
    Global(LabelShape),
    /// Connects to the sheet symbol's pin of the same name one level up.
    Hierarchical(LabelShape),
}

/// A net label. `text.at` is its connection point.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Label {
    pub id: Uuid,
    pub kind: LabelKind,
    pub text: Text,
    /// Extra fields of a global label (its intersheet references).
    pub fields: Vec<Field>,
}

/// A free text note.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Note {
    pub id: Uuid,
    pub text: Text,
}

/// A drawn line, rectangle, circle, arc or curve.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Drawing {
    pub id: Uuid,
    pub shape: Shape,
}

/// One schematic sheet.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Sheet {
    pub id: Uuid,
    pub name: String,
    pub paper: Paper,
    pub title_block: TitleBlock,
    pub symbols: Vec<PlacedSymbol>,
    pub wires: Vec<Wire>,
    pub buses: Vec<Wire>,
    pub junctions: Vec<Junction>,
    pub no_connects: Vec<NoConnect>,
    pub labels: Vec<Label>,
    pub notes: Vec<Note>,
    pub drawings: Vec<Drawing>,
}

/// A schematic: its sheets and the library symbols they use (copies, so the design never
/// depends on a library that changes).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Schematic {
    pub symbols: Vec<Symbol>,
    pub sheets: Vec<Sheet>,
}

impl Schematic {
    pub fn symbol(&self, id: &str) -> Option<&Symbol> {
        self.symbols.iter().find(|s| s.id == id)
    }

    /// A placed symbol's pins in sheet coordinates: the pin and where wires connect to it.
    pub fn placed_pins<'a>(&'a self, s: &'a PlacedSymbol) -> impl Iterator<Item = (&'a Pin, Pt)> + 'a {
        self.symbol(&s.symbol)
            .into_iter()
            .flat_map(move |sym| sym.unit_pins(s.unit, s.style))
            .map(move |p| (p, s.placement.apply(p.at)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placement_mirrors_then_rotates() {
        let p = Pt::mm(2.54, 1.27);
        let pl = |angle, mirror| Placement { at: Pt::mm(10.0, 20.0), angle, mirror };
        assert_eq!(pl(0.0, Mirror::None).apply(p), Pt::mm(12.54, 21.27));
        assert_eq!(pl(90.0, Mirror::None).apply(p), Pt::mm(10.0 - 1.27, 22.54));
        assert_eq!(pl(0.0, Mirror::Y).apply(p), Pt::mm(10.0 - 2.54, 21.27));
        assert_eq!(pl(90.0, Mirror::X).apply(p), Pt::mm(10.0 + 1.27, 22.54));
        assert_eq!(pl(0.0, Mirror::Y).apply_angle(0.0), 180.0);
        assert_eq!(pl(90.0, Mirror::X).apply_angle(90.0), 0.0);
    }
}
