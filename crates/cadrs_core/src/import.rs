//! The **Import** feature (Onshape's Import, `importForeign`): parts made from a CAD file stored
//! with the document. Its rebuild is in `rebuild/kernel_ops/linked.rs`.
//!
//! - The file's bytes are a blob ([`crate::blobs`]) named by their content hash; the feature
//!   keeps the hash, the original file name and the format. [`ImportFeature::from_file`] (or the
//!   [`AddImport`] command) stores the bytes and makes the feature, without the app.
//! - **STEP**: one part per solid, named after the STEP product it belongs to
//!   ([`step_occurrences`], [`assign_names`]); lengths are converted to mm by the reader.
//! - **IGES**, and STEP read **with its assembly structure** ([`ImportFeature::structure`]):
//!   the kernel's XDE reader gives each distinct part once with its occurrences (placements
//!   and names), and the feature makes either every occurrence where the file puts it
//!   ([`ImportMode::Flatten`]) or each distinct part once where its first occurrence is
//!   ([`ImportMode::Parts`], for *Keep assembly structure*, which also adds an Assembly tab).
//!   Invalid shapes are healed on the way in. The files flow that makes new tabs or a new
//!   document from a file (P3F.2, T8.1: *Import as*) is [`structure`].
//! - **STL**: the triangles of each connected piece of the mesh are sewn into a closed solid, one
//!   part per piece, named after the file. STL has no units: Onshape reads them as millimetres
//!   unless *Specify units* picks another unit ([`ImportFeature::units`]); cadrs does the same.
//! - **Y axis is up** turns the file's +Y to +Z (a quarter turn about X), as Onshape does for
//!   files from Y-up programs. *Flatten* and *composite parts* have no effect here (a Part
//!   Studio import always makes separate parts).
//! - **Face names**: face `i` of body `k` (in the file's order) is
//!   `FaceOrigin::Imported { body: k, face: i }` under the feature's id, the same on every
//!   rebuild of the same file, so later features can refer to imported faces and edges.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::command::{Command, CommandError, Scope};
use crate::document::{Document, FeatureKind};
use crate::ids::{ElementId, FeatureId};

pub mod structure;
pub use structure::{
    ImportAs, ImportFile, ImportIds, ImportMode, ImportPlan, PlanOccurrence, PlanPart, file_is_y_up, import_elements,
    imported_document, stem, y_up_turn,
};

/// The file formats Import reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ImportFormat {
    Step,
    Stl,
    /// IGES (P3F.2): read through the kernel's XDE reader, as STEP with its structure.
    Iges,
}

impl ImportFormat {
    /// The format of a file with this name (by its extension), if Import reads it.
    pub fn of_file_name(name: &str) -> Option<Self> {
        let ext = std::path::Path::new(name).extension()?.to_str()?.to_ascii_lowercase();
        match ext.as_str() {
            "step" | "stp" | "p21" => Some(Self::Step),
            "stl" => Some(Self::Stl),
            "iges" | "igs" => Some(Self::Iges),
            _ => None,
        }
    }

    /// [`Self::of_file_name`] of a path's file name.
    pub fn of_path(path: &std::path::Path) -> Option<Self> {
        Self::of_file_name(path.file_name()?.to_str()?)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Step => "STEP",
            Self::Stl => "STL",
            Self::Iges => "IGES",
        }
    }

    /// The kernel's B-rep exchange format (the XDE reader), for STEP and IGES.
    pub fn kernel(self) -> Option<cadrs_kernel::exchange::ExchangeFormat> {
        match self {
            Self::Step => Some(cadrs_kernel::exchange::ExchangeFormat::Step),
            Self::Iges => Some(cadrs_kernel::exchange::ExchangeFormat::Iges),
            Self::Stl => None,
        }
    }

    /// The extensions of the B-rep files the kernel's XDE reader takes (the files flow's
    /// picker, [`structure`]).
    pub const EXCHANGE_EXTENSIONS: [&'static str; 4] = ["step", "stp", "iges", "igs"];

    /// The extensions of every file the Import feature reads.
    pub const EXTENSIONS: [&'static str; 6] = ["step", "stp", "iges", "igs", "stl", "p21"];
}

/// A length unit for unitless mesh files (*Specify units*).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ImportUnit {
    #[default]
    Millimeter,
    Centimeter,
    Meter,
    Inch,
    Foot,
}

