//! The IDF data model (X2): board (.emn) and library (.emp) files, independent of the version.
//!
//! Every length is stored in the file's own units ([`Board::units`], [`Package::units`]); use
//! [`Units::to_mm`] or [`Board::converted`] / [`Package::converted`] to get millimetres.
//! Fields that only one version of IDF can hold are `Option`s or have a neutral default;
//! [`Board::for_version`] and [`Library::for_version`] project data onto what a version can hold,
//! and the writer always writes that projection.

use serde::{Deserialize, Serialize};

use crate::geom::Loop;

/// The source-system string written by cadrs.
pub const CADRS_SOURCE: &str = "cadrs PCB Studio v0.1";
/// The date written when a header has none (`yyyy/mm/dd.hh:mm:ss`, as the spec requires).
pub const DEFAULT_DATE: &str = "2026/09/29.12:00:00";

/// IDF version. 1.0 predates the sectioned format and isn't supported.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum IdfVersion {
    V2,
    V3,
}

impl IdfVersion {
    pub fn as_str(self) -> &'static str {
        match self {
            IdfVersion::V2 => "2.0",
            IdfVersion::V3 => "3.0",
        }
    }
}

/// Length units. `Tnm` (ten nanometres) exists only in IDF 2.0; 3.0 dropped it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Units {
    Mm,
    Thou,
    Tnm,
}

impl Units {
    /// Exact size of one unit in nanometres (1 mm = 1e6, 1 thou = 25400, 1 TNM = 10).
    /// Conversions multiply by this and divide by the target's, so exact inputs such as
    /// 4000 thou give correctly rounded results (101.6 mm).
    pub fn nm_per_unit(self) -> f64 {
        match self {
            Units::Mm => 1e6,
            Units::Thou => 25400.0,
            Units::Tnm => 10.0,
        }
    }

    /// Millimetres per unit (1 thou = 0.0254 mm, 1 TNM = 1e-5 mm).
    pub fn mm_per_unit(self) -> f64 {
        self.to_mm(1.0)
    }

    /// Convert a value in `self` units to `to` units.
    pub fn convert(self, v: f64, to: Units) -> f64 {
        if self == to { v } else { v * self.nm_per_unit() / to.nm_per_unit() }
    }

    pub fn to_mm(self, v: f64) -> f64 {
        self.convert(v, Units::Mm)
    }

    pub fn from_mm(self, mm: f64) -> f64 {
        Units::Mm.convert(mm, self)
    }

    pub fn keyword(self) -> &'static str {
        match self {
            Units::Mm => "MM",
            Units::Thou => "THOU",
            Units::Tnm => "TNM",
        }
    }
}

/// Header record 2, `file_type version "source" date file_version`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Header {
    pub file_type: FileType,
    pub version: IdfVersion,
    pub source_system: String,
    /// `yyyy/mm/dd.hh:mm:ss` per the spec (both spec examples actually use `mm/dd/yy.hh:mm:ss`),
    /// kept as written.
    pub date: String,
    pub file_version: u32,
}

impl Header {
    /// A header stamped with cadrs's own source string.
    pub fn cadrs(file_type: FileType, version: IdfVersion, date: &str) -> Header {
        Header {
            file_type,
            version,
            source_system: CADRS_SOURCE.to_string(),
            date: date.to_string(),
            file_version: 1,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FileType {
    Board,
    /// IDF 3.0 only.
    Panel,
    Library,
}

impl FileType {
    pub fn keyword(self) -> &'static str {
        match self {
            FileType::Board => "BOARD_FILE",
            FileType::Panel => "PANEL_FILE",
            FileType::Library => "LIBRARY_FILE",
        }
    }
}

/// Which system owns an entity (IDF 3.0). IDF 2.0 has no owners; it reads as `Unowned`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Owner {
    Ecad,
    Mcad,
    #[default]
    Unowned,
}

impl Owner {
    pub fn keyword(self) -> &'static str {
        match self {
            Owner::Ecad => "ECAD",
            Owner::Mcad => "MCAD",
            Owner::Unowned => "UNOWNED",
        }
    }
}

/// Board side of a keep area / region / place outline.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Side {
    Top,
    Bottom,
    Both,
}

impl Side {
    pub fn keyword(self) -> &'static str {
        match self {
            Side::Top => "TOP",
            Side::Bottom => "BOTTOM",
            Side::Both => "BOTH",
        }
    }
}

