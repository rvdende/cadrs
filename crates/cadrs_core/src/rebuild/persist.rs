//! The rebuilder's per-feature outputs as bytes: a **session snapshot**, so a document opened
//! again (in a new session, or on another machine running the same build) takes its parts from
//! the snapshot instead of rebuilding them, and an edit then recomputes only from the edited
//! feature on (the outputs before it are in the cache, as in the session that made them).
//!
//! - **[`Persist`]**: how a value is written and read back. Plain data is written as it is
//!   ([`plain!`]); data the outputs share (meshes, names, swept geometry, states, …) is
//!   written once into a table and referred to by index, and read back as one shared `Arc`
//!   again; a kernel body is written once (the kernel's exact format, see
//!   [`cadrs_kernel::Kernel::write_body`]) and read back into this session, under its new id,
//!   the first time an output refers to it. Maps are written in key order and tables in the
//!   order they are met, so the same outputs always give the same bytes.
//! - **The blob**: `CADRSNAP`, the format version, the RON of the tables and outputs, the
//!   meshes (bincode: they are nearly all of a snapshot, and as text ten times the size and
//!   slower to read), then the bodies; each part length-prefixed. What it is kept under (the outputs' feature chain, the geometry fingerprint) is
//!   up to the caller ([`super::session`]).

use std::collections::HashMap;
use std::hash::Hash;
use std::sync::Arc;
use std::time::Duration;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use cadrs_kernel::{BodyId, BodyNames, Kernel, OpId};
use cadrs_sketch::{PlaneFrame, Vec3};

use super::{Contacts, Entry, Output, PartState, Rebuilder, Stage, State};
use crate::brep::{Geoms, OpGeom};
use crate::derived::DerivedOutput;
use crate::document::{Feature, PartProps};
use crate::ids::{FeatureId, PartId};
use crate::parts::{Part, PartKind};
use crate::solid::Solid;

/// The blob format: bump when [`Snapshot`] or a `Saved` form changes.
pub const FORMAT: u32 = 2;

const MAGIC: &[u8; 8] = b"CADRSNAP";

/// A value written into a snapshot and read back from it.
pub trait Persist: Sized {
    /// What is written for it.
    type Saved: Serialize + DeserializeOwned;
    fn save(&self, w: &mut Writer) -> Self::Saved;
    fn load(saved: Self::Saved, r: &mut Reader) -> Result<Self, String>;
}

/// Plain data: written as it is.
macro_rules! plain {
    ($($t:ty),* $(,)?) => {$(
        impl Persist for $t {
            type Saved = $t;
            fn save(&self, _: &mut Writer) -> $t {
                self.clone()
            }
            fn load(saved: $t, _: &mut Reader) -> Result<$t, String> {
                Ok(saved)
            }
        }
    )*};
}

plain!(
    bool,
    u32,
    String,
    Vec3,
    FeatureId,
    PartId,
    PartKind,
    PlaneFrame,
    Contacts,
    cadrs_kernel::MassProperties,
    crate::transform::Composite,
    crate::pattern::InstanceDot,
    crate::surfacing::HelixGeom,
);

impl<T: Persist> Persist for Option<T> {
    type Saved = Option<T::Saved>;
    fn save(&self, w: &mut Writer) -> Self::Saved {
        self.as_ref().map(|v| v.save(w))
    }
    fn load(saved: Self::Saved, r: &mut Reader) -> Result<Self, String> {
        saved.map(|s| T::load(s, r)).transpose()
    }
}

impl<T: Persist> Persist for Vec<T> {
    type Saved = Vec<T::Saved>;
    fn save(&self, w: &mut Writer) -> Self::Saved {
        self.iter().map(|v| v.save(w)).collect()
    }
    fn load(saved: Self::Saved, r: &mut Reader) -> Result<Self, String> {
        saved.into_iter().map(|s| T::load(s, r)).collect()
    }
}