impl ImportUnit {
    pub const ALL: [ImportUnit; 5] = [Self::Millimeter, Self::Centimeter, Self::Meter, Self::Inch, Self::Foot];

    /// Millimetres per unit.
    pub fn mm(self) -> f64 {
        match self {
            Self::Millimeter => 1.0,
            Self::Centimeter => 10.0,
            Self::Meter => 1000.0,
            Self::Inch => 25.4,
            Self::Foot => 304.8,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Millimeter => "Millimeter",
            Self::Centimeter => "Centimeter",
            Self::Meter => "Meter",
            Self::Inch => "Inch",
            Self::Foot => "Foot",
        }
    }
}

/// An Import feature: a file stored with the document, turned into parts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImportFeature {
    /// The file's content hash ([`crate::blobs::hash_of`]); empty until a file is chosen.
    #[serde(default)]
    pub blob: String,
    /// The file's name as imported ("bracket.step"); STL parts are named after its stem.
    #[serde(default)]
    pub file_name: String,
    #[serde(default = "default_format")]
    pub format: ImportFormat,
    /// Y axis is up: the file's +Y becomes +Z.
    #[serde(default)]
    pub y_axis_up: bool,
    /// Flatten assemblies (kept for Onshape's option; a Part Studio import is always flat).
    #[serde(default)]
    pub flatten: bool,
    /// Specify units (STL): the unit of the file's numbers; `None` reads them as mm.
    #[serde(default)]
    pub units: Option<ImportUnit>,
    /// Read the file with its assembly structure (the kernel's XDE reader; P3F.2): every
    /// occurrence where the file puts it, or each distinct part once. `None` reads STEP one
    /// part per solid (as Onshape does); IGES always goes through XDE (as `Flatten`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub structure: Option<ImportMode>,
}

fn default_format() -> ImportFormat {
    ImportFormat::Step
}

impl Default for ImportFeature {
    fn default() -> Self {
        Self {
            blob: String::new(),
            file_name: String::new(),
            format: ImportFormat::Step,
            y_axis_up: false,
            flatten: false,
            units: None,
            structure: None,
        }
    }
}

impl ImportFeature {
    /// An Import of the file `file_name` with contents `bytes`: the bytes are stored as a blob
    /// ([`crate::blobs::insert`]) and the format is taken from the name (else sniffed from the
    /// contents).
    pub fn from_file(file_name: &str, bytes: Vec<u8>) -> Result<Self, String> {
        let format = ImportFormat::of_file_name(file_name)
            .or_else(|| sniff(&bytes))
            .ok_or_else(|| format!("{file_name}: not a STEP, IGES or STL file"))?;
        let blob = crate::blobs::insert(bytes);
        Ok(Self { blob, file_name: file_name.to_string(), format, ..Self::default() })
    }

    /// Why it can't be built, if it can't.
    pub fn problem(&self) -> Option<&'static str> {
        self.blob.is_empty().then_some("Select a file to import")
    }

    /// The file name without its extension ("bracket"), for part names.
    pub fn stem(&self) -> String {
        let s = std::path::Path::new(&self.file_name).file_stem().and_then(|s| s.to_str()).unwrap_or("");
        if s.is_empty() { "Imported part".to_string() } else { s.to_string() }
    }

    /// The extension its blob file gets.
    pub fn extension(&self) -> String {
        std::path::Path::new(&self.file_name)
            .extension()
            .and_then(|s| s.to_str())
            .map(|s| s.to_ascii_lowercase())
            .unwrap_or_else(|| match self.format {
                ImportFormat::Step => "step".into(),
                ImportFormat::Stl => "stl".into(),
                ImportFormat::Iges => "igs".into(),
            })
    }

    /// The factor from the file's numbers to mm (STL; STEP files carry their own units).
    pub fn scale(&self) -> f64 {
        match self.format {
            ImportFormat::Stl => self.units.unwrap_or_default().mm(),
            ImportFormat::Step | ImportFormat::Iges => 1.0,
        }
    }
}

