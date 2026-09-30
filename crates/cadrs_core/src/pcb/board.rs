//! [`PcbBoard`]: one board of a PCB Studio, in mm, with stable ids for its items (moved here
//! from `cadrs_pcb` in P3H.3 so the PCB Studio element can hold boards; `cadrs_pcb::board`
//! re-exports it).

use cadrs_idf::{Board, Layers, Library, Loop, MountSide, Placement, Side, Units};
use serde::{Deserialize, Serialize};

/// A stable id for a board item (a component placement or a keep area). It is given when the
/// board is made and kept while the item exists, so selections, the BOM and sync can refer to
/// items across edits. Ids are unique within one [`PcbBoard`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ItemId(pub u64);

/// The IDF section a keep area comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum KeepKind {
    /// `.PLACE_KEEPOUT`: no components (taller than its height) here.
    PlaceKeepout,
    /// `.PLACE_REGION`: a keep-in for a component group.
    PlaceRegion,
    /// `.ROUTE_KEEPOUT`: no routing here.
    RouteKeepout,
    /// `.VIA_KEEPOUT`: no vias here.
    ViaKeepout,
    /// `.ROUTE_OUTLINE`: routing stays inside (a keep-in).
    RouteOutline,
    /// `.PLACE_OUTLINE`: components stay inside (a keep-in).
    PlaceOutline,
    /// `.OTHER_OUTLINE`: a heatsink, a board core, ... (a solid of its thickness on one side).
    OtherOutline,
}

impl KeepKind {
    /// Every kind, in [`PcbBoard::keep_areas`] order.
    pub const ALL: [KeepKind; 7] = [
        KeepKind::PlaceKeepout,
        KeepKind::PlaceRegion,
        KeepKind::RouteKeepout,
        KeepKind::ViaKeepout,
        KeepKind::RouteOutline,
        KeepKind::PlaceOutline,
        KeepKind::OtherOutline,
    ];

    /// Keep-outs, as opposed to keep-ins and other outlines.
    pub fn is_keepout(self) -> bool {
        matches!(self, KeepKind::PlaceKeepout | KeepKind::RouteKeepout | KeepKind::ViaKeepout)
    }

    pub fn is_keepin(self) -> bool {
        matches!(self, KeepKind::PlaceRegion | KeepKind::RouteOutline | KeepKind::PlaceOutline)
    }

    /// The name of its body ("Keep-out 1", "Route keep-out 2", ...).
    pub fn label(self) -> &'static str {
        match self {
            KeepKind::PlaceKeepout => "Keep-out",
            KeepKind::PlaceRegion => "Keep-in",
            KeepKind::RouteKeepout => "Route keep-out",
            KeepKind::ViaKeepout => "Via keep-out",
            KeepKind::RouteOutline => "Route keep-in",
            KeepKind::PlaceOutline => "Place keep-in",
            KeepKind::OtherOutline => "Other outline",
        }
    }
}

/// One keep area (or other outline) of a board, flattened from the IDF sections, in mm.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct KeepArea {
    pub id: ItemId,
    pub kind: KeepKind,
    /// Top, Bottom or Both (route layers and 2.0 defaults mapped onto a side).
    pub side: Side,
    /// Its height above (below) the board face, if the file gives one greater than 0.
    pub height: Option<f64>,
    /// Outline loops, board coordinates in mm.
    pub loops: Vec<Loop>,
    /// The place region's group, the other outline's id; empty otherwise.
    pub label: String,
    /// Its place in its IDF section (0-based).
    pub index: usize,
}

/// One board of a PCB Studio: the IDF board and library converted to mm, with a stable id for
/// every component placement and keep area.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PcbBoard {
    /// The board file, all lengths in mm.
    pub board: Board,
    /// The library, every package in mm.
    pub library: Library,
    /// One id per `board.placements` entry, in the same order.
    pub component_ids: Vec<ItemId>,
    /// The keep areas' ids, per IDF section (one per entry of that section, in the same
    /// order), so adding or removing an area of one kind never changes another area's id.
    pub keep_ids: KeepIds,
    next_id: u64,
}

/// The ids of a board's keep areas, one list per IDF section ([`KeepKind`]), parallel to that
/// section's entries.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct KeepIds {
    pub place_keepouts: Vec<ItemId>,
    pub place_regions: Vec<ItemId>,
    pub route_keepouts: Vec<ItemId>,
    pub via_keepouts: Vec<ItemId>,
    pub route_outlines: Vec<ItemId>,
    pub place_outlines: Vec<ItemId>,
    pub other_outlines: Vec<ItemId>,
}