impl<A: Persist, B: Persist> Persist for (A, B) {
    type Saved = (A::Saved, B::Saved);
    fn save(&self, w: &mut Writer) -> Self::Saved {
        (self.0.save(w), self.1.save(w))
    }
    fn load(saved: Self::Saved, r: &mut Reader) -> Result<Self, String> {
        Ok((A::load(saved.0, r)?, B::load(saved.1, r)?))
    }
}

/// A map, in key order.
impl<K, V> Persist for HashMap<K, V>
where
    K: Persist + Ord + Hash + Clone,
    V: Persist,
{
    type Saved = Vec<(K::Saved, V::Saved)>;
    fn save(&self, w: &mut Writer) -> Self::Saved {
        let mut keys: Vec<&K> = self.keys().collect();
        keys.sort();
        keys.into_iter().map(|k| (k.save(w), self[k].save(w))).collect()
    }
    fn load(saved: Self::Saved, r: &mut Reader) -> Result<Self, String> {
        saved.into_iter().map(|(k, v)| Ok((K::load(k, r)?, V::load(v, r)?))).collect()
    }
}

/// A kernel body: written once, read back into this session the first time it is met.
impl Persist for BodyId {
    type Saved = u32;
    fn save(&self, w: &mut Writer) -> u32 {
        if let Some(i) = w.body_index.get(self) {
            return *i;
        }
        let i = w.bodies.len() as u32;
        let bytes = w.kernel.write_body(*self).unwrap_or_else(|e| {
            w.error.get_or_insert_with(|| format!("a body could not be written: {e}"));
            Vec::new()
        });
        w.bodies.push(bytes);
        w.body_index.insert(*self, i);
        i
    }
    fn load(i: u32, r: &mut Reader) -> Result<Self, String> {
        let i = i as usize;
        if let Some(Some(b)) = r.loaded.get(i) {
            return Ok(*b);
        }
        let bytes = r.bodies.get(i).ok_or("a body is missing")?;
        let b = r.kernel.read_body(bytes).map_err(|e| format!("a body could not be read: {e}"))?;
        r.loaded[i] = Some(b);
        r.added.push(b);
        Ok(b)
    }
}

/// A table of shared values: each pointer written once.
struct Table<S> {
    index: HashMap<usize, u32>,
    items: Vec<Option<S>>,
}

impl<S> Default for Table<S> {
    fn default() -> Self {
        Self { index: HashMap::new(), items: Vec::new() }
    }
}

impl<S> Table<S> {
    fn into_items(self) -> Vec<S> {
        self.items.into_iter().map(|s| s.expect("every table entry is filled")).collect()
    }
}

/// A table read back: each entry built once, on first use.
struct Shared<S, T> {
    saved: Vec<Option<S>>,
    built: Vec<Option<Arc<T>>>,
}

impl<S, T> Shared<S, T> {
    fn new(saved: Vec<S>) -> Self {
        let n = saved.len();
        Self { saved: saved.into_iter().map(Some).collect(), built: (0..n).map(|_| None).collect() }
    }
}

/// `Arc<$t>` shared through table `$table`: written as its index; `$saved` is what the table
/// holds, `$save` makes it from `&$t` (with the writer), `$load` makes the value back (with the
/// reader).
macro_rules! shared {
    ($t:ty, $table:ident, $saved:ty, |$s:ident, $w:ident| $save:expr, |$v:ident, $r:ident| $load:expr) => {
        impl Persist for Arc<$t> {
            type Saved = u32;
            fn save(&self, w: &mut Writer) -> u32 {
                let p = Arc::as_ptr(self) as *const () as usize;
                if let Some(i) = w.$table.index.get(&p) {
                    return *i;
                }
                let i = w.$table.items.len() as u32;
                w.$table.items.push(None);
                w.$table.index.insert(p, i);
                let saved: $saved = {
                    let $s: &$t = self;
                    let $w = &mut *w;
                    $save
                };
                w.$table.items[i as usize] = Some(saved);
                i
            }
            fn load(i: u32, r: &mut Reader) -> Result<Self, String> {
                let n = i as usize;
                if let Some(Some(a)) = r.$table.built.get(n) {
                    return Ok(a.clone());
                }
                let $v: $saved = r.$table.saved.get_mut(n).and_then(Option::take).ok_or("a shared value is missing")?;
                let value: $t = {
                    let $r = &mut *r;
                    $load
                };
                let a = Arc::new(value);
                r.$table.built[n] = Some(a.clone());
                Ok(a)
            }
        }
    };
}