/// The format of a file by its contents.
fn sniff(bytes: &[u8]) -> Option<ImportFormat> {
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(512)]).to_ascii_uppercase();
    if head.contains("ISO-10303-21") {
        return Some(ImportFormat::Step);
    }
    // IGES: 80-column records, the first a Start ('S') line.
    if head.lines().next().is_some_and(|l| l.trim_end_matches('\r').len() == 80 && l.as_bytes()[72] == b'S') {
        return Some(ImportFormat::Iges);
    }
    if parse_stl(bytes).is_ok() {
        return Some(ImportFormat::Stl);
    }
    None
}

/// Inserts an Import of a file (its bytes stored with the document) at the end of a Part
/// Studio: "Import N". The bytes are kept in the blob cache ([`crate::blobs`]); the Store writes
/// them next to the document when it is saved.
#[derive(Debug, Clone)]
pub struct AddImport {
    pub element: ElementId,
    pub feature: FeatureId,
    pub file_name: String,
    pub bytes: Arc<Vec<u8>>,
    pub y_axis_up: bool,
    pub units: Option<ImportUnit>,
}

impl Command for AddImport {
    fn label(&self) -> String {
        "Insert import".into()
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.element)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        let mut x = ImportFeature::from_file(&self.file_name, self.bytes.to_vec()).map_err(CommandError::Invalid)?;
        x.y_axis_up = self.y_axis_up;
        x.units = self.units;
        crate::commands::AddFeature {
            element: self.element,
            feature: self.feature,
            base_name: "Import".into(),
            kind: FeatureKind::Import(x),
        }
        .apply(doc)
    }
}

// ---------------------------------------------------------------------------------------------
// STL

/// A triangle (three corners, mm or the file's units).
pub type Triangle = [[f64; 3]; 3];

/// The triangles of an STL file, binary or ASCII.
pub fn parse_stl(bytes: &[u8]) -> Result<Vec<Triangle>, String> {
    if bytes.len() >= 84 {
        let n = u32::from_le_bytes([bytes[80], bytes[81], bytes[82], bytes[83]]) as usize;
        if n.checked_mul(50).and_then(|b| b.checked_add(84)) == Some(bytes.len()) {
            let f = |o: usize| f32::from_le_bytes([bytes[o], bytes[o + 1], bytes[o + 2], bytes[o + 3]]) as f64;
            return Ok((0..n)
                .map(|i| {
                    let o = 84 + i * 50 + 12;
                    [0, 1, 2].map(|v| [f(o + v * 12), f(o + v * 12 + 4), f(o + v * 12 + 8)])
                })
                .collect());
        }
    }
    let text = std::str::from_utf8(bytes).map_err(|_| "not an STL file".to_string())?;
    if !text.trim_start().to_ascii_lowercase().starts_with("solid") {
        return Err("not an STL file".into());
    }
    let mut out = Vec::new();
    let mut corners: Vec<[f64; 3]> = Vec::new();
    for line in text.lines() {
        let mut w = line.split_whitespace();
        match w.next() {
            Some(k) if k.eq_ignore_ascii_case("vertex") => {
                let v: Vec<f64> = w.take(3).map(|s| s.parse::<f64>()).collect::<Result<_, _>>().map_err(|e| format!("bad vertex: {e}"))?;
                if v.len() != 3 {
                    return Err("bad vertex".into());
                }
                corners.push([v[0], v[1], v[2]]);
            }
            Some(k) if k.eq_ignore_ascii_case("endloop") => {
                // A facet with more corners is a fan.
                for i in 1..corners.len().saturating_sub(1) {
                    out.push([corners[0], corners[i], corners[i + 1]]);
                }
                corners.clear();
            }
            _ => {}
        }
    }
    if out.is_empty() {
        return Err("the STL file has no triangles".into());
    }
    Ok(out)
}

