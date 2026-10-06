//! Board layers and layer sets.

use serde::{Deserialize, Serialize};

/// The most inner copper layers a board can have.
pub const MAX_INNER: u8 = 30;
/// The most user layers.
pub const MAX_USER: u8 = 9;

/// A board layer. Copper is numbered from the top: top, inner 1…30, bottom.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Layer {
    TopCopper,
    /// Inner copper 1…[`MAX_INNER`], counted from the top.
    Inner(u8),
    BottomCopper,
    TopSilk,
    BottomSilk,
    TopMask,
    BottomMask,
    TopPaste,
    BottomPaste,
    TopAdhesive,
    BottomAdhesive,
    TopCourtyard,
    BottomCourtyard,
    TopFab,
    BottomFab,
    /// The board outline and cut-outs.
    Outline,
    Margin,
    Drawings,
    Comments,
    Eco1,
    Eco2,
    /// User layer 1…[`MAX_USER`].
    User(u8),
}

/// The board side a layer or footprint is on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Side {
    #[default]
    Top,
    Bottom,
}

impl Side {
    pub fn flipped(self) -> Side {
        match self {
            Side::Top => Side::Bottom,
            Side::Bottom => Side::Top,
        }
    }
}

use Layer::*;

/// The non-copper layers, in the order the layer panel lists them after the copper.
pub const TECHNICAL: [Layer; 19] = [
    TopAdhesive,
    BottomAdhesive,
    TopPaste,
    BottomPaste,
    TopSilk,
    BottomSilk,
    TopMask,
    BottomMask,
    Drawings,
    Comments,
    Eco1,
    Eco2,
    Outline,
    Margin,
    TopCourtyard,
    BottomCourtyard,
    TopFab,
    BottomFab,
    User(1),
];

impl Layer {
    pub fn is_copper(self) -> bool {
        matches!(self, TopCopper | BottomCopper | Inner(_))
    }

    /// The side a one-sided layer belongs to (`None` for inner copper and the shared layers).
    pub fn side(self) -> Option<Side> {
        match self {
            TopCopper | TopSilk | TopMask | TopPaste | TopAdhesive | TopCourtyard | TopFab => Some(Side::Top),
            BottomCopper | BottomSilk | BottomMask | BottomPaste | BottomAdhesive | BottomCourtyard | BottomFab => {
                Some(Side::Bottom)
            }
            _ => None,
        }
    }

    /// The matching layer on the other side (itself when it has no side).
    pub fn flipped(self) -> Layer {
        match self {
            TopCopper => BottomCopper,
            BottomCopper => TopCopper,
            TopSilk => BottomSilk,
            BottomSilk => TopSilk,
            TopMask => BottomMask,
            BottomMask => TopMask,
            TopPaste => BottomPaste,
            BottomPaste => TopPaste,
            TopAdhesive => BottomAdhesive,
            BottomAdhesive => TopAdhesive,
            TopCourtyard => BottomCourtyard,
            BottomCourtyard => TopCourtyard,
            TopFab => BottomFab,
            BottomFab => TopFab,
            l => l,
        }
    }

    /// The name shown in the layer panel.
    pub fn name(self) -> String {
        match self {
            TopCopper => "Top copper".into(),
            Inner(n) => format!("Inner copper {n}"),
            BottomCopper => "Bottom copper".into(),
            TopSilk => "Top silkscreen".into(),
            BottomSilk => "Bottom silkscreen".into(),
            TopMask => "Top solder mask".into(),
            BottomMask => "Bottom solder mask".into(),
            TopPaste => "Top paste".into(),
            BottomPaste => "Bottom paste".into(),
            TopAdhesive => "Top adhesive".into(),
            BottomAdhesive => "Bottom adhesive".into(),
            TopCourtyard => "Top courtyard".into(),
            BottomCourtyard => "Bottom courtyard".into(),
            TopFab => "Top fabrication".into(),
            BottomFab => "Bottom fabrication".into(),
            Outline => "Board outline".into(),
            Margin => "Margin".into(),
            Drawings => "Drawings".into(),
            Comments => "Comments".into(),
            Eco1 => "Eco 1".into(),
            Eco2 => "Eco 2".into(),
            User(n) => format!("User {n}"),
        }
    }