/// Side a component (or other outline) is mounted on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MountSide {
    Top,
    Bottom,
}

impl MountSide {
    pub fn keyword(self) -> &'static str {
        match self {
            MountSide::Top => "TOP",
            MountSide::Bottom => "BOTTOM",
        }
    }
}

/// Routing layers of a route outline or route keepout. `Inner` is IDF 3.0 only.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Layers {
    Top,
    Bottom,
    Both,
    Inner,
    All,
}

impl Layers {
    pub fn keyword(self) -> &'static str {
        match self {
            Layers::Top => "TOP",
            Layers::Bottom => "BOTTOM",
            Layers::Both => "BOTH",
            Layers::Inner => "INNER",
            Layers::All => "ALL",
        }
    }
}

/// `.BOARD_OUTLINE` / `.PANEL_OUTLINE`: the first loop is the outline (label 0), later loops
/// are cut-outs (labels 1..n).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BoardOutline {
    pub owner: Owner,
    pub thickness: f64,
    pub loops: Vec<Loop>,
}

/// `.OTHER_OUTLINE` (heatsink, board core, ...).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OtherOutline {
    pub owner: Owner,
    pub id: String,
    pub thickness: f64,
    /// IDF 3.0 only.
    pub side: Option<MountSide>,
    pub loops: Vec<Loop>,
}

/// `.ROUTE_OUTLINE`: routing must stay inside (a keep-in).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RouteOutline {
    pub owner: Owner,
    /// IDF 3.0 only; 2.0 route outlines apply to all layers.
    pub layers: Option<Layers>,
    pub loops: Vec<Loop>,
}

/// `.PLACE_OUTLINE`: components must stay inside (a keep-in).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlaceOutline {
    pub owner: Owner,
    /// IDF 3.0 only; 2.0 place outlines apply to both sides.
    pub side: Option<Side>,
    /// IDF 3.0 only, optional there too (missing = no height restriction).
    pub height: Option<f64>,
    pub loops: Vec<Loop>,
}

/// `.ROUTE_KEEPOUT`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RouteKeepout {
    pub owner: Owner,
    pub layers: Layers,
    pub loops: Vec<Loop>,
}

/// `.VIA_KEEPOUT`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ViaKeepout {
    pub owner: Owner,
    pub loops: Vec<Loop>,
}

/// `.PLACE_KEEPOUT`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlaceKeepout {
    pub owner: Owner,
    pub side: Side,
    /// Components taller than this are excluded; 0 excludes all. 2.0 calls it the maximum
    /// height.
    pub height: Option<f64>,
    /// IDF 2.0 only: the minimum height.
    pub min_height: Option<f64>,
    pub loops: Vec<Loop>,
}

/// `.PLACE_REGION`: a placement group area (keep-in for a named component group).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlaceRegion {
    pub owner: Owner,
    pub side: Side,
    pub group: String,
    pub loops: Vec<Loop>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Plating {
    Pth,
    Npth,
}

impl Plating {
    pub fn keyword(self) -> &'static str {
        match self {
            Plating::Pth => "PTH",
            Plating::Npth => "NPTH",
        }
    }
}

/// Drilled-hole "associated part".
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum HoleAssoc {
    Board,
    NoRefdes,
    /// IDF 3.0 only.
    Panel,
    Refdes(String),
}

/// Drilled-hole type (IDF 3.0 only).
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum HoleKind {
    Pin,
    Via,
    Mtg,
    Tool,
    Other(String),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DrilledHole {
    pub dia: f64,
    pub x: f64,
    pub y: f64,
    pub plating: Plating,
    pub assoc: HoleAssoc,
    /// IDF 3.0 only.
    pub kind: Option<HoleKind>,
    /// IDF 3.0 only (2.0 reads as `Unowned`).
    pub owner: Owner,
}

/// `.NOTES` record (IDF 3.0 only).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Note {
    pub x: f64,
    pub y: f64,
    pub text_height: f64,
    pub text_length: f64,
    pub text: String,
}

/// Placement status. `Fixed` is IDF 2.0's; `Mcad`/`Ecad` are 3.0's owned-fixed statuses.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Status {
    Placed,
    Unplaced,
    Mcad,
    Ecad,
    Fixed,
}

impl Status {
    pub fn keyword(self) -> &'static str {
        match self {
            Status::Placed => "PLACED",
            Status::Unplaced => "UNPLACED",
            Status::Mcad => "MCAD",
            Status::Ecad => "ECAD",
            Status::Fixed => "FIXED",
        }
    }
}