/// The triangles in separate connected pieces (triangles sharing a corner are connected), in
/// the order of their first triangle. Degenerate triangles are dropped.
pub fn stl_pieces(tris: &[Triangle]) -> Vec<Vec<Triangle>> {
    use std::collections::HashMap;
    let key = |p: &[f64; 3]| p.map(f64::to_bits);
    let mut vid: HashMap<[u64; 3], usize> = HashMap::new();
    let mut idx: Vec<[usize; 3]> = Vec::with_capacity(tris.len());
    for t in tris {
        let ids = t.map(|p| {
            let n = vid.len();
            *vid.entry(key(&p)).or_insert(n)
        });
        idx.push(ids);
    }
    let mut parent: Vec<usize> = (0..vid.len()).collect();
    fn root(p: &mut [usize], mut i: usize) -> usize {
        while p[i] != i {
            p[i] = p[p[i]];
            i = p[i];
        }
        i
    }
    for t in &idx {
        for k in 1..3 {
            let (a, b) = (root(&mut parent, t[0]), root(&mut parent, t[k]));
            if a != b {
                parent[a.max(b)] = a.min(b);
            }
        }
    }
    let mut pieces: Vec<(usize, Vec<Triangle>)> = Vec::new();
    for (t, ids) in tris.iter().zip(&idx) {
        if ids[0] == ids[1] || ids[1] == ids[2] || ids[0] == ids[2] {
            continue;
        }
        let r = root(&mut parent, ids[0]);
        match pieces.iter_mut().find(|(k, _)| *k == r) {
            Some((_, v)) => v.push(*t),
            None => pieces.push((r, vec![*t])),
        }
    }
    pieces.into_iter().map(|(_, v)| v).collect()
}

// ---------------------------------------------------------------------------------------------
// STEP product names

#[derive(Debug, Clone)]
enum Val {
    Str(String),
    Ref(u64),
    List(Vec<Val>),
    Other,
}

impl Val {
    fn as_ref(&self) -> Option<u64> {
        match self {
            Val::Ref(r) => Some(*r),
            _ => None,
        }
    }
    fn refs(&self) -> Vec<u64> {
        match self {
            Val::List(v) => v.iter().filter_map(Val::as_ref).collect(),
            Val::Ref(r) => vec![*r],
            _ => Vec::new(),
        }
    }
    fn text(&self) -> &str {
        match self {
            Val::Str(s) => s,
            _ => "",
        }
    }
}

/// A simple entity instance: its type and arguments (complex instances are left out).
struct Entity {
    kind: String,
    args: Vec<Val>,
}

/// Decodes STEP's `\X2\…\X0\` (UTF-16 hex) and `\X\hh` escapes; other text as it is.
fn decode_step_string(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(i) = rest.find('\\') {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        if let Some(body) = tail.strip_prefix("\\X2\\")
            && let Some(end) = body.find("\\X0\\")
        {
            let units: Vec<u16> = body.as_bytes()[..end]
                .chunks(4)
                .filter_map(|c| u16::from_str_radix(std::str::from_utf8(c).ok()?, 16).ok())
                .collect();
            out.push_str(&String::from_utf16_lossy(&units));
            rest = &body[end + 4..];
        } else if let Some(body) = tail.strip_prefix("\\X\\")
            && body.len() >= 2
            && let Ok(b) = u8::from_str_radix(&body[..2], 16)
        {
            out.push(b as char);
            rest = &body[2..];
        } else {
            out.push('\\');
            rest = &tail[1..];
        }
    }
    out.push_str(rest);
    out
}

