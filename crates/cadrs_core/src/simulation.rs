//! Simulation (P3F.5; `intro-to-parametric-cad.md` P3.5, `intro-to-assemblies.md` A1.7 Loads,
//! A1.8 Simulation, A6.3 Simulation connection, X16): the loads a Part Studio or an Assembly
//! stores for a linear static analysis, and the [`cadrs_fea::Model`] made from them.
//!
//! - **Loads** ([`SimLoad`], the Loads list): **Fixed** faces; a **Force** (N) spread over its
//!   faces, along their normals (pushing in) or along X, Y or Z, with Flip; a **Pressure** on
//!   faces (pushing in). Each names its faces by part and persistent face name ([`SimFace`]; in
//!   an assembly the part is the instance's view part, [`crate::assembly::InstanceId::part_id`]).
//!   They are stored with the element ([`crate::Element::simulation`]) and edited through
//!   [`AddLoad`], [`SetLoad`], [`DeleteLoad`] and [`SetMeshDensity`], so they undo.
//! - **Materials**: each part's library material (Young's modulus and Poisson's ratio,
//!   [`crate::material`]). A part without one can't be analysed.
//! - **Connections**: in an assembly, a mate with **Simulation connection** checked bonds its two
//!   parts where they touch (Fastened is bonded; the analysis treats every connected mate as
//!   bonded); in a Part Studio, parts that touch are bonded.
//! - [`setup`] turns all this into the solver's model: the parts that carry a load or are bonded
//!   to one that does (others are left out, with a note), their surfaces from the kernel's
//!   tessellation, materials in MPa, forces in N.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::command::{Command, CommandError, Scope};
use crate::document::{Document, PartProps};
use crate::ids::{ElementId, PartId};
use crate::parts::Part;
use crate::solid::Solid;
pub use cadrs_sketch::FaceName;

/// Identifies a load.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct LoadId(pub Uuid);

impl LoadId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    pub const fn from_u128(v: u128) -> Self {
        Self(Uuid::from_u128(v))
    }
}

impl Default for LoadId {
    fn default() -> Self {
        Self::new()
    }
}

/// A face a load acts on.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SimFace {
    pub part: PartId,
    pub face: FaceName,
}

/// Which way a force points.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ForceDirection {
    /// Along the faces' normals, into the part.
    #[default]
    Normal,
    X,
    Y,
    Z,
}

impl ForceDirection {
    pub const ALL: [ForceDirection; 4] = [ForceDirection::Normal, ForceDirection::X, ForceDirection::Y, ForceDirection::Z];

    pub fn label(self) -> &'static str {
        match self {
            ForceDirection::Normal => "Normal to faces",
            ForceDirection::X => "Along X",
            ForceDirection::Y => "Along Y",
            ForceDirection::Z => "Along Z",
        }
    }
}

/// What a load does.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum LoadKind {
    Fixed,
    /// A total force in N; `flip` reverses it.
    Force {
        newtons: f64,
        #[serde(default)]
        direction: ForceDirection,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        flip: bool,
    },
    /// A pressure in Pa, pushing into the faces.
    Pressure { pascals: f64 },
}

impl LoadKind {
    /// "Fixed", "Force", "Pressure": the kind's name, and the stem of the default load names.
    pub fn label(&self) -> &'static str {
        match self {
            LoadKind::Fixed => "Fixed",
            LoadKind::Force { .. } => "Force",
            LoadKind::Pressure { .. } => "Pressure",
        }
    }

    /// A short description for the Loads list: "100 N along −Z", "1 MPa".
    pub fn summary(&self) -> String {
        match *self {
            LoadKind::Fixed => String::new(),
            LoadKind::Force { newtons, direction, flip } => {
                let dir = match (direction, flip) {
                    (ForceDirection::Normal, false) => "normal".to_string(),
                    (ForceDirection::Normal, true) => "normal, out".to_string(),
                    (d, f) => format!("along {}{}", if f { "−" } else { "+" }, &d.label()[6..]),
                };
                format!("{} {dir}", format_force(newtons))
            }
            LoadKind::Pressure { pascals } => format_pressure(pascals),
        }
    }
}

/// A load in the Loads list.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SimLoad {
    pub id: LoadId,
    pub name: String,
    pub kind: LoadKind,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub faces: Vec<SimFace>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub suppressed: bool,
}

/// How fine the mesh is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum MeshDensity {
    Coarse,
    #[default]
    Medium,
    Fine,
}

impl MeshDensity {
    pub const ALL: [MeshDensity; 3] = [MeshDensity::Coarse, MeshDensity::Medium, MeshDensity::Fine];