shared!(Solid, solids, Solid, |s, _w| s.clone(), |v, _r| v);
shared!(BodyNames, names, BodyNames, |s, _w| s.clone(), |v, _r| v);
shared!(OpGeom, op_geoms, OpGeom, |s, _w| s.clone(), |v, _r| v);
shared!(PartProps, part_props, PartProps, |s, _w| s.clone(), |v, _r| v);
shared!(Vec<Feature>, sketch_lists, Vec<Feature>, |s, _w| s.clone(), |v, _r| v);
shared!(
    HashMap<FeatureId, DerivedOutput>,
    derived_maps,
    Vec<(FeatureId, DerivedOutput)>,
    |s, _w| {
        let mut v: Vec<_> = s.iter().map(|(k, d)| (*k, d.clone())).collect();
        v.sort_by_key(|(k, _)| *k);
        v
    },
    |v, _r| v.into_iter().collect()
);
shared!(
    Geoms,
    geom_maps,
    Vec<(OpId, u32)>,
    |s, w| {
        let mut ops: Vec<&OpId> = s.keys().collect();
        ops.sort();
        ops.into_iter().map(|op| (*op, s[op].save(w))).collect()
    },
    |v, r| v.into_iter().map(|(op, g)| Ok((op, Arc::<OpGeom>::load(g, r)?))).collect::<Result<Geoms, String>>()?
);
shared!(State, states, SavedState, |s, w| SavedState::of(s, w), |v, r| v.state(r)?);

#[derive(Serialize, Deserialize)]
pub struct SavedPart {
    id: PartId,
    feature: FeatureId,
    name: String,
    kind: PartKind,
    palette: u32,
    solid: u32,
    mass: Option<cadrs_kernel::MassProperties>,
    features: Vec<FeatureId>,
    source: Option<PartId>,
    derived: Option<u32>,
}

impl Persist for Part {
    type Saved = SavedPart;
    fn save(&self, w: &mut Writer) -> SavedPart {
        SavedPart {
            id: self.id,
            feature: self.feature,
            name: self.name.clone(),
            kind: self.kind,
            palette: self.palette,
            solid: self.solid.save(w),
            mass: self.mass,
            features: self.features.clone(),
            source: self.source,
            derived: self.derived.save(w),
        }
    }
    fn load(s: SavedPart, r: &mut Reader) -> Result<Self, String> {
        Ok(Part {
            id: s.id,
            feature: s.feature,
            name: s.name,
            kind: s.kind,
            palette: s.palette,
            solid: Persist::load(s.solid, r)?,
            mass: s.mass,
            features: s.features,
            source: s.source,
            derived: Persist::load(s.derived, r)?,
        })
    }
}

impl Persist for PartState {
    type Saved = (SavedPart, Option<u32>, u32);
    fn save(&self, w: &mut Writer) -> Self::Saved {
        (self.part.save(w), self.body.save(w), self.names.save(w))
    }
    fn load(s: Self::Saved, r: &mut Reader) -> Result<Self, String> {
        Ok(PartState { part: Persist::load(s.0, r)?, body: Persist::load(s.1, r)?, names: Persist::load(s.2, r)? })
    }
}