fn parse_args(s: &[u8], i: &mut usize) -> Vec<Val> {
    // At '('.
    *i += 1;
    let mut out = Vec::new();
    loop {
        while *i < s.len() && (s[*i] as char).is_whitespace() {
            *i += 1;
        }
        if *i >= s.len() {
            return out;
        }
        match s[*i] {
            b')' => {
                *i += 1;
                return out;
            }
            b',' => *i += 1,
            b'\'' => {
                *i += 1;
                let mut bytes = Vec::new();
                while *i < s.len() {
                    if s[*i] == b'\'' {
                        if s.get(*i + 1) == Some(&b'\'') {
                            bytes.push(b'\'');
                            *i += 2;
                            continue;
                        }
                        *i += 1;
                        break;
                    }
                    bytes.push(s[*i]);
                    *i += 1;
                }
                out.push(Val::Str(decode_step_string(&String::from_utf8_lossy(&bytes))));
            }
            b'#' => {
                *i += 1;
                let st = *i;
                while *i < s.len() && s[*i].is_ascii_digit() {
                    *i += 1;
                }
                out.push(std::str::from_utf8(&s[st..*i]).ok().and_then(|t| t.parse().ok()).map_or(Val::Other, Val::Ref));
            }
            b'(' => out.push(Val::List(parse_args(s, i))),
            _ => {
                // A number, an enumeration, $, *, or a typed value NAME(...).
                while *i < s.len() && !matches!(s[*i], b',' | b')' | b'(') {
                    *i += 1;
                }
                if *i < s.len() && s[*i] == b'(' {
                    parse_args(s, i);
                }
                out.push(Val::Other);
            }
        }
    }
}

/// The simple entity instances of a STEP file's DATA section, by id.
fn step_entities(text: &str) -> std::collections::BTreeMap<u64, Entity> {
    let mut out = std::collections::BTreeMap::new();
    let Some(start) = text.find("DATA;") else { return out };
    let s = &text.as_bytes()[start + 5..];
    let mut i = 0;
    while i < s.len() {
        // Find the next '#'.
        match s[i] {
            b'#' => {}
            b'/' if s.get(i + 1) == Some(&b'*') => {
                i += 2;
                while i + 1 < s.len() && !(s[i] == b'*' && s[i + 1] == b'/') {
                    i += 1;
                }
                i += 2;
                continue;
            }
            b'\'' => {
                // A stray string (shouldn't happen outside instances): skip it.
                i += 1;
                while i < s.len() && s[i] != b'\'' {
                    i += 1;
                }
                i += 1;
                continue;
            }
            _ => {
                i += 1;
                continue;
            }
        }
        i += 1;
        let st = i;
        while i < s.len() && s[i].is_ascii_digit() {
            i += 1;
        }
        let Some(id) = std::str::from_utf8(&s[st..i]).ok().and_then(|t| t.parse::<u64>().ok()) else { continue };
        while i < s.len() && (s[i] as char).is_whitespace() {
            i += 1;
        }
        if s.get(i) != Some(&b'=') {
            continue;
        }
        i += 1;
        while i < s.len() && (s[i] as char).is_whitespace() {
            i += 1;
        }
        if s.get(i) == Some(&b'(') {
            // A complex instance: skip to its ';' (outside strings).
            let _ = parse_args(s, &mut i);
        } else {
            let st = i;
            while i < s.len() && (s[i].is_ascii_alphanumeric() || s[i] == b'_') {
                i += 1;
            }
            let kind = String::from_utf8_lossy(&s[st..i]).to_ascii_uppercase();
            while i < s.len() && (s[i] as char).is_whitespace() {
                i += 1;
            }
            if s.get(i) == Some(&b'(') {
                let args = parse_args(s, &mut i);
                out.insert(id, Entity { kind, args });
            }
        }
        while i < s.len() && s[i] != b';' {
            i += 1;
        }
    }
    out
}

