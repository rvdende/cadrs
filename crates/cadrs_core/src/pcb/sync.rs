//! **Sync a Part Studio or assembly with PCB Studio** (P3H.5, PCB5.2–5.6, PCB9.6, X8): the
//! board made from a tab's parts ([`SyncBoard`], computed by `cadrs_pcb::sync`) goes into the
//! PCB Studio as a new board named after the tab, or — when a board was synced from that tab
//! before — **updates that board in place** (its outline, keep areas and placements; one undo
//! step), keeping its name and the ids of the keep areas that come from the same parts (by part
//! name) and of the components with the same designator.
//!
//! The board remembers where it came from ([`McadSource`]): the tab and the plane (the Sync
//! dialog pre-fills them for a re-sync), which part each keep area came from, and each synced
//! component's package frame in its part's own coordinates, so a later sync reads a moved
//! instance's placement from the same frame.

use cadrs_idf::{Board, Library};
use serde::{Deserialize, Serialize};

use super::{BoardId, BoardSource, ItemId, KeepKind, PcbBoard, studio_mut};
use crate::assembly::Pose;
use crate::command::{Command, CommandError, Scope};
use crate::document::Document;
use crate::ids::ElementId;

/// "Top face of board part is parallel to" (PCB5.2): the default planes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SyncPlaneChoice {
    #[default]
    Top,
    Front,
    Right,
}

impl SyncPlaneChoice {
    pub const ALL: [SyncPlaneChoice; 3] = [SyncPlaneChoice::Top, SyncPlaneChoice::Front, SyncPlaneChoice::Right];

    /// The dropdown's label ("Top Plane").
    pub fn label(self) -> &'static str {
        match self {
            SyncPlaneChoice::Top => "Top Plane",
            SyncPlaneChoice::Front => "Front Plane",
            SyncPlaneChoice::Right => "Right Plane",
        }
    }
}

/// Where a synced board came from, and what keeps its ids across re-syncs.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct McadSource {
    /// The Part Studio or Assembly tab.
    pub element: ElementId,
    pub plane: SyncPlaneChoice,
    /// Each keep area's id and the name of the part it came from.
    #[serde(default)]
    pub keep_parts: Vec<(ItemId, String)>,
    /// Each synced component's package frame in its part's own coordinates, by designator.
    #[serde(default)]
    pub frames: Vec<(String, Pose)>,
}

impl McadSource {
    pub fn frame(&self, refdes: &str) -> Option<Pose> {
        self.frames.iter().find(|(r, _)| r == refdes).map(|(_, p)| *p)
    }
}

impl super::PcbStudio {
    /// The board last synced from the tab `element` (the shown one first when several were),
    /// else the board Create assembly made the tab from (P3H.6).
    pub fn synced_from(&self, element: ElementId, shown: Option<BoardId>) -> Option<BoardId> {
        let from = |b: &&super::StudioBoard| matches!(&b.source, BoardSource::Mcad(m) if m.element == element);
        if let Some(s) = shown.and_then(|id| self.board(id)).filter(|b| from(b)) {
            return Some(s.id);
        }
        // P3H.6: the tabs Create assembly made from a board sync back into that board.
        self.boards.iter().find(|b| from(b)).map(|b| b.id).or_else(|| self.generated_from(element).map(|g| g.board).filter(|b| self.board(*b).is_some()))
    }
}

/// What a sync computed (`cadrs_pcb::sync`), to be put into a PCB Studio.
#[derive(Debug, Clone)]
pub struct SyncBoard {
    pub element: ElementId,
    /// The board to update (a re-sync); `None` adds a new board named `name`.
    pub target: Option<BoardId>,
    pub name: String,
    /// The board in mm and its library.
    pub board: Box<Board>,
    pub library: Library,
    /// The part each `.PLACE_KEEPOUT` and each `.PLACE_REGION` came from, in order.
    pub keepout_parts: Vec<String>,
    pub keepin_parts: Vec<String>,
    pub source: ElementId,
    pub plane: SyncPlaneChoice,
    /// The synced components' package frames (see [`McadSource::frames`]).
    pub frames: Vec<(String, Pose)>,
}

impl SyncBoard {
    fn part_of(&self, kind: KeepKind, i: usize) -> Option<&String> {
        match kind {
            KeepKind::PlaceKeepout => self.keepout_parts.get(i),
            KeepKind::PlaceRegion => self.keepin_parts.get(i),
            _ => None,
        }
    }

    fn keep_parts(&self, b: &PcbBoard) -> Vec<(ItemId, String)> {
        let mut out = Vec::new();
        for kind in [KeepKind::PlaceKeepout, KeepKind::PlaceRegion] {
            for (i, id) in b.keep_ids.of(kind).iter().enumerate() {
                if let Some(n) = self.part_of(kind, i) {
                    out.push((*id, n.clone()));
                }
            }
        }
        out
    }
}

impl Command for SyncBoard {
    fn label(&self) -> String {
        if self.target.is_some() { format!("Sync {}", self.name) } else { format!("Sync {} (new board)", self.name) }
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.element)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        let s = studio_mut(doc, self.element)?;
        match self.target {
            Some(id) => {
                let sb = s.board_mut(id).ok_or_else(|| CommandError::Invalid("the board to update is gone".into()))?;
                let old = match &sb.source {
                    BoardSource::Mcad(m) => m.clone(),
                    _ => McadSource::default(),
                };
                let before = sb.board.clone();
                // Keep areas: the same part (by name) in the same section keeps its id.
                let keep = |kind: KeepKind, i: usize| -> Option<ItemId> {
                    let name = self.part_of(kind, i)?;
                    old.keep_parts.iter().find(|(id, n)| n == name && before.keep_position(*id).is_some_and(|(k, _)| k == kind)).map(|(id, _)| *id)
                };
                // Components: the same designator keeps its id.
                let comp = |i: usize| -> Option<ItemId> {
                    let r = &self.board.placements.get(i)?.refdes;
                    before.components().find(|(_, p)| p.refdes.eq_ignore_ascii_case(r)).map(|(id, _)| id)
                };
                sb.board.resync(&self.board, &self.library, &keep, &comp);
                let keep_parts = self.keep_parts(&sb.board);
                let mut frames = old.frames.clone();
                for (r, p) in &self.frames {
                    match frames.iter_mut().find(|(q, _)| q == r) {
                        Some(f) => f.1 = *p,
                        None => frames.push((r.clone(), *p)),
                    }
                }
                sb.source = BoardSource::Mcad(McadSource { element: self.source, plane: self.plane, keep_parts, frames });
                s.active = Some(id);
            }
            None => {
                let mut b = PcbBoard::new(&self.board, &self.library);
                b.board.name = s.free_name(&self.name);
                let keep_parts = self.keep_parts(&b);
                let source = BoardSource::Mcad(McadSource { element: self.source, plane: self.plane, keep_parts, frames: self.frames.clone() });
                let id = s.add_board(b, source);
                s.active = Some(id);
            }
        }
        Ok(())
    }
}