/// `.PLACEMENT` record pair.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Placement {
    pub package: String,
    pub part_number: String,
    /// Reference designator, `NOREFDES` for mechanical parts, `BOARD` for boards on a panel.
    pub refdes: String,
    pub x: f64,
    pub y: f64,
    /// IDF 3.0 only (0 in 2.0).
    pub mount_offset: f64,
    /// Degrees, counter-clockwise in the component's own frame.
    pub rotation: f64,
    pub side: MountSide,
    pub status: Status,
}

/// A board (or panel) file, `.emn`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Board {
    pub header: Header,
    pub name: String,
    pub units: Units,
    pub outline: Option<BoardOutline>,
    pub other_outlines: Vec<OtherOutline>,
    pub route_outlines: Vec<RouteOutline>,
    pub place_outlines: Vec<PlaceOutline>,
    pub route_keepouts: Vec<RouteKeepout>,
    pub via_keepouts: Vec<ViaKeepout>,
    pub place_keepouts: Vec<PlaceKeepout>,
    pub place_regions: Vec<PlaceRegion>,
    pub holes: Vec<DrilledHole>,
    pub notes: Vec<Note>,
    pub placements: Vec<Placement>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PackageKind {
    Electrical,
    Mechanical,
}

/// `.ELECTRICAL` / `.MECHANICAL` component in the library.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Package {
    pub kind: PackageKind,
    pub name: String,
    pub part_number: String,
    pub units: Units,
    pub height: f64,
    pub loops: Vec<Loop>,
    /// `PROP name value` records (IDF 3.0 only, electrical components).
    pub props: Vec<(String, String)>,
}

/// A library file, `.emp`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Library {
    pub header: Header,
    pub packages: Vec<Package>,
}

fn scale_loops(loops: &mut [Loop], k: &impl Fn(f64) -> f64) {
    for l in loops {
        l.map_coords(k);
    }
}

impl Board {
    /// An empty board with a cadrs header.
    pub fn new(name: &str, units: Units, version: IdfVersion) -> Board {
        Board {
            header: Header::cadrs(FileType::Board, version, DEFAULT_DATE),
            name: name.to_string(),
            units,
            outline: None,
            other_outlines: vec![],
            route_outlines: vec![],
            place_outlines: vec![],
            route_keepouts: vec![],
            via_keepouts: vec![],
            place_keepouts: vec![],
            place_regions: vec![],
            holes: vec![],
            notes: vec![],
            placements: vec![],
        }
    }

    /// Board thickness in mm (0 without an outline).
    pub fn thickness_mm(&self) -> f64 {
        self.outline.as_ref().map_or(0.0, |o| self.units.to_mm(o.thickness))
    }