/// The solids of a STEP file as its assembly structure places them, in order: for each, the
/// name of the product it belongs to and its number of faces. Products are visited depth first
/// from the top-level ones through their `NEXT_ASSEMBLY_USAGE_OCCURRENCE`s (a product used
/// twice appears twice), each product's own solids first.
pub fn step_occurrences(text: &str) -> Vec<(String, usize)> {
    use std::collections::{BTreeMap, HashMap};
    let ents = step_entities(text);
    let arg = |e: &Entity, i: usize| e.args.get(i).cloned().unwrap_or(Val::Other);
    let mut product_name: HashMap<u64, String> = HashMap::new();
    let mut pdf_product: HashMap<u64, u64> = HashMap::new();
    let mut pd_pdf: HashMap<u64, u64> = HashMap::new();
    let mut pds_def: HashMap<u64, u64> = HashMap::new();
    let mut sdr: Vec<(u64, u64)> = Vec::new();
    let mut rep_items: HashMap<u64, Vec<u64>> = HashMap::new();
    let mut srr: Vec<(u64, u64)> = Vec::new();
    let mut brep_shells: BTreeMap<u64, Vec<u64>> = BTreeMap::new();
    let mut shell_faces: HashMap<u64, usize> = HashMap::new();
    let mut nauo: Vec<(u64, u64)> = Vec::new();
    for (id, e) in &ents {
        match e.kind.as_str() {
            "PRODUCT" => {
                let name = arg(e, 1).text().trim().to_string();
                let name = if name.is_empty() { arg(e, 0).text().trim().to_string() } else { name };
                product_name.insert(*id, name);
            }
            "PRODUCT_DEFINITION_FORMATION" | "PRODUCT_DEFINITION_FORMATION_WITH_SPECIFIED_SOURCE" => {
                if let Some(p) = arg(e, 2).as_ref() {
                    pdf_product.insert(*id, p);
                }
            }
            "PRODUCT_DEFINITION" | "PRODUCT_DEFINITION_WITH_ASSOCIATED_DOCUMENTS" => {
                if let Some(p) = arg(e, 2).as_ref() {
                    pd_pdf.insert(*id, p);
                }
            }
            "PRODUCT_DEFINITION_SHAPE" => {
                if let Some(d) = arg(e, 2).as_ref() {
                    pds_def.insert(*id, d);
                }
            }
            "SHAPE_DEFINITION_REPRESENTATION" => {
                if let (Some(a), Some(b)) = (arg(e, 0).as_ref(), arg(e, 1).as_ref()) {
                    sdr.push((a, b));
                }
            }
            "SHAPE_REPRESENTATION_RELATIONSHIP" => {
                if let (Some(a), Some(b)) = (arg(e, 2).as_ref(), arg(e, 3).as_ref()) {
                    srr.push((a, b));
                }
            }
            "MANIFOLD_SOLID_BREP" | "FACETED_BREP" => {
                brep_shells.insert(*id, arg(e, 1).refs());
            }
            "BREP_WITH_VOIDS" => {
                let mut s = arg(e, 1).refs();
                s.extend(arg(e, 2).refs());
                brep_shells.insert(*id, s);
            }
            "CLOSED_SHELL" | "ORIENTED_CLOSED_SHELL" => {
                shell_faces.insert(*id, arg(e, 1).refs().len());
            }
            "NEXT_ASSEMBLY_USAGE_OCCURRENCE" => {
                if let (Some(a), Some(b)) = (arg(e, 3).as_ref(), arg(e, 4).as_ref()) {
                    nauo.push((a, b));
                }
            }
            k if k.ends_with("REPRESENTATION") => {
                rep_items.insert(*id, arg(e, 1).refs());
            }
            _ => {}
        }
    }
    let pd_name = |pd: u64| -> String {
        pd_pdf.get(&pd).and_then(|f| pdf_product.get(f)).and_then(|p| product_name.get(p)).cloned().unwrap_or_default()
    };
    // Which product definition each representation belongs to.
    let mut rep_pd: HashMap<u64, u64> = HashMap::new();
    for (pds, rep) in &sdr {
        if let Some(pd) = pds_def.get(pds).filter(|d| pd_pdf.contains_key(d)) {
            rep_pd.insert(*rep, *pd);
        }
    }
    loop {
        let mut changed = false;
        for (a, b) in &srr {
            match (rep_pd.get(a).copied(), rep_pd.get(b).copied()) {
                (Some(p), None) => {
                    rep_pd.insert(*b, p);
                    changed = true;
                }
                (None, Some(p)) => {
                    rep_pd.insert(*a, p);
                    changed = true;
                }
                _ => {}
            }
        }
        if !changed {
            break;
        }
    }
    let mut pd_breps: HashMap<u64, Vec<(u64, usize)>> = HashMap::new();
    for (rep, items) in &rep_items {
        let Some(pd) = rep_pd.get(rep) else { continue };
        for it in items {
            if let Some(shells) = brep_shells.get(it) {
                let faces = shells.iter().filter_map(|s| shell_faces.get(s)).sum();
                pd_breps.entry(*pd).or_default().push((*it, faces));
            }
        }
    }
    for v in pd_breps.values_mut() {
        v.sort();
        v.dedup();
    }
    let children: std::collections::HashSet<u64> = nauo.iter().map(|(_, c)| *c).collect();
    let mut roots: Vec<u64> = pd_pdf.keys().copied().filter(|pd| !children.contains(pd)).collect();
    roots.sort();
    let mut out = Vec::new();
    fn visit(
        pd: u64,
        depth: usize,
        nauo: &[(u64, u64)],
        pd_breps: &HashMap<u64, Vec<(u64, usize)>>,
        name: &dyn Fn(u64) -> String,
        out: &mut Vec<(String, usize)>,
    ) {
        if depth > 64 {
            return;
        }
        for (_, faces) in pd_breps.get(&pd).map_or(&[][..], |v| v) {
            out.push((name(pd), *faces));
        }
        for (p, c) in nauo {
            if *p == pd {
                visit(*c, depth + 1, nauo, pd_breps, name, out);
            }
        }
    }
    for r in roots {
        visit(r, 0, &nauo, &pd_breps, &pd_name, &mut out);
    }
    out
}