impl KeepIds {
    pub fn of(&self, kind: KeepKind) -> &Vec<ItemId> {
        match kind {
            KeepKind::PlaceKeepout => &self.place_keepouts,
            KeepKind::PlaceRegion => &self.place_regions,
            KeepKind::RouteKeepout => &self.route_keepouts,
            KeepKind::ViaKeepout => &self.via_keepouts,
            KeepKind::RouteOutline => &self.route_outlines,
            KeepKind::PlaceOutline => &self.place_outlines,
            KeepKind::OtherOutline => &self.other_outlines,
        }
    }

    pub fn of_mut(&mut self, kind: KeepKind) -> &mut Vec<ItemId> {
        match kind {
            KeepKind::PlaceKeepout => &mut self.place_keepouts,
            KeepKind::PlaceRegion => &mut self.place_regions,
            KeepKind::RouteKeepout => &mut self.route_keepouts,
            KeepKind::ViaKeepout => &mut self.via_keepouts,
            KeepKind::RouteOutline => &mut self.route_outlines,
            KeepKind::PlaceOutline => &mut self.place_outlines,
            KeepKind::OtherOutline => &mut self.other_outlines,
        }
    }
}

fn layers_side(l: Layers) -> Side {
    match l {
        Layers::Top => Side::Top,
        Layers::Bottom => Side::Bottom,
        Layers::Both | Layers::Inner | Layers::All => Side::Both,
    }
}

fn positive(h: Option<f64>) -> Option<f64> {
    h.filter(|h| *h > 0.0)
}

impl PcbBoard {
    /// Takes an IDF pair (any units) and converts it to mm.
    pub fn new(board: &Board, library: &Library) -> PcbBoard {
        let board = board.converted(Units::Mm);
        let mut library = library.clone();
        for p in &mut library.packages {
            *p = p.converted(Units::Mm);
        }
        let mut b = PcbBoard { board, library, component_ids: vec![], keep_ids: KeepIds::default(), next_id: 1 };
        b.component_ids = (0..b.board.placements.len()).map(|_| b.fresh_id()).collect();
        b.fill_ids();
        b
    }

    /// Gives an id to every keep area and component that has none yet (entries added at the
    /// end of a section or of the placements since the ids were last filled); ids of existing
    /// entries are kept.
    pub fn fill_ids(&mut self) {
        for kind in KeepKind::ALL {
            let n = self.keep_count_of(kind);
            while self.keep_ids.of(kind).len() < n {
                let id = self.fresh_id();
                self.keep_ids.of_mut(kind).push(id);
            }
            self.keep_ids.of_mut(kind).truncate(n);
        }
        while self.component_ids.len() < self.board.placements.len() {
            let id = self.fresh_id();
            self.component_ids.push(id);
        }
        self.component_ids.truncate(self.board.placements.len());
    }

    /// The number of entries of one keep section.
    pub fn keep_count_of(&self, kind: KeepKind) -> usize {
        let b = &self.board;
        match kind {
            KeepKind::PlaceKeepout => b.place_keepouts.len(),
            KeepKind::PlaceRegion => b.place_regions.len(),
            KeepKind::RouteKeepout => b.route_keepouts.len(),
            KeepKind::ViaKeepout => b.via_keepouts.len(),
            KeepKind::RouteOutline => b.route_outlines.len(),
            KeepKind::PlaceOutline => b.place_outlines.len(),
            KeepKind::OtherOutline => b.other_outlines.len(),
        }
    }

    /// The id of the `index`th entry of a keep section.
    pub fn keep_id(&self, kind: KeepKind, index: usize) -> Option<ItemId> {
        self.keep_ids.of(kind).get(index).copied()
    }

    /// Where a keep area is: its section and its place in it.
    pub fn keep_position(&self, id: ItemId) -> Option<(KeepKind, usize)> {
        KeepKind::ALL.into_iter().find_map(|k| self.keep_ids.of(k).iter().position(|i| *i == id).map(|i| (k, i)))
    }

    /// Removes a keep area (its IDF entry and its id); the other areas keep their ids.
    pub fn remove_keep_area(&mut self, id: ItemId) -> bool {
        let Some((kind, i)) = self.keep_position(id) else {
            return false;
        };
        let b = &mut self.board;
        match kind {
            KeepKind::PlaceKeepout => {
                b.place_keepouts.remove(i);
            }
            KeepKind::PlaceRegion => {
                b.place_regions.remove(i);
            }
            KeepKind::RouteKeepout => {
                b.route_keepouts.remove(i);
            }
            KeepKind::ViaKeepout => {
                b.via_keepouts.remove(i);
            }
            KeepKind::RouteOutline => {
                b.route_outlines.remove(i);
            }
            KeepKind::PlaceOutline => {
                b.place_outlines.remove(i);
            }
            KeepKind::OtherOutline => {
                b.other_outlines.remove(i);
            }
        }
        self.keep_ids.of_mut(kind).remove(i);
        true
    }