    pub fn label(self) -> &'static str {
        match self {
            MeshDensity::Coarse => "Coarse",
            MeshDensity::Medium => "Medium",
            MeshDensity::Fine => "Fine",
        }
    }

    /// About how many quadratic tetrahedra the mesh has.
    pub fn target_elements(self) -> usize {
        match self {
            MeshDensity::Coarse => 2_500,
            MeshDensity::Medium => 8_000,
            MeshDensity::Fine => 20_000,
        }
    }
}

/// An element's simulation setup.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Simulation {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub loads: Vec<SimLoad>,
    #[serde(default, skip_serializing_if = "is_medium")]
    pub mesh: MeshDensity,
}

fn is_medium(m: &MeshDensity) -> bool {
    *m == MeshDensity::Medium
}

impl Simulation {
    pub fn is_empty(&self) -> bool {
        self.loads.is_empty() && self.mesh == MeshDensity::Medium
    }

    pub fn load(&self, id: LoadId) -> Option<&SimLoad> {
        self.loads.iter().find(|l| l.id == id)
    }

    /// The next free default name for a load of this kind: "Fixed 1", "Force 2", ….
    pub fn next_name(&self, kind: &LoadKind) -> String {
        let stem = kind.label();
        (1..).map(|n| format!("{stem} {n}")).find(|n| !self.loads.iter().any(|l| l.name == *n)).unwrap_or_default()
    }
}

// ---------------------------------------------------------------------------------------------
// Units

fn split_number(text: &str) -> Result<(f64, String), String> {
    let t = text.trim().replace(',', ".");
    let end = t.find(|c: char| !(c.is_ascii_digit() || matches!(c, '.' | '-' | '+' | 'e' | 'E'))).unwrap_or(t.len());
    // "e" may start a unit only if nothing numeric follows; keep it simple: a trailing 'e'/'E'
    // with no exponent digits belongs to the unit.
    let (mut num, mut unit) = t.split_at(end);
    while num.ends_with(['e', 'E']) {
        let k = num.len() - 1;
        num = &t[..k];
        unit = &t[k..];
    }
    let v: f64 = num.trim().parse().map_err(|_| format!("\"{}\" isn't a number", text.trim()))?;
    Ok((v, unit.trim().to_string()))
}

/// A force typed with its unit (N, kN, lbf; a bare number is N), in N.
pub fn parse_force(text: &str) -> Result<f64, String> {
    let (v, unit) = split_number(text)?;
    let k = match unit.to_ascii_lowercase().as_str() {
        "" | "n" => 1.0,
        "kn" => 1e3,
        "mn" if unit == "MN" => 1e6,
        "lbf" | "lb" => 4.448_221_615_260_5,
        _ => return Err(format!("unknown force unit \"{unit}\" (N, kN, lbf)")),
    };
    Ok(v * k)
}

/// A pressure typed with its unit (Pa, kPa, MPa, GPa, psi, bar; a bare number is MPa), in Pa.
pub fn parse_pressure(text: &str) -> Result<f64, String> {
    let (v, unit) = split_number(text)?;
    let k = match unit.to_ascii_lowercase().as_str() {
        "" | "mpa" | "n/mm²" | "n/mm2" => 1e6,
        "pa" => 1.0,
        "kpa" => 1e3,
        "gpa" => 1e9,
        "psi" => 6_894.757_293_168,
        "bar" => 1e5,
        _ => return Err(format!("unknown pressure unit \"{unit}\" (Pa, kPa, MPa, psi, bar)")),
    };
    Ok(v * k)
}

fn trim_number(v: f64) -> String {
    let s = format!("{v:.4}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".into() } else { s.to_string() }
}

/// "100 N", "2.5 kN".
pub fn format_force(n: f64) -> String {
    if n.abs() >= 1e4 { format!("{} kN", trim_number(n / 1e3)) } else { format!("{} N", trim_number(n)) }
}

/// "1 MPa", "250 kPa", "12 Pa".
pub fn format_pressure(pa: f64) -> String {
    if pa.abs() >= 1e5 {
        format!("{} MPa", trim_number(pa / 1e6))
    } else if pa.abs() >= 1e2 {
        format!("{} kPa", trim_number(pa / 1e3))
    } else {
        format!("{} Pa", trim_number(pa))
    }
}

// ---------------------------------------------------------------------------------------------
// Commands