/// Names for the solids a STEP file gave, in the reader's order with their face counts: each
/// takes the first unused occurrence ([`step_occurrences`]) with the same number of faces, else
/// the next unused one, else `fallback`.
pub fn assign_names(solid_faces: &[usize], occurrences: &[(String, usize)], fallback: &str) -> Vec<String> {
    let mut used = vec![false; occurrences.len()];
    solid_faces
        .iter()
        .map(|n| {
            let pick = (0..occurrences.len())
                .find(|&i| !used[i] && occurrences[i].1 == *n)
                .or_else(|| (0..occurrences.len()).find(|&i| !used[i]));
            match pick {
                Some(i) => {
                    used[i] = true;
                    let s = occurrences[i].0.trim();
                    if s.is_empty() { fallback.to_string() } else { s.to_string() }
                }
                None => fallback.to_string(),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const ASM: &str = "ISO-10303-21;\nHEADER;\nENDSEC;\nDATA;\n\
#1=PRODUCT('asm','Top asm','',(#90));\n#2=PRODUCT_DEFINITION_FORMATION('','',#1);\n#3=PRODUCT_DEFINITION('design','',#2,#91);\n\
#4=PRODUCT('bolt','Bolt \\X2\\00E9\\X0\\','',(#90));\n#5=PRODUCT_DEFINITION_FORMATION('','',#4);\n#6=PRODUCT_DEFINITION('design','',#5,#91);\n\
#7=PRODUCT('plate','','',(#90));\n#8=PRODUCT_DEFINITION_FORMATION('','',#7);\n#9=PRODUCT_DEFINITION('design','',#8,#91);\n\
#10=NEXT_ASSEMBLY_USAGE_OCCURRENCE('1','','',#3,#9,$);\n#11=NEXT_ASSEMBLY_USAGE_OCCURRENCE('2','','',#3,#6,$);\n#12=NEXT_ASSEMBLY_USAGE_OCCURRENCE('3','','',#3,#6,$);\n\
#20=PRODUCT_DEFINITION_SHAPE('','',#6);\n#21=SHAPE_DEFINITION_REPRESENTATION(#20,#22);\n#22=SHAPE_REPRESENTATION('',(#99),#92);\n\
#23=SHAPE_REPRESENTATION_RELATIONSHIP('','',#22,#24);\n#24=ADVANCED_BREP_SHAPE_REPRESENTATION('',(#25,#99),#92);\n\
#25=MANIFOLD_SOLID_BREP('',#26);\n#26=CLOSED_SHELL('',(#40,#41,#42));\n\
#30=PRODUCT_DEFINITION_SHAPE('','',#9);\n#31=SHAPE_DEFINITION_REPRESENTATION(#30,#32);\n#32=ADVANCED_BREP_SHAPE_REPRESENTATION('',(#33),#92);\n\
#33=MANIFOLD_SOLID_BREP('',#34);\n#34=CLOSED_SHELL('',(#40,#41,#42,#43,#44,#45));\n\
#50=(REPRESENTATION_RELATIONSHIP('','',#22,#32)REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION(#60)SHAPE_REPRESENTATION_RELATIONSHIP());\n\
ENDSEC;\nEND-ISO-10303-21;\n";

    #[test]
    fn step_products_in_assembly_order() {
        let occ = step_occurrences(ASM);
        assert_eq!(
            occ,
            vec![("plate".to_string(), 6), ("Bolt é".to_string(), 3), ("Bolt é".to_string(), 3)]
        );
        // The reader's order differs: matched by face count.
        let names = assign_names(&[3, 6, 3, 9], &occ, "file");
        assert_eq!(names, ["Bolt é", "plate", "Bolt é", "file"]);
    }

    #[test]
    fn stl_ascii_binary_and_pieces() {
        let tri = |o: f64| [[o, 0.0, 0.0], [o + 1.0, 0.0, 0.0], [o, 1.0, 0.0]];
        let ascii = "solid t\nfacet normal 0 0 1\n outer loop\n vertex 0 0 0\n vertex 1 0 0\n vertex 0 1 0\n endloop\nendfacet\nendsolid t\n";
        assert_eq!(parse_stl(ascii.as_bytes()).unwrap(), vec![tri(0.0)]);
        let mut bin = vec![0u8; 80];
        bin.extend_from_slice(&2u32.to_le_bytes());
        for t in [tri(0.0), tri(5.0)] {
            bin.extend_from_slice(&[0u8; 12]);
            for v in t {
                for c in v {
                    bin.extend_from_slice(&(c as f32).to_le_bytes());
                }
            }
            bin.extend_from_slice(&[0u8; 2]);
        }
        let tris = parse_stl(&bin).unwrap();
        assert_eq!(tris, vec![tri(0.0), tri(5.0)]);
        assert_eq!(stl_pieces(&tris).len(), 2);
        assert!(parse_stl(b"garbage").is_err());
        assert_eq!(ImportFormat::of_file_name("a/B.STP"), Some(ImportFormat::Step));
        assert_eq!(ImportFormat::of_file_name("x.stl"), Some(ImportFormat::Stl));
        assert_eq!(ImportFormat::of_file_name("x.IGS"), Some(ImportFormat::Iges));
    }
}