impl Persist for Stage {
    type Saved = (Vec<SavedPart>, u32);
    fn save(&self, w: &mut Writer) -> Self::Saved {
        (self.before.save(w), self.tool.save(w))
    }
    fn load(s: Self::Saved, r: &mut Reader) -> Result<Self, String> {
        Ok(Stage { before: Persist::load(s.0, r)?, tool: Persist::load(s.1, r)? })
    }
}

#[derive(Serialize, Deserialize)]
pub struct SavedState {
    parts: Vec<<PartState as Persist>::Saved>,
    next_part: u32,
    next_surface: u32,
    geoms: u32,
    planes: Vec<(FeatureId, PlaneFrame)>,
    connectors: Vec<(FeatureId, PlaneFrame)>,
    connector_owners: Vec<(FeatureId, PartId)>,
    composites: Vec<crate::transform::Composite>,
    curves: Vec<(FeatureId, crate::surfacing::HelixGeom)>,
    derived: u32,
    derived_sketches: u32,
}

impl SavedState {
    fn of(s: &State, w: &mut Writer) -> Self {
        // Every field is named here, so a new field of State can't be left out unnoticed.
        let State { parts, next_part, next_surface, geoms, planes, connectors, connector_owners, composites, curves, derived, derived_sketches } = s;
        SavedState {
            parts: parts.save(w),
            next_part: *next_part,
            next_surface: *next_surface,
            geoms: geoms.save(w),
            planes: planes.save(w),
            connectors: connectors.save(w),
            connector_owners: connector_owners.save(w),
            composites: composites.clone(),
            curves: curves.save(w),
            derived: derived.save(w),
            derived_sketches: derived_sketches.save(w),
        }
    }

    fn state(self, r: &mut Reader) -> Result<State, String> {
        Ok(State {
            parts: Persist::load(self.parts, r)?,
            next_part: self.next_part,
            next_surface: self.next_surface,
            geoms: Persist::load(self.geoms, r)?,
            planes: Persist::load(self.planes, r)?,
            connectors: Persist::load(self.connectors, r)?,
            connector_owners: Persist::load(self.connector_owners, r)?,
            composites: self.composites,
            curves: Persist::load(self.curves, r)?,
            derived: Persist::load(self.derived, r)?,
            derived_sketches: Persist::load(self.derived_sketches, r)?,
        })
    }
}

#[derive(Serialize, Deserialize)]
pub struct SavedOutput {
    state: u32,
    error: Option<String>,
    warning: Option<String>,
    contacts: Option<Contacts>,
    owned: Vec<u32>,
    stage: Option<<Stage as Persist>::Saved>,
    axis: Option<(Vec3, Vec3)>,
    dots: Option<Vec<crate::pattern::InstanceDot>>,
    uses: Vec<FeatureId>,
    arrows: Vec<(Vec3, Vec3)>,
}

impl Persist for Output {
    type Saved = SavedOutput;
    fn save(&self, w: &mut Writer) -> SavedOutput {
        let Output { state, error, warning, contacts, owned, stage, axis, dots, uses, arrows } = self;
        SavedOutput {
            state: state.save(w),
            error: error.clone(),
            warning: warning.clone(),
            contacts: contacts.clone(),
            owned: owned.save(w),
            stage: stage.save(w),
            axis: *axis,
            dots: dots.clone(),
            uses: uses.clone(),
            arrows: arrows.clone(),
        }
    }
    fn load(s: SavedOutput, r: &mut Reader) -> Result<Self, String> {
        Ok(Output {
            state: Persist::load(s.state, r)?,
            error: s.error,
            warning: s.warning,
            contacts: s.contacts,
            owned: Persist::load(s.owned, r)?,
            stage: Persist::load(s.stage, r)?,
            axis: s.axis,
            dots: s.dots,
            uses: s.uses,
            arrows: s.arrows,
        })
    }
}