fn simulation_mut(doc: &mut Document, element: ElementId) -> Result<&mut Simulation, CommandError> {
    let el = doc.element_mut(element).ok_or(CommandError::ElementNotFound(element))?;
    if !matches!(el.kind, crate::document::ElementKind::PartStudio { .. } | crate::document::ElementKind::Assembly) {
        return Err(CommandError::Invalid("simulations belong to Part Studios and Assemblies".into()));
    }
    Ok(&mut el.simulation)
}

fn check_load(load: &SimLoad) -> Result<(), CommandError> {
    if load.name.trim().is_empty() {
        return Err(CommandError::Invalid("a load needs a name".into()));
    }
    match load.kind {
        LoadKind::Force { newtons, .. } if !newtons.is_finite() => Err(CommandError::Invalid("the force isn't a number".into())),
        LoadKind::Pressure { pascals } if !pascals.is_finite() => Err(CommandError::Invalid("the pressure isn't a number".into())),
        _ => Ok(()),
    }
}

/// Adds a load to the Loads list.
#[derive(Debug, Clone)]
pub struct AddLoad {
    pub element: ElementId,
    pub load: SimLoad,
}

impl Command for AddLoad {
    fn label(&self) -> String {
        format!("Add {}", self.load.name)
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.element)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        check_load(&self.load)?;
        let sim = simulation_mut(doc, self.element)?;
        if sim.load(self.load.id).is_some() {
            return Err(CommandError::Invalid("the load exists".into()));
        }
        sim.loads.push(self.load.clone());
        Ok(())
    }
}

/// Replaces a load (its dialog's ✓, Suppress, Rename).
#[derive(Debug, Clone)]
pub struct SetLoad {
    pub element: ElementId,
    pub load: SimLoad,
}

impl Command for SetLoad {
    fn label(&self) -> String {
        format!("Edit {}", self.load.name)
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.element)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        check_load(&self.load)?;
        let sim = simulation_mut(doc, self.element)?;
        let slot = sim.loads.iter_mut().find(|l| l.id == self.load.id).ok_or_else(|| CommandError::Invalid("load not found".into()))?;
        *slot = self.load.clone();
        Ok(())
    }
}

/// Deletes a load.
#[derive(Debug, Clone)]
pub struct DeleteLoad {
    pub element: ElementId,
    pub load: LoadId,
}

impl Command for DeleteLoad {
    fn label(&self) -> String {
        "Delete load".into()
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.element)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        let sim = simulation_mut(doc, self.element)?;
        let n = sim.loads.len();
        sim.loads.retain(|l| l.id != self.load);
        if sim.loads.len() == n {
            return Err(CommandError::Invalid("load not found".into()));
        }
        Ok(())
    }
}

/// Sets the mesh density.
#[derive(Debug, Clone)]
pub struct SetMeshDensity {
    pub element: ElementId,
    pub mesh: MeshDensity,
}

impl Command for SetMeshDensity {
    fn label(&self) -> String {
        format!("Mesh {}", self.mesh.label())
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.element)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        simulation_mut(doc, self.element)?.mesh = self.mesh;
        Ok(())
    }
}

// ---------------------------------------------------------------------------------------------
// The model

/// Two parts bonded for the analysis.
#[derive(Debug, Clone, PartialEq)]
pub struct Bonded {
    pub name: String,
    pub a: PartId,
    pub b: PartId,
    /// A mate asks for it (an error if they don't touch); Part Studio contacts are optional.
    pub required: bool,
}

/// The bonds an assembly's mates make: every mate with Simulation connection checked, not
/// suppressed, between two part instances. A mate on a subassembly instance is noted and left
/// out.
pub fn assembly_bonds(asm: &crate::assembly::Assembly) -> (Vec<Bonded>, Vec<String>) {
    use crate::assembly::mate::MateKind;
    let mut bonds = Vec::new();
    let mut notes = Vec::new();
    for f in asm.mates.iter().filter(|f| !f.suppressed) {
        let MateKind::Mate(m) = &f.kind else { continue };
        if !m.simulation {
            continue;
        }
        let [a, b] = [&m.connectors[0], &m.connectors[1]].map(|c| asm.instances.iter().find(|i| i.id == c.instance));
        match (a, b) {
            (Some(a), Some(b)) if a.source.part().is_some() && b.source.part().is_some() && !a.suppressed && !b.suppressed => {
                bonds.push(Bonded { name: f.name.clone(), a: a.id.part_id(), b: b.id.part_id(), required: true });
            }
            _ => notes.push(format!("{}: only mates between part instances connect in the simulation", f.name)),
        }
    }
    (bonds, notes)
}