    /// All placements of a package (by package name).
    pub fn placements_for<'a>(&'a self, package: &'a str) -> impl Iterator<Item = &'a Placement> + 'a {
        self.placements.iter().filter(move |p| p.package == package)
    }

    pub fn placement(&self, refdes: &str) -> Option<&Placement> {
        self.placements.iter().find(|p| p.refdes == refdes)
    }

    /// A placed component's outline loops in board coordinates, in mm (see
    /// [`Placement::place_loops`]). `None` if the library has no such package.
    pub fn placed_outline(&self, placement: &Placement, library: &Library) -> Option<Vec<Loop>> {
        let pkg = library.package(&placement.package, &placement.part_number)?;
        Some(placement.place_loops(pkg, self.units))
    }

    /// The same board with every length converted to `units`.
    pub fn converted(&self, units: Units) -> Board {
        let mut b = self.clone();
        let from = self.units;
        b.units = units;
        if from == units {
            return b;
        }
        let k = &|v: f64| from.convert(v, units);
        if let Some(o) = &mut b.outline {
            o.thickness = k(o.thickness);
            scale_loops(&mut o.loops, k);
        }
        for o in &mut b.other_outlines {
            o.thickness = k(o.thickness);
            scale_loops(&mut o.loops, k);
        }
        for o in &mut b.route_outlines {
            scale_loops(&mut o.loops, k);
        }
        for o in &mut b.place_outlines {
            o.height = o.height.map(k);
            scale_loops(&mut o.loops, k);
        }
        for o in &mut b.route_keepouts {
            scale_loops(&mut o.loops, k);
        }
        for o in &mut b.via_keepouts {
            scale_loops(&mut o.loops, k);
        }
        for o in &mut b.place_keepouts {
            o.height = o.height.map(k);
            o.min_height = o.min_height.map(k);
            scale_loops(&mut o.loops, k);
        }
        for o in &mut b.place_regions {
            scale_loops(&mut o.loops, k);
        }
        for h in &mut b.holes {
            h.dia = k(h.dia);
            h.x = k(h.x);
            h.y = k(h.y);
        }
        for n in &mut b.notes {
            n.x = k(n.x);
            n.y = k(n.y);
            n.text_height = k(n.text_height);
            n.text_length = k(n.text_length);
        }
        for p in &mut b.placements {
            p.x = k(p.x);
            p.y = k(p.y);
            p.mount_offset = k(p.mount_offset);
        }
        b
    }

    /// True if the board has any keep area: route/place outlines (keep-ins), route/via/place
    /// keepouts, or place regions.
    pub fn has_keep_areas(&self) -> bool {
        !(self.route_outlines.is_empty()
            && self.place_outlines.is_empty()
            && self.route_keepouts.is_empty()
            && self.via_keepouts.is_empty()
            && self.place_keepouts.is_empty()
            && self.place_regions.is_empty())
    }

    /// The board without keep-in/keep-out areas (what PCB Studio's "IDF 2.0" export writes;
    /// see the crate docs).
    pub fn without_keep_areas(&self) -> Board {
        let mut b = self.clone();
        b.route_outlines.clear();
        b.place_outlines.clear();
        b.route_keepouts.clear();
        b.via_keepouts.clear();
        b.place_keepouts.clear();
        b.place_regions.clear();
        b
    }

    /// The board projected onto what `version` can hold. Writing always writes this, so
    /// `parse(write(b, v)) == b.for_version(v)`.
    ///
    /// Both: an empty header date becomes [`DEFAULT_DATE`].
    ///
    /// To 2.0: owners become UNOWNED; notes, mount offsets, hole types, other-outline sides,
    /// route-outline layers and place-outline side/height are dropped; INNER layers become ALL;
    /// MCAD/ECAD statuses become FIXED; PANEL becomes BOARD; 360° circles become two 180° arcs;
    /// place keepouts get a max and min height (0 when missing).
    ///
    /// To 3.0: TNM units become MM; FIXED becomes MCAD; missing hole types become OTHER,
    /// missing other-outline sides TOP, route layers ALL and place-outline sides BOTH (the 2.0
    /// meanings); 2.0 minimum keepout heights are dropped.
    pub fn for_version(&self, version: IdfVersion) -> Board {
        let mut b = if version == IdfVersion::V3 && self.units == Units::Tnm {
            self.converted(Units::Mm)
        } else {
            self.clone()
        };
        b.header.version = version;
        if b.header.date.is_empty() {
            b.header.date = DEFAULT_DATE.to_string();
        }
        match version {
            IdfVersion::V2 => {
                if b.header.file_type == FileType::Panel {
                    b.header.file_type = FileType::Board;
                }
                b.notes.clear();
                if let Some(o) = &mut b.outline {
                    o.owner = Owner::Unowned;
                    loops_to_v2(&mut o.loops);
                }
                for o in &mut b.other_outlines {
                    o.owner = Owner::Unowned;
                    o.side = None;
                    loops_to_v2(&mut o.loops);
                }
                for o in &mut b.route_outlines {
                    o.owner = Owner::Unowned;
                    o.layers = None;
                    loops_to_v2(&mut o.loops);
                }
                for o in &mut b.place_outlines {
                    o.owner = Owner::Unowned;
                    o.side = None;
                    o.height = None;
                    loops_to_v2(&mut o.loops);
                }
                for o in &mut b.route_keepouts {
                    o.owner = Owner::Unowned;
                    if o.layers == Layers::Inner {
                        o.layers = Layers::All;
                    }
                    loops_to_v2(&mut o.loops);
                }
                for o in &mut b.via_keepouts {
                    o.owner = Owner::Unowned;
                    loops_to_v2(&mut o.loops);
                }
                for o in &mut b.place_keepouts {
                    o.owner = Owner::Unowned;
                    o.height = Some(o.height.unwrap_or(0.0));
                    o.min_height = Some(o.min_height.unwrap_or(0.0));
                    loops_to_v2(&mut o.loops);
                }
                for o in &mut b.place_regions {
                    o.owner = Owner::Unowned;
                    loops_to_v2(&mut o.loops);
                }
                for h in &mut b.holes {
                    h.kind = None;
                    h.owner = Owner::Unowned;
                    if h.assoc == HoleAssoc::Panel {
                        h.assoc = HoleAssoc::Board;
                    }
                }
                for p in &mut b.placements {
                    p.mount_offset = 0.0;
                    if matches!(p.status, Status::Mcad | Status::Ecad) {
                        p.status = Status::Fixed;
                    }
                }
            }
            IdfVersion::V3 => {
                for o in &mut b.other_outlines {
                    o.side.get_or_insert(MountSide::Top);
                }
                for o in &mut b.route_outlines {
                    o.layers.get_or_insert(Layers::All);
                }
                for o in &mut b.place_outlines {
                    o.side.get_or_insert(Side::Both);
                }
                for o in &mut b.place_keepouts {
                    o.min_height = None;
                }
                for h in &mut b.holes {
                    h.kind.get_or_insert_with(|| HoleKind::Other("OTHER".to_string()));
                }
                for p in &mut b.placements {
                    if p.status == Status::Fixed {
                        p.status = Status::Mcad;
                    }
                }
            }
        }
        b
    }
}

