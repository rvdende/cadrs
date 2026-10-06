//! Board layouts: placed footprints, tracks, vias, zones, outline and graphics.
//!
//! Board coordinates are nanometres, Y up, viewed from the top. Nets are named; the empty name
//! is "no net".

use crate::footprint::{Footprint, FootprintPlacement, ZoneConnection};
use crate::graphics::{Shape, Text};
use crate::layer::{Layer, LayerSet};
use crate::units::{Nm, Pt, mm};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A footprint placed on the board, with its own copy of the definition (pads carry nets).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlacedFootprint {
    pub id: Uuid,
    pub footprint: Footprint,
    pub placement: FootprintPlacement,
    pub locked: bool,
    /// The schematic symbol it was placed for.
    pub symbol: Option<Uuid>,
}

impl PlacedFootprint {
    pub fn reference(&self) -> &str {
        self.footprint.field(crate::symbol::fields::REFERENCE).map_or("", |f| &f.text.text.text)
    }
}

/// A copper track segment; an arc when `mid` is set.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Track {
    pub id: Uuid,
    pub a: Pt,
    pub mid: Option<Pt>,
    pub b: Pt,
    pub width: Nm,
    pub layer: Layer,
    pub net: String,
    pub locked: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ViaKind {
    #[default]
    Through,
    Blind,
    Micro,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Via {
    pub id: Uuid,
    pub at: Pt,
    pub diameter: Nm,
    pub drill: Nm,
    pub kind: ViaKind,
    /// The copper layers it spans, top first.
    pub from: Layer,
    pub to: Layer,
    pub net: String,
    pub locked: bool,
    /// Covered by solder mask on each side.
    pub tented: Option<(bool, bool)>,
}

/// What a keep-out zone forbids.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Keepout {
    pub tracks: bool,
    pub vias: bool,
    pub pads: bool,
    pub copper_pour: bool,
    pub footprints: bool,
}

/// How a copper zone fills.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ZoneFill {
    pub clearance: Nm,
    pub min_width: Nm,
    pub pad_connection: ZoneConnection,
    pub thermal_gap: Nm,
    pub thermal_spoke: Nm,
    /// Remove islands not connected to the net.
    pub remove_islands: bool,
    pub hatched: Option<(Nm, Nm)>,
}

impl Default for ZoneFill {
    fn default() -> Self {
        ZoneFill {
            clearance: mm(0.5),
            min_width: mm(0.25),
            pad_connection: ZoneConnection::Thermal,
            thermal_gap: mm(0.5),
            thermal_spoke: mm(0.5),
            remove_islands: true,
            hatched: None,
        }
    }
}

/// A closed loop of points.
pub type Ring = Vec<Pt>;

/// A polygon: an outer ring and holes.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Polygon {
    pub outer: Ring,
    pub holes: Vec<Ring>,
}

/// A copper pour or keep-out area.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Zone {
    pub id: Uuid,
    pub name: String,
    pub net: String,
    pub layers: LayerSet,
    pub priority: u32,
    /// The outline the user drew (several when it was split).
    pub outline: Vec<Ring>,
    pub fill: ZoneFill,
    /// A keep-out rather than copper.
    pub keepout: Option<Keepout>,
    pub locked: bool,
    /// The last fill result, per layer.
    pub filled: Vec<(Layer, Vec<Polygon>)>,
}

/// A drawn shape on a board layer (the outline lives on [`Layer::Outline`]).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BoardShape {
    pub id: Uuid,
    pub shape: Shape,
    pub layer: Layer,
    pub locked: bool,
    /// A copper shape's net.
    pub net: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BoardText {
    pub id: Uuid,
    pub text: Text,
    pub layer: Layer,
    pub locked: bool,
    /// Drawn as a hole in a filled box.
    pub knockout: bool,
}

/// A group of items moved and selected together.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Group {
    pub id: Uuid,
    pub name: String,
    pub members: Vec<Uuid>,
    pub locked: bool,
}

/// Rules for a class of nets.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NetClass {
    pub name: String,
    pub clearance: Nm,
    pub track_width: Nm,
    pub via_diameter: Nm,
    pub via_drill: Nm,
    pub diff_pair_width: Nm,
    pub diff_pair_gap: Nm,
    /// Net name patterns (`*` wildcards) that belong to this class.
    pub patterns: Vec<String>,
}