/// A Part Studio's contacts: every pair of parts whose bounds touch (optional bonds).
pub fn studio_bonds(parts: &[Part]) -> Vec<Bonded> {
    let bounds: Vec<Option<([f64; 3], [f64; 3])>> = parts.iter().map(|p| p.solid.bounds()).collect();
    let mut out = Vec::new();
    for i in 0..parts.len() {
        for j in i + 1..parts.len() {
            let (Some((alo, ahi)), Some((blo, bhi))) = (bounds[i], bounds[j]) else { continue };
            let tol = 1e-6 * (0..3).map(|k| ahi[k] - alo[k]).fold(1.0, f64::max);
            if (0..3).all(|k| alo[k] <= bhi[k] + tol && blo[k] <= ahi[k] + tol) {
                out.push(Bonded { name: format!("{} – {}", parts[i].name, parts[j].name), a: parts[i].id, b: parts[j].id, required: false });
            }
        }
    }
    out
}

/// A part's closed surface for the mesher: the kernel's tessellation, each triangle tagged with
/// its face's index in [`Solid::faces`].
pub fn surface(solid: &Solid) -> cadrs_fea::Surface {
    let mut faces = vec![0u32; solid.indices.len() / 3];
    for (fi, f) in solid.faces.iter().enumerate() {
        for t in f.first_triangle..(f.first_triangle + f.triangle_count).min(faces.len()) {
            faces[t] = fi as u32;
        }
    }
    cadrs_fea::Surface {
        positions: solid.positions.clone(),
        triangles: solid.indices.chunks_exact(3).map(|c| [c[0], c[1], c[2]]).collect(),
        faces,
    }
}

/// The parts an analysis takes (of `parts`, in their order): those a load acts on, and those
/// bonded to them, directly or through others.
pub fn parts_in_play(sim: &Simulation, parts: &[PartId], bonds: &[Bonded]) -> Vec<PartId> {
    let mut used: Vec<bool> = parts.iter().map(|p| sim.loads.iter().filter(|l| !l.suppressed).any(|l| l.faces.iter().any(|f| f.part == *p))).collect();
    let at = |id: PartId| parts.iter().position(|p| *p == id);
    let mut grew = true;
    while grew {
        grew = false;
        for b in bonds {
            if let (Some(i), Some(j)) = (at(b.a), at(b.b))
                && used[i] != used[j]
            {
                used[i] = true;
                used[j] = true;
                grew = true;
            }
        }
    }
    parts.iter().zip(used).filter(|(_, u)| *u).map(|(p, _)| *p).collect()
}

/// The solver's model and which part each body is.
#[derive(Debug, Clone)]
pub struct Setup {
    pub model: cadrs_fea::Model,
    pub parts: Vec<PartId>,
    pub options: cadrs_fea::Options,
    /// Things worth saying (parts left out, mates not connected).
    pub notes: Vec<String>,
}