/// Writes values into a snapshot (see [`Persist`]).
pub struct Writer<'k> {
    kernel: &'k dyn Kernel,
    bodies: Vec<Vec<u8>>,
    body_index: HashMap<BodyId, u32>,
    error: Option<String>,
    solids: Table<Solid>,
    names: Table<BodyNames>,
    op_geoms: Table<OpGeom>,
    part_props: Table<PartProps>,
    sketch_lists: Table<Vec<Feature>>,
    derived_maps: Table<Vec<(FeatureId, DerivedOutput)>>,
    geom_maps: Table<Vec<(OpId, u32)>>,
    states: Table<SavedState>,
}

/// Reads values back from a snapshot (see [`Persist`]).
pub struct Reader<'k> {
    kernel: &'k mut dyn Kernel,
    bodies: Vec<Vec<u8>>,
    loaded: Vec<Option<BodyId>>,
    /// The bodies read into the session so far.
    added: Vec<BodyId>,
    solids: Shared<Solid, Solid>,
    names: Shared<BodyNames, BodyNames>,
    op_geoms: Shared<OpGeom, OpGeom>,
    part_props: Shared<PartProps, PartProps>,
    sketch_lists: Shared<Vec<Feature>, Vec<Feature>>,
    derived_maps: Shared<Vec<(FeatureId, DerivedOutput)>, HashMap<FeatureId, DerivedOutput>>,
    geom_maps: Shared<Vec<(OpId, u32)>, Geoms>,
    states: Shared<SavedState, State>,
}

/// The RON part of a blob.
#[derive(Serialize, Deserialize)]
struct Snapshot {
    format: u32,
    fingerprint: String,
    /// Written apart, in bincode (see the module docs).
    #[serde(skip)]
    solids: Vec<Solid>,
    names: Vec<BodyNames>,
    op_geoms: Vec<OpGeom>,
    part_props: Vec<PartProps>,
    sketch_lists: Vec<Vec<Feature>>,
    derived_maps: Vec<Vec<(FeatureId, DerivedOutput)>>,
    geom_maps: Vec<Vec<(OpId, u32)>>,
    states: Vec<SavedState>,
    /// (cache key, how long computing it took in ns, the output).
    entries: Vec<(u64, u64, SavedOutput)>,
}