/// Replace 360° circles (IDF 3.0) with two 180° arcs (IDF 2.0 has no circle form; its own
/// example writes circular cut-outs this way).
fn loops_to_v2(loops: &mut [Loop]) {
    for l in loops {
        *l = l.without_circles();
    }
}

impl Package {
    /// The same package with lengths converted to `units`.
    pub fn converted(&self, units: Units) -> Package {
        let mut p = self.clone();
        let from = self.units;
        p.units = units;
        if from != units {
            let k = &|v: f64| from.convert(v, units);
            p.height = k(p.height);
            scale_loops(&mut p.loops, k);
        }
        p
    }

    pub fn height_mm(&self) -> f64 {
        self.units.to_mm(self.height)
    }
}

impl Library {
    pub fn new(version: IdfVersion) -> Library {
        Library { header: Header::cadrs(FileType::Library, version, DEFAULT_DATE), packages: vec![] }
    }

    /// The package with this geometry name and part number.
    pub fn package(&self, name: &str, part_number: &str) -> Option<&Package> {
        self.packages.iter().find(|p| p.name == name && p.part_number == part_number)
    }

    /// See [`Board::for_version`]: 2.0 drops PROP records and circles; 3.0 converts TNM to MM.
    pub fn for_version(&self, version: IdfVersion) -> Library {
        let mut l = self.clone();
        l.header.version = version;
        l.header.file_type = FileType::Library;
        if l.header.date.is_empty() {
            l.header.date = DEFAULT_DATE.to_string();
        }
        for p in &mut l.packages {
            match version {
                IdfVersion::V2 => {
                    p.props.clear();
                    loops_to_v2(&mut p.loops);
                }
                IdfVersion::V3 => {
                    if p.units == Units::Tnm {
                        *p = p.converted(Units::Mm);
                    }
                }
            }
        }
        l
    }
}

impl Placement {
    /// The package outline placed on the board, in board coordinates in mm.
    ///
    /// Per the spec: the package origin goes to (x, y); a BOTTOM component is flipped about its
    /// local Y axis; the rotation is counter-clockwise *in the component's own frame*. For a
    /// bottom component that frame is mirrored, so the rotation reads clockwise from the top
    /// (this matches the spec's Figure 1). Net map: `p' = (x, y) + M · R(rot) · p`, with
    /// `M = diag(-1, 1)` on the bottom. Mirrored loops get their arc angles negated so the
    /// geometry stays the same curve.
    pub fn place_loops(&self, package: &Package, board_units: Units) -> Vec<Loop> {
        let pu = package.units;
        let (s, c) = crate::geom::sin_cos_deg(self.rotation);
        let (ox, oy) = (board_units.to_mm(self.x), board_units.to_mm(self.y));
        let bottom = self.side == MountSide::Bottom;
        package
            .loops
            .iter()
            .map(|l| {
                l.map_points(bottom, |x, y| {
                    let (x, y) = (pu.to_mm(x), pu.to_mm(y));
                    let (rx, ry) = (c * x - s * y, s * x + c * y);
                    let rx = if bottom { -rx } else { rx };
                    (ox + rx, oy + ry)
                })
            })
            .collect()
    }
}