/// Builds the analysis from the loads, the parts (with their current solids) and the bonds.
/// Errors are in words for the Simulation panel.
pub fn setup(sim: &Simulation, parts: &[Part], props: &[PartProps], bonds: &[Bonded]) -> Result<Setup, String> {
    let loads: Vec<&SimLoad> = sim.loads.iter().filter(|l| !l.suppressed).collect();
    if !loads.iter().any(|l| l.kind == LoadKind::Fixed) {
        return Err("Add a Fixed load: something has to hold the model.".into());
    }
    if !loads.iter().any(|l| l.kind != LoadKind::Fixed) {
        return Err("Add a Force or a Pressure to load the model.".into());
    }
    for l in &loads {
        if l.faces.is_empty() {
            return Err(format!("{} has no faces: edit it and pick some.", l.name));
        }
    }
    // The parts in play: those loaded, and those bonded to them.
    let part_of = |id: PartId| parts.iter().position(|p| p.id == id);
    for l in &loads {
        if l.faces.iter().any(|f| part_of(f.part).is_none()) {
            return Err(format!("{}: one of its parts is gone (the model changed). Edit the load.", l.name));
        }
    }
    let ids: Vec<PartId> = parts.iter().map(|p| p.id).collect();
    let in_play = parts_in_play(sim, &ids, bonds);
    let used: Vec<bool> = ids.iter().map(|id| in_play.contains(id)).collect();
    let mut notes = Vec::new();
    let mut model = cadrs_fea::Model::default();
    let mut ids = Vec::new();
    let mut body_of = vec![usize::MAX; parts.len()];
    for (i, p) in parts.iter().enumerate() {
        if !used[i] {
            notes.push(format!("{} carries no load and isn't connected: left out.", p.name));
            continue;
        }
        let m = crate::parts::part_material(p, props).ok_or_else(|| format!("{} has no material: assign one (right-click it → Assign material).", p.name))?;
        let (Some(e), Some(nu)) = (m.youngs_modulus, m.poisson) else {
            return Err(format!("{}'s material ({}) has no Young's modulus or Poisson's ratio.", p.name, m.name));
        };
        body_of[i] = model.bodies.len();
        ids.push(p.id);
        model.bodies.push(cadrs_fea::Body { name: p.name.clone(), surface: surface(&p.solid), material: cadrs_fea::Material { youngs: e / 1e6, poisson: nu } });
    }
    for l in &loads {
        let mut targets: Vec<(usize, Vec<u32>)> = Vec::new();
        for f in &l.faces {
            let i = part_of(f.part).expect("checked");
            let Some(fi) = parts[i].solid.faces.iter().position(|x| x.name == f.face) else {
                return Err(format!("{}: one of its faces is gone (the model changed). Edit the load.", l.name));
            };
            let b = body_of[i];
            match targets.iter_mut().find(|t| t.0 == b) {
                Some(t) => t.1.push(fi as u32),
                None => targets.push((b, vec![fi as u32])),
            }
        }
        let kind = match l.kind {
            LoadKind::Fixed => cadrs_fea::LoadKind::Fixed,
            LoadKind::Force { newtons, direction, flip } => {
                let s = if flip { -newtons } else { newtons };
                match direction {
                    ForceDirection::Normal => cadrs_fea::LoadKind::NormalForce(s),
                    ForceDirection::X => cadrs_fea::LoadKind::Force([s, 0.0, 0.0]),
                    ForceDirection::Y => cadrs_fea::LoadKind::Force([0.0, s, 0.0]),
                    ForceDirection::Z => cadrs_fea::LoadKind::Force([0.0, 0.0, s]),
                }
            }
            LoadKind::Pressure { pascals } => cadrs_fea::LoadKind::Pressure(pascals / 1e6),
        };
        model.loads.push(cadrs_fea::Load { name: l.name.clone(), targets, kind });
    }
    for b in bonds {
        let (Some(i), Some(j)) = (part_of(b.a), part_of(b.b)) else {
            if b.required {
                notes.push(format!("{}: a part is missing, so it doesn't connect.", b.name));
            }
            continue;
        };
        if body_of[i] == usize::MAX || body_of[j] == usize::MAX {
            continue;
        }
        model.bonds.push(cadrs_fea::Bond { name: b.name.clone(), a: body_of[i], b: body_of[j], required: b.required });
    }
    let options = cadrs_fea::Options { target_elements: sim.mesh.target_elements(), ..Default::default() };
    Ok(Setup { model, parts: ids, options, notes })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn units_read_and_print() {
        assert_eq!(parse_force("100").unwrap(), 100.0);
        assert_eq!(parse_force("100 N").unwrap(), 100.0);
        assert_eq!(parse_force("2.5 kN").unwrap(), 2500.0);
        assert!((parse_force("10 lbf").unwrap() - 44.482_216).abs() < 1e-5);
        assert!(parse_force("10 furlong").is_err());
        assert_eq!(parse_pressure("1").unwrap(), 1e6);
        assert_eq!(parse_pressure("250 kPa").unwrap(), 250e3);
        assert_eq!(parse_pressure("1e5 Pa").unwrap(), 1e5);
        assert!((parse_pressure("1 psi").unwrap() - 6894.757).abs() < 1e-3);
        assert_eq!(format_force(100.0), "100 N");
        assert_eq!(format_force(25_000.0), "25 kN");
        assert_eq!(format_pressure(1e6), "1 MPa");
        assert_eq!(format_pressure(2.5e4), "25 kPa");
        let f = LoadKind::Force { newtons: 100.0, direction: ForceDirection::Z, flip: true };
        assert_eq!(f.summary(), "100 N along −Z");
        assert_eq!(LoadKind::Force { newtons: 5.0, direction: ForceDirection::Normal, flip: false }.summary(), "5 N normal");
    }

    #[test]
    fn default_names_count_up() {
        let mut sim = Simulation::default();
        assert_eq!(sim.next_name(&LoadKind::Fixed), "Fixed 1");
        sim.loads.push(SimLoad { id: LoadId::new(), name: "Fixed 1".into(), kind: LoadKind::Fixed, faces: vec![], suppressed: false });
        assert_eq!(sim.next_name(&LoadKind::Fixed), "Fixed 2");
        assert_eq!(sim.next_name(&LoadKind::Pressure { pascals: 1.0 }), "Pressure 1");
    }
}