impl Rebuilder {
    /// The cached outputs `keys` (those that are cached), with their bodies, as a blob.
    pub fn snapshot(&self, keys: &[u64]) -> Result<Vec<u8>, String> {
        let mut keys: Vec<u64> = keys.iter().copied().filter(|k| self.entries.contains_key(k)).collect();
        keys.sort_unstable();
        keys.dedup();
        let mut w = Writer {
            kernel: &self.kernel,
            bodies: Vec::new(),
            body_index: HashMap::new(),
            error: None,
            solids: Table::default(),
            names: Table::default(),
            op_geoms: Table::default(),
            part_props: Table::default(),
            sketch_lists: Table::default(),
            derived_maps: Table::default(),
            geom_maps: Table::default(),
            states: Table::default(),
        };
        let entries: Vec<(u64, u64, SavedOutput)> = keys
            .iter()
            .map(|k| {
                let e = &self.entries[k];
                (*k, e.time.as_nanos() as u64, e.output.save(&mut w))
            })
            .collect();
        if let Some(e) = w.error {
            return Err(e);
        }
        let snapshot = Snapshot {
            format: FORMAT,
            fingerprint: super::GEOMETRY_FINGERPRINT.into(),
            solids: w.solids.into_items(),
            names: w.names.into_items(),
            op_geoms: w.op_geoms.into_items(),
            part_props: w.part_props.into_items(),
            sketch_lists: w.sketch_lists.into_items(),
            derived_maps: w.derived_maps.into_items(),
            geom_maps: w.geom_maps.into_items(),
            states: w.states.into_items(),
            entries,
        };
        let ron = ron::to_string(&snapshot).map_err(|e| e.to_string())?;
        let solids = bincode::serialize(&snapshot.solids).map_err(|e| e.to_string())?;
        let mut out = Vec::with_capacity(ron.len() + solids.len() + w.bodies.iter().map(|b| b.len() + 8).sum::<usize>() + 32);
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&FORMAT.to_le_bytes());
        out.extend_from_slice(&(ron.len() as u64).to_le_bytes());
        out.extend_from_slice(ron.as_bytes());
        out.extend_from_slice(&(solids.len() as u64).to_le_bytes());
        out.extend_from_slice(&solids);
        out.extend_from_slice(&(w.bodies.len() as u32).to_le_bytes());
        for b in &w.bodies {
            out.extend_from_slice(&(b.len() as u64).to_le_bytes());
            out.extend_from_slice(b);
        }
        Ok(out)
    }

    /// Adds the outputs of a [`Self::snapshot`] blob to the cache (those not cached already),
    /// reading their bodies into this session. Returns how many were added. A blob of another
    /// format or fingerprint, or a broken one, adds nothing.
    pub fn restore(&mut self, blob: &[u8]) -> Result<usize, String> {
        let (snapshot, bodies) = parse(blob)?;
        if snapshot.format != FORMAT || snapshot.fingerprint != super::GEOMETRY_FINGERPRINT {
            return Err("the snapshot is from another build".into());
        }
        let generation = self.generation;
        let n = bodies.len();
        let mut r = Reader {
            kernel: &mut self.kernel,
            bodies,
            loaded: vec![None; n],
            added: Vec::new(),
            solids: Shared::new(snapshot.solids),
            names: Shared::new(snapshot.names),
            op_geoms: Shared::new(snapshot.op_geoms),
            part_props: Shared::new(snapshot.part_props),
            sketch_lists: Shared::new(snapshot.sketch_lists),
            derived_maps: Shared::new(snapshot.derived_maps),
            geom_maps: Shared::new(snapshot.geom_maps),
            states: Shared::new(snapshot.states),
        };
        let mut read = Vec::new();
        for (key, nanos, saved) in snapshot.entries {
            if self.entries.contains_key(&key) {
                continue;
            }
            match Output::load(saved, &mut r) {
                Ok(output) => read.push((key, Entry { output, time: Duration::from_nanos(nanos), last_used: generation, sources: Vec::new() })),
                Err(e) => {
                    for b in r.added {
                        r.kernel.release(b);
                    }
                    return Err(e);
                }
            }
        }
        let added = read.len();
        self.entries.extend(read);
        Ok(added)
    }
}

/// The RON part and the bodies of a blob.
fn parse(blob: &[u8]) -> Result<(Snapshot, Vec<Vec<u8>>), String> {
    let mut at = 0usize;
    let mut take = |n: usize| -> Result<&[u8], String> {
        let s = blob.get(at..at + n).ok_or("the snapshot is cut short")?;
        at += n;
        Ok(s)
    };
    if take(8)? != MAGIC {
        return Err("not a snapshot".into());
    }
    let format = u32::from_le_bytes(take(4)?.try_into().unwrap());
    if format != FORMAT {
        return Err(format!("snapshot format {format}"));
    }
    let len = u64::from_le_bytes(take(8)?.try_into().unwrap()) as usize;
    let text = std::str::from_utf8(take(len)?).map_err(|e| e.to_string())?;
    let mut snapshot: Snapshot = ron::from_str(text).map_err(|e| e.to_string())?;
    let len = u64::from_le_bytes(take(8)?.try_into().unwrap()) as usize;
    snapshot.solids = bincode::deserialize(take(len)?).map_err(|e| e.to_string())?;
    let count = u32::from_le_bytes(take(4)?.try_into().unwrap()) as usize;
    let mut bodies = Vec::with_capacity(count);
    for _ in 0..count {
        let len = u64::from_le_bytes(take(8)?.try_into().unwrap()) as usize;
        bodies.push(take(len)?.to_vec());
    }
    Ok((snapshot, bodies))
}