impl Default for NetClass {
    fn default() -> Self {
        NetClass {
            name: "Default".into(),
            clearance: mm(0.2),
            track_width: mm(0.2),
            via_diameter: mm(0.6),
            via_drill: mm(0.3),
            diff_pair_width: mm(0.2),
            diff_pair_gap: mm(0.25),
            patterns: vec![],
        }
    }
}

/// Board-wide manufacturing limits (Board setup → Constraints, GS13) and the net classes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Rules {
    pub min_clearance: Nm,
    pub min_track_width: Nm,
    pub min_connection_width: Nm,
    pub min_annular_ring: Nm,
    pub min_via_diameter: Nm,
    /// Copper to hole.
    pub hole_clearance: Nm,
    pub copper_edge_clearance: Nm,
    pub min_drill: Nm,
    pub hole_to_hole: Nm,
    pub min_uvia_diameter: Nm,
    pub min_uvia_drill: Nm,
    pub silk_clearance: Nm,
    pub min_text_height: Nm,
    pub min_text_thickness: Nm,
    /// Solder mask expansion around pads.
    pub mask_margin: Nm,
    pub net_classes: Vec<NetClass>,
    /// Explicit net → class assignments (on top of the patterns).
    pub net_class_of: Vec<(String, String)>,
}

impl Default for Rules {
    fn default() -> Self {
        Rules {
            min_clearance: 0,
            min_track_width: mm(0.2),
            min_connection_width: 0,
            min_annular_ring: mm(0.1),
            min_via_diameter: mm(0.5),
            hole_clearance: mm(0.25),
            copper_edge_clearance: mm(0.5),
            min_drill: mm(0.3),
            hole_to_hole: mm(0.25),
            min_uvia_diameter: mm(0.2),
            min_uvia_drill: mm(0.1),
            silk_clearance: 0,
            min_text_height: mm(0.8),
            min_text_thickness: mm(0.08),
            mask_margin: 0,
            net_classes: vec![NetClass::default()],
            net_class_of: vec![],
        }
    }
}

impl Rules {
    /// The class a net belongs to: an explicit assignment, else the first class whose
    /// pattern matches, else Default (the first class).
    pub fn class_of(&self, net: &str) -> &NetClass {
        let by_name = |n: &str| self.net_classes.iter().find(|c| c.name == n);
        if let Some(c) = self.net_class_of.iter().find(|(n, _)| n == net).and_then(|(_, c)| by_name(c)) {
            return c;
        }
        self.net_classes
            .iter()
            .skip(1)
            .find(|c| c.patterns.iter().any(|p| crate::library::glob(p, net)))
            .or(self.net_classes.first())
            .expect("a Default net class")
    }

    /// The clearance two nets need: the larger of their classes' (and the board minimum).
    pub fn clearance(&self, a: &str, b: &str) -> Nm {
        self.class_of(a).clearance.max(self.class_of(b).clearance).max(self.min_clearance)
    }

    pub fn class_mut(&mut self, name: &str) -> Option<&mut NetClass> {
        self.net_classes.iter_mut().find(|c| c.name == name)
    }
}

/// What a stackup layer is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum StackKind {
    Silkscreen,
    Paste,
    Mask,
    Copper,
    /// Core or prepreg.
    Dielectric,
}

/// One layer of the physical stackup, top to bottom (GS13).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StackLayer {
    pub name: String,
    pub kind: StackKind,
    pub layer: Option<Layer>,
    pub thickness: Nm,
    pub material: String,
    pub epsilon_r: f64,
    pub loss_tangent: f64,
}

/// The physical stackup.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Stackup {
    pub layers: Vec<StackLayer>,
}