    /// The bit of this layer in a [`LayerSet`].
    fn bit(self) -> u32 {
        match self {
            TopCopper => 0,
            Inner(n) => n.clamp(1, MAX_INNER) as u32,
            BottomCopper => 31,
            TopSilk => 32,
            BottomSilk => 33,
            TopMask => 34,
            BottomMask => 35,
            TopPaste => 36,
            BottomPaste => 37,
            TopAdhesive => 38,
            BottomAdhesive => 39,
            TopCourtyard => 40,
            BottomCourtyard => 41,
            TopFab => 42,
            BottomFab => 43,
            Outline => 44,
            Margin => 45,
            Drawings => 46,
            Comments => 47,
            Eco1 => 48,
            Eco2 => 49,
            User(n) => 49 + n.clamp(1, MAX_USER) as u32,
        }
    }

    fn from_bit(b: u32) -> Layer {
        match b {
            0 => TopCopper,
            1..=30 => Inner(b as u8),
            31 => BottomCopper,
            32 => TopSilk,
            33 => BottomSilk,
            34 => TopMask,
            35 => BottomMask,
            36 => TopPaste,
            37 => BottomPaste,
            38 => TopAdhesive,
            39 => BottomAdhesive,
            40 => TopCourtyard,
            41 => BottomCourtyard,
            42 => TopFab,
            43 => BottomFab,
            44 => Outline,
            45 => Margin,
            46 => Drawings,
            47 => Comments,
            48 => Eco1,
            49 => Eco2,
            _ => User((b - 49) as u8),
        }
    }

    /// Copper layers of a board with `count` copper layers (2 or more), top to bottom.
    pub fn copper(count: u8) -> impl Iterator<Item = Layer> {
        let inner = count.saturating_sub(2).min(MAX_INNER);
        std::iter::once(TopCopper).chain((1..=inner).map(Inner)).chain(std::iter::once(BottomCopper))
    }
}

/// A set of layers (a pad's, a zone's).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(into = "Vec<Layer>", from = "Vec<Layer>")]
pub struct LayerSet(u64);

impl LayerSet {
    pub const EMPTY: LayerSet = LayerSet(0);
    /// Every copper layer (a through-hole pad's copper, whatever the board's layer count).
    pub const ALL_COPPER: LayerSet = LayerSet((1 << 32) - 1);

    pub fn of(layers: &[Layer]) -> LayerSet {
        let mut s = LayerSet::EMPTY;
        for &l in layers {
            s.insert(l);
        }
        s
    }

    pub fn insert(&mut self, l: Layer) {
        self.0 |= 1 << l.bit();
    }

    pub fn remove(&mut self, l: Layer) {
        self.0 &= !(1 << l.bit());
    }

    pub fn contains(self, l: Layer) -> bool {
        self.0 & (1 << l.bit()) != 0
    }

    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    pub fn union(self, o: LayerSet) -> LayerSet {
        LayerSet(self.0 | o.0)
    }

    pub fn intersects(self, o: LayerSet) -> bool {
        self.0 & o.0 != 0
    }

    /// Its copper layers only.
    pub fn copper(self) -> LayerSet {
        LayerSet(self.0 & LayerSet::ALL_COPPER.0)
    }

    /// Every layer swapped for the one on the other side.
    pub fn flipped(self) -> LayerSet {
        LayerSet::of(&self.iter().map(Layer::flipped).collect::<Vec<_>>())
    }

    pub fn iter(self) -> impl Iterator<Item = Layer> {
        (0..64u32).filter(move |b| self.0 & (1 << b) != 0).map(Layer::from_bit)
    }
}

impl From<LayerSet> for Vec<Layer> {
    fn from(s: LayerSet) -> Vec<Layer> {
        s.iter().collect()
    }
}

impl From<Vec<Layer>> for LayerSet {
    fn from(v: Vec<Layer>) -> LayerSet {
        LayerSet::of(&v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bits_round_trip() {
        let all: Vec<Layer> = Layer::copper(32).chain(TECHNICAL).chain((2..=MAX_USER).map(User)).collect();
        assert_eq!(all.len(), 32 + 19 + 8);
        let set = LayerSet::of(&all);
        assert_eq!(set.iter().count(), all.len());
        for l in all {
            assert_eq!(Layer::from_bit(l.bit()), l);
        }
    }

    #[test]
    fn flips_sides() {
        let s = LayerSet::of(&[TopCopper, TopMask, TopPaste, Outline]);
        assert_eq!(s.flipped(), LayerSet::of(&[BottomCopper, BottomMask, BottomPaste, Outline]));
        assert!(LayerSet::ALL_COPPER.contains(Inner(7)));
        assert!(!LayerSet::ALL_COPPER.contains(TopSilk));
        assert_eq!(Layer::copper(4).collect::<Vec<_>>(), vec![TopCopper, Inner(1), Inner(2), BottomCopper]);
    }
}