    /// Removes a component placement and its id; the others keep theirs.
    pub fn remove_component(&mut self, id: ItemId) -> bool {
        let Some(i) = self.component_ids.iter().position(|c| *c == id) else {
            return false;
        };
        self.component_ids.remove(i);
        self.board.placements.remove(i);
        true
    }

    /// Replaces the board and library (a re-sync, PCB5.6, X8) keeping this board's name and its
    /// id counter: each keep area gets the id `keep` gives for its section and place (else a new
    /// one), each component the id `component` gives for its place (else a new one). An id is
    /// used once; a repeat gets a new one.
    pub fn resync(&mut self, board: &Board, library: &Library, keep: &dyn Fn(KeepKind, usize) -> Option<ItemId>, component: &dyn Fn(usize) -> Option<ItemId>) {
        let fresh = PcbBoard::new(board, library);
        let name = std::mem::take(&mut self.board.name);
        self.board = fresh.board;
        self.board.name = name;
        self.library = fresh.library;
        let mut used = std::collections::HashSet::new();
        let mut ids = KeepIds::default();
        for kind in KeepKind::ALL {
            for i in 0..self.keep_count_of(kind) {
                let id = keep(kind, i).filter(|id| used.insert(*id)).unwrap_or_else(|| self.fresh_id());
                ids.of_mut(kind).push(id);
            }
        }
        self.keep_ids = ids;
        self.component_ids = (0..self.board.placements.len())
            .map(|i| component(i).filter(|id| used.insert(*id)).unwrap_or_else(|| self.fresh_id()))
            .collect();
    }

    /// Parses an `.emn` / `.emp` pair.
    pub fn read(emn: &str, emp: &str) -> Result<PcbBoard, cadrs_idf::IdfError> {
        let (b, l) = cadrs_idf::read_pair(emn, emp)?;
        Ok(PcbBoard::new(&b, &l))
    }

    /// A new id, never given before on this board.
    pub fn fresh_id(&mut self) -> ItemId {
        let id = ItemId(self.next_id);
        self.next_id += 1;
        id
    }

    pub fn name(&self) -> &str {
        &self.board.name
    }

    /// Board thickness (mm).
    pub fn thickness(&self) -> f64 {
        self.board.thickness_mm()
    }

    /// Every keep area and other outline, in the order: place keep-outs, place regions, route
    /// keep-outs, via keep-outs, route outlines, place outlines, other outlines.
    pub fn keep_areas(&self) -> Vec<KeepArea> {
        let b = &self.board;
        let mut out: Vec<KeepArea> = Vec::new();
        let ids = &self.keep_ids;
        let mut push = |kind: KeepKind, side, height, loops: &Vec<Loop>, label: String, index: usize| {
            let id = ids.of(kind).get(index).copied().unwrap_or(ItemId(0));
            out.push(KeepArea { id, kind, side, height, loops: loops.clone(), label, index })
        };
        for (i, k) in b.place_keepouts.iter().enumerate() {
            push(KeepKind::PlaceKeepout, k.side, positive(k.height), &k.loops, String::new(), i);
        }
        for (i, k) in b.place_regions.iter().enumerate() {
            push(KeepKind::PlaceRegion, k.side, None, &k.loops, k.group.clone(), i);
        }
        for (i, k) in b.route_keepouts.iter().enumerate() {
            push(KeepKind::RouteKeepout, layers_side(k.layers), None, &k.loops, String::new(), i);
        }
        for (i, k) in b.via_keepouts.iter().enumerate() {
            push(KeepKind::ViaKeepout, Side::Both, None, &k.loops, String::new(), i);
        }
        for (i, k) in b.route_outlines.iter().enumerate() {
            let side = k.layers.map_or(Side::Both, layers_side);
            push(KeepKind::RouteOutline, side, None, &k.loops, String::new(), i);
        }
        for (i, k) in b.place_outlines.iter().enumerate() {
            push(KeepKind::PlaceOutline, k.side.unwrap_or(Side::Both), positive(k.height), &k.loops, String::new(), i);
        }
        for (i, k) in b.other_outlines.iter().enumerate() {
            let side = match k.side {
                Some(MountSide::Bottom) => Side::Bottom,
                _ => Side::Top,
            };
            push(KeepKind::OtherOutline, side, positive(Some(k.thickness)), &k.loops, k.id.clone(), i);
        }
        out
    }

    /// The component placements with their ids.
    pub fn components(&self) -> impl Iterator<Item = (ItemId, &Placement)> {
        self.component_ids.iter().copied().zip(&self.board.placements)
    }

    pub fn component(&self, id: ItemId) -> Option<&Placement> {
        self.components().find(|(i, _)| *i == id).map(|(_, p)| p)
    }
}