impl Stackup {
    /// The usual stackup for `copper` layers and a board `thickness`: 35 µm copper, 10 µm
    /// mask, the rest FR4 split evenly between the cores.
    pub fn standard(copper: u8, thickness: Nm) -> Stackup {
        let copper = copper.max(2);
        let (cu, mask) = (mm(0.035), mm(0.01));
        let cores = (copper - 1) as Nm;
        let core = (thickness - cu * copper as Nm - 2 * mask) / cores;
        let mut layers = vec![];
        let simple = |name: &str, kind, layer, t| StackLayer { name: name.into(), kind, layer: Some(layer), thickness: t, material: String::new(), epsilon_r: 0.0, loss_tangent: 0.0 };
        layers.push(simple("F.Silkscreen", StackKind::Silkscreen, Layer::TopSilk, 0));
        layers.push(simple("F.Paste", StackKind::Paste, Layer::TopPaste, 0));
        layers.push(StackLayer { epsilon_r: 3.3, ..simple("F.Mask", StackKind::Mask, Layer::TopMask, mask) });
        for (i, l) in Layer::copper(copper).enumerate() {
            let name = match l {
                Layer::TopCopper => "F.Cu".to_string(),
                Layer::BottomCopper => "B.Cu".to_string(),
                Layer::Inner(n) => format!("In{n}.Cu"),
                _ => unreachable!(),
            };
            layers.push(simple(&name, StackKind::Copper, l, cu));
            if (i as u8) < copper - 1 {
                layers.push(StackLayer {
                    name: format!("Dielectric {}", i + 1),
                    kind: StackKind::Dielectric,
                    layer: None,
                    thickness: core,
                    material: "FR4".into(),
                    epsilon_r: 4.5,
                    loss_tangent: 0.02,
                });
            }
        }
        layers.push(StackLayer { epsilon_r: 3.3, ..simple("B.Mask", StackKind::Mask, Layer::BottomMask, mask) });
        layers.push(simple("B.Paste", StackKind::Paste, Layer::BottomPaste, 0));
        layers.push(simple("B.Silkscreen", StackKind::Silkscreen, Layer::BottomSilk, 0));
        Stackup { layers }
    }

    pub fn thickness(&self) -> Nm {
        self.layers.iter().map(|l| l.thickness).sum()
    }
}

impl Default for Stackup {
    fn default() -> Self {
        Stackup::standard(2, mm(1.6))
    }
}

/// A board layout.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Board {
    /// Copper layers, 2 or more (even).
    pub copper_layers: u8,
    pub thickness: Nm,
    /// Every net name used on the board (sorted; "" is not listed).
    pub nets: Vec<String>,
    pub footprints: Vec<PlacedFootprint>,
    pub tracks: Vec<Track>,
    pub vias: Vec<Via>,
    pub zones: Vec<Zone>,
    pub shapes: Vec<BoardShape>,
    pub texts: Vec<BoardText>,
    pub groups: Vec<Group>,
    pub rules: Rules,
    #[serde(default)]
    pub stackup: Stackup,
    /// Page settings for plots of the layout (GS13).
    #[serde(default)]
    pub paper: crate::schematic::Paper,
    #[serde(default)]
    pub title_block: crate::schematic::TitleBlock,
}

impl Default for Board {
    fn default() -> Self {
        Board {
            copper_layers: 2,
            thickness: mm(1.6),
            nets: vec![],
            footprints: vec![],
            tracks: vec![],
            vias: vec![],
            zones: vec![],
            shapes: vec![],
            texts: vec![],
            groups: vec![],
            rules: Rules::default(),
            stackup: Stackup::default(),
            paper: Default::default(),
            title_block: Default::default(),
        }
    }
}

impl Board {
    /// The copper layers this board has, top to bottom.
    pub fn copper(&self) -> impl Iterator<Item = Layer> {
        Layer::copper(self.copper_layers)
    }

    pub fn footprint(&self, reference: &str) -> Option<&PlacedFootprint> {
        self.footprints.iter().find(|f| f.reference() == reference)
    }

    /// The outline shapes (on [`Layer::Outline`]), the board's own and its footprints'.
    pub fn outline_shapes(&self) -> Vec<Shape> {
        let own = self.shapes.iter().filter(|s| s.layer == Layer::Outline).map(|s| s.shape.clone());
        let fps = self.footprints.iter().flat_map(|f| {
            f.footprint.shapes.iter().filter(|s| f.placement.layer(s.layer) == Layer::Outline).map(|s| {
                let g = if f.placement.angle % 90.0 == 0.0 { s.shape.geom.clone() } else { s.shape.geom.rect_as_polyline() };
                Shape { geom: g.map(|p| f.placement.apply(p)), ..s.shape.clone() }
            })
        });
        own.chain(fps).collect()
    }

    /// A new board with a rectangular outline `w` × `h` with its bottom-left corner at the
    /// origin.
    pub fn with_rect_outline(w: Nm, h: Nm) -> Board {
        let mut b = Board::default();
        b.shapes.push(BoardShape {
            id: Uuid::new_v4(),
            shape: Shape {
                geom: crate::graphics::Geom::Rect { a: Pt::ZERO, b: Pt::new(w, h) },
                stroke: crate::graphics::Stroke::width(mm(0.05)),
                fill: crate::graphics::Fill::None,
            },
            layer: Layer::Outline,
            locked: false,
            net: String::new(),
        });
        b
    }
}
